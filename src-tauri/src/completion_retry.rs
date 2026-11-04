//! Retry individual provider requests without replaying completed agent tools.

use std::time::Duration;

use futures::{stream, StreamExt};
use rig::agent::{
    AgentBuilder, AgentHook, HookContext, ModelSelection, ModelSelectionAction, NoToolConfig,
};
use rig::core::effect::{family, EffectKind, HandlerDescriptor, Outcome};
use rig::core::error::ErrorReport;
use rig::core::serve::{adapters::ModelAdapter, Dispatch, Reply, Serve};
use rig::streaming::{Relayed, StreamEvents};

use crate::provider_client::CompletionModel;

pub(crate) fn agent_builder(model: CompletionModel) -> AgentBuilder<NoToolConfig> {
    // Rig 0.44 exposes custom completion handlers through model routes.
    AgentBuilder::new(model.clone())
        .model_route_handler("retry", CompletionRetry(model))
        .add_hook(SelectRetryModel)
}

struct SelectRetryModel;

impl AgentHook for SelectRetryModel {
    fn on_model_select(
        &self,
        _ctx: &HookContext,
        _event: ModelSelection<'_>,
    ) -> ModelSelectionAction {
        ModelSelectionAction::select("retry")
    }
}

fn retry_delay(error: &ErrorReport, retries: u32) -> Option<Duration> {
    if retries >= 3 || error.refusal {
        return None;
    }
    // Rig 0.44 does not classify statusless SSE error envelopes.
    let transient_envelope = error.provider_response.as_ref().is_some_and(|response| {
        response.status.is_none_or(|status| status.is_success())
            && !response.refusal
            && matches!(
                response
                    .machine_code()
                    .or_else(|| {
                        let body: serde_json::Value = serde_json::from_str(&response.body).ok()?;
                        let error = body.pointer("/response/error")?;
                        ["code", "type"].iter().find_map(|key| {
                            error
                                .get(key)?
                                .as_str()
                                .filter(|code| !code.is_empty())
                                .map(str::to_owned)
                        })
                    })
                    .as_deref(),
                Some(
                    "upstream_error"
                        | "server_error"
                        | "internal_server_error"
                        | "overloaded_error"
                        | "api_error"
                        | "rate_limit_error"
                        | "rate_limit_exceeded"
                        | "too_many_requests"
                )
            )
    });
    (error.is_retryable() || transient_envelope).then(|| Duration::from_secs(1 << retries))
}

async fn backoff(retries: u32, delay: Duration) {
    tracing::warn!(retry = retries + 1, ?delay, "retrying provider completion");
    tokio::time::sleep(delay).await;
}

struct CompletionRetry(CompletionModel);

impl Serve for CompletionRetry {
    type Family = family::Completion;

    fn descriptor(&self) -> HandlerDescriptor {
        ModelAdapter::new("retry", self.0.clone()).descriptor()
    }

