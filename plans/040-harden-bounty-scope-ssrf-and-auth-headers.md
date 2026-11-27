# Plan 040: Harden Bounty Scope SSRF/IPv6 Checks and Auth-Profile Header Handling

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 04a60d3..HEAD -- src-tauri/src/bounty/scope.rs src-tauri/src/bounty/http_tool.rs`
> If either file changed since this plan was written, compare the "Current
> state" excerpts against the live code before proceeding; on a mismatch,
> treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED — adding a custom `reqwest::dns::Resolve` wrapper that validates resolved socket addresses must preserve `allow_private_hosts: true` and mock-server unit tests on `127.0.0.1`.
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `04a60d3`, 2026-11-25

## Why this matters

`BountyHttpTool` and `BountyScope` are the sole network authorization boundary for network-enabled custom agents (ADR-0016). Today, two security gaps break that contract:

1. **Private-host / SSRF bypass**: `is_private_host` passes `url.host_str()` directly to `lower.parse::<IpAddr>()`, which always fails on bracketed IPv6 literals (`[::1]`, `[fe80::1]`, `[::ffff:127.0.0.1]`), `is_private_ip` omits IPv4-mapped IPv6 (`to_ipv4_mapped()`) and CGNAT (`100.64.0.0/10`), and `BountyHttpTool`'s `reqwest::Client` performs standard DNS resolution without checking the resolved IP — allowing an in-scope hostname, CNAME, or DNS rebinding target that resolves to `127.0.0.1`, RFC 1918, or `169.254.169.254` to bypass `allow_private_hosts: false`.
2. **Auth-profile secret leaks**: `redact_header_value` only redacts 6 hardcoded header names, so `AuthProfile` entries of `type: header` with a custom header name (e.g., `X-Session-Token`) are persisted in plaintext in `.gospel/bounty/runs/<run-id>/evidence/<uuid>.json`; `build_request_headers` inserts case-sensitively into a `HashMap<String, String>` (allowing a lowercase user header to coexist with `"Authorization"`); and redirect handling only strips `authorization`/`cookie` on host changes, leaking `x-api-key`, `x-auth-token`, `proxy-authorization`, and custom auth-profile headers across hosts, or leaking `Authorization`/`Cookie` on `https` -> `http` or port changes on the same host.

## Current state

Relevant files:

- `src-tauri/src/bounty/scope.rs` — defines `is_private_host` / `is_private_ip` (lines 258–291), `AuthProfile::header_pair` (lines 128–146), and `BountyScope::decide` (lines 423–497).
- `src-tauri/src/bounty/http_tool.rs` — constructs `reqwest::Client` (lines 55–68), redacts headers (`redact_header_value`, `redact_request_headers`, lines 249–273), builds headers (`build_request_headers`, lines 311–342), follows redirects (lines 650–706), and writes `EvidenceRecord` (lines 750–766).

Current excerpts:

```rust
// src-tauri/src/bounty/scope.rs:258-291
fn is_private_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower == "metadata.google.internal"
    {
        return true;
    }
    match lower.parse::<IpAddr>() {
        Ok(ip) => is_private_ip(ip),
        Err(_) => false,
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_unspecified()
                || ip.is_documentation()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}
```

```rust
// src-tauri/src/bounty/http_tool.rs:55-68
        let client = reqwest::Client::builder()
            .user_agent("gospel-bounty/0.1")
            .redirect(redirect_policy)
            .timeout(
                scope
                    .as_ref()
                    .map(|scope| scope.timeout)
                    .unwrap_or(Duration::from_secs(30)),
            )
            .build()
            .map_err(|error| BountyHttpError::Client(error.to_string()))?;
```

```rust
// src-tauri/src/bounty/http_tool.rs:249-265
fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "authorization"
            | "cookie"
            | "set-cookie"
            | "proxy-authorization"
            | "x-api-key"
            | "x-auth-token"
    ) {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}
```

```rust
// src-tauri/src/bounty/http_tool.rs:690-698
            // Authorization/Cookie must not leak across a host change.
            if next_url.host_str() != current_url.host_str() {
                current_headers.retain(|name, _| {
                    !matches!(
                        name.to_ascii_lowercase().as_str(),
                        "authorization" | "cookie"
                    )
                });
            }
```

Documented constraints to honor (ADR-0016):

- *"Named `auth_profiles` resolve secrets from `env:` or `keychain:` references at request time ... the raw secret never enters prompts, tool output, evidence, or audit logs."*
- *"When `follow_redirects: true`, redirects are followed manually hop-by-hop (cap 5): each `Location` target re-passes `decide()` with the method it will actually be sent with ... `Authorization`/`Cookie` headers are stripped on host changes."*
- *"private and localhost hosts need `allow_private_hosts: true`."*

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Clippy | `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` | exit 0, no warnings |
| Targeted Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml bounty::` | all pass |
| Full Rust tests | `cargo test --manifest-path src-tauri/Cargo.toml` | all pass |

