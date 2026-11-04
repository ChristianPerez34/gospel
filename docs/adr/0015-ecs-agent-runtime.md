# ADR 0015: Rig ECS Executes Agent Runs

Chat, exploration, verification, review, and title generation use a shared Rig 0.46 ECS runtime instead of separate classic agent loops. Rig owns the model/tool lifecycle and retry scheduling; Gospel supplies role budgets, loop guards, provider configuration, tools, and approval policy. This gives all roles the same recovery behavior without maintaining a second retry state machine.

Each invocation owns an ECS world on a blocking worker. Handler futures enter Gospel's Tokio runtime on every poll so existing HTTP, process, MCP, and approval tools keep working. Dropping the caller cancels the turn and pending approvals. Session IDs remain stable prompt-cache identities across worlds.

Gospel's SQLite Session Turn adapter remains the sole persistence owner. The ECS conversation starts from existing Model History and returns successful history through that adapter; Rig's session journal is not enabled. Failed attempts discard their streamed text and reasoning, while completed tools and earlier successful model calls stay in the current turn. A retry repeats only the failed model request, never the whole user turn.

Transient model failures receive up to three retries with native exponential backoff starting at one second, including Rig's jitter and Retry-After handling. A narrow classification adjustment marks the gateway's `upstream_error` code retryable because Rig 0.46 does not recognize it; refusals and permanent HTTP errors retain their native verdicts. Remove this adjustment when Rig classifies that code itself.

The upgrade requires Rust 1.97.1 and rmcp 3.x. Existing ChatGPT auth stays at its current path. Legacy Copilot token files migrate into Rig's combined private token record on first use, and sign-out removes both formats.
