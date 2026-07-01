# Gospel Context

## Glossary

- **Credentialed Provider**: A supported model provider for which Gospel can access a usable credential source. API-key providers are credentialed when a key exists in the OS keychain. OAuth providers are credentialed when a reusable provider-specific OAuth session exists in local auth storage.
- **OAuth Provider Credential**: A durable, locally stored OAuth session that lets Gospel derive provider access without a user-entered API key. ChatGPT Plus/Pro uses OpenAI/ChatGPT OAuth-managed auth data; GitHub Copilot uses GitHub/Copilot OAuth token files under Gospel config.
- **OAuth Authority**: The external account system that issues an OAuth Provider Credential. OpenAI/ChatGPT is the authority for the ChatGPT provider; GitHub.com is the authority for the GitHub Copilot provider.
- **Provider Visibility**: A non-secret user preference that determines whether a credentialed provider should contribute models to the model picker. Missing visibility data defaults to visible.
- **Available Model**: A backend-returned provider/model entry that is selectable because its provider is credentialed, visible, and model loading returned a live or cached model list.
- **Live Workspace Tools**: Workspace-scoped chat tools that inspect and narrowly mutate the active Gospel workspace directly, including safe file reads, code search, directory discovery, and exact source edits. These are the source of truth for file contents.
- **Review Focus**: The lens applied during one code review invocation. The supported focus names are `Security`, `BugHunt`, `Architecture`, `Performance`, and `Style`; a review invocation carries exactly one focus.
- **Review Comment**: A structured finding produced by the review pipeline. A Review Comment identifies the affected file and line range, severity, category, Review Focus, optional focus subcategory, evidence, rationale, suggested fix, verification plan, stable comment ID, and signal tier.
- **Review Result**: The structured result of one review invocation, including its Review Focus, mode, run ID, visible comments, summary, validation state, warnings, scanned-file count, suppression count, signal-to-noise percentage, and user-visibility state.
- **Review Run Record**: A persisted record of one review invocation used to validate later finding outcomes. It stores the run ID, timestamp, Review Focus, mode, and original comments for that run.
- **Source Edit Tool**: The `source_edit` Live Workspace Tool registered only for the main workspace-aware agent. It applies exactly one in-place replacement to an existing safe UTF-8 file, rejects unsafe or ambiguous targets, returns a capped replacement-scoped diff preview, and redacts raw replacement snippets from UI and Trace Log argument payloads. See ADR-0006.
- **Exact Replacement**: A source-edit safety contract where the agent supplies non-empty `old_text` that must occur exactly once in the target file and distinct `new_text` to replace it. Missing, repeated, or no-op replacement attempts fail without mutating the file.
- **Exploration Agent**: A backend-only helper Agent that investigates broad multi-file or architectural questions inside the active workspace and returns a structured report to the main Agent.
- **Turn**: One LLM inference cycle, which may include tool execution. A turn ends when the LLM produces a final text response (not a tool call).
- **Session Turn**: The backend orchestration of one user-submitted prompt against an optional Session, from preparing Turn context through streaming, persistence, failure handling, and follow-up verification. A Session Turn owns resolution of the Active Workspace Context for that Turn, including corpus availability fallback before constructing Live Workspace Tools. It owns the persistence policy for successful, failed, and controlled-stopped Turns: Display Transcript updates, Model History preservation or replacement, in-memory Conversation continuation, and draft-to-active Session status. It also owns the policy for whether a Verification Agent should run after the Turn; storage, event emission, LLM streaming, and background verification execution sit behind adapters.
- **Session Turn Event**: An observable event emitted while a Session Turn runs, such as streamed text, Tool activity, loop warnings, corpus fallback, Turn completion, Turn failure, or Verification Agent scheduling. The Session Turn module decides when these events occur; the Tauri adapter maps them to frontend events and Trace Log entries.
- **Conversation**: A sequence of user/agent message pairs within a session. Conversations are identified by session ID and stored server-side.
- **Tool**: A registered function the LLM can invoke during a turn. Tools execute and return results that feed back into the next turn.
- **Skill**: A user-authored directive (a `SKILL.md` file with YAML frontmatter and a markdown body) that can be injected into the LLM preamble to steer behaviour. Skills may bundle executable scripts. Skills are discovered from `<workspace>/.agents/skills/` and the global user data directory.
- **Skill Source**: The origin of a discovered skill. Either `Workspace` (from the active workspace's `.agents/skills/` directory) or `Global` (from the user's global data directory). Workspace skills take precedence over global skills when names collide.
- **Skill Match**: An automatic per-turn process that compares the user's prompt tokens against the name and description of all discovered skills. The top-3 matches above a score threshold are emitted as a `## Active Skills` section in the system preamble. Workspace skills win score ties over global skills.
- **Skill Invocation**: A user-initiated slash command (`/<skill-name>`) that suppresses the auto-match list and injects the full skill body into the system preamble for that turn. Unknown skill names fall back to normal turn behaviour with a warning.
- **Agent Harness**: The totality of surfaces through which a human steers an AI agent's behaviour over a session or across sessions. Borrowed from the framing that code plays three roles: instruction (telling the agent what to do), verification (checking what the agent did), and context (giving the agent information to work with).
- **Harness Interface**: The complete set of touch-points where steering signals enter the system — the system preamble, skills, tools, conversation history, and any persistent artifacts. The Harness Interface is the boundary between the human's intent and the agent's execution.
- **Harness Mechanism**: A concrete, nameable component within the Harness Interface that fulfils one or more of the three roles (instruction, verification, context). Examples: the skill system, the TDD workflow, the exploration chain, and explicit planning.
- **PEV Loop**: Plan → Execute → Verify. The minimal outer loop for long-horizon agent work: maintain a plan, execute the next step, verify the result, then update the plan. The PEV Loop is the rhythm that explicit planning makes visible.
- **Shared Harness Substrate**: A persistent, workspace-scoped location (`.gospel/`) where harness artifacts live across turns and sessions. Shared between the human (who can inspect and edit) and the agent (who can read and write via tools).
- **Harness Control Area**: The `.gospel/` directory and its contents, treated as the agent's persistent control surface. The primary artifact is `PLAN.md`.
- **Conversation Export**: A serialized snapshot of a Conversation's message history, emitted as raw rig::Message JSON for external analysis.
- **Session**: A backend-owned persistent unit of interaction identified by a UUID. A Session carries a Display Transcript (user-visible messages), backend Model History (provider continuation state), a workspace binding or unscoped flag, and metadata (title, provider, model, status, Session Mode). Sessions are stored in app-global SQLite, not in the shared harness substrate. See ADR-0004 and ADR-0005.
- **Session Mode**: A persisted Session metadata flag that controls whether workspace source-mutation tools are available on future turns. `Build` mode registers the normal main-agent source mutation path. `ReadOnly` mode withholds workspace source mutation tools while preserving the Session's Display Transcript, Model History, workspace binding, and harness substrate access. See ADR-0008.
- **Read-Only Session**: A Session whose Session Mode is `ReadOnly`. The main agent can inspect the active workspace and update Harness Control Area artifacts such as `.gospel/PLAN.md`, but it cannot receive `source_edit` for workspace source changes on subsequent turns.
- **Display Transcript**: The user-visible message history within a Session — user prompts and assistant replies in a clean, exportable format. Stored separately from Model History so it can be shared, exported, or deleted without exposing backend internals. See ADR-0005.
- **Model History**: The full provider-native conversation state (including tool calls, tool results, and internal context) needed to continue a conversation with the same provider/model. Stored separately from the Display Transcript and only updated on successful turn completion. Not exposed through UI or normal export flows. See ADR-0005.
- **Workspace-Affine Session**: A Session bound to a specific workspace. Only appears in the session list when that workspace is active. Backend rejects attempts to continue a workspace-affine Session from a different active workspace.
- **Unscoped Session**: A Session with no workspace binding. Appears only in unscoped mode (no active workspace). Useful for general-purpose chat that is not tied to a specific codebase.
- **Draft Session**: A Session created on first message send but before a successful turn completion. Drafts with zero display messages are hidden by default and cleaned up when stale.
- **Active Workspace Context**: The resolved active workspace path plus availability metadata needed to run workspace-aware harness behaviour.
- **Trace Log**: A redacted, capped JSONL file recording agent activity (role, timing, tool calls, warnings, stops, errors) for observability. Stored in app-global storage, capped at 250 MB global with 30-day retention. Never exposed as agent-readable memory.
- **Controlled Stop**: A clean agent termination triggered by run guards (e.g., identical tool call loops or repeated deterministic failures). Emits warning and stopped events, persists a plain-language stopped assistant message, and does not update Model History. Distinct from provider/runtime errors.
- **Loop Detection**: The mechanism that identifies repeated identical tool calls by tool name plus canonicalized JSON arguments. Thresholds are role-specific: the default general agent warns at three consecutive identical calls and controlled-stops at five, while the Verification role warns at two and controlled-stops at three. Warn events surface the loop risk; controlled-stop events terminate the agent cleanly before Model History is updated.
- **Verification Agent**: A backend sub-agent that runs asynchronously after high-risk completed responses to verify correctness. Uses read-only tools, lower turn budgets, and produces pass/concerns/fail/unavailable results. Failure or timeout is non-blocking.
- **Context Search**: An offline broad-retrieval index built on SQLite FTS as part of the corpus subsystem. Covers safe source files, documentation, workspace skills, and harness planning artifacts. Used by agents to find likely relevant areas, then verified with live workspace tools.
- **Session Context Note**: A session-scoped note injected into future turns as context (not persisted into provider Model History). Version one creates notes from Verification Agent concerns. The note shape is generic for future note kinds.

