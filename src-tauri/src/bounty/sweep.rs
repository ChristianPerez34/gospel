use crate::agents::AgentDefinition;
use crate::bounty::scope::BountyScope;
use crate::bounty::signal::{load_signals, SignalKind, SignalRecord};
use crate::ecs_runtime::{self, RunRequest};
use crate::harness_profile::{
    resolve_custom_agent_profile, ActiveWorkspaceContext, CustomAgentProfileRequest,
    HarnessRunGuards, LoopGuardPolicy,
};
use crate::llm::LlmError;
use crate::provider_client::completion_model;
use crate::session_turn::SessionTurnEvent;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const MAX_INITIAL_HYPOTHESES: usize = 24;
const MAX_FEEDBACK_HYPOTHESES: usize = 8;
const MAX_FEEDBACK_ROUNDS: usize = 2;
const WORKER_CONCURRENCY: usize = 4;
const WORKER_MAX_TURNS: usize = 12;
const WORKER_DEADLINE: Duration = Duration::from_secs(300);
const PHASE_DEADLINE: Duration = Duration::from_secs(180);
const CONTEXT_FILE_CAP: usize = 16 * 1024;
const TRIAGE_SIGNALS_CAP: usize = 48 * 1024;

const WORKER_PREAMBLE: &str = "You are a bug-bounty worker. You test exactly ONE hypothesis on an authorized target. Use http_request for every network call — it is already scope-gated; a denied request means the hypothesis is out of scope, record nothing. Compare behavior across auth_profile values where relevant. When evidence supports a security issue, call record_bounty_signal with the evidence_ids. When inconclusive or clearly secure, record nothing. Do not broaden the test beyond the stated hypothesis.";

#[derive(Debug, Clone, Deserialize)]
pub struct Hypothesis {
    pub kind: SignalKind,
    pub endpoint: String,
    #[serde(default)]
    pub method: Option<String>,
    pub hypothesis: String,
    #[serde(default)]
    pub test: Option<String>,
}

