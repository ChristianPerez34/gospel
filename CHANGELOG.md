# Changelog

All notable changes to Gospel will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

## [0.1.0-alpha.2] - 2026-11-20

### Added

- A workspace command center and task rail with running-session previews, status filters, and approval guidance.
- MCP servers over stdio and HTTP, including OAuth sign-in and tool approval controls.
- Custom OpenAI-compatible providers: add your own endpoint (for example a local or self-hosted gateway) from Settings, with an optional API key, manual model IDs, or model discovery from the endpoint. Custom providers join the provider registry and work across the model picker, visibility toggles, and chat.
- Grok and OpenCode Go provider support, a searchable model picker with favorites, and live model discovery.
- Live subagent activity and a specialized bug-bounty agent with scoped network tools.

### Changed

- Session switches load recent transcript pages first and prefetch history for faster navigation.
- Auto mode runs command and skill-script requests without approval, including destructive or restricted commands; user deny rules still prompt. External file-access and MCP tool approvals remain separate.
- Plan mode was removed; main-agent sessions now run in Build mode, while subagents retain their role-based restrictions.
- Auto-archive now defaults to Off. Explicitly saved archive policies are unchanged.

### Removed

- The corpus knowledge graph and context-search index; workspace exploration now uses live file and search tools.
- The undocumented `GOSPEL_MAX_TURNS` and `GOSPEL_AGENT_MAX_TURNS` environment variables no longer override agent model-call limits. Custom agents still take `max_turns` from `AGENT.md`.

### Fixed

- Session history loads after switching to the session’s workspace, and rapid workspace switches keep the latest selection.
- MCP tool cards distinguish display targets from approval rules.
- Code-block copy actions use the native clipboard in the desktop app.
- Resolved approvals no longer duplicate tool activity, and denied and cancelled tool calls remain visible.
- Response verification no longer gives up with "Agent reached its 6 model-call limit" when it checks several files. It is now bounded only by its 90-second deadline and loop guard, and a timeout reports as a verification timeout.
- ChatGPT now lists every model the signed-in plan offers, including GPT-6 and GPT-6.1, instead of a fixed list that stopped at GPT-5.6. Offline fallback lists for ChatGPT and Anthropic now follow Rig's bundled catalog.
- Session history stays visible after restarts and when several workspaces are registered. Listing history only runs archive maintenance for the active workspace, and a workspace that fails to load no longer hides history from the others.

## [0.1.0-alpha.1] - 2026-07-29

### Added

- First public Gospel alpha for Apple Silicon Macs.
- Initial Gospel desktop app capabilities.
- Version bump tooling that increments the canonical Cargo version and syncs derived package metadata.
- Version sync tooling that derives package metadata from `src-tauri/Cargo.toml`.
- Tag-driven GitHub Actions release pipeline for Apple Silicon DMG artifacts.

### Known limitations

- The alpha is available only for Apple Silicon Macs.
- The DMG is ad-hoc signed and not notarized, so macOS will require the standard Gatekeeper override on first launch.

[Unreleased]: https://github.com/ChristianPerez34/gospel/compare/v0.1.0-alpha.2...HEAD
[0.1.0-alpha.2]: https://github.com/ChristianPerez34/gospel/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[0.1.0-alpha.1]: https://github.com/ChristianPerez34/gospel/releases/tag/v0.1.0-alpha.1
