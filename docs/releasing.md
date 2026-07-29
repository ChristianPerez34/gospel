# Releasing Gospel

Gospel releases use the version in `src-tauri/Cargo.toml`. The sync script propagates that version to `package.json` and `src-tauri/tauri.conf.json`.

## Alpha Releases

1. Set the intended SemVer prerelease, such as `bun run version:bump -- --set 0.1.0-alpha.1`.
2. Sync the derived version files with `bun run version:sync:release`.
3. Run `bun run check`, `bun run test`, and `cargo test --locked --manifest-path src-tauri/Cargo.toml`.
4. Confirm Cargo, the lockfile, and both derived version files agree with `bun run version:check`.
5. Build the local candidate with `APPLE_SIGNING_IDENTITY="-" bun tauri build --target aarch64-apple-darwin`.
6. Commit the release-preparation files and merge them into `main`.
7. Tag that verified merge commit with the matching version, for example `git tag v0.1.0-alpha.1`, then push the tag.

The `Release` GitHub Actions workflow validates that the tag and committed versions match, reruns the quality gates, and creates a draft prerelease when the tag contains a prerelease suffix such as `-alpha.1`. Inspect the uploaded DMG and release copy before manually publishing the draft.

## Candidate Smoke Test

Test the packaged app rather than the development server:

- Open the DMG and drag Gospel to Applications.
- Confirm the documented Gatekeeper override works and the app launches.
- Add a workspace and confirm it remains available after restarting the app.
- Configure a provider, restart, and confirm its credential remains available through Keychain.
- Send a prompt in read-only mode and confirm workspace tools cannot edit files.
- Approve one harmless edit or command and deny another to exercise both approval paths.
- Confirm the app reports the intended version and does not require a font or UI network request.

## Distribution Constraints

- The current alpha pipeline targets Apple Silicon Macs only.
- Builds use ad-hoc signing (`APPLE_SIGNING_IDENTITY: "-"`) and are not notarized. Testers need to use the standard macOS Gatekeeper override on first launch.
- Automatic updates are not included; testers must download each alpha manually.
- The `com.christianperez.gospel` bundle identifier is the stable macOS identity for alpha builds.
- Do not tag a release from an uncommitted working tree. Release only a verified commit on `main`.
