# Plan 039: Validate MCP OAuth `auth_url` Scheme and Harden Loopback Callback

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 04a60d3..HEAD -- src-tauri/src/mcp_runtime.rs src-tauri/src/lib.rs`
> If either file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW — rejecting non-`http`/`https` authorization URLs only blocks invalid/unsafe OAuth metadata, and looping past non-`/callback` requests only ignores stray browser connections.
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `04a60d3`, 2026-11-25

## Why this matters

When a developer initiates OAuth sign-in for a custom HTTP MCP server, `start_mcp_oauth` obtains `auth_url` from the remote server's OAuth discovery metadata and `start_mcp_server_oauth` passes it directly to `app.opener().open_url(...)` with no scheme validation. A malicious or compromised remote MCP server can return a `file://`, `smb://`, or custom OS URI scheme as its `authorization_endpoint` and trigger local file/application launch on the developer's machine. In addition, the loopback listener `accept_oauth_callback` calls `listener.accept().await` once and performs a single `stream.read(...)` call, so a browser speculative preconnect (0-byte socket), `/favicon.ico` request, or TCP-fragmented HTTP request line consumes the single accept slot and aborts sign-in.

## Current state

Relevant files:

- `src-tauri/src/mcp_runtime.rs` — implements `start_mcp_oauth` (lines 515–542), `McpOAuthFlow::wait_for_callback` (lines 546–582), and `accept_oauth_callback` (lines 584–612).
- `src-tauri/src/lib.rs` — `start_mcp_server_oauth` Tauri command (lines 697–724) opens `flow.auth_url` with `app.opener().open_url`.

Current excerpts:

```rust
// src-tauri/src/lib.rs:710-716
    let flow = mcp_runtime::start_mcp_oauth(&server).await?;
    app.opener()
        .open_url(&flow.auth_url, None::<String>)
        .map_err(|e| format!("Failed to open browser: {e}"))?;
    if let Err(error) = flow.wait_for_callback(&id).await {
```

```rust
// src-tauri/src/mcp_runtime.rs:530-541
    let auth_url = state
        .get_authorization_url()
        .await
        .map_err(|e| format!("OAuth setup failed: {e}"))?;
    Ok(McpOAuthFlow {
        state,
        auth_url,
        listener,
    })
```

```rust
// src-tauri/src/mcp_runtime.rs:584-611
/// Accept the first loopback request and hand back the full callback URL.
/// Responds with a plain "close this tab" page.
async fn accept_oauth_callback(listener: &tokio::net::TcpListener) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (mut stream, _peer) = listener
        .accept()
        .await
        .map_err(|e| format!("OAuth callback listener failed: {e}"))?;
    let mut buffer = vec![0u8; 8192];
    let n = stream
        .read(&mut buffer)
        .await
        .map_err(|e| format!("OAuth callback read failed: {e}"))?;
    let request = String::from_utf8_lossy(&buffer[..n]);
    let request_line = request.lines().next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "OAuth callback was malformed.".to_string())?
        .to_string();

    let response = "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: 146\r\nconnection: close\r\n\r\n<!doctype html><title>Gospel sign-in complete</title><body style=\"font-family:system-ui;padding:2rem\">Sign-in complete. You can close this tab and return to Gospel.</body>";
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok(format!("http://127.0.0.1:{port}{path}"))
}
```

Repo conventions:

- `src-tauri/src/mcp_runtime.rs` uses `reqwest_rmcp` / `reqwest::Url` (note: check existing imports at top of `mcp_runtime.rs` — both `reqwest` and `reqwest_rmcp` are in `Cargo.toml`; `reqwest::Url::parse` is available via `reqwest`).
- Unit tests in `src-tauri/src/mcp_runtime.rs` live inside `#[cfg(test)] mod tests` (lines 614–1042) using `tokio::runtime::Runtime` or `#[tokio::test]` and local `tokio::net::TcpListener::bind("127.0.0.1:0")`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0, no warnings |
| Targeted Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml mcp_runtime::tests` | all pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/mcp_runtime.rs`
- `src-tauri/src/lib.rs` (only defensive validation right before `app.opener().open_url` in `start_mcp_server_oauth`)

**Out of scope** (do NOT touch):
- `src-tauri/src/oauth.rs` or `src-tauri/src/grok_oauth.rs` — built-in provider OAuth endpoints are hardcoded constants, not remote discovery metadata.
- `src-tauri/Cargo.toml` — no new dependencies needed.
- Frontend MCP components (`McpSettingsPanel.tsx`, `useMcpServers.ts`).

## Git workflow

