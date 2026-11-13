# Changelog

All notable changes to Gospel will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Changed

- Auto-archive now defaults to Off. Explicitly saved archive policies are unchanged.

### Removed

- The undocumented `GOSPEL_MAX_TURNS` and `GOSPEL_AGENT_MAX_TURNS` environment variables no longer override agent model-call limits. Custom agents still take `max_turns` from `AGENT.md`.

### Fixed

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

[Unreleased]: https://github.com/ChristianPerez34/gospel/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/ChristianPerez34/gospel/releases/tag/v0.1.0-alpha.1
