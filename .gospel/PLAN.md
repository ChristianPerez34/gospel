# Goal

Complete the architecture-review handoff by selecting a deepening candidate, designing the interface, applying the focused implementation, and recording verification.

# Steps

- [x] Read `/tmp/gospel_architecture_review_handoff.md`.
- [x] Check the referenced HTML report and live source state.
- [x] Select the strongest available candidate from the handoff.
- [x] Apply codebase-design vocabulary and deletion test to the candidate.
- [x] Implement the deepened module interfaces and update callers.
- [x] Run targeted and full Rust verification.

# Evidence / Verification

- `/tmp/architecture-review-1740645440.html` was not present, so candidate selection used the handoff's ranked recommendation plus live source inspection.
- Selected module: JSON canonicalization for stable tool-call loop hashing.
- Dependency category: in-process. The module is pure `serde_json::Value` computation, with no I/O and no adapter.
- Interface selected: `canonical_json_string(&serde_json::Value) -> String`.
- Rejected alternatives:
  - Moving `sort_json_keys` unchanged: shallow; callers still own serialization and fallback behavior.
  - Exposing sorted `serde_json::Value`: more flexible than current callers need, increasing interface knowledge.
  - Trait/adapter seam: unjustified; there is no second adapter and the dependency is pure in-process logic.
- Deletion test: deleting the new module would put recursive object-key ordering and canonical serialization back into LLM loop-detection code. The module earns its keep by concentrating that behavior behind one small interface.
- Secondary module: UTF-8-safe text truncation.
- Secondary interface selected: `truncate_text_bytes(&str, usize) -> (String, bool)`.
- Secondary deletion test: deleting the module would return generic truncation logic to `workspace_tools.rs` while `conversation.rs` and `llm.rs` would again depend on a workspace-tools module for unrelated text behavior.
- Domain alignment: no `CONTEXT.md` update needed. This is an implementation utility and does not introduce or rename a Gospel domain term.
- Verification passed:
  - `cargo test --manifest-path src-tauri/Cargo.toml utils`
  - `cargo test --manifest-path src-tauri/Cargo.toml conversation::tests`
  - `cargo test --manifest-path src-tauri/Cargo.toml workspace_tools::tests::source_edit_diff_preview_reports_line_truncation`
  - `cargo test --manifest-path src-tauri/Cargo.toml llm::tests`
  - `cargo test --manifest-path src-tauri/Cargo.toml` (228 library tests, 1 keychain integration test, doc tests with 1 ignored doctest)

# Open Questions / Risks

- The handoff's secondary `truncate_text_bytes` location was stale: live code had the function in `workspace_tools.rs`, imported by `conversation.rs` and `llm.rs`. It has been moved to the text utility module.
- The missing HTML report means the original visual comparison artifact could not be rechecked.

# Next Action

Ready for review.
