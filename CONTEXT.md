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
- **Agent Harness**: The totality of surfaces through which a human steers an AI agent's behaviour over a session or across sessions. Borrowed from the framing that code plays three roles: instruction (telling the agent what to do), verification (checking what the agent did), and context (giving the agent information to work with).
- **Harness Interface**: The complete set of touch-points where steering signals enter the system — the system preamble, skills, tools, conversation history, and any persistent artifacts. The Harness Interface is the boundary between the human's intent and the agent's execution.
- **Harness Mechanism**: A concrete, nameable component within the Harness Interface that fulfils one or more of the three roles (instruction, verification, context). Examples: the skill system, the TDD workflow, the exploration chain, and explicit planning.
- **PEV Loop**: Plan → Execute → Verify. The minimal outer loop for long-horizon agent work: maintain a plan, execute the next step, verify the result, then update the plan. The PEV Loop is the rhythm that explicit planning makes visible.
- **Shared Harness Substrate**: A persistent, workspace-scoped location (`.gospel/`) where harness artifacts live across turns and sessions. Shared between the human (who can inspect and edit) and the agent (who can read and write via tools).
- **Harness Control Area**: The `.gospel/` directory and its contents, treated as the agent's persistent control surface. The primary artifact is `PLAN.md`.

## Harness Interface Baseline

Code plays three roles in any agent-assisted workflow:

1. **Instruction** — telling the agent what to do (system preamble, skills, user prompts).
2. **Verification** — checking what the agent did (tests, type checks, lint, manual review).
3. **Context** — giving the agent information to work with (file contents, corpus, conversation history).

Before explicit planning, Gospel's Harness Interface consisted of:
- **System preamble**: assembled from workspace tools prompt, corpus prompt, delegation prompt, and matched/invoked skills.
- **Skills**: user-authored directives (`SKILL.md`) injected into the preamble, steering behaviour per-turn or per-invocation.
- **Live Workspace Tools**: four safe, workspace-scoped tools (read_file, search_code, find_files, list_directory) that give the agent source-of-truth access to the codebase.
- **Exploration Agent**: a delegated sub-agent for broad architectural investigation.
- **Conversation history**: the implicit memory within a session.

These mechanisms handle instruction and context well, but verification and long-horizon progress tracking were limited to what fits in conversational memory.

## Explicit Planning Mechanism

The first deliberate Harness Mechanism addition: a persistent, inspectable plan file that serves as the agent's outer loop for multi-step work.

**Substrate**: `.gospel/` directory (the shared harness substrate). The `.gospel/corpus/` subdirectory is reserved for internal corpus data.

**Primary artifact**: `.gospel/PLAN.md` — a lightweight plan file with required structure:
- **Goal**: one-sentence description of what we are trying to accomplish.
- **Steps**: checklist of completed and pending steps.
- **Evidence / Verification**: what has been verified so far (test results, manual checks, tool outputs).
- **Open Questions / Risks**: blockers, unknowns, decisions still needed.
- **Next Action**: the single most important thing to do next.

**Tool-driven contract**: The `write_harness_file` tool allows the main agent to create and update files under `.gospel/`. It enforces the `.gospel/` prefix, creates parent directories, and caps content at 1 MiB. The tool is registered only for the main agent loop; the exploration sub-agent remains read-only for the substrate.

**Visibility**: The `.gospel/` directory is added to the hidden allowlist, making it traversable by all Live Workspace Tools. Every workspace session preamble includes a `## Harness Control Area` section documenting the substrate, the PLAN.md structure, and guidance on maintaining it.

**Skill-agnostic contract**: The plan maintenance guidance lives in the harness prompt, not in individual skills. Skills (like `/tdd` and `/diagnose`) are external plugins that govern workflow discipline; the plan is the outer persistent record that tracks goal, progress, and evidence across turns regardless of which skill is active. This keeps skills decoupled from gospel-specific concerns while still giving the agent a clear contract for when and how to maintain the plan.
