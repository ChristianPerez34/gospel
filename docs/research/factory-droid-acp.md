# Feasibility & Architecture Research: Factory Droid ACP Integration in Gospel

**Date:** 2026-11-27  
**Branch / Worktree:** `research/factory-droid-acp` (based on `origin/dev` at `02819e3`)  
**Status:** Research & Architecture Proposal (no runtime code modified yet)

---

## 1. Executive Summary

**Verdict:** Integrating **Factory Droid** via the **Agent Client Protocol (ACP)** (`droid exec --output-format acp`) is **feasible and ToS-compliant** for enabling users to run their Factory Droid subscriptions (`Pro`, `Plus`, `Max`, `Teams`, `Enterprise`, or BYOK/API key) through Gospel.

However, **Factory Droid ACP is not a drop-in LLM provider for `rig-core`**. Every provider currently in Gospel (`providers.rs`, `provider_client.rs`, `agent_runtime.rs`) is a stateless completion endpoint (`rig::DynModel<rig::operation::Completion>`) driven by Gospel's own tool loop. Factory Droid over ACP is an **autonomous external agent server**: it owns its own LLM routing, tool loop, context management, and subscription authentication, while communicating with Gospel over stdio JSON-RPC 2.0 (`initialize` → `authenticate` → `session/new` → `session/prompt` ↔ `session/update` / `session/request_permission`).

Supporting Factory Droid cleanly requires adding an **ACP Agent Backend** seam in `session_turn.rs` alongside `agent_runtime.rs`, persisting an external ACP session identifier alongside `Conversation`, bridging ACP permission requests to `ApprovalBroker`, and explicitly handling `rig`-only subsystems (`ReviewEngine`, `BountySweep`, `VerificationAgent`, `generate_session_title`).

---

## 2. Terms of Service & Community Precedent

### 2.1 Why Token Extraction / Proxy Routing Violates ToS

Factory's SaaS Product Terms, EULA, and Individual Plan terms (hosted at `factory.com` / `factory.ai`) include standard prohibitions against:

- Reverse engineering, decompiling, disassembling, or seeking to access source code or non-public APIs.
- Bypassing rate limits, access restrictions, or security controls.
- Using the product or APIs to build a competing service or sublicense/provide third-party access.

On developer subreddits (`r/FactoryAi`, `r/opencodeCLI`, `r/ClaudeAI`), community discussions around OAuth token extraction and relay proxies (`DroidProxy`, `CLIProxyAPI`, `VibeProxy`, and OpenCode consumer-OAuth plugins) highlight two consistent realities:

