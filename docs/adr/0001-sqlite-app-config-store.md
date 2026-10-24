# ADR 0001: SQLite App Config Store

## Status

Accepted. Amended 2026-10-23: provider API keys moved from the OS keychain to `~/.config/gospel/credentials.json` (`0600`). Gospel builds are ad-hoc signed, so every rebuild produces a new code identity that the Keychain silently denies access to items written by earlier builds — the store could never be durable.

## Context

Gospel needs to persist non-secret application preferences such as Provider Visibility. These preferences are not credentials and should not be stored in the OS keychain, but they must survive app restarts and be owned by backend commands so Settings and model availability share one source of truth.

## Decision

Store non-secret app preferences in a SQLite database under the app data directory. Access stays behind Rust commands, and missing Provider Visibility rows default to visible.

## Consequences

- Settings can read and update Provider Visibility without duplicating a frontend registry.
- Unknown provider IDs are rejected before writing preferences.
- If SQLite initialization fails, Gospel falls back to default-visible providers and returns a recoverable availability warning.
- Secret material remains in the OS keychain or provider OAuth cache, not in SQLite.
