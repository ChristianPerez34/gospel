# Gospel Context

## Glossary

- **Credentialed Provider**: A supported model provider for which Gospel can access a usable credential source. API-key providers are credentialed when a key exists in the OS keychain. ChatGPT Plus/Pro is credentialed when a reusable OAuth session exists in the local ChatGPT auth cache.
- **Provider Visibility**: A non-secret user preference that determines whether a credentialed provider should contribute models to the model picker. Missing visibility data defaults to visible.
- **Available Model**: A backend-returned provider/model entry that is selectable because its provider is credentialed, visible, and model loading returned a live or cached model list.
- **Live Workspace Tools**: Workspace-scoped chat tools that inspect the active Gospel workspace directly, including safe file reads, code search, and directory discovery. These are the source of truth for file contents.
- **Exploration Agent**: A backend-only helper Agent that investigates broad multi-file or architectural questions inside the active workspace and returns a structured report to the main Agent.
- **Turn**: One LLM inference cycle, which may include tool execution. A turn ends when the LLM produces a final text response (not a tool call).
- **Conversation**: A sequence of user/agent message pairs within a session. Conversations are identified by session ID and stored server-side.
- **Tool**: A registered function the LLM can invoke during a turn. Tools execute and return results that feed back into the next turn.
- **Skill**: A user-authored directive (a `SKILL.md` file with YAML frontmatter and a markdown body) that can be injected into the LLM preamble to steer behaviour. Skills may bundle executable scripts. Skills are discovered from `<workspace>/.agents/skills/` and the global user data directory.
- **Skill Source**: The origin of a discovered skill. Either `Workspace` (from the active workspace's `.agents/skills/` directory) or `Global` (from the user's global data directory). Workspace skills take precedence over global skills when names collide.
- **Skill Match**: An automatic per-turn process that compares the user's prompt tokens against the name and description of all discovered skills. The top-3 matches above a score threshold are emitted as a `## Active Skills` section in the system preamble. Workspace skills win score ties over global skills.
- **Skill Invocation**: A user-initiated slash command (`/<skill-name>`) that suppresses the auto-match list and injects the full skill body into the system preamble for that turn. Unknown skill names fall back to normal turn behaviour with a warning.
