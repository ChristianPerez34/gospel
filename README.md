# Gospel

Gospel is a desktop harness for LLM-powered coding agents. It keeps workspace context, sessions, tool activity, and code changes in one focused interface for developers.

## Alpha builds

Alpha builds are published as GitHub prereleases for Apple Silicon (M-series) Macs. Download the DMG from the [Releases page](https://github.com/ChristianPerez34/gospel/releases), drag Gospel to Applications, then launch it.

The alpha is ad-hoc signed but not notarized. If macOS blocks the first launch, Control-click the app and choose **Open**, or use **Open Anyway** from **System Settings > Privacy & Security**. Do not disable Gatekeeper system-wide.

Please report bugs, regressions, and feedback through [GitHub Issues](https://github.com/ChristianPerez34/gospel/issues).

## Custom providers

Under Settings > Models, add a custom OpenAI-compatible provider with its API base URL (for example `https://example.com/v1` or `http://localhost:11434/v1`). An API key is optional. Leave model IDs empty to discover them from `/models`, or enter model IDs manually, one per line or separated by commas. Requests use the streaming Chat Completions API; the selected model must support tool calling for agent actions.

Provider definitions are stored in Gospel's app settings database. API keys use the existing `~/.config/gospel/credentials.json` credential store, not the provider definition. HTTP connections send prompts and keys unencrypted. A provider's base URL cannot be changed after creation; create a new provider for a different endpoint.

## Development

Install dependencies with `bun install`, then run `bun tauri dev`.

The canonical app version is stored in `src-tauri/Cargo.toml`. See [the release guide](docs/releasing.md) before publishing a build.