## Scope

**In scope** (the only files you should modify):
- `src-tauri/src/bounty/scope.rs`
- `src-tauri/src/bounty/http_tool.rs`

**Out of scope** (do NOT touch):
- `src-tauri/src/bounty/surface.rs` — delegates HTTP execution to `BountyHttpTool`, so fixing `BountyHttpTool` automatically protects `bounty_surface`.
- `src-tauri/src/bounty/sweep.rs` and `src-tauri/src/bounty/signal.rs` — separate rate-limiting and concurrency work.
- `src-tauri/Cargo.toml` — `reqwest::dns::{Addrs, Name, Resolve, Resolving}` and `tokio::net::lookup_host` are already available with current dependencies.

## Git workflow

- Branch: `advisor/040-harden-bounty-scope-ssrf-and-auth-headers`
- Commit style: conventional commits matching `git log` (e.g., `fix(bounty): enforce resolved-IP private-host checks and harden auth header redaction`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Fix bracketed IPv6, IPv4-mapped IPv6, and CGNAT detection in `src-tauri/src/bounty/scope.rs`

1. Make `is_private_ip` `pub(crate)` so `http_tool.rs` can reuse the exact same IP classifier.
2. In `is_private_host(host: &str)`:
   - Trim trailing dot (`trim_end_matches('.')`) and strip optional IPv6 brackets (`strip_prefix('[').and_then(|s| s.strip_suffix(']'))`) before calling `.parse::<IpAddr>()`.
3. In `is_private_ip(ip: IpAddr)`:
   - For `IpAddr::V4(v4)`: keep existing checks AND add CGNAT `100.64.0.0/10` (`(v4.octets()[0] == 100 && (v4.octets()[1] & 0b1100_0000) == 64)`), plus `v4.is_multicast()`, `0.0.0.0/8` (`v4.octets()[0] == 0`), and `240.0.0.0/4` (`v4.octets()[0] >= 240`).
   - For `IpAddr::V6(v6)`: keep existing checks AND add `v6.is_multicast()` AND check `if let Some(mapped) = v6.to_ipv4_mapped() { return is_private_ip(IpAddr::V4(mapped)); }` (and `v6.to_ipv4()` for deprecated IPv4-compatible `::a.b.c.d` addresses when `!v6.is_loopback() && !v6.is_unspecified()`).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml bounty::scope::tests` → all existing tests pass.

### Step 2: Wire a validated `reqwest::dns::Resolve` into `BountyHttpTool`'s HTTP client

In `src-tauri/src/bounty/http_tool.rs`:

1. Implement a private `ScopeDnsResolver { allow_private_hosts: bool }` implementing `reqwest::dns::Resolve`:
   - In `fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving`:
     - Spawn/run `tokio::net::lookup_host((name.as_str(), 0))` in the returned boxed future.
     - Collect the resolved `SocketAddr`s.
     - If `!self.allow_private_hosts`: check every resolved address with `super::scope::is_private_ip(addr.ip())`. If ANY resolved IP is private/local (or the list is empty), return `Err(Box::new(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("resolved private/local IP for host '{}' while allow_private_hosts is false", host))))`.
     - Return `Ok(Box::new(addrs.into_iter()))` so `reqwest` connects directly to the validated socket address (preventing TOCTOU DNS rebinding between check and connect).
2. In `BountyHttpTool::new`, attach `.dns_resolver(Arc::new(ScopeDnsResolver { allow_private_hosts }))` to `reqwest::Client::builder()`, where `allow_private_hosts = scope.as_ref().is_some_and(|s| s.allow_private_hosts)`.
3. Verify that `BountyHttpTool::new` does NOT break existing `allow_private_hosts: true` mock-server tests on `127.0.0.1` or `::1`.

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml bounty::http_tool::tests` → all existing tests pass.

### Step 3: Normalize auth-profile headers, redact custom auth headers in evidence, and tighten redirect stripping

In `src-tauri/src/bounty/http_tool.rs`:

1. Define a helper `fn is_sensitive_header(name: &str, extra_sensitive_header: Option<&str>) -> bool`:
   - Matches the existing 6 lowercase names (`authorization`, `cookie`, `set-cookie`, `proxy-authorization`, `x-api-key`, `x-auth-token`) OR case-insensitive equality against `extra_sensitive_header`.
2. Update `build_request_headers` to return `Result<(HashMap<String, String>, Option<String>), String>` (the headers map plus the optional injected auth-profile header name):
   - When `auth_profile` resolves `(name, value)`, remove any existing keys in `headers` matching `name.eq_ignore_ascii_case(existing_key)` before calling `headers.insert(name.clone(), value)` so a user-supplied lowercase `"authorization"` cannot coexist alongside `"Authorization"`.
   - Return `Some(name)` as the `extra_sensitive_header`.