    async fn serve(&self, kind: EffectKind, dispatch: Dispatch) -> Reply {
        let EffectKind::Completion { request, stream } = kind else {
            return ModelAdapter::new("retry", self.0.clone())
                .serve(kind, dispatch)
                .await;
        };
        let context = dispatch.adapter_context();
        if !stream {
            let mut retries = 0;
            loop {
                let result = match context.clone() {
                    Some(context) => self.0.call_observed(request.clone(), context).await,
                    None => self.0.call(request.clone()).await,
                }
                .map_err(ErrorReport::from);
                if let Err(error) = &result {
                    if let Some(delay) = retry_delay(error, retries) {
                        backoff(retries, delay).await;
                        retries += 1;
                        continue;
                    }
                }
                return Reply::Outcome(result.map(Outcome::Completion));
            }
        }

        let model = self.0.clone();
        let open = move || -> StreamEvents {
            let result = match context.clone() {
                Some(context) => model.stream_observed(request.clone(), context),
                None => model.stream(request.clone()),
            };
            match result {
                Ok(stream) => stream.into_relay(),
                Err(error) => Box::pin(stream::iter([Err(error.into())])),
            }
        };
        let initial = open();
        Reply::Stream(Box::pin(stream::unfold(
            (initial, 0, false, false),
            move |(mut stream, mut retries, mut started, done)| {
                let open = open.clone();
                async move {
                    if done {
                        return None;
                    }
                    loop {
                        let item = stream.next().await?;
                        if let Err(error) = &item {
                            // shortcut: partial streams fail closed; retrying them needs an attempt-reset protocol.
                            if !started {
                                if let Some(delay) = retry_delay(error, retries) {
                                    backoff(retries, delay).await;
                                    retries += 1;
                                    stream = open();
                                    continue;
                                }
                            }
                        }
                        started |= matches!(&item, Ok(Relayed::Item(_)));
                        let done = item.is_err() || matches!(&item, Ok(Relayed::Done(_)));
                        return Some((item, (stream, retries, started, done)));
                    }
                }
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use rig::agent::MultiTurnStreamItem;
    use rig::core::test_utils::{MockHttpResponse, SequencedHttpClient};
    use rig::http_client::{HeaderMap, HeaderValue};
    use rig::providers::openai;
    use rig::ProviderResponseError;

    async fn collect(agent: rig::agent::Agent) -> Result<String, String> {
        let mut stream = agent.prompt("Hello").stream();
        let mut output = String::new();
        while let Some(item) = stream.next().await {
            if let MultiTurnStreamItem::FinalResponse(response) =
                item.map_err(|error| error.to_string())?
            {
                output = response.output();
            }
        }
        Ok(output)
    }

    const UPSTREAM_ERROR: &str = r#"{"error":{"message":"Upstream request failed: Upstream stream ended without a terminal response event","type":"server_error","param":null,"code":"upstream_error"}}"#;

    fn sse(body: String) -> MockHttpResponse {
        let mut headers = HeaderMap::new();
        headers.insert(
            "content-type",
            HeaderValue::from_static("text/event-stream"),
        );
        MockHttpResponse::SuccessWithHeaders(body.into(), headers)
    }

    fn upstream_error() -> MockHttpResponse {
        sse(format!("data: {UPSTREAM_ERROR}\n\n"))
    }

    fn success() -> MockHttpResponse {
        sse(concat!(
            "data: {\"id\":\"reply\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Recovered\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"reply\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        ).to_string())
    }

    fn model(http: SequencedHttpClient) -> CompletionModel {
        openai::OpenAIConfig::new("test-key")
            .connect(http)
            .chat("test-model")
            .into()
    }

    #[tokio::test]
    async fn retries_the_reported_in_band_upstream_error() {
        let http = SequencedHttpClient::new([upstream_error(), success()]);
        let agent = agent_builder(model(http.clone()))
            .default_max_turns(1)
            .build();
        assert_eq!(collect(agent).await.unwrap(), "Recovered");
        assert_eq!(http.requests().len(), 2);
        assert_eq!(http.requests()[0].body, http.requests()[1].body);
    }

    #[tokio::test]
    async fn retries_responses_api_failure_envelopes() {
        let response = serde_json::json!({
            "id": "reply", "object": "response", "created_at": 0,
            "status": "failed", "model": "test-model", "output": [], "tools": [],
            "error": serde_json::from_str::<serde_json::Value>(UPSTREAM_ERROR).unwrap()["error"]
        });
        let created = serde_json::json!({"type": "response.created", "response": {
            "id": "reply", "status": "in_progress", "output": []
        }});
        let failure = serde_json::json!({"type": "response.failed", "response": response});
        let completed = serde_json::json!({"type": "response.completed", "response": {
            "id": "reply", "object": "response", "created_at": 0, "status": "completed",
            "model": "test-model", "tools": [], "output": [{
                "id": "message", "type": "message", "status": "completed", "role": "assistant",
                "content": [{"type": "output_text", "text": "Recovered", "annotations": []}]
            }]
        }});
        let http = SequencedHttpClient::new([
            sse(format!("data: {created}\n\ndata: {failure}\n\n")),
            sse(format!("data: {completed}\n\n")),
        ]);
        let model = openai::OpenAIConfig::new("test-key")
            .connect(http.clone())
            .responses("test-model");
        let agent = agent_builder(model.into()).default_max_turns(1).build();
        assert_eq!(collect(agent).await.unwrap(), "Recovered");
        assert_eq!(http.requests().len(), 2);
    }

    #[test]
    fn backoff_is_exponential_and_bounded() {
        let error = rig::ProviderError::ProviderResponse(ProviderResponseError::without_status(
            UPSTREAM_ERROR,
        ))
        .report();
        assert_eq!(retry_delay(&error, 0), Some(Duration::from_secs(1)));
        assert_eq!(retry_delay(&error, 1), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(&error, 2), Some(Duration::from_secs(4)));
        assert_eq!(retry_delay(&error, 3), None);
    }

    #[test]
    fn only_transient_failures_are_retried() {
        for status in [408, 425, 429, 500, 502, 503, 504] {
            let error = rig::ProviderError::ProviderResponse(ProviderResponseError::new(
                http::StatusCode::from_u16(status).unwrap(),
                "busy",
            ))
            .report();
            assert!(retry_delay(&error, 0).is_some(), "{status}");
        }
        for status in [400, 401, 403, 404, 422] {
            let error = rig::ProviderError::ProviderResponse(ProviderResponseError::new(
                http::StatusCode::from_u16(status).unwrap(),
                UPSTREAM_ERROR,
            ))
            .report();
            assert_eq!(retry_delay(&error, 0), None, "{status}");
        }
        for body in [
            r#"{"error":{"code":"invalid_api_key"}}"#,
            r#"{"error":{"code":"insufficient_quota"}}"#,
            r#"{"error":{"code":"unknown_error"}}"#,
            "invalid response",
        ] {
            let error =
                rig::ProviderError::ProviderResponse(ProviderResponseError::without_status(body))
                    .report();
            assert_eq!(retry_delay(&error, 0), None, "{body}");
        }
        let refusal = rig::ProviderError::ProviderResponse(
            ProviderResponseError::without_status(UPSTREAM_ERROR).with_refusal(true),
        )
        .report();
        assert_eq!(retry_delay(&refusal, 0), None);
        assert!(retry_delay(&rig::ProviderError::Truncated.report(), 0).is_some());
    }

    #[tokio::test]
    async fn stops_after_three_retries_and_keeps_the_last_error() {
        let http = SequencedHttpClient::new([
            upstream_error(),
            upstream_error(),
            upstream_error(),
            sse(
                "data: {\"error\":{\"code\":\"server_error\",\"message\":\"last failure\"}}\n\n"
                    .into(),
            ),
            success(),
        ]);
        let agent = agent_builder(model(http.clone()))
            .default_max_turns(1)
            .build();
        let start = tokio::time::Instant::now();
        let error = collect(agent).await.unwrap_err();
        assert!(error.to_string().contains("last failure"), "{error}");
        assert_eq!(http.requests().len(), 4);
        assert_eq!(http.remaining_responses(), 1);
        assert!(start.elapsed() >= Duration::from_secs(7));
    }

    #[tokio::test]
    async fn permanent_errors_fail_immediately() {
        let http = SequencedHttpClient::new([
            MockHttpResponse::ErrorResponse(http::StatusCode::UNAUTHORIZED, "invalid key".into()),
            success(),
        ]);
        let agent = agent_builder(model(http.clone())).build();
        assert!(collect(agent).await.is_err());
        assert_eq!(http.requests().len(), 1);
    }

    #[tokio::test]
    async fn partial_streams_are_not_replayed() {
        let http = SequencedHttpClient::new([
            sse(format!(
                "data: {{\"id\":\"reply\",\"choices\":[{{\"index\":0,\"delta\":{{\"role\":\"assistant\",\"content\":\"Partial\"}},\"finish_reason\":null}}]}}\n\ndata: {UPSTREAM_ERROR}\n\n"
            )),
            success(),
        ]);
        let agent = agent_builder(model(http.clone())).build();
        assert!(collect(agent).await.is_err());
        assert_eq!(http.requests().len(), 1);
    }

    #[tokio::test]
    async fn dropping_a_request_cancels_backoff() {
        let http = SequencedHttpClient::new([upstream_error(), success()]);
        let agent = agent_builder(model(http.clone())).build();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), collect(agent))
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert_eq!(http.requests().len(), 1);
    }

    #[tokio::test]
    async fn unary_requests_use_the_same_retry_policy() {
        let http = SequencedHttpClient::new([
            MockHttpResponse::ErrorResponse(http::StatusCode::SERVICE_UNAVAILABLE, "busy".into()),
            MockHttpResponse::Success(serde_json::to_vec(&serde_json::json!({
                "id": "reply", "object": "chat.completion", "created": 0, "model": "test-model",
                "choices": [{"index": 0, "message": {"role": "assistant", "content": "Recovered"}, "finish_reason": "stop"}]
            })).unwrap().into()),
        ]);
        let agent = agent_builder(model(http.clone()))
            .default_max_turns(1)
            .build();
        let response = agent.prompt("Hello").await.unwrap();
        assert_eq!(response.output(), "Recovered");
        assert_eq!(http.requests().len(), 2);
        assert_eq!(http.requests()[0].body, http.requests()[1].body);
    }

    #[tokio::test]
    async fn retries_after_a_tool_without_repeating_it() {
        let tool_response = serde_json::json!({
            "id": "reply", "choices": [{"index": 0, "delta": {
                "role": "assistant", "tool_calls": [{"index": 0, "id": "call-add", "type": "function",
                    "function": {"name": "add", "arguments": "{\"x\":1,\"y\":2}"}}]
            }, "finish_reason": null}]
        });
        let http = SequencedHttpClient::new([
            sse(format!("data: {tool_response}\n\ndata: {{\"id\":\"reply\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n")),
            upstream_error(), success(),
        ]);
        let agent = agent_builder(model(http.clone()))
            .tool(rig::test_utils::MockAddTool)
            .default_max_turns(2)
            .build();
        let mut stream = agent.prompt("Add 1 and 2").stream();
        let mut tool_calls = 0;
        let mut tool_results = 0;
        let mut output = String::new();
        while let Some(item) = stream.next().await {
            match item.unwrap() {
                MultiTurnStreamItem::ToolCall { .. } => tool_calls += 1,
                MultiTurnStreamItem::ToolResult { .. } => tool_results += 1,
                MultiTurnStreamItem::FinalResponse(response) => output = response.output(),
                _ => {}
            }
        }
        assert_eq!(output, "Recovered");
        assert_eq!((tool_calls, tool_results), (1, 1));
        let requests = http.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1].body, requests[2].body);
        let body: serde_json::Value = serde_json::from_slice(&requests[2].body).unwrap();
        assert!(body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool"));
    }
}
