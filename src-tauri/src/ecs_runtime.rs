//! The shared ECS execution adapter. Gospel owns sessions and approval policy;
//! Rig owns model/tool execution, retry scheduling, and turn lifecycle.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, SystemTime};

use futures::future::poll_fn;
use rig::catalog::{ModelSpec, ReasoningSupport};
use rig::completion::message::AssistantContent;
use rig::completion::{Effort, Message};
use rig::effect::{family, EffectKind, HandlerDescriptor};
use rig::providers::registry::ProviderId;
use rig::serve::{adapters::ModelAdapter, Dispatch, ErasedHandler, Reply, Serve};
use rig::streaming::{PartKind, StreamEvent as ProviderEvent};
use rig::tool::{RegisteredTool, ToolSet};
use rig_ecs::effects::Handler;
use rig_ecs::prelude::*;
use rig_ecs::retry::RetryPolicy;
use rig_ecs::stage::{ReadyToSettle, ReadyToStart};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

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

/// Bevy polls handlers on its pools. Enter Tokio on every poll, including
/// stream polls, so existing HTTP, process, MCP and approval futures work.
struct TokioHandler {
    inner: ErasedHandler,
    runtime: Handle,
}

impl Serve for TokioHandler {
    type Family = family::Dynamic;
    fn descriptor(&self) -> HandlerDescriptor {
        self.inner.descriptor()
    }
    async fn serve(&self, kind: EffectKind, dispatch: Dispatch) -> Reply {
        let mut future = self.inner.handle(kind, dispatch);
        let reply = poll_fn(|cx| {
            let _entered = self.runtime.enter();
            future.as_mut().poll(cx)
        })
        .await;
        match reply {
            Reply::Stream(mut stream) => {
                let runtime = self.runtime.clone();
                Reply::Stream(Box::pin(futures::stream::poll_fn(move |cx| {
                    let _entered = runtime.enter();
                    stream.as_mut().poll_next(cx)
                })))
            }
            reply => reply,
        }
    }
}

struct Cancellation {
    cancelled: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
}

impl Drop for Cancellation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.wake.try_send(());
    }
}

pub(crate) async fn run<F>(
    request: RunRequest,
    mut on_event: F,
) -> Result<StreamCompletionResult, LlmError>
where
    F: FnMut(StreamEvent),
{
    let deadline = request.guards.deadline;
    let work = async {
        let (sender, mut received) = unbounded_channel();
        if let Some(relay) = &request.subagents {
            relay.attach(&sender);
        }
        let (wake, wakes) = mpsc::sync_channel(1);
        let cancellation = Cancellation {
            cancelled: Arc::new(AtomicBool::new(false)),
            wake,
        };
        let cancelled = cancellation.cancelled.clone();
        let wake = cancellation.wake.clone();
        let runtime = Handle::current();
        let worker = tokio::task::spawn_blocking(move || {
            kernel(request, runtime, sender, cancelled, wake, wakes)
        });
        while let Some(event) = received.recv().await {
            on_event(event);
        }
        worker
            .await
            .map_err(|error| LlmError::ProviderError(format!("Agent runtime failed: {error}")))?
    };
    match deadline {
        Some(deadline) => tokio::time::timeout(deadline, work).await.map_err(|_| {
            LlmError::ControlledStop(format!(
                "Agent timed out after {} seconds",
                deadline.as_secs()
            ))
        })?,
        None => work.await,
    }
}

#[derive(Resource)]
struct RunState {
    sender: UnboundedSender<StreamEvent>,
    subagents: Option<Arc<SubagentRelay>>,
    guards: HarnessRunGuards,
    detector: LoopDetector,
    additional_params: Option<serde_json::Value>,
    reasoning: Option<Effort>,
    calls: usize,
    tool_calls: usize,
    source_edit_succeeded: bool,
    live_text: String,
    attempt_start: usize,
    reasoning_ids: Vec<String>,
    active_reasoning: Option<String>,
    reasoning_sequence: usize,
    finished: Option<TurnOutcome>,
    controlled_stop: Option<String>,
}

impl RunState {
    fn emit(&self, event: StreamEvent) {
        let _ = self.sender.send(event);
    }
    fn stop(&mut self, commands: &mut Commands, agent: Entity, reason: String) {
        self.controlled_stop = Some(reason.clone());
        commands.trigger(StopTurn {
            entity: agent,
            reason: Some(reason),
        });
    }
}

