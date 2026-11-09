use super::*;
use crate::approval_broker::{
    ApprovalBroker, ApprovalDecision, ApprovalEventEmitter, ApprovalRequest, ApprovalResolution,
};
use rig::test_utils::{MockCompletionModel, MockError, MockStreamEvent};
use rig::tool::{Tool, ToolContext, ToolExecutionError};
use rig::ProviderResponseError;
use serde_json::{json, Value};
use std::sync::atomic::AtomicUsize;

fn answer(text: &str) -> Vec<MockStreamEvent> {
    vec![
        MockStreamEvent::text(text),
        MockStreamEvent::final_response_with_default_usage(),
    ]
}

fn failed() -> MockStreamEvent {
    MockStreamEvent::Error(MockError::ProviderResponse(
        ProviderResponseError::without_status(
            r#"{"error":{"message":"Upstream stream ended without a terminal response event","code":"upstream_error","type":"server_error"}}"#,
        ),
    ))
}

fn request(model: &MockCompletionModel) -> RunRequest {
    let mut guards =
        crate::harness_profile::guards_for_role(crate::harness_profile::AgentRole::Main);
    guards.deadline = Some(Duration::from_secs(15));
    RunRequest {
        session_id: None,
        model: model.clone().into(),
        prompt: "Continue".into(),
        preamble: Some("Test role".into()),
        history: Vec::new(),
        tools: Vec::new(),
        guards,
        additional_params: None,
        reasoning: None,
        subagents: None,
    }
}

struct CountingTool {
    executions: Arc<AtomicUsize>,
    approval: Option<Arc<ApprovalBroker>>,
    output: Value,
}

impl Tool for CountingTool {
    const NAME: &'static str = "count";
    type Args = Value;
    type Output = Value;
    type Error = ToolExecutionError;
    fn description(&self) -> String {
        "Counts approved executions".into()
    }
    fn parameters(&self) -> Value {
        json!({"type":"object", "properties":{"attempt":{"type":"integer"}}})
    }
    async fn call(&self, _: &mut ToolContext, _: Value) -> Result<Value, ToolExecutionError> {
        if let Some(broker) = &self.approval {
            if !broker
                .request(ApprovalRequest::mcp_tool(
                    "count",
                    "mcp://test/count",
                    "Test approval",
                    Some("test-run"),
                ))
                .await
            {
                return Err(ToolExecutionError::permission_denied("Denied"));
            }
        }
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(self.output.clone())
    }
}

fn tool_turn(id: &str) -> Vec<MockStreamEvent> {
    vec![
        MockStreamEvent::tool_call(id, "count", json!({})),
        MockStreamEvent::final_response_with_default_usage(),
    ]
}

#[tokio::test]
async fn streams_and_preserves_existing_history_and_variant_parameters() {
    let model = MockCompletionModel::from_stream_turns([answer("Done")]);
    let mut input = request(&model);
    input.history = vec![Message::user("Before"), Message::assistant("Previously")];
    input.session_id = Some("existing-session".into());
    input.additional_params = Some(json!({"reasoning":{"effort":"high"}}));
    let mut text = String::new();
    let result = run(input, |event| {
        if let StreamEvent::Text(delta) = event {
            text.push_str(&delta)
        }
    })
    .await
    .unwrap();
    assert_eq!(text, "Done");
    assert_eq!(result.full_response, "Done");
    let history = result.history.unwrap();
    assert_eq!(history.len(), 4);
    assert_eq!(history[0], Message::user("Before"));
    assert_eq!(
        model.requests()[0].additional_params,
        Some(json!({"reasoning":{"effort":"high"}}))
    );
    assert_eq!(
        model.requests()[0].options.cache_key.as_deref(),
        Some("existing-session")
    );
}

#[tokio::test]
async fn applies_selected_reasoning_effort_to_model_requests() {
    let model = MockCompletionModel::from_stream_turns([answer("Done")]);
    let mut input = request(&model);
    input.reasoning = Some(Effort::High);
    run(input, |_| {}).await.unwrap();
    assert_eq!(
        model.requests()[0].options.reasoning,
        Some(rig::completion::Reasoning::Effort(Effort::High))
    );
}