### Versioning

- **App Version**: The human-readable SemVer string identifying a Gospel release.
- **Dev Build**: A locally produced build whose App Version carries a `-dev` suffix. Dev Builds are not distributed.
- **Release Build**: A build produced from a versioned git tag for distribution to users.
- **Version Sync**: The release-preparation step that keeps Gospel's public package metadata aligned to one App Version.

## Harness Interface Baseline

Code plays three roles in any agent-assisted workflow:

1. **Instruction** — telling the agent what to do (system preamble, skills, user prompts).
2. **Verification** — checking what the agent did (tests, type checks, lint, manual review).
3. **Context** — giving the agent information to work with (file contents, corpus, conversation history).

Before explicit planning, Gospel's Harness Interface consisted of:
- **System preamble**: assembled from workspace tools prompt, corpus prompt, delegation prompt, and matched/invoked skills.
- **Skills**: user-authored directives (`SKILL.md`) injected into the preamble, steering behaviour per-turn or per-invocation.
- **Live Workspace Tools**: safe, workspace-scoped tools (read_file, search_code, find_files, list_directory, source_edit) that give the main agent source-of-truth access to the codebase and a narrow exact-replacement mutation path.
- **Exploration Agent**: a delegated sub-agent for broad architectural investigation.
- **Conversation history**: the implicit memory within a session.