#[derive(Component)]
struct Gated;

#[derive(Component)]
struct Reviewed;

fn prepare_models(
    mut models: Query<&mut ModelRequest, (ModelCalls, Added<Admitted>)>,
    state: Res<RunState>,
) {
    for mut model in &mut models {
        if let Some(params) = &state.additional_params {
            model.request_mut().additional_params = Some(params.clone());
        }
        if let Some(effort) = state.reasoning {
            model.request_mut().options.reasoning = Some(effort.into());
        }
    }
}

#[allow(
    clippy::type_complexity,
    reason = "Bevy systems declare component access in query types"
)]
fn gate_calls(
    models: Query<(Entity, &ForAgent, &CallOf), (ModelCalls, ReadyToStart, Without<Gated>)>,
    tools: Query<(Entity, &ForAgent, &ToolRequest), (ToolCalls, ReadyToStart, Without<Gated>)>,
    recoveries: Query<&Recovery>,
    mut state: ResMut<RunState>,
    mut commands: Commands,
) {
    if state.controlled_stop.is_some() {
        return;
    }
    for (entity, agent, of) in &models {
        commands.entity(entity).insert(Gated);
        let retry = recoveries
            .get(of.0)
            .is_ok_and(|recovery| recovery.retries > 0);
        if !retry {
            if let Some(max_turns) = state.guards.max_turns {
                if max_turns > 0 && state.calls >= max_turns {
                    state.stop(
                        &mut commands,
                        agent.0,
                        format!("Agent reached its {max_turns} model-call limit"),
                    );
                    return;
                }
            }
            state.calls += 1;
        }
        state.attempt_start = state.live_text.len();
        state.reasoning_ids.clear();
        state.active_reasoning = None;
    }
    for (entity, agent, request) in &tools {
        commands.entity(entity).insert(Gated);
        let name = request.call.function.name.to_string();
        let arguments = serde_json::Value::Object(request.call.function.arguments.clone());
        match state.detector.record_call(&name, &arguments) {
            LoopStatus::Warning(count) => state.emit(StreamEvent::LoopWarning {
                count,
                tool_name: name.clone(),
            }),
            LoopStatus::Stop => {
                let count = state.detector.consecutive_count();
                let message = format!(
                    "Agent stopped: repeated identical tool call '{name}' detected {count} times."
                );
                state.emit(StreamEvent::LoopStopped {
                    count,
                    tool_name: name,
                    message: message.clone(),
                });
                state.stop(&mut commands, agent.0, message);
                return;
            }
            LoopStatus::Ok => {}
        }
        state.tool_calls += 1;
        if let Some(relay) = &state.subagents {
            relay.record_call(&request.call.id.to_string(), &name, &arguments);
        }
        state.emit(StreamEvent::ToolCall {
            id: request.call.id.to_string(),
            name,
            arguments,
        });
    }
}

fn read_deltas(mut deltas: MessageReader<StreamDelta>, mut state: ResMut<RunState>) {
    for delta in deltas.read() {
        match &delta.event {
            ProviderEvent::Start {
                kind: PartKind::Reasoning,
                ..
            } => state.active_reasoning = None,
            ProviderEvent::Text { text, .. } => {
                state.live_text.push_str(text);
                state.emit(StreamEvent::Text(text.clone()));
            }
            ProviderEvent::Reasoning { text, .. } if !text.is_empty() => {
                let id = reasoning_id(&mut state);
                state.emit(StreamEvent::Reasoning(ReasoningPayload {
                    id,
                    text: text.clone(),
                    phase: ReasoningPhase::Delta,
                }));
            }
            ProviderEvent::End {
                content: AssistantContent::Reasoning(reasoning),
                ..
            } => {
                let id = reasoning_id(&mut state);
                state.active_reasoning = None;
                if !reasoning.redacted && !reasoning.text.is_empty() {
                    state.emit(StreamEvent::Reasoning(ReasoningPayload {
                        id,
                        text: reasoning.text.clone(),
                        phase: ReasoningPhase::Complete,
                    }));
                }
            }
            _ => {}
        }
    }
}

fn reasoning_id(state: &mut RunState) -> String {
    if let Some(id) = &state.active_reasoning {
        return id.clone();
    }
    let id =
        llm::ensure_active_reasoning_id(&mut state.active_reasoning, &mut state.reasoning_sequence);
    state.reasoning_ids.push(id.clone());
    id
}