#[tokio::test]
async fn concurrent_sessions_keep_their_output_and_history_separate() {
    let first = MockCompletionModel::from_stream_turns([answer("First session")]);
    let second = MockCompletionModel::from_stream_turns([answer("Second session")]);
    let (first, second) = tokio::join!(run(request(&first), |_| {}), run(request(&second), |_| {}));
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(first.full_response, "First session");
    assert_eq!(second.full_response, "Second session");
    assert!(!serde_json::to_string(&first.history)
        .unwrap()
        .contains("Second session"));
    assert!(!serde_json::to_string(&second.history)
        .unwrap()
        .contains("First session"));
}

#[tokio::test]
async fn retry_discards_partial_output_without_reexecuting_completed_tools() {
    let model = MockCompletionModel::from_stream_turns([
        tool_turn("call-1"),
        vec![
            MockStreamEvent::reasoning("Failed thoughts"),
            MockStreamEvent::text("Broken partial"),
            failed(),
        ],
        answer("Recovered"),
    ]);
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.max_turns = Some(2);
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success":true}),
    }));
    let mut resets = Vec::new();
    let result = run(input, |event| {
        if let StreamEvent::RetryReset {
            text,
            reasoning_ids,
        } = event
        {
            resets.push((text, reasoning_ids));
        }
    })
    .await
    .unwrap();
    assert_eq!(result.full_response, "Recovered");
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(model.request_count(), 3);
    assert_eq!(resets.len(), 1);
    assert_eq!(resets[0].0, "");
    assert!(!resets[0].1.is_empty());
    let requests = model.requests();
    assert_eq!(requests[1].chat_history, requests[2].chat_history);
    assert!(!serde_json::to_string(&result.history)
        .unwrap()
        .contains("Broken partial"));
}

#[tokio::test]
async fn retries_at_most_three_times() {
    let model = MockCompletionModel::from_stream_turns([
        vec![failed()],
        vec![failed()],
        vec![failed()],
        vec![failed()],
        answer("Unused"),
    ]);
    assert!(run(request(&model), |_| {}).await.is_err());
    assert_eq!(model.request_count(), 4);
}

#[tokio::test]
async fn permanent_errors_are_not_retried() {
    let model = MockCompletionModel::from_stream_turns([
        vec![MockStreamEvent::Error(MockError::ProviderResponse(
            ProviderResponseError::new(
                http::StatusCode::UNAUTHORIZED,
                r#"{"error":{"code":"upstream_error","type":"server_error"}}"#,
            ),
        ))],
        answer("Unused"),
    ]);
    assert!(run(request(&model), |_| {}).await.is_err());
    assert_eq!(model.request_count(), 1);
}

#[tokio::test]
async fn source_edits_still_trigger_follow_up_verification() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("main.rs"), "old").unwrap();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call(
                "edit-1",
                "source_edit",
                json!({
                    "path":"main.rs", "old_text":"old", "new_text":"new"
                }),
            ),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        answer("Edited"),
    ]);
    let mut input = request(&model);
    input.tools.push(RegisteredTool::from_tool(
        crate::workspace_tools::create_source_edit_tool(workspace.path().to_path_buf()),
    ));
    let result = run(input, |_| {}).await.unwrap();
    assert!(result.source_edit_succeeded);
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("main.rs")).unwrap(),
        "new"
    );
}

