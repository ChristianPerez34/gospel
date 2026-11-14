//! Direct desktop agent runtime: sequential model→tool loop on rig-core, no Bevy.
//!
//! Gospel owns sessions and approval policy; rig-core owns model/tool wire
//! calls. Transient model failures retry with bounded backoff (up to 3
//! retries, 1s doubling, provider Retry-After honored up to 60s). Loop
//! guards, stream events, and history stay Gospel-owned.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::StreamExt;
use rig::completion::{
    AssistantContent, CompletionRequest, CompletionResponse, Effort, GenerationOptions, Message,
    ToolDefinition,
};
use rig::error::{ErrorReport, ProviderError};
use rig::streaming::{Item as StreamItem, PartKind, StreamEvent as ProviderEvent};
use rig::tool::{RegisteredTool, ToolContext, ToolSet};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::harness_profile::{HarnessRunGuards, LoopDetector, LoopStatus};
use crate::llm::{
    self, LlmError, ReasoningPayload, ReasoningPhase, StreamCompletionResult, StreamEvent,
};
use crate::provider_client::CompletionModel;
use crate::subagent::SubagentRelay;

#[cfg(test)]
mod tests;

pub(crate) struct RunRequest {
    pub session_id: Option<String>,
    pub model: CompletionModel,
    pub prompt: String,
    pub preamble: Option<String>,
    pub history: Vec<Message>,
    pub tools: Vec<RegisteredTool>,
    pub guards: HarnessRunGuards,
    pub additional_params: Option<serde_json::Value>,
    pub reasoning: Option<Effort>,
    /// Lets sub-agent tools in this run report live steps on its channel.
    pub subagents: Option<Arc<SubagentRelay>>,
}

const MAX_RETRIES: u32 = 3;
const FIRST_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const MAX_ASKED_WAIT: Duration = Duration::from_secs(60);

/// Doubling backoff, capped at 30s.
// ponytail: no jitter — single desktop user, no thundering herd to spread.
fn backoff_for_attempt(retries: u32) -> Duration {
    let doubled = FIRST_BACKOFF.saturating_mul(1 << retries.min(16));
    doubled.min(MAX_BACKOFF)
}

/// How long to wait before re-sending a failed model call, or `None` to fail.
fn retry_wait(error: &ProviderError, retries: u32) -> Option<Duration> {
    if retries >= MAX_RETRIES {
        return None;
    }
    let mut report = ErrorReport::from(error);
    // Rig 0.46 does not recognize this gateway code yet.
    if report.code.as_deref() == Some("upstream_error")
        && !report.refusal
        && report
            .http_status
            .is_none_or(|status| (200..300).contains(&status))
    {
        report.retryable = true;
    }
    if report.is_context_overflow() || !report.is_retryable() {
        return None;
    }
    match report.retry_after(SystemTime::now()) {
        Some(asked) if asked > MAX_ASKED_WAIT => None,
        Some(asked) => Some(asked),
        None => Some(backoff_for_attempt(retries)),
    }
}

/// Empty when the answer has no text; callers decide whether to show a
/// fallback, since non-chat callers (e.g. title generation) treat empty output
/// as "no result".
fn final_response(message: &Message) -> String {
    message.final_answer().unwrap_or_else(|| {
        let Message::Assistant(assistant) = message else {
            return String::new();
        };
        assistant
            .content
            .iter()
            .rev()
            .find_map(|part| match part {
                AssistantContent::Text(text) if !text.text.trim().is_empty() => {
                    Some(text.text.trim().to_string())
                }
                _ => None,
            })
            .unwrap_or_default()
    })
}

fn reasoning_id(
    ids: &mut Vec<String>,
    active: &mut Option<String>,
    sequence: &mut usize,
) -> String {
    if let Some(id) = active.clone() {
        return id;
    }
    let id = llm::ensure_active_reasoning_id(active, sequence);
    ids.push(id.clone());
    id
}

fn build_request(
    history: &[Message],
    preamble: Option<&str>,
    definitions: &[Arc<ToolDefinition>],
    additional_params: Option<&serde_json::Value>,
    options: &GenerationOptions,
) -> CompletionRequest {
    let mut tail = history.iter();
    let prompt = tail.next_back().cloned().expect("history holds the prompt");
    // Set additional params before tools: a `tools` key in params replaces
    // provider tools added before it.
    let mut request = CompletionRequest::new(prompt).messages(tail.cloned());
    if let Some(preamble) = preamble {
        request = request.preamble(preamble.to_owned());
    }
    request = request.additional_params(additional_params.cloned());
    if !definitions.is_empty() {
        request = request.tools(definitions.iter().cloned());
    }
    if !options.is_default() {
        request = request.options(options.clone());
    }
    request
}