fn final_response(message: &Message) -> String {
    message.final_answer().unwrap_or_else(|| {
        if let Message::Assistant(assistant) = message {
            if let Some(text) = assistant.content.iter().rev().find_map(|part| match part {
                AssistantContent::Text(text) if !text.text.trim().is_empty() => {
                    Some(text.text.trim().to_string())
                }
                _ => None,
            }) {
                return text;
            }
        }
        "The agent ended this turn without a text response.".to_string()
    })
}

#[allow(
    clippy::type_complexity,
    reason = "Bevy systems declare component access in query types"
)]
fn review_retries(
    mut failures: Query<
        (Entity, &Outcome, &mut RetryVerdict, &CallOf),
        (ModelCalls, ReadyToSettle, Without<Reviewed>),
    >,
    recoveries: Query<&Recovery>,
    mut state: ResMut<RunState>,
    mut commands: Commands,
) {
    let policy = RetryPolicy {
        max_retries: 3,
        first_backoff: Duration::from_secs(1),
        ..RetryPolicy::DEFAULT
    };
    for (entity, outcome, mut verdict, of) in &mut failures {
        commands.entity(entity).insert(Reviewed);
        let Err(report) = &outcome.0 else {
            continue;
        };
        let retries = recoveries.get(of.0).map_or(0, |recovery| recovery.retries);
        let mut report = report.clone();
        // Rig 0.46 does not recognize this gateway code yet.
        if report.code.as_deref() == Some("upstream_error")
            && !report.refusal
            && report
                .http_status
                .is_none_or(|status| (200..300).contains(&status))
        {
            report.retryable = true;
        }
        verdict.0 = policy.verdict(&report, retries, SystemTime::now());
        if matches!(verdict.0, Verdict::Retry(_)) {
            let start = state.attempt_start;
            state.live_text.truncate(start);
            state.emit(StreamEvent::RetryReset {
                text: state.live_text.clone(),
                reasoning_ids: state.reasoning_ids.clone(),
            });
        }
    }
}

fn settled(
    event: On<Add<Settled>>,
    tools: Query<(&ToolRequest, &Outcome, &ForAgent)>,
    turns: Query<&TurnOutcome>,
    mut state: ResMut<RunState>,
    mut commands: Commands,
) {
    if let Ok(outcome) = turns.get(event.entity) {
        state.finished = Some(outcome.clone());
    }
    if let Ok((request, outcome, agent)) = tools.get(event.entity) {
        let name = request.call.function.name.to_string();
        let result = llm::tool_result_summary(outcome.tool_result().output().as_content());
        if name == "source_edit" && llm::source_edit_result_changed(&result) {
            state.source_edit_succeeded = true;
        }
        if let Some(relay) = &state.subagents {
            relay.forget(&request.call.id.to_string());
        }
        state.emit(StreamEvent::ToolResult {
            id: request.call.id.to_string(),
            name: name.clone(),
            result: result.clone(),
        });
        if let Some((reason, status)) =
            llm::record_tool_result_failure(&mut state.detector, &result)
        {
            match status {
                LoopStatus::Warning(count) => state.emit(StreamEvent::LoopWarning {
                    count,
                    tool_name: name,
                }),
                LoopStatus::Stop => {
                    let count = state.detector.failure_streak();
                    let message = format!("Agent stopped: repeated deterministic failure '{reason}' detected {count} times.");
                    state.emit(StreamEvent::LoopStopped {
                        count,
                        tool_name: name,
                        message: message.clone(),
                    });
                    state.stop(&mut commands, agent.0, message);
                }
                LoopStatus::Ok => {}
            }
        }
    }
}