#[tokio::test]
async fn deterministic_failures_stop_even_when_call_arguments_change() {
    let model = MockCompletionModel::from_stream_turns((0..5).map(|n| {
        vec![
            MockStreamEvent::tool_call(format!("call-{n}"), "count", json!({"attempt":n})),
            MockStreamEvent::final_response_with_default_usage(),
        ]
    }));
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.loop_guard.stop_threshold = 3;
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success":false,"reason":"blocked"}),
    }));
    let result = run(input, |_| {}).await;
    assert!(
        matches!(result, Err(LlmError::ControlledStop(_))),
        "{result:?}; executions={}",
        executions.load(Ordering::SeqCst)
    );
    assert_eq!(executions.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn loop_guards_stop_before_another_tool_runs() {
    let model =
        MockCompletionModel::from_stream_turns((0..5).map(|n| tool_turn(&format!("call-{n}"))));
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.loop_guard.stop_threshold = 3;
    input.guards.loop_guard.warning_threshold = 2;
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success":true}),
    }));
    assert!(matches!(
        run(input, |_| {}).await,
        Err(LlmError::ControlledStop(_))
    ));
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

struct ApprovalEvents(UnboundedSender<ApprovalRequest>);
impl ApprovalEventEmitter for ApprovalEvents {
    fn emit_requested(&self, request: &ApprovalRequest) {
        let _ = self.0.send(request.clone());
    }
    fn emit_resolved(&self, _: &ApprovalResolution) {}
}

