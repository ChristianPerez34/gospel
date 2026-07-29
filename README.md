# Gospel

Gospel is a desktop harness for LLM-powered coding agents. It keeps workspace context, sessions, tool activity, and code changes in one focused interface for developers.

## Alpha builds

Alpha builds are published as GitHub prereleases for Apple Silicon (M-series) Macs. Download the DMG from the [Releases page](https://github.com/ChristianPerez34/gospel/releases), drag Gospel to Applications, then launch it.

The alpha is ad-hoc signed but not notarized. If macOS blocks the first launch, Control-click the app and choose **Open**, or use **Open Anyway** from **System Settings > Privacy & Security**. Do not disable Gatekeeper system-wide.

Please report bugs, regressions, and feedback through [GitHub Issues](https://github.com/ChristianPerez34/gospel/issues).

## Development

Install dependencies with `bun install`, then run `bun tauri dev`.

The canonical app version is stored in `src-tauri/Cargo.toml`. See [the release guide](docs/releasing.md) before publishing a build.