3. Update `redact_header_value` and `redact_request_headers` to take `extra_sensitive_header: Option<&str>` and redact any header where `is_sensitive_header(name, extra_sensitive_header)` is true.
   - Pass `auth_header_name.as_deref()` when building `EvidenceRecord::request_headers` (and use `&current_headers` or `&headers` consistently — note: verify whether evidence records the final hop's request headers or initial headers; keep existing tests green while ensuring the custom auth header is always `[REDACTED]`).
4. In the redirect loop (`http_tool.rs:690-698`):
   - Define `same_origin = next_url.scheme().eq_ignore_ascii_case(current_url.scheme()) && next_url.host_str().map(str::to_ascii_lowercase) == current_url.host_str().map(str::to_ascii_lowercase) && next_url.port_or_known_default() == current_url.port_or_known_default()`.
   - When `!same_origin`, strip every header matching `is_sensitive_header(name, auth_header_name.as_deref())` from `current_headers` (covering `authorization`, `cookie`, `proxy-authorization`, `x-api-key`, `x-auth-token`, and any custom `auth_profile` header across host, scheme, or port changes).

**Verify**: `cargo test --manifest-path src-tauri/Cargo.toml bounty::` → all existing tests pass.

### Step 4: Add comprehensive unit tests in `scope.rs` and `http_tool.rs`

Add the unit tests described in the Test plan, then run Clippy and the full Rust test suite.

**Verify**: `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo test --manifest-path src-tauri/Cargo.toml` → exit 0, all tests pass.

## Test plan

1. **`src-tauri/src/bounty/scope.rs`** (`mod tests`):
   - `decide` with `allow_private_hosts: false` rejects bracketed IPv6 loopback (`https://[::1]/`), IPv6 link-local (`https://[fe80::1]/`), IPv6 unique-local (`https://[fc00::1]/`), IPv4-mapped IPv6 loopback/private (`https://[::ffff:127.0.0.1]/`, `https://[::ffff:169.254.169.254]/`), and CGNAT (`https://100.64.0.1/`).
   - `is_private_ip` returns `false` for public IPv4 (`93.184.216.34`) and public IPv6 (`2606:2800:220:1:248:1893:25c8:1946`).
2. **`src-tauri/src/bounty/http_tool.rs`** (`mod tests`):
   - `ScopeDnsResolver` with `allow_private_hosts: false` rejects `"localhost"` (which resolves to `127.0.0.1` / `::1`) and succeeds with `allow_private_hosts: true`.
   - Custom `AuthProfile` of `type: header` (e.g., `header: "X-Custom-Secret"`) is sent on the wire to the local test server, redacted to `"[REDACTED]"` in the saved evidence JSON file, and stripped on cross-origin redirect (e.g., port change between two local test servers on `127.0.0.1`) along with `X-Api-Key`.
   - `build_request_headers` replaces a user-supplied lowercase `"authorization"` header when a bearer `auth_profile` is applied (no duplicate authorization header entries).
   - Redirect from `https` to `http` (or different port on same host) strips `Authorization`, `Cookie`, `X-Api-Key`, and the custom auth-profile header.

**Verification**: `cargo test --manifest-path src-tauri/Cargo.toml bounty::` → all pass, including ≥ 5 new tests.

## Done criteria

- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` exits 0
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml` exits 0; all `bounty::` tests including the ≥ 5 new security tests pass
- [ ] `ScopeDnsResolver` is wired into `BountyHttpTool::new` via `.dns_resolver(...)`
- [ ] Only in-scope files are modified (`git status`)
- [ ] `plans/README.md` status row for `040` updated to `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- `reqwest` 0.12's `dns::Resolve` trait requires an extra Cargo feature that is not enabled in `src-tauri/Cargo.toml` (check before editing `Cargo.toml` — `Cargo.toml` is out of scope).
- `BountyScope::decide` or `BountyHttpTool::new` does not match the "Current state" excerpts.
- Any existing `bounty::` test breaks because a test server relies on `https -> http` or cross-port credential forwarding.
- A step's verification fails twice after a reasonable fix attempt.

## Maintenance notes

- If `reqwest` is later upgraded from `0.12` to `0.13` (to deduplicate `reqwest-rmcp`), re-check the `reqwest::dns::Resolve` trait signature (`reqwest::dns::Name`, `Resolving`).
- `BountyScope::decide` still runs before DNS resolution for fast lexical rejection, while `ScopeDnsResolver` enforces the ground-truth post-DNS boundary on every connection and redirect hop.
- Separate follow-up: sharing `HostRateLimiter` + `max_concurrent` across `BountyHttpTool`, `BountySurfaceTool`, and `sweep.rs` workers is out of scope here.