#[allow(clippy::too_many_arguments)]
async fn stream_response<F>(
    model: &CompletionModel,
    request: CompletionRequest,
    live_text: &mut String,
    reasoning_ids: &mut Vec<String>,
    active_reasoning: &mut Option<String>,
    reasoning_sequence: &mut usize,
    on_event: &mut F,
) -> Result<CompletionResponse, Box<ProviderError>>
where
    F: FnMut(StreamEvent),
{
    let mut stream = model.stream(request).map_err(Box::new)?;
    while let Some(item) = stream.next().await {
        match item {
            Ok(StreamItem::Event(event)) => match event {
                ProviderEvent::Start {
                    kind: PartKind::Reasoning,
                    ..
                } => *active_reasoning = None,
                ProviderEvent::Text { text, .. } => {
                    live_text.push_str(&text);
                    on_event(StreamEvent::Text(text));
                }
                ProviderEvent::Reasoning { text, .. } if !text.is_empty() => {
                    let id = reasoning_id(reasoning_ids, active_reasoning, reasoning_sequence);
                    on_event(StreamEvent::Reasoning(ReasoningPayload {
                        id,
                        text,
                        phase: ReasoningPhase::Delta,
                    }));
                }
                ProviderEvent::End {
                    content: AssistantContent::Reasoning(reasoning),
                    ..
                } => {
                    let id = reasoning_id(reasoning_ids, active_reasoning, reasoning_sequence);
                    *active_reasoning = None;
                    if !reasoning.redacted && !reasoning.text.is_empty() {
                        on_event(StreamEvent::Reasoning(ReasoningPayload {
                            id,
                            text: reasoning.text.clone(),
                            phase: ReasoningPhase::Complete,
                        }));
                    }
                }
                _ => {}
            },
            Ok(StreamItem::Unknown(_)) => {}
            Err(error) => return Err(Box::new(error)),
        }
    }
    stream.finish().await.map_err(Box::new)
}

fn drain_steps<F>(received: &mut UnboundedReceiver<StreamEvent>, on_event: &mut F)
where
    F: FnMut(StreamEvent),
{
    while let Ok(event) = received.try_recv() {
        on_event(event);
    }
}

pub(crate) async fn run<F>(
    request: RunRequest,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent),
{
    match request.guards.deadline {
        Some(deadline) => tokio::time::timeout(deadline, drive(request, &mut on_event))
            .await
            .map_err(|_| {
                LlmError::ControlledStop(format!(
                    "Agent timed out after {} seconds",
                    deadline.as_secs()
                ))
            })?,
        None => drive(request, &mut on_event).await,
    }
}