#[derive(Debug, Serialize)]
struct HypothesisRecord {
    id: String,
    round: usize,
    kind: SignalKind,
    endpoint: String,
    method: String,
    hypothesis: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    test: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TriageVerdict {
    id: String,
    status: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    rationale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TriageOutput {
    #[serde(default)]
    verdicts: Vec<TriageVerdict>,
    #[serde(default)]
    report: String,
}

pub struct SweepRequest<'a> {
    pub agent: &'a AgentDefinition,
    pub task: &'a str,
    pub lead_provider: &'a str,
    pub lead_model: &'a str,
    pub lead_api_key: &'a str,
    pub worker_provider: &'a str,
    pub worker_model: &'a str,
    pub worker_api_key: &'a str,
    pub workspace: &'a ActiveWorkspaceContext,
    pub run_id: &'a str,
    pub session_id: Option<&'a str>,
}

/// Extracts the first JSON array from model output, tolerating prose around it.
fn extract_json_array(text: &str) -> Option<&str> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

fn parse_hypotheses(text: &str, cap: usize) -> Result<Vec<Hypothesis>, String> {
    let raw = extract_json_array(text).ok_or("mutator output contained no JSON array")?;
    let mut hypotheses: Vec<Hypothesis> =
        serde_json::from_str(raw).map_err(|error| format!("invalid hypothesis JSON: {error}"))?;
    hypotheses.retain(|h| !h.endpoint.trim().is_empty() && !h.hypothesis.trim().is_empty());
    hypotheses.truncate(cap);
    Ok(hypotheses)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(&text[start..=end])
}

fn run_dir(workspace_path: &Path, run_id: &str) -> PathBuf {
    super::http_tool::run_dir(workspace_path, run_id)
}

fn read_optional(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    Some(crate::text_utils::truncate_text_bytes(&raw, CONTEXT_FILE_CAP).0)
}

fn append_jsonl(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

fn signals_path(workspace_path: &Path, run_id: &str) -> PathBuf {
    run_dir(workspace_path, run_id).join("signals.jsonl")
}

fn phase_guards() -> HarnessRunGuards {
    HarnessRunGuards {
        max_turns: 4,
        deadline: Some(PHASE_DEADLINE),
        loop_guard: LoopGuardPolicy {
            warning_threshold: 2,
            stop_threshold: 3,
        },
    }
}

fn mutator_prompt(
    scope: &BountyScope,
    request: &SweepRequest<'_>,
    attached_skills: &str,
    round: usize,
    max_new: usize,
    prior: &[Hypothesis],
    signals: &[SignalRecord],
) -> String {
    let mut prompt = format!(
        "You are the hypothesis mutator for an authorized bug bounty sweep.\n\nScope:\n{}\n\nTask:\n{}\n",
        scope.describe(),
        request.task.trim()
    );
    if !attached_skills.is_empty() {
        prompt.push_str(&format!("\n{attached_skills}\n"));
    }
    let dir = run_dir(&request.workspace.workspace_path, request.run_id);
    for (name, label) in [
        ("recon.md", "Recon notes"),
        ("threat-model.md", "Threat model"),
    ] {
        if let Some(content) = read_optional(&dir.join(name)) {
            prompt.push_str(&format!("\n## {label}\n\n{content}\n"));
        }
    }
    for (name, label) in [
        ("recon.md", "Recon notes"),
        ("threat-model.md", "Threat model"),
    ] {
        let fallback = request
            .workspace
            .workspace_path
            .join(".gospel")
            .join("bounty")
            .join(name);
        if let Some(content) = read_optional(&fallback) {
            prompt.push_str(&format!("\n## {label}\n\n{content}\n"));
        }
    }
    if !signals.is_empty() {
        prompt.push_str("\n## Signals already recorded (do not repeat them)\n\n");
        for signal in signals.iter().take(40) {
            prompt.push_str(&format!(
                "- [{}] {} {}: {}\n",
                serde_json::to_value(signal.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default(),
                signal.method,
                signal.endpoint,
                signal.summary
            ));
        }
    }
    if !prior.is_empty() {
        prompt.push_str("\n## Hypotheses already tested (do not repeat them)\n\n");
        for hypothesis in prior.iter().take(60) {
            prompt.push_str(&format!("- {} {}\n", hypothesis.method.as_deref().unwrap_or("GET"), hypothesis.endpoint));
        }
    }
    let kinds = [
        "enforcement_gap", "inconsistent_enforcement", "unchecked_assumption",
        "undocumented_dependency", "composition_vulnerability", "inconsistent_access_path",
        "identity_rebinding", "failed_request_side_effect", "ordering_dependency",
        "partial_rollback_side_effect", "post_delete_access", "unauthorized_state_reversal",
        "contradictory_state", "other",
    ];
    if round == 0 {
        prompt.push_str(&format!(
            "\nProduce AT MOST {max_new} bounded, falsifiable hypotheses targeting the in-scope surface. \
            Each hypothesis names an endpoint, a method, and the minimal request(s) that distinguish secure from insecure behavior.\n"
        ));
    } else {
        prompt.push_str(&format!(
            "\nThe signals above are leads from earlier rounds. Produce AT MOST {max_new} follow-up hypotheses that \
            confirm, extend, or pivot on those leads — do not repeat prior hypotheses or signals.\n"
        ));
    }
    prompt.push_str(&format!(
        "\nOutput ONLY a JSON array, no prose:\n\
        [{{\"kind\": <one of {:?}>, \"endpoint\": \"https://in-scope-url/path\", \"method\": \"GET\", \"hypothesis\": \"one sentence\", \"test\": \"minimal requests to run\"}}]",
        kinds
    ));
    prompt
}

fn worker_prompt(hypothesis: &Hypothesis) -> String {
    format!(
        "Test exactly this hypothesis on the authorized target.\n\n\
        kind: {}\nendpoint: {}\nmethod: {}\nhypothesis: {}\nsuggested test: {}\n\n\
        Use http_request for every network call. If the evidence supports the hypothesis, \
        call record_bounty_signal with the evidence_ids. If inconclusive or clearly secure, \
        record nothing and end with one sentence stating why.",
        serde_json::to_value(hypothesis.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "other".to_string()),
        hypothesis.endpoint,
        hypothesis.method.as_deref().unwrap_or("GET"),
        hypothesis.hypothesis,
        hypothesis.test.as_deref().unwrap_or("none given"),
    )
}

fn triage_prompt(signals_jsonl: &str) -> String {
    format!(
        "You are the triage stage of an authorized bug bounty sweep. Below is the raw signals.jsonl, \
        one JSON signal per line. For each signal, decide a status: confirmed, rejected, or needs_review. \
        Confirmed requires the summary to describe reproducible evidence; needs_review fits plausible leads \
        missing reproduction.\n\nSignals:\n{signals_jsonl}\n\n\
        Output ONLY one JSON object, no prose:\n\
        {{\"verdicts\": [{{\"id\": \"<signal id>\", \"status\": \"confirmed|rejected|needs_review\", \
        \"severity\": \"info|low|medium|high\", \"rationale\": \"one line\"}}], \
        \"report\": \"markdown summary of the sweep: confirmed findings, rejected hypotheses, next reads\"}}"
    )
}

async fn run_phase(
    request: &SweepRequest<'_>,
    preamble: &str,
    prompt: &str,
) -> Result<String, LlmError> {
    let model = completion_model(
        request.lead_provider,
        request.lead_model,
        request.lead_api_key,
        request.session_id,
    )
    .await?;
    let result = ecs_runtime::run(
        RunRequest {
            session_id: request.session_id.map(str::to_owned),
            model,
            prompt: prompt.to_string(),
            preamble: Some(preamble.to_string()),
            history: Vec::new(),
            tools: Vec::new(),
            guards: phase_guards(),
            additional_params: None,
            reasoning: None,
        },
        |_| {},
    )
    .await?;
    Ok(result.full_response)
}

async fn run_worker(
    request: &SweepRequest<'_>,
    hypothesis: &Hypothesis,
    on_event: &(dyn Fn(SessionTurnEvent) + Sync),
    call_id: &str,
) -> String {
    on_event(SessionTurnEvent::ToolCall {
        id: call_id.to_string(),
        name: "bounty_sweep_worker".to_string(),
        arguments: json!({
            "kind": hypothesis.kind,
            "endpoint": hypothesis.endpoint,
            "method": hypothesis.method.as_deref().unwrap_or("GET"),
            "hypothesis": hypothesis.hypothesis,
        }),
    });

    let mut worker_agent = request.agent.clone();
    worker_agent.max_turns = worker_agent.max_turns.min(WORKER_MAX_TURNS);
    worker_agent.deadline = Some(
        worker_agent
            .deadline
            .map(|d| d.min(WORKER_DEADLINE))
            .unwrap_or(WORKER_DEADLINE),
    );
    worker_agent.body = String::new();
    worker_agent.tools = vec![
        "read_file".to_string(),
        "http_request".to_string(),
        "record_bounty_signal".to_string(),
    ];

    let profile = resolve_custom_agent_profile(CustomAgentProfileRequest {
        agent: worker_agent,
        workspace_path: request.workspace.workspace_path.clone(),
        run_id: request.run_id.to_string(),
        attached_skills_section: None,
    });

    let outcome = async {
        let model = completion_model(
            request.worker_provider,
            request.worker_model,
            request.worker_api_key,
            request.session_id,
        )
        .await?;
        ecs_runtime::run(
            RunRequest {
                session_id: request.session_id.map(str::to_owned),
                model,
                prompt: worker_prompt(hypothesis),
                preamble: Some(format!(
                    "{WORKER_PREAMBLE}\n\n{}",
                    profile.preamble.unwrap_or_default()
                )),
                history: Vec::new(),
                tools: profile.tools,
                guards: profile.guards,
                additional_params: None,
                reasoning: None,
            },
            |_| {},
        )
        .await
    }
    .await;

    let summary = match &outcome {
        Ok(result) => crate::text_utils::truncate_text_bytes(&result.full_response, 400).0,
        Err(error) => format!("worker error: {error}"),
    };
    on_event(SessionTurnEvent::ToolResult {
        id: call_id.to_string(),
        name: "bounty_sweep_worker".to_string(),
        result: json!({ "outcome": summary }).to_string(),
    });
    summary
}

/// Deterministic bounty sweep: mutator → worker fan-out → feedback rounds →
/// triage. Returns the report text for the delegated card.
pub async fn run_sweep(
    request: &SweepRequest<'_>,
    on_event: &(dyn Fn(SessionTurnEvent) + Sync),
) -> Result<String, LlmError> {
    let workspace_path = request.workspace.workspace_path.clone();
    let dir = run_dir(&workspace_path, request.run_id);
    let signals_file = signals_path(&workspace_path, request.run_id);
    let hypotheses_file = dir.join("hypotheses.jsonl");

    // Scope gate: fail closed before any model call.
    let scope_result = BountyScope::load(&workspace_path);
    on_event(SessionTurnEvent::ToolCall {
        id: "sweep-scope".to_string(),
        name: "bounty_sweep_scope".to_string(),
        arguments: json!({}),
    });
    let scope = match scope_result {
        Ok(scope) => {
            on_event(SessionTurnEvent::ToolResult {
                id: "sweep-scope".to_string(),
                name: "bounty_sweep_scope".to_string(),
                result: format!("authorized: {}", scope.program),
            });
            scope
        }
        Err(error) => {
            on_event(SessionTurnEvent::ToolResult {
                id: "sweep-scope".to_string(),
                name: "bounty_sweep_scope".to_string(),
                result: error.to_string(),
            });
            return Ok(format!(
                "Sweep refused: {error}\n\nConfigure `.gospel/bounty/scope.yaml` with `authorized: true` and in-scope rules first."
            ));
        }
    };

    let skills = crate::skills::discover_skills(
        Some(&workspace_path),
        crate::skills::global_skills_dir().as_deref(),
    );
    let attached_skills_section = {
        let attached: Vec<&crate::skills::Skill> = skills
            .iter()
            .filter(|skill| request.agent.skills.iter().any(|name| name == &skill.name))
            .collect();
        if attached.is_empty() {
            String::new()
        } else {
            let mut section = String::from("## Attached Skills\n\n");
            for skill in attached {
                section.push_str(&format!("### {}\n\n{}\n\n", skill.name, skill.body));
            }
            section
        }
    };

    let mut all_hypotheses: Vec<Hypothesis> = Vec::new();
    let mut worker_outcomes: Vec<String> = Vec::new();

    for round in 0..=MAX_FEEDBACK_ROUNDS {
        let signals_before = load_signals(&signals_file);
        let cap = if round == 0 {
            MAX_INITIAL_HYPOTHESES
        } else {
            MAX_FEEDBACK_HYPOTHESES
        };

        let mutate_id = format!("sweep-mutate-{round}");
        on_event(SessionTurnEvent::ToolCall {
            id: mutate_id.clone(),
            name: "bounty_sweep_mutate".to_string(),
            arguments: json!({ "round": round }),
        });
        let prompt = mutator_prompt(
            &scope,
            request,
            &attached_skills_section,
            round,
            cap,
            &all_hypotheses,
            &signals_before,
        );
        let raw = run_phase(request, "You are a precise hypothesis mutator. Output only the requested JSON.", &prompt).await;
        let new_hypotheses = match raw.and_then(|text| {
            parse_hypotheses(&text, cap).map_err(LlmError::ProviderError)
        }) {
            Ok(h) => h,
            Err(error) => {
                on_event(SessionTurnEvent::ToolResult {
                    id: mutate_id,
                    name: "bounty_sweep_mutate".to_string(),
                    result: error.to_string(),
                });
                if round == 0 {
                    return Ok(format!("Sweep aborted: mutator produced no usable hypotheses: {error}"));
                }
                break;
            }
        };
        if new_hypotheses.is_empty() {
            on_event(SessionTurnEvent::ToolResult {
                id: mutate_id,
                name: "bounty_sweep_mutate".to_string(),
                result: "0 hypotheses".to_string(),
            });
            if round == 0 {
                return Ok("Sweep finished: mutator produced no hypotheses.".to_string());
            }
            break;
        }
        on_event(SessionTurnEvent::ToolResult {
            id: mutate_id,
            name: "bounty_sweep_mutate".to_string(),
            result: format!("{} hypotheses", new_hypotheses.len()),
        });

        for hypothesis in &new_hypotheses {
            let record = HypothesisRecord {
                id: Uuid::new_v4().to_string(),
                round,
                kind: hypothesis.kind,
                endpoint: hypothesis.endpoint.clone(),
                method: hypothesis.method.clone().unwrap_or_else(|| "GET".into()),
                hypothesis: hypothesis.hypothesis.clone(),
                test: hypothesis.test.clone(),
            };
            let _ = append_jsonl(&hypotheses_file, &record);
        }

        let outcomes = stream::iter(new_hypotheses.iter().cloned().enumerate())
            .map(|(index, hypothesis)| {
                let call_id = format!("sweep-worker-r{round}-{index}");
                async move { run_worker(request, &hypothesis, on_event, &call_id).await }
            })
            .buffer_unordered(WORKER_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        all_hypotheses.extend(new_hypotheses);
        worker_outcomes.extend(outcomes);

        let signals_after = load_signals(&signals_file);
        let new_signal_count = signals_after.len().saturating_sub(signals_before.len());
        if round == MAX_FEEDBACK_ROUNDS || new_signal_count == 0 {
            break;
        }
    }

    // Triage on the lead model.
    let signals = load_signals(&signals_file);
    let signals_jsonl = signals
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .collect::<Vec<_>>()
        .join("\n");
    let signals_jsonl =
        crate::text_utils::truncate_text_bytes(&signals_jsonl, TRIAGE_SIGNALS_CAP).0;

    on_event(SessionTurnEvent::ToolCall {
        id: "sweep-triage".to_string(),
        name: "bounty_sweep_triage".to_string(),
        arguments: json!({ "signals": signals.len() }),
    });
    let triage_output = if signals.is_empty() {
        TriageOutput {
            verdicts: Vec::new(),
            report: "Sweep completed; workers recorded no signals.".to_string(),
        }
    } else {
        match run_phase(
            request,
            "You are a precise signal triage stage. Output only the requested JSON.",
            &triage_prompt(&signals_jsonl),
        )
        .await
        {
            Ok(text) => extract_json_object(&text)
                .and_then(|raw| serde_json::from_str::<TriageOutput>(raw).ok())
                .unwrap_or(TriageOutput {
                    verdicts: Vec::new(),
                    report: text,
                }),
            Err(error) => {
                on_event(SessionTurnEvent::ToolResult {
                    id: "sweep-triage".to_string(),
                    name: "bounty_sweep_triage".to_string(),
                    result: error.to_string(),
                });
                return Ok(format!(
                    "Sweep ran {} hypotheses across {} worker runs but triage failed: {error}\n\nSignals remain in `{}`.",
                    all_hypotheses.len(),
                    worker_outcomes.len(),
                    signals_file.display()
                ));
            }
        }
    };

    let triage_file = dir.join("triage.jsonl");
    for verdict in &triage_output.verdicts {
        let _ = append_jsonl(&triage_file, verdict);
    }
    let report_file = dir.join("sweep-report.md");
    let _ = std::fs::write(&report_file, &triage_output.report);

    on_event(SessionTurnEvent::ToolResult {
        id: "sweep-triage".to_string(),
        name: "bounty_sweep_triage".to_string(),
        result: format!(
            "{} verdicts ({} confirmed, {} rejected, {} needs_review)",
            triage_output.verdicts.len(),
            triage_output.verdicts.iter().filter(|v| v.status == "confirmed").count(),
            triage_output.verdicts.iter().filter(|v| v.status == "rejected").count(),
            triage_output.verdicts.iter().filter(|v| v.status == "needs_review").count(),
        ),
    });

    Ok(format!(
        "# Sweep report\n\n{}\n\n---\n{} hypotheses tested across {} worker runs; {} signals recorded.\nArtifacts: `{}`",
        triage_output.report.trim(),
        all_hypotheses.len(),
        worker_outcomes.len(),
        signals.len(),
        dir.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hypotheses_extracts_json_amid_prose() {
        let text = "Here you go:\n[{\"kind\":\"enforcement_gap\",\"endpoint\":\"https://t.example/a\",\"method\":\"GET\",\"hypothesis\":\"h1\",\"test\":\"t1\"}] trailing";
        let parsed = parse_hypotheses(text, 24).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].endpoint, "https://t.example/a");
    }

    #[test]
    fn parse_hypotheses_caps_and_drops_empty() {
        let text = "[{\"kind\":\"other\",\"endpoint\":\"\",\"hypothesis\":\"x\"},{\"kind\":\"other\",\"endpoint\":\"https://a\",\"hypothesis\":\"h\"},{\"kind\":\"other\",\"endpoint\":\"https://b\",\"hypothesis\":\"h\"}]";
        let parsed = parse_hypotheses(text, 1).unwrap();
        assert_eq!(parsed.len(), 1);
        let parsed = parse_hypotheses(text, 24).unwrap();
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn parse_hypotheses_rejects_missing_array() {
        assert!(parse_hypotheses("no json here", 24).is_err());
    }

    #[test]
    fn extract_json_object_handles_wrapped_output() {
        let text = "prefix {\"verdicts\":[], \"report\":\"r\"} suffix";
        let parsed: TriageOutput =
            serde_json::from_str(extract_json_object(text).unwrap()).unwrap();
        assert_eq!(parsed.report, "r");
    }

    #[tokio::test]
    async fn sweep_refuses_without_scope() {
        let dir = tempfile::tempdir().unwrap();
        let agent = AgentDefinition {
            name: "sweep".into(),
            description: "d".into(),
            source: crate::agents::AgentSource::Workspace,
            body: String::new(),
            tools: vec![],
            skills: vec![],
            model: None,
            worker_model: None,
            orchestration: Some(crate::agents::AgentOrchestration::Sweep),
            max_turns: 10,
            deadline: None,
            network: true,
        };
        let workspace = ActiveWorkspaceContext {
            workspace_path: dir.path().to_path_buf(),
            source_edit_allowed: true,
        };
        let request = SweepRequest {
            agent: &agent,
            task: "scan",
            lead_provider: "openai",
            lead_model: "gpt-4o",
            lead_api_key: "x",
            worker_provider: "openai",
            worker_model: "gpt-4o-mini",
            worker_api_key: "x",
            workspace: &workspace,
            run_id: "run-1",
            session_id: None,
        };
        let report = run_sweep(&request, &|_| {}).await.unwrap();
        assert!(report.contains("Sweep refused"));
    }
}