1. Routing consumer/subscription OAuth tokens through unofficial harnesses or local reverse-proxies sits squarely against provider ToS (most visibly enforced by Anthropic's crackdowns on Claude Pro/Max OAuth tokens inside third-party harnesses).
2. Community consensus labels proxy/token-ripping setups **"use at your own risk"**, with higher risk of account suspension under automated or high-frequency loops.

### 2.2 Why Official ACP (`droid exec --output-format acp`) Is the Right Route

Running Factory Droid via **`droid exec --output-format acp`** avoids all token-ripping and private-API emulation:

- **First-party supported interface:** Factory officially ships `--output-format acp` inside `@factory/cli` (`droid`), publishes `factory-droid` in the official ACP Registry (`agentclientprotocol/registry`), and documents ACP setup for **Zed** and **JetBrains IDEs**.
- **Official subscription auth over ACP:** Probing the locally installed `droid` v1.21.0 binary shows `initialize` explicitly advertises `device-pairing` ("Authenticate with Factory using a device pairing code in your browser") alongside `factory-api-key` in `authMethods`, plus `auth/status` and `auth/logout` capabilities.
- **Factory keeps full control:** Factory's own CLI binary handles API requests, telemetry, rolling rate-limit windows (5h / 7d / 30d), Droid Core fallback, and Extra Usage credits.

**Required Gospel guardrails to stay cleanly inside ToS:**
- Never read, copy, or parse `~/.factory/` auth files or OS keychain secrets directly.
- Never emulate Factory CLI headers or hit Factory inference/telemetry endpoints directly from Gospel's HTTP client.
- Spawn only the user's locally installed `droid` executable on their own machine (`droid exec --output-format acp`) for their own interactive desktop use; never proxy or multiplex one subscription across multiple users.

---

## 3. How Other Editors (Zed & JetBrains) Integrate Factory Droid

### 3.1 Zed (`zed-industries/zed`)

Zed acts as an **ACP Client** via `crates/agent_servers` (using the Rust `agent-client-protocol` and `agent-client-protocol-schema` crates):

- **Registration:** Configured either from the ACP Registry (`https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, entry `factory-droid`) or manually under `agent_servers` in `settings.json` (`command: "droid"` or `"npx"`, `args: ["droid", "exec", "--output-format", "acp"]`).
- **Runtime separation:** Zed's `AcpThread` owns UI rendering, `@`-tagged workspace context (`embeddedContext`), tool-call timeline cards (`session/update`), permission prompts (`session/request_permission`), and forwarding configured MCP servers in `session/new`. The external `droid` process owns model calls, credentials, and agent execution.
- **Ecosystem evolution:** Zed deprecated ad-hoc per-agent extension glue in favor of the ACP Registry and ACP v1/v2 session lifecycle improvements (async/background tool tracking, `configOptions`, `session/load` / `session/resume` / `session/fork`).

### 3.2 JetBrains IDEs (2025.3+)

JetBrains AI Assistant installs `Factory Droid` directly from the ACP Registry (or `~/.jetbrains/acp.json`), spawns `droid exec --output-format acp`, and triggers Factory's browser device-login flow from the AI Chat panel.

### 3.3 Known Pitfalls Reported by Editors & Community

1. **The `npx` / native-binary launch bug (Zed `#62716`, `#64429`):** Factory's npm `droid` package replaces its JS shim with a hardlinked native Mach-O/ELF binary during postinstall. Editors that assume `npx`/npm bins are JS files and launch `node .../bin/droid` crash with `SyntaxError: Invalid or unexpected token`. Gospel must invoke the `droid` executable directly.
2. **macOS GUI `PATH` mismatch:** Desktop apps launched from Finder/Dock do not inherit interactive shell `PATH` entries such as `~/.local/bin` (where `curl -fsSL https://app.factory.ai/cli | sh` installs `droid` — e.g. `/Users/<user>/.local/bin/droid` on this machine) or `/opt/homebrew/bin`. Gospel must probe common install paths alongside `PATH` and allow a custom binary override.
3. **Permission prompt deadlocks / UI races:** Zed hit edge cases where concurrent `session/update` frames or dropped `session/request_permission` dialogs stalled turns on an infinite spinner. Request-ID tracking, timeout handling, and cancellation must be airtight.
4. **Client capability vs. sandbox illusion:** Disabling ACP `fs/*` or `terminal/*` client capabilities does not automatically sandbox an external CLI if it falls back to its own local tools unless autonomy flags and `session/request_permission` enforce boundaries.
5. **Session reload caveats:** Factory's Zed documentation historically noted sessions didn't auto-restore across panel reloads, even though `droid` v1.21.0 now advertises `loadSession`, `list`, `resume`, `fork`, and `close`. Gospel should still persist its own `Display Transcript` locally so transcripts remain viewable offline/after process exit.

---

## 4. Verified Capabilities of Local `droid` CLI (`v1.21.0`)

Probing the installed `/Users/lifelinelogics/.local/bin/droid` (`1.21.0`) inside an isolated sandbox (`HOME=/tmp/droid-acp-probe`, no network) verified the following exact ACP handshake and CLI surface:

### 4.1 `initialize` Response

Sending ACP `initialize` (`protocolVersion: 1`) to `droid exec --output-format acp` returned:

- **`agentInfo`:** `name: "@factory/cli"`, `title: "Factory Droid"`, `version: "1.21.0"`
- **`protocolVersion`:** `1`
- **`agentCapabilities`:**
  - `loadSession: true`
  - `sessionCapabilities`: `{ "list": {}, "resume": {}, "close": {}, "fork": {} }`
  - `promptCapabilities`: `{ "image": true, "embeddedContext": true }`
  - `mcpCapabilities`: `{ "http": true, "sse": true }` *(per ACP spec, `stdio` is baseline/always supported)*
  - `auth`: `{ "logout": {}, "status": {} }`
  - `_meta.terminal-auth`: `true`
  - `_meta.factory`: `{ "modelsList": true, "promptWhileRunning": true, "messageRetraction": true }`
- **`authMethods`:**
  1. `id: "device-pairing"` — `"Login"`: *"Authenticate with Factory using a device pairing code in your browser."*
  2. `id: "factory-api-key"` — `"Factory API Key"`: *"Authenticate using a Factory API key set in the FACTORY_API_KEY environment variable."*

### 4.2 Unauthenticated `auth/status` & `session/new`

- `auth/status` returns `{"state": "unauthenticated"}` without network access.
- `session/new` returns JSON-RPC error `-32000`: *"Authentication required: Click the \"Login\" button to authenticate, or set a FACTORY_API_KEY environment variable."*
- Note: `factory/models/list` returned `-32601` (Method not found) when unauthenticated; the exact authenticated model-discovery payload (`configOptions` on `session/new` vs `models/list` RPC) remains to be verified in an authenticated online environment.

### 4.3 `droid exec --help` Surface Relevant to Gospel

- **Autonomy tiers:** Default (`read-only`), `--auto low|medium|high`, `--skip-permissions-unsafe`. (To preserve Gospel's `Ask` safety mode, Gospel must **not** pass `--skip-permissions-unsafe` or `--auto high` silently.)
- **ACP caveat in CLI help:** `--disable-builtin-skills` explicitly states `(not supported in ACP modes)`.
- **Models & reasoning effort:** `droid exec --help` lists `auto`, tier aliases (`anthropic-latest-*`, `openai-latest-*`, `oss-latest-*`), custom models (`custom:*`), and 40+ curated models (Claude, GPT, Gemini, Grok, GLM, Kimi, Qwen, DeepSeek, MiniMax, Inkling, Mistral, Nemotron) with per-model reasoning efforts (`off`/`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`).

---

## 5. Gospel Codebase Fit & Gap Analysis

| Gospel Subsystem | Current Assumption (`origin/dev`) | Impact with Factory Droid ACP |
|---|---|---|
| `src-tauri/src/providers.rs` & `provider_credentials.rs` | Every provider is `ApiKey` or `Oauth` producing a key/token for `rig` (`api_key_for_rig`). | ACP agents have no HTTP API key for `rig`; readiness depends on local `droid` binary availability + ACP `auth/status` (or optional `FACTORY_API_KEY` env override). |
| `src-tauri/src/provider_client.rs` | Dispatches every `(provider, model)` to `rig::DynModel<Completion>`. | Cannot construct a `CompletionModel` for `factory_droid`. Must not route `factory_droid` through `completion_model`. |
| `src-tauri/src/model_fetch.rs` & `models.rs` | Fetches HTTP `/models` catalogs or hardcoded lists; maps `ModelVariant` via `models.dev` + rig parameters. | Models and reasoning efforts come from ACP (`session/new` `configOptions` / `models` or CLI catalog fallback) and switch via `session/set_config_option` / `session/set_model`. |
| `src-tauri/src/session_turn.rs` & `agent_runtime.rs` | `TauriSessionTurnAdapters` delegates `SessionTurnLlm::stream_completion` to `llm::stream_completion` → `agent_runtime::run`. | Needs a backend branch in `SessionTurnLlm` (or `SessionExecutionBackend`): Rig runtime vs. ACP runtime (`droid exec --output-format acp`). |
| `src-tauri/src/harness_profile.rs` | Builds Gospel preamble, `workspace_tools` (`source_edit`, `write_harness_file`), `shell_tools`, `delegate_exploration`, `run_skill_script`, and `LoopDetector`. | Droid runs its own internal tool loop and builtin system prompt (`--disable-builtin-skills` is unsupported in ACP). Matched/invoked `SKILL.md` content, unresolved `SessionNote`s, and Memory recall can be passed in the ACP prompt context; Gospel tools/loop guards do not wrap Droid's internal steps unless routed through ACP `fs/*` / `terminal/*` client handlers. |
| `src-tauri/src/approval_broker.rs` | `ApprovalKind` supports `Command`, `ExternalPath`, `McpTool` with `Ask`/`Auto` modes and `approval-requested` / `approval-resolved` UI events. | Add/bridge ACP `session/request_permission` into `ApprovalBroker` (either a new `ApprovalKind::ExternalAgent` or mapped variants) so `ApprovalCard.tsx` handles Droid's `allow_once` / `allow_always` / `reject_once` / `reject_always`. |
| `src-tauri/src/mcp_runtime.rs` | Connects enabled+trusted stdio/HTTP MCP servers via `rmcp` and wraps non-`readOnlyHint` tools with `ApprovalBroker`. | ACP `session/new` accepts `mcpServers` (`http`, `sse`, `stdio`). Note: passing Gospel's MCP server definitions directly to Droid lets Droid call MCP tools natively, bypassing Gospel's `ApprovalKind::McpTool` unless mediated via `session/request_permission` or a local proxy. |
| `src-tauri/src/conversation/` & `session_store.rs` | Single-owner invariant (ADR-0019): `Model History` (`Vec<rig::Message>`) and `Display Transcript` stay in sync; `Display Transcript` rebuilds from `Model History`; `Retry`/`Fork` slice `Vec<rig::Message>`. | **Largest architectural delta.** ACP turns do not produce `Vec<rig::Message>`. Must persist `external_session_id` (plus backend kind) on `sessions`, store `Display Transcript` turns directly for ACP sessions without requiring `rig::Message` rebuilds, map `Fork` to ACP `session/fork` (if supported), and define explicit behavior for `Retry` and mid-session Rig↔ACP switching. |
| `src-tauri/src/verification.rs`, `review/mod.rs`, `bounty/sweep.rs`, `llm.rs::generate_session_title` | All call `provider_client::completion_model` directly for structured JSON outputs or `BountyHttpTool`. | Cannot run `rig` structured/tool loops on an ACP agent backend. Review & Bounty should remain Rig-provider scoped; Title generation & Verification should use a fallback Rig provider if configured (or heuristic title / `VerificationStatus::Unavailable` when only ACP is active). |
| `src/components/ProviderSelector.tsx`, `oauthProviders.ts`, `AppShell.tsx`, `useSessionManager.ts`, `useChatStream.ts` | Assumes every provider is `api_key` or `oauth` (`start_provider_oauth`) and every session can freely swap models mid-thread. | Settings needs binary detection + ACP `device-pairing` auth (plus optional `FACTORY_API_KEY` fallback); session switching/model changes need clear handling if switching between Rig and ACP sessions. |

---

## 6. Recommended Architecture

### 6.1 Core Decision: Backend Seam Alongside Rig (Do Not Spoof `rig::DynModel`)

Introduce a clean execution-backend abstraction behind `SessionTurnLlm` rather than trying to fake an HTTP completion endpoint:

- **`BackendKind::Rig`** — existing built-in & Custom Providers running `agent_runtime.rs`.
- **`BackendKind::Acp`** — external ACP agent servers running via the official **`agent-client-protocol`** Rust crate (`3.x` + `agent-client-protocol-schema` `1.14.0`), starting with `factory_droid`.

### 6.2 Key Design Choices

1. **Process Lifecycle & Pooling:**
   - Spawn `droid exec --output-format acp` with `--cwd <workspace>` (or unscoped temp/home fallback if no workspace) using direct binary path resolution (checking `GOSPEL_DROID_PATH` / app config override, `~/.local/bin/droid`, `/opt/homebrew/bin/droid`, `/usr/local/bin/droid`, then `PATH` — never `node .../bin/droid`).
   - Reuse one long-lived ACP client connection per `(workspace_path, auth_mode)` or per active session so repeated turns reuse the warm `droid` process instead of re-initializing each turn; terminate cleanly on app shutdown, logout, or `cancel_streaming` timeout.
2. **Authentication & Subscription Preservation:**
   - Default to **Droid CLI / `device-pairing` auth**: run `initialize` + `auth/status` to detect existing `droid` login automatically; if unauthenticated, invoke ACP `authenticate` (`device-pairing`) and surface the verification URL / device code via Gospel's existing `OauthChallenge::DeviceCode` / `Browser` UI in `ProviderSelector.tsx` (or `terminal-auth` fallback if `device-pairing` requires TTY).
   - Optionally accept `FACTORY_API_KEY` in Gospel's `credentials.json` and pass it **only** in the spawned subprocess environment when set, allowing headless/CI or API-key users to work alongside subscription users.
3. **Security & `ApprovalBroker` Mapping:**
   - Never pass `--skip-permissions-unsafe`.
   - Implement ACP `Client::request_permission` so Droid's `session/request_permission` emits a Gospel `ApprovalRequest` (`approval-requested`) and awaits `ApprovalBroker` (`Ask` mode prompts the user in `ApprovalCard.tsx`; `Auto` mode auto-approves within Gospel's policy).
   - Enable ACP `fs/read_text_file`, `fs/write_text_file`, and `terminal/*` client handlers if supported by Droid ACP, routing them through Gospel's `validate_workspace_path`, `symlink_guard`, and `CommandPolicy` so workspace boundaries stay enforced by Gospel rather than trusting subprocess defaults.
4. **Conversation Persistence & Session Continuity:**
   - Add nullable `acp_backend TEXT` and `acp_session_id TEXT` columns to `sessions` in `session_store.rs`.
   - For ACP sessions, persist `Display Transcript` directly from captured turn blocks (`TextToken`, `ToolCall`, `ToolResult`, error rows) and bypass `Model History`-based transcript rebuild (`conversation/transcript.rs`) when `acp_session_id` is set.
   - Resume sessions across restarts via ACP `session/load` (falling back to a concise display-transcript context preamble if `session/load` fails or the Droid session expired).
   - Map `Fork` to ACP `session/fork`; disable or guard mid-thread `Retry` unless `messageRetraction` / turn-rollback is verified against live `droid`.
   - Disallow seamlessly changing an already-started Rig session into an ACP session (and vice versa) — prompt the user to start a new session or fork when crossing backend kinds.

---

## 7. Proposed Phased Implementation Plan

### Phase 1 — ACP Client Core & MVP Chat Turns
- Add `agent-client-protocol` and `agent-client-protocol-schema` to `src-tauri/Cargo.toml`.
- Build `src-tauri/src/acp/` module:
  - Binary discovery (`droid` path resolution, version check, clear install instructions if missing).
  - `AcpClient` managing stdio subprocess, `initialize`, `auth/status`, `authenticate` (`device-pairing`), `session/new`, `session/prompt`, `session/cancel`, and `session/set_config_option`.
  - Event translator: `session/update` (`agent_message_chunk`, thoughts, `tool_call`, `tool_call_update`) → `SessionTurnEvent`.
  - Permission bridge: `session/request_permission` → `ApprovalBroker` → `ApprovalCard.tsx`.
- Wire `factory_droid` into Settings (`ProviderSelector.tsx` + `oauthProviders.ts`) and `ModelAvailabilitySnapshot` (using authenticated `configOptions` / CLI model catalog).
- Persist `acp_backend` / `acp_session_id` in `session_store.rs` and `Display Transcript` in `conversation/`.
- Gate Rig-only features (`ReviewEngine`, `BountySweep`, `VerificationAgent`, `generate_session_title`) gracefully with fallback Rig model or explicit unsupported/heuristic behavior.

### Phase 2 — Session Resume, Fork, Variants & Workspace Enforcement
- Wire `session/load` / `session/resume` on reopened ACP sessions and `session/fork` on Gospel Fork.
- Map `ModelVariant` (`reasoning-*`) to Droid's `thought_level` `configOptions` (or `session/set_config_option`).
- Implement ACP `fs/read_text_file`, `fs/write_text_file`, and `terminal/*` client callbacks backed by Gospel's `symlink_guard` and `CommandPolicy`.
- Forward matched/invoked Gospel `SKILL.md` sections and unresolved notes cleanly in `session/prompt` context blocks.

### Phase 3 — MCP Forwarding & Diagnostics
- Optionally pass trusted, enabled Gospel MCP servers into `session/new` `mcpServers` (with explicit user opt-in warning when Gospel cannot gate individual MCP tool calls inside Droid, unless intercepted via `session/request_permission`).
- Surface Factory Droid diagnostics (`auth/status`, binary path, version, active ACP session ID) in Settings Studio.

---

## 8. Open Decisions Before Implementation

1. **Authenticated ACP handshake verification:** Run one authenticated `droid exec --output-format acp` check (`device-pairing` + `session/new`) to confirm the exact `configOptions` / `models` schema, whether `droid` uses ACP `fs/*` & `terminal/*` vs built-in tools, and how `--auto` interacts with `session/request_permission`.
2. **Utility model policy for ACP sessions:** When a user selects a Factory Droid model, should `generate_session_title` and `VerificationAgent` fall back to another configured Rig provider (or a lightweight heuristic/skip), or attempt an isolated no-tool ACP session?
3. **Mid-session Rig ↔ ACP model switching:** Confirm whether the model picker should prevent switching an active Rig session to `factory_droid` (requiring a new task/fork), or create a fresh ACP session seeded with a transcript summary.
4. **Scope of Phase 1:** Confirm whether Phase 1 should target `factory_droid` specifically behind a generic `acp` module (recommended — clean `Agent Client Protocol` seam without premature UI for arbitrary external agents yet).