#[tokio::test]
async fn approvals_suspend_execution_and_cancellation_clears_pending_requests() {
    for cancel in [false, true] {
        let (sent, mut requested) = unbounded_channel();
        let broker = Arc::new(ApprovalBroker::new(Arc::new(ApprovalEvents(sent))));
        let executions = Arc::new(AtomicUsize::new(0));
        let model =
            MockCompletionModel::from_stream_turns([tool_turn("call-1"), answer("Approved")]);
        let mut input = request(&model);
        input.tools.push(RegisteredTool::from_tool(CountingTool {
            executions: executions.clone(),
            approval: Some(broker.clone()),
            output: json!({"success":true}),
        }));
        let task = tokio::spawn(run(input, |_| {}));
        let approval = tokio::time::timeout(Duration::from_secs(5), requested.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(executions.load(Ordering::SeqCst), 0);
        assert_eq!(approval.run_id.as_deref(), Some("test-run"));
        if cancel {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            tokio::time::timeout(Duration::from_secs(2), async {
                while broker.pending_count() != 0 {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("cancellation drops the pending approval");
            assert_eq!(executions.load(Ordering::SeqCst), 0);
            assert!(!broker.resolve(&approval.id, ApprovalDecision::Approve));
        } else {
            assert!(broker.resolve(&approval.id, ApprovalDecision::Approve));
            assert_eq!(task.await.unwrap().unwrap().full_response, "Approved");
            assert_eq!(executions.load(Ordering::SeqCst), 1);
        }
    }
}

#[tokio::test]
async fn cancellation_during_backoff_stops_further_requests() {
    let model = MockCompletionModel::from_stream_turns([vec![failed()], answer("Unused")]);
    let (sent, mut received) = unbounded_channel();
    let task = tokio::spawn(run(request(&model), move |event| {
        if matches!(event, StreamEvent::RetryReset { .. }) {
            let _ = sent.send(());
        }
    }));
    tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .unwrap()
        .unwrap();
    task.abort();
    let _ = task.await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(model.request_count(), 1);
}

#[tokio::test]
async fn role_deadline_cancels_pending_work() {
    let model = MockCompletionModel::from_stream_turns([vec![failed()], answer("Unused")]);
    let mut input = request(&model);
    input.guards.deadline = Some(Duration::from_millis(200));
    assert!(
        matches!(run(input, |_| {}).await, Err(LlmError::ControlledStop(reason)) if reason.contains("timed out"))
    );
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(model.request_count(), 1);
}

struct FakeSubagentTool {
    relay: Arc<crate::subagent::SubagentRelay>,
}

impl Tool for FakeSubagentTool {
    const NAME: &'static str = "delegate_exploration";
    type Args = Value;
    type Output = Value;
    type Error = ToolExecutionError;
    fn description(&self) -> String {
        "Delegates".into()
    }
    fn parameters(&self) -> Value {
        json!({"type":"object", "properties":{"task":{"type":"string"}}})
    }
    async fn call(&self, _: &mut ToolContext, args: Value) -> Result<Value, ToolExecutionError> {
        let emitter = crate::subagent::SubagentEmitter::claim(
            Some(&self.relay),
            Self::NAME,
            &args,
            crate::subagent::SubagentKind::Exploration,
        );
        let task = args["task"].as_str().unwrap_or_default().to_string();
        emitter.emit(crate::subagent::SubagentStep::Phase {
            label: format!("{task} started"),
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        emitter.emit(crate::subagent::SubagentStep::Text {
            text: format!("{task} report"),
        });
        Ok(json!({"report": task}))
    }
}

#[tokio::test]
async fn subagent_steps_reach_their_parent_call_between_call_and_result() {
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("call-a", "delegate_exploration", json!({"task": "alpha"})),
            MockStreamEvent::tool_call("call-b", "delegate_exploration", json!({"task": "beta"})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        answer("Done"),
    ]);
    let relay = crate::subagent::SubagentRelay::new();
    let mut input = request(&model);
    input.subagents = Some(relay.clone());
    input
        .tools
        .push(RegisteredTool::from_tool(FakeSubagentTool { relay }));
    let mut log: Vec<String> = Vec::new();
    run(input, |event| match event {
        StreamEvent::ToolCall { id, .. } => log.push(format!("call {id}")),
        StreamEvent::ToolResult { id, .. } => log.push(format!("result {id}")),
        StreamEvent::Subagent {
            parent_id, step, ..
        } => {
            let label = match step {
                crate::subagent::SubagentStep::Phase { label } => label,
                crate::subagent::SubagentStep::Text { text } => text,
                other => format!("{other:?}"),
            };
            log.push(format!("step {parent_id} {label}"));
        }
        _ => {}
    })
    .await
    .unwrap();

    let position = |entry: &str| {
        log.iter()
            .position(|item| item == entry)
            .unwrap_or_else(|| panic!("missing {entry:?} in {log:?}"))
    };
    for (id, task) in [("call-a", "alpha"), ("call-b", "beta")] {
        let call = position(&format!("call {id}"));
        let started = position(&format!("step {id} {task} started"));
        let report = position(&format!("step {id} {task} report"));
        let result = position(&format!("result {id}"));
        assert!(
            call < started && started < report && report < result,
            "{log:?}"
        );
    }
}

#[tokio::test]
async fn max_turns_limit_stops_agent_with_controlled_stop() {
    let model = MockCompletionModel::from_stream_turns([
        tool_turn("call-1"),
        tool_turn("call-2"),
        answer("Should not reach"),
    ]);
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.max_turns = Some(2);
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success": true}),
    }));
    let result = run(input, |_| {}).await;
    assert!(
        matches!(result, Err(LlmError::ControlledStop(ref reason)) if reason == "Agent reached its 2 model-call limit"),
        "{result:?}"
    );
    assert_eq!(executions.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn unlimited_max_turns_allows_more_than_two_calls() {
    let model = MockCompletionModel::from_stream_turns([
        tool_turn("call-1"),
        tool_turn("call-2"),
        answer("Done"),
    ]);
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.max_turns = None;
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success": true}),
    }));
    let result = run(input, |_| {}).await.unwrap();
    assert_eq!(result.full_response, "Done");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(model.request_count(), 3);
}

#[tokio::test]
async fn zero_max_turns_is_treated_as_unlimited() {
    let model = MockCompletionModel::from_stream_turns([
        tool_turn("call-1"),
        tool_turn("call-2"),
        answer("Done"),
    ]);
    let executions = Arc::new(AtomicUsize::new(0));
    let mut input = request(&model);
    input.guards.max_turns = Some(0);
    input.tools.push(RegisteredTool::from_tool(CountingTool {
        executions: executions.clone(),
        approval: None,
        output: json!({"success": true}),
    }));
    let result = run(input, |_| {}).await.unwrap();
    assert_eq!(result.full_response, "Done");
    assert_eq!(executions.load(Ordering::SeqCst), 2);
    assert_eq!(model.request_count(), 3);
}