fn kernel(
    request: RunRequest,
    runtime: Handle,
    sender: UnboundedSender<StreamEvent>,
    cancelled: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
    wakes: mpsc::Receiver<()>,
) -> Result<StreamCompletionResult, LlmError> {
    let mut app = App::new();
    app.add_plugins((bevy_app::TaskPoolPlugin::default(), rig_ecs::AgentPlugin));
    app.insert_resource(Wake::new(move || {
        let _ = wake.try_send(());
    }));
    app.insert_resource(RunState {
        sender,
        subagents: request.subagents,
        guards: request.guards,
        detector: LoopDetector::new(request.guards.loop_guard),
        additional_params: request.additional_params,
        reasoning: request.reasoning,
        calls: 0,
        tool_calls: 0,
        source_edit_succeeded: false,
        live_text: String::new(),
        attempt_start: 0,
        reasoning_ids: Vec::new(),
        active_reasoning: None,
        reasoning_sequence: 0,
        finished: None,
        controlled_stop: None,
    });
    app.add_systems(AgentPass, prepare_models.in_set(PrepareSystems::Edit));
    app.add_systems(AgentPass, gate_calls.in_set(Stage::Gate));
    app.add_systems(
        AgentPass,
        (read_deltas, review_retries)
            .chain()
            .in_set(ReviewSystems::Edit),
    );
    app.add_observer(settled);
    let mut tools = ToolSet::default();
    for tool in request.tools {
        if tool.is_live() {
            tools.add_registered_tool(tool);
        }
    }
    let catalog = tools.catalog();
    for definition in catalog.definitions() {
        let tool = catalog
            .get(definition.name.as_str())
            .expect("catalog registration");
        let handler = ErasedHandler::new(TokioHandler {
            inner: tool.handler().clone(),
            runtime: runtime.clone(),
        });
        app.world_mut()
            .spawn_tool((**definition).clone(), handler, ToolOptions::default());
    }
    // Gospel discovers live models independently of Rig's built-in catalog.
    // Leave unknown limits/options unconstrained; provider encoders validate them.
    let provider = ProviderId::catalog(request.model.name())
        .unwrap_or_else(|| ProviderId::catalog("openai").expect("built-in provider"));
    let mut spec = ModelSpec::new(provider, request.model.id().unwrap_or("model")).with_tools(true);
    spec.reasoning = ReasoningSupport::Unknown { default: None };
    let inner = ErasedHandler::new(ModelAdapter::new(spec.reference(), request.model));
    let connection = Connection {
        spec: Arc::new(spec),
        handler: Handler(ErasedHandler::new(TokioHandler { inner, runtime })),
    };
    let prompt_tokens = llm::estimate_tokens(&request.prompt)
        + request
            .history
            .iter()
            .map(llm::estimate_message_tokens)
            .sum::<usize>();
    let conversation = Conversation(rig::conversation::Conversation::from_messages(
        request.history,
    ));
    let agent_id = request.session_id.map(AgentId).unwrap_or_default();
    let agent = app
        .world_mut()
        .spawn((
            Agent,
            agent_id,
            conversation,
            connection,
            SystemPrompt(request.preamble.unwrap_or_default()),
        ))
        .id();
    app.finish();
    app.world_mut()
        .resource_mut::<Time<bevy_time::Virtual>>()
        .set_max_delta(Duration::from_secs(60));
    app.update();
    app.world_mut()
        .trigger(Deliver::new(agent, request.prompt, DeliveryMode::Steer));
    loop {
        if cancelled.load(Ordering::Acquire)
            || app.world().resource::<RunState>().sender.is_closed()
        {
            app.world_mut().trigger(StopTurn::new(agent));
            app.update();
            return Err(LlmError::ControlledStop("Agent cancelled".into()));
        }
        app.update();
        if let Some(outcome) = app.world_mut().resource_mut::<RunState>().finished.take() {
            let state = app.world().resource::<RunState>();
            if let Some(reason) = &state.controlled_stop {
                return Err(LlmError::ControlledStop(reason.clone()));
            }
            return match outcome {
                TurnOutcome::Answered(message) => {
                    let full_response = final_response(&message);
                    let history = app.world().get::<Conversation>(agent).map(|conversation| {
                        conversation
                            .messages()
                            .iter()
                            .map(|message| (**message).clone())
                            .collect()
                    });
                    Ok(StreamCompletionResult {
                        response_tokens: llm::estimate_tokens(&full_response),
                        full_response,
                        history,
                        source_edit_succeeded: state.source_edit_succeeded,
                        prompt_tokens,
                        tool_calls: state.tool_calls,
                    })
                }
                TurnOutcome::Failed(reason) => Err(LlmError::ProviderError(reason)),
                TurnOutcome::Stopped(reason) => Err(LlmError::ControlledStop(
                    reason.unwrap_or_else(|| "Agent stopped".into()),
                )),
            };
        }
        let backoff = app
            .world_mut()
            .query::<&Backoff>()
            .iter(app.world())
            .next()
            .is_some();
        let wait = if backoff {
            Duration::from_millis(50)
        } else {
            Duration::from_secs(60)
        };
        let _ = wakes.recv_timeout(wait);
    }
}