These mechanisms handle instruction and context well, but verification and long-horizon progress tracking were limited to what fits in conversational memory.

## Explicit Planning Mechanism

The first deliberate Harness Mechanism addition: a persistent, inspectable plan file that serves as the agent's outer loop for multi-step work.

**Substrate**: `.gospel/` directory (the shared harness substrate). The `.gospel/corpus/` subdirectory is reserved for internal corpus data.

**Primary artifact**: `.gospel/PLAN.md` — a lightweight plan file with required structure:
- **Goal**: one-sentence description of what we are trying to accomplish.
- **Steps**: checklist of completed and pending steps.
- **Evidence / Verification**: what has been verified so far (test results, manual checks, tool outputs).
- **Open Questions / Risks**: blockers, unknowns, decisions still needed.
- **Next Action**: the single most important thing to do next.

**Tool-driven contract**: The `write_harness_file` tool allows the main agent to create and update files under `.gospel/`. It enforces the `.gospel/` prefix, creates parent directories, and caps content at 1 MiB. The tool is registered only for the main agent loop; the exploration sub-agent remains read-only for the substrate.

**Visibility**: The `.gospel/` directory is added to the hidden allowlist, making it traversable by all Live Workspace Tools. Every workspace session preamble includes a `## Harness Control Area` section documenting the substrate, the PLAN.md structure, and guidance on maintaining it.

**Skill-agnostic contract**: The plan maintenance guidance lives in the harness prompt, not in individual skills. Skills (like `/tdd` and `/diagnose`) are external plugins that govern workflow discipline; the plan is the outer persistent record that tracks goal, progress, and evidence across turns regardless of which skill is active. This keeps skills decoupled from gospel-specific concerns while still giving the agent a clear contract for when and how to maintain the plan.

## Source Edit Mechanism

The second Harness Mechanism addition: a narrow mutation tool for source files.

**Tool contract**: The `source_edit` tool applies one exact in-place replacement to an existing UTF-8 file in the active workspace. It fails when `old_text` is empty, appears zero times, appears more than once, or matches `new_text`.

**Safety policy**: Source edits reject workspace escapes, symlinked files, `.gospel/**`, hidden control directories, secret-like files, generated/noisy directories, lockfiles, generated files, binary or invalid UTF-8 files, and oversized files. Harness files remain writable only through `write_harness_file`.

**Visibility and verification**: Successful source edits render as **Edit file** action cards with capped diff previews. Raw replacement snippets are redacted from UI tool-call payloads and Trace Log arguments. Completed turns with successful source edits schedule the read-only Verification Agent even when the assistant response is otherwise short.
