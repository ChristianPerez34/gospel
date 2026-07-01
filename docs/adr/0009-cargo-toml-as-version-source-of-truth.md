# ADR 0009: Cargo.toml as Version Source of Truth

## Status

Accepted

## Context

Gospel publishes the same App Version through Rust package metadata, Tauri bundle metadata, and frontend package metadata. Manually editing `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, and `package.json` would make releases easy to drift, but a separate `VERSION` file or `build.rs`-driven propagation would add extra moving parts for a small Tauri app.

## Decision

Use `src-tauri/Cargo.toml` as the canonical App Version. Keep the Cargo version clean, such as `0.1.0`, and use `scripts/bump-version.py` to increment that canonical SemVer before releases. The bump script updates Cargo metadata first, then runs `scripts/sync-version.py --release` to propagate the clean version into `package.json` and `src-tauri/tauri.conf.json`. Development metadata is derived only in the generated targets: `sync-version.py --dev` writes a `-dev` suffix, while `sync-version.py --release` writes the clean release version.

Release Builds are created by pushing `v*` git tags. The GitHub Actions release workflow syncs the clean version, builds an Apple Silicon DMG, and attaches it to the matching GitHub Release.

## Consequences

- Contributors run one bump command before tagging a release.
- Local Dev Builds can carry a `-dev` suffix without committing that suffix to Cargo metadata.
- Future changes to the version source require changing both the sync script and release workflow, so this decision is recorded instead of hidden in tooling.