- Branch: `advisor/039-harden-mcp-oauth-opener-and-callback`
- Commit style: conventional commits matching `git log` (e.g., `fix(mcp): validate OAuth auth URL scheme and harden callback listener`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `validate_oauth_auth_url` in `src-tauri/src/mcp_runtime.rs` and enforce it at both boundaries

1. Add a `pub(crate) fn validate_oauth_auth_url(raw: &str) -> Result<(), String>` helper in `src-tauri/src/mcp_runtime.rs`:
   - Parse with `reqwest::Url::parse(raw).map_err(|_| "OAuth authorization URL is invalid.".to_string())?`.
   - Require `matches!(parsed.scheme(), "https" | "http")`; reject any other scheme (`file`, `javascript`, `smb`, custom schemes, etc.) with a clear error message naming only the disallowed scheme (never echoing query params or secrets).
   - Require `parsed.has_host()` and `parsed.username().is_empty()` and `parsed.password().is_none()`.
2. Call `validate_oauth_auth_url(&auth_url)?;` in `start_mcp_oauth` immediately after `state.get_authorization_url().await` (before returning `Ok(McpOAuthFlow { ... })`).
3. Call `mcp_runtime::validate_oauth_auth_url(&flow.auth_url)?;` in `src-tauri/src/lib.rs` (`start_mcp_server_oauth`, right before `app.opener().open_url(&flow.auth_url, None::<String>)`) as defense in depth.

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` → exit 0.

### Step 2: Harden `accept_oauth_callback` against preconnects, non-callback paths, and fragmented reads

Update `accept_oauth_callback` in `src-tauri/src/mcp_runtime.rs`:

1. Read the port once from `listener.local_addr()`.
2. Loop calling `listener.accept().await`:
   - For each accepted `stream`, read incrementally into a `Vec<u8>` up to a `MAX_REQUEST_BYTES = 8192` cap (with a short per-connection read timeout, e.g., `Duration::from_secs(5)`, so a stalled idle browser preconnect cannot starve the real callback) until either `\r\n` (end of request line) is found, EOF is reached, or the cap is hit.
   - If 0 bytes were read (speculative preconnect closed without sending data) or the read timed out before receiving a request line, close the socket and `continue`.
   - Extract the first line (up to `\r\n` or `\n`) and parse the HTTP request target (`split_whitespace().nth(1)`). If missing or not starting with `"/callback"` (specifically, require `path == "/callback"` or `path.starts_with("/callback?")`), write a minimal `HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n`, shut down the stream, and `continue`.
   - Once a valid `/callback` (or `/callback?...`) request is received, write the existing `200 OK` HTML response, shut down the stream, and return `Ok(format!("http://127.0.0.1:{port}{path}"))`.
3. Confirm the existing outer `tokio::time::timeout(OAUTH_CALLBACK_TIMEOUT, accept_oauth_callback(&self.listener))` in `wait_for_callback` still bounds the entire accept loop.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml mcp_runtime::tests` → all existing tests pass.

### Step 3: Add unit tests for URL scheme validation and loopback callback resilience

Add the unit tests described in the Test plan to `src-tauri/src/mcp_runtime.rs` inside `#[cfg(test)] mod tests`, then run Clippy and the full Rust test suite.

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml` → exit 0, all tests pass.

## Test plan

Add unit tests in `src-tauri/src/mcp_runtime.rs` (`mod tests`, modeled after existing async listener tests in that module):

1. `validate_oauth_auth_url_accepts_https_and_http`: accepts `https://auth.example.com/authorize?client_id=abc` and `http://127.0.0.1:8080/authorize`.
2. `validate_oauth_auth_url_rejects_unsafe_schemes_and_invalid_urls`: rejects `file:///etc/passwd`, `smb://host/share`, `javascript:alert(1)`, `ssh://host`, `https://user:pass@example.com/auth`, and empty/relative URLs.
3. `accept_oauth_callback_ignores_empty_preconnect_and_favicon_before_callback`: connects a 0-byte client that immediately drops, then sends `GET /favicon.ico HTTP/1.1\r\n\r\n` (asserting 404 response), then sends `GET /callback?code=abc&state=xyz HTTP/1.1\r\n\r\n` (asserting 200 response and returned URL ending with `/callback?code=abc&state=xyz`).
4. `accept_oauth_callback_handles_fragmented_request_line`: writes `GET /call` then flushes, pauses briefly, then writes `back?code=split HTTP/1.1\r\n\r\n` over the same TCP stream and asserts the full callback URL is returned intact.

**Verification**: `cargo test --manifest-path src-tauri/Cargo.toml mcp_runtime::tests` → all pass, including 4 new tests.

## Done criteria

- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0, including the 4 new `mcp_runtime::tests` cases
- [ ] `validate_oauth_auth_url` is called in both `mcp_runtime::start_mcp_oauth` and `lib.rs::start_mcp_server_oauth` before `open_url`
- [ ] Only in-scope files are modified (`git status`)
- [ ] `plans/README.md` status row for `039` updated to `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- `start_mcp_oauth` or `accept_oauth_callback` no longer matches the excerpts in "Current state".
- `reqwest::Url` is not accessible directly in `mcp_runtime.rs` (check whether `reqwest_rmcp::Url` or `reqwest::Url` is imported before adding any crate).
- Any test fails twice after a reasonable fix attempt.

## Maintenance notes

- Do not tighten `validate_oauth_auth_url` to reject `http://` completely without checking local dev MCP servers (`http://localhost:...` / `http://127.0.0.1:...`), which legitimate developers use for local OAuth test harnesses.
- If other Tauri commands ever call `app.opener().open_url(...)` with server-provided URLs (rather than hardcoded constants), reuse `validate_oauth_auth_url` or an equivalent scheme guard.
