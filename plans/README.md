# Implementation Plans

Plans 027–031 were generated on 2026-08-02 at HEAD `9df8439`.

Plans 022–026 were generated on 2026-07-31 at HEAD `0ec1edd`. Plans 001–010 generated on 2026-07-11 / 2026-07-17 and all DONE. Plans 011–021 generated on 2026-07-20 (HEAD `72819cd`). Execute in the order below unless dependencies say otherwise. Each executor: read the plan fully before starting, honor its STOP conditions, and update your row when done.

## Execution order & status

| Plan | Title | Priority | Effort | Depends on | Status |
|------|-------|----------|--------|------------|--------|
| 001  | Fix Frontend Vitest Harness and Test Execution | P1 | S | None | DONE |
| 002  | Add Biome Lint, Format, and Typecheck Configuration | P1 | S | 001 | DONE |
| 003  | Fix Loop Detector False Positive on Successful No-Op Results | P1 | S | 002 | DONE |
| 004  | Close Shell Flag-Style Path Escape | P1 | S | 003 | DONE |
| 005  | Route Skill Script Execution Through Approval Broker | P1 | M | 004 | DONE |
| 006  | Close Whitespace-Containing Shell Path Bypass | P1 | M | None | DONE |
| 007  | Harden Skill Script Directory Sandbox | P1 | S | None | DONE |
| 008  | Harden Corpus Symlink Boundaries | P1 | M | None | DONE |
| 009  | Fix Local Session Streaming Fallback | P1 | S | None | DONE |
| 010  | Bound Subprocess Output Capture | P1 | M | 006, 007 | DONE |
| 011  | Persist TopBar Inline Session-Title Edits | P1 | S | None | DONE |
| 012  | Characterization Tests for `useChatStream` (Streaming Foundation) | P1 | M | None | DONE |
| 013  | Gate Workspace Switcher + TopBar Switch Button While Streaming | P1 | M | 012 | DONE |
| 014  | Cancel Streaming + Per-Run Event Isolation | P1 | M | 012 | DONE |
| 015  | Delete Dead/Duplicate Components and Misleading Tests | P2 | S | None | DONE |
| 016  | Hard-Block `rm --recursive /` and Long-Form Destructive Patterns | P1 | S | None | DONE |
| 017  | Surface Resolved Skill-Script Interpreter in Approval Request | P1 | S | None | DONE |
| 018  | Isolate Untrusted Content in Verification/Review Prompts | P1 | M | None | DONE |
| 019  | Harden Trace Log Redaction for Free-Form Strings and Pretty-Printed JSON | P2 | S | None | DONE |
| 020  | Migrate `serde_yaml` → `yaml_serde` on the Skill-Frontmatter Parse Path | P2 | S | None | TODO — `yaml_serde` 0.10.4 is the selected maintained YAML-organization fork. |
| 021  | (D1 Spike) Investigate Surfacing `.gospel/PLAN.md` as a First-Class UI Panel | P2 | M | None | DONE |
| 022  | Close Cross-Workspace Session IPC Authorization Gaps | P1 | M | None | DONE |
| 023  | Make Session Exports Safe at the Share Boundary | P1 | M | None | DONE |
| 024  | Sanitize Provider Errors Before User-Facing Persistence | P1 | S | None | DONE |
| 025  | Reject Stale Aggregate Review Events | P1 | S | None | DONE |
| 026  | Coalesce Streamed Text Updates Without Reordering Blocks | P1 | M | None | DONE |
| 027  | Resilient Mutex Error Handling in `AppConfigStore` | P1 | S | None | TODO |
| 028  | Add `version:check` Step to PR CI Workflow and Move `shadcn` to `devDependencies` | P1 | S | None | TODO |
| 029  | Optimize Corpus Persistence Search Connection Reuse and Query Indexing | P1 | S | None | TODO |
| 030  | Add Unmount Safety and Unit Tests to `useMcpServers` Hook | P1 | S | None | TODO |
| 031  | Direct Workspace Search and Switching in Command Palette (`Cmd+K`) | P2 | S | None | TODO |

Status values: `TODO` | `IN PROGRESS` | `DONE` | `BLOCKED` (with one-line reason) | `REJECTED` (with one-line rationale)

## Dependency notes

- Plans 001–026 dependency notes remain as previously recorded.
- **Plans 027, 028, 029, 030, 031 are independent** and can start in any order.
  - Plan 027 targets backend `app_config.rs` Mutex error resilience.
  - Plan 028 targets `.github/workflows/pr.yml` CI verification and `package.json` dependency cleanup.
  - Plan 029 targets backend `corpus/persistence.rs` connection pooling & read-only search optimization.
  - Plan 030 targets frontend `useMcpServers.ts` unmount safety and unit tests.
  - Plan 031 targets frontend `CommandPalette.tsx` workspace search and switching.