async fn drive<F>(request: RunRequest, on_event: &mut F) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent),
{
    let mut toolset = ToolSet::default();
    for tool in request.tools {
        if tool.is_live() {
            toolset.add_registered_tool(tool);
        }
    }
    let catalog = toolset.catalog();
    let definitions: Vec<Arc<ToolDefinition>> = catalog.definitions().to_vec();

    let mut options = GenerationOptions::new();
    if let Some(effort) = request.reasoning {
        options = options.reasoning(effort);
    }
    // Stable prompt-cache identity across turns of one session.
    if let Some(session_id) = request.session_id.as_deref() {
        options = options.cache_key(session_id.to_owned());
    }

    let prompt_tokens = llm::estimate_tokens(&request.prompt)
        + request
            .history
            .iter()
            .map(llm::estimate_message_tokens)
            .sum::<usize>();
    let mut history = request.history;
    history.push(Message::user(request.prompt.clone()));

    // The relay holds a weak ref; the sender must outlive the run.
    let (sender, mut received) = unbounded_channel();
    if let Some(relay) = &request.subagents {
        relay.attach(&sender);
    }

    let mut detector = LoopDetector::new(request.guards.loop_guard);
    let mut live_text = String::new();
    let mut reasoning_ids: Vec<String> = Vec::new();
    let mut active_reasoning: Option<String> = None;
    let mut reasoning_sequence = 0usize;
    let mut calls = 0usize;
    let mut tool_calls = 0usize;
    let mut source_edit_succeeded = false;

    loop {
        if let Some(max_turns) = request.guards.max_turns {
            if max_turns > 0 && calls >= max_turns {
                return Err(LlmError::ControlledStop(format!(
                    "Agent reached its {max_turns} model-call limit"
                )));
            }
        }
        calls += 1;
        let attempt_start = live_text.len();
        // Retries re-send this snapshot; committed tool history only grows
        // after a successful model response, so tools never re-execute.
        let turn_history = history.clone();

        let mut retries = 0u32;
        let response = loop {
            let completion_request = build_request(
                &turn_history,
                request.preamble.as_deref(),
                &definitions,
                request.additional_params.as_ref(),
                &options,
            );
            match stream_response(
                &request.model,
                completion_request,
                &mut live_text,
                &mut reasoning_ids,
                &mut active_reasoning,
                &mut reasoning_sequence,
                on_event,
            )
            .await
            {
                Ok(response) => break response,
                Err(error) => {
                    let Some(wait) = retry_wait(&error, retries) else {
                        return Err(LlmError::ProviderError(error.to_string()));
                    };
                    retries += 1;
                    live_text.truncate(attempt_start);
                    on_event(StreamEvent::RetryReset {
                        text: live_text.clone(),
                        reasoning_ids: reasoning_ids.clone(),
                    });
                    reasoning_ids.clear();
                    active_reasoning = None;
                    tokio::time::sleep(wait).await;
                }
            }
        };

        let assistant = response
            .message()
            .unwrap_or_else(|| Message::assistant(response.text()));
        let full_response = final_response(&assistant);
        let turn_calls: Vec<_> = response.tool_calls().cloned().collect();
        if turn_calls.is_empty() {
            history.push(assistant);
            return Ok(StreamCompletionResult {
                response_tokens: llm::estimate_tokens(&full_response),
                full_response,
                history: Some(history),
                source_edit_succeeded,
                prompt_tokens,
                tool_calls,
            });
        }

        // Gate every call before running any, so a stopped turn executes nothing.
        for call in &turn_calls {
            let name = call.function.name.to_string();
            let arguments = serde_json::Value::Object(call.function.arguments.clone());
            match detector.record_call(&name, &arguments) {
                LoopStatus::Ok => {}
                LoopStatus::Warning(count) => on_event(StreamEvent::LoopWarning {
                    count,
                    tool_name: name.clone(),
                }),
                LoopStatus::Stop => {
                    let count = detector.consecutive_count();
                    let message = format!(
                        "Agent stopped: repeated identical tool call '{name}' detected {count} times."
                    );
                    on_event(StreamEvent::LoopStopped {
                        count,
                        tool_name: name,
                        message: message.clone(),
                    });
                    return Err(LlmError::ControlledStop(message));
                }
            }
            tool_calls += 1;
            if let Some(relay) = &request.subagents {
                relay.record_call(&call.id.to_string(), &name, &arguments);
            }
            on_event(StreamEvent::ToolCall {
                id: call.id.to_string(),
                name,
                arguments,
            });
        }

        let mut tool_results = Vec::with_capacity(turn_calls.len());
        for call in &turn_calls {
            let name = call.function.name.to_string();
            let id = call.id.to_string();
            let Some(tool) = catalog.get(name.as_str()) else {
                return Err(LlmError::ProviderError(format!(
                    "model attempted to call unknown tool `{name}`"
                )));
            };
            let args = serde_json::Value::Object(call.function.arguments.clone()).to_string();
            let mut context = ToolContext::default();
            let output = tool.execute(args, &mut context).await;
            let result = llm::tool_result_summary(output.output().as_content());
            if name == "source_edit" && llm::source_edit_result_changed(&result) {
                source_edit_succeeded = true;
            }
            if let Some(relay) = &request.subagents {
                relay.forget(&id);
            }
            drain_steps(&mut received, on_event);
            on_event(StreamEvent::ToolResult {
                id: id.clone(),
                name: name.clone(),
                result: result.clone(),
            });
            if let Some((reason, status)) = llm::record_tool_result_failure(&mut detector, &result)
            {
                match status {
                    LoopStatus::Warning(count) => on_event(StreamEvent::LoopWarning {
                        count,
                        tool_name: name,
                    }),
                    LoopStatus::Stop => {
                        let count = detector.failure_streak();
                        let message = format!("Agent stopped: repeated deterministic failure '{reason}' detected {count} times.");
                        on_event(StreamEvent::LoopStopped {
                            count,
                            tool_name: name,
                            message: message.clone(),
                        });
                        return Err(LlmError::ControlledStop(message));
                    }
                    LoopStatus::Ok => {}
                }
            }
            tool_results.push(call.answer(&output));
        }
        drain_steps(&mut received, on_event);
        history.push(assistant);
        history.push(Message::tool_results(tool_results));
    }
}