## Execution order suggestion

Recommended order for a single executor working sequentially:

1. **027** (Resilient Mutex lock error handling) — prevents app-crashing panicking locks on config reads/writes.
2. **028** (PR workflow version check & shadcn devDep) — cheap, high CI safety leverage.
3. **029** (Corpus search optimization) — backend performance win for corpus retrieval.
4. **030** (useMcpServers unmount safety & tests) — frontend async safety & hook test coverage.
5. **031** (Command palette workspace search) — keyboard-first productivity feature for solo developers.

## Findings considered and rejected (this run, 2026-08-02)

- **React 18 → 19 migration**: Deferred; React 18 is stable across `@base-ui/react` and Vitest testing setup. Not worth upgrading until Base UI or key ecosystem dependencies require React 19.
- **Decompose backend god modules (`workspace_tools.rs`, `lib.rs`, `session_turn.rs`)**: High cognitive load, but L effort and high refactor risk across active Tauri IPC paths. Defer in favor of targeted bug and performance fixes.
- **Component test suites for stateful settings panels (`McpSettingsPanel`, `SettingsModal`)**: High confidence test gap, but M effort; prioritized hook-level tests and CI version validation first. Revisit after UI settings modal refactoring.
- **Pre-commit git hooks framework (Husky/Lefthook)**: Real DX opportunity, but local developers already run `bun run check` and CI enforces lint/format. Avoid introducing external git hook dependencies unless team onboarding calls for it.
- **`.env.example` template file**: Minor onboarding friction, but Gospel does not require external environment variables to run locally (`bun tauri dev` works out of the box with OS keychain).

The 2026-08-02 audit selected findings 1–5 for plans 027–031. Existing plan 020 remains TODO.

## Findings considered and rejected (prior run, 2026-07-31)

- **Corpus first-turn blocking**: downgraded and not planned because the primary frontend path uses `complete_streaming`, while corpus auto-build is scheduled in the startup background task.
- **Repository `AGENTS.md` and numbered plan prose as runtime prompt injection**: not promoted because repository guidance is not automatically injected into the product's runtime prompt path.
- **Archive maintenance batching**: lower leverage than selected reliability plans.
- **IPC contract centralization**: L effort and not selected in this pass.
- **Corpus-search round-trip coverage**: test gap, deferred to follow-up plan.
- **Rust CI locking/toolchain/formatting**: local verification baseline is green; deferred.
- **Bun audit advisories**: report mixes dev-only and platform-specific paths.

## Findings considered and rejected (prior run, 2026-07-20)

- **Skill-script env-key denylist for custom MCP servers (SEC-05)**: narrow threat model.
- **GitHub Copilot OAuth tokens mode enforcement (SEC-01)**: deferred until rig provider ADR.
- **`env -S` argv split mismatch (SEC-07)**: classifier never launches `env -S`.
- **Quote-containing `argument_path_value` escape (CORR-02)**: LOW confidence.
- **`truncate_text_bytes` tiny-cap seam (CORR-03)**: latent only.
- **`SessionStore::get_archive_policy(None)` mis-reports `uses_workspace_override` (CORR-04)**: latent.
- **ReviewerPanelCard smooth-scroll without `prefers-reduced-motion` (FE-CORR-06)**: fold into future a11y pass.
- **Nested `aria-live` regions (FE-CORR-05)** and **DESIGN.md side-stripe anti-pattern in production CSS (FE-CORR-08)**: bundled into deferred a11y pass.
- **`dead package-lock.json` cleanup (TECH-DEBT-01)** and **dead `greet` template command (TECH-DEBT-02)**: fold into routine cleanup commit.
- **`rusqlite` 0.32 → 0.40 migration (DEPS-03)** and **`tree-sitter` 0.23 → 0.26 migration (DEPS-04)**: deferred until required by transitive deps.
- **Bun CI cache (DX-02)**: small savings.
- **README replacement (DX-01)**: maintainer QoL edit.
- **Lint noise from throwaway prototype (TECH-DEBT-03)**: prototype is dev-gated.
- **`lib.rs` god-module split (TECH-DEBT-04)**: reject as standalone refactor.
- **SessionDrawer/ReviewResults virtualization (PERF-01)**: recommend dedicated plan after UI work.
- **`useConstellation` / `ChatView` memoization (PERF-02)**: defer until profiling shows jank.
