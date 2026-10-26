# Gospel design system

Gospel is an agent command center for a developer working across several projects. The primary surface answers three questions: what is running, what needs me, and which workspace does it belong to?

## Reference lock

Primary: [Linear project issues on Mobbin](https://mobbin.com/screens/ef923b0e-9a40-4fdd-80f2-f3aafb4f36ce). Adapt its persistent navigation, restrained row separators, compact metadata, and clear status hierarchy. Task content carries the emphasis; the surrounding chrome stays quiet.

Secondary: [Motion tasks on Mobbin](https://mobbin.com/screens/0955ab12-2387-492c-a404-07d7f4b55f1d). Adapt explicit workspace attribution and an all-workspace task view. Workspace filtering changes the view, while switching a workspace changes where a new task runs.

The reference screenshots are light interfaces. Gospel retains its existing dark-default theme and Geist typography. Emil Kowalski's interaction principles guide fast feedback, instant repeated navigation, and reduced motion.

| Decision | Implementation | Reason |
| --- | --- | --- |
| Overview first | Command center opens on launch | Several projects stay visible together |
| Tasks are sessions | Each task opens its existing conversation | Avoid a second task model with inconsistent state |
| Workspace always visible | A workspace column, project groups, and monitor labels | Make project context explicit before opening a task |
| Actual activity | Tool labels, pending approval titles, and response previews | Show what an agent is doing, rather than invented progress |
| Persistent monitoring | Task rail plus a wide-screen activity monitor | Opening a conversation preserves awareness of other runs |
| Low visual noise | Flat surfaces, fine dividers, modest radii | Keep long work sessions easy to scan |

## Foundations

Tokens live in `src/styles/tokens.css`. The existing semantic aliases remain the public styling interface; components should not invent their own palette.

| Token | Dark value | Role |
| --- | --- | --- |
| `--surface-sunken` | `#111215` | Persistent navigation and secondary monitor |
| `--surface-base` | `#151619` | Main working surface |
| `--surface-elevated` | `#1b1c20` | Composer, tools, hover feedback |
| `--surface-overlay` | `#25262c` | Selected items and inset controls |
| `--surface-line` | `#303138` | Boundaries and row dividers |
| `--text-primary` | `#ededf0` | Titles, conversation, primary information |
| `--text-secondary` | `#b7b8c2` | Task previews and supporting information |
| `--text-muted` | `#9697a3` | Timestamps, metadata, secondary labels |
| `--accent-action` | `#6e64cf` | Primary action, selection and focus |
| `--accent-structure` | `#aba6ee` | Selected navigation and links on dark surfaces |
| `--status-success` | `#79bd9a` | Running work |
| `--accent-signal` | `#dfb56c` | Approval requests and unread replies |
| `--status-error` | Existing semantic error token | Failed tasks and actionable errors |

Light mode swaps surfaces and text, with darker green, amber, and violet foregrounds for contrast. State always includes a written label. Do not reuse the running color for decorative accents.

Geist is the interface and conversation font. Monospace is reserved for code, model names, and compact timers. Main headings are 24–34px, task titles 13px, navigation 12px, supporting text 11–12px, and short metadata 9–10px. Use the existing 4px spacing scale. Default control radius is 6–8px; the composer uses 12px.

## Application structure

The 60px header contains Gospel's home action, task navigation toggle, current workspace, current task title, model/status, and settings. The header identifies the current workspace. Task setup explicitly selects the workspace used for new work. Clicking Gospel returns to the command center and leaves the focused task running in the background.

The task rail is 264px wide and stays visible on desktop. It contains the command center link, task search, new task action, Needs you, Running, project groups, and archive access. Status sections take priority over project groups. Rows keep their location while the pointer is over the list. Keyboard shortcuts follow the rail's displayed order. Running rows include a compact activity preview.

The command center contains a title, small cross-workspace status summary, status filters, search, workspace filter, and a task list. Rows show title, latest activity, model, workspace, state, and age or elapsed run time. Tasks needing attention come first, followed by running tasks and project groups. Rows keep their place while the pointer is over the list. Status filters update as work changes state. Filtering never switches the execution workspace. Counts describe all sessions, while the footer describes the current filtered view.

Selecting a task opens its conversation. The task rail remains available; at 1280px and wider, a 250px monitor also shows other running tasks and tasks needing attention. Each monitor item identifies its workspace and exposes one action: open the task.

The command center never exposes a chat composer, including when empty. Every New task entry point, including the keyboard shortcut, opens task setup. The [ClickUp task creation reference on Mobbin](https://mobbin.com/screens/acf62c3a-89ae-4cee-a4cf-121ecbb5bea4) informs its explicit project context, task name, instruction, and final creation action.

Task setup requires a workspace, task name, instruction, and available model. The workspace defaults visibly to the current project and shows its full path. Choosing another workspace in the form does not change global execution context until submission. Model and variant are reviewable before starting. Without a workspace or model, the form offers the existing workspace or model setup actions.

Create and start task switches to the chosen workspace, saves a named session there, and then opens its conversation and sends the instruction. The instruction states the outcome, scope, and constraints; follow-up messages belong to that created session. No run begins before creation succeeds. Saving failures retain the complete form and show an inline error; this flow never falls back to an unsaved session. Duplicate submission and navigation during creation are blocked. Other tasks continue running throughout setup and creation.

Returning to the overview clears the focused session. The conversation composer is available only inside an existing task. Model and variant selection, skills in conversation, cancellation, and provider recovery remain available.

## Conversations and tools

Conversation content uses a centered reading width of 860px. Agent text is unboxed; user messages have a subtle inset surface. Tool activity uses compact disclosure rows, with details available on demand. Do not decorate tool rows with simulated window controls, terminal prompts, large colored borders, or glow effects.

Previews prefer a pending approval title, then a currently calling tool, then public response text. They never show reasoning or raw tool arguments. Session records loaded without messages show an honest continuation prompt until opened. Idle does not imply task completion: there is no persisted completion state in the current session model.

Settings, provider configuration, archive operations, approval cards, and command search reuse the same semantic foundations. Retain their validation and error recovery behavior.

## Interaction and accessibility

Session switching and command search opening are instant. Historical messages do not replay entrance animations. Use brief transform feedback on occasional pointer presses, with no layout animation during task updates. Existing reduced-motion rules disable movement and transitions.

Every control has a visible keyboard focus state. State includes text as well as color. Native select and search controls remain keyboard operable. Important task previews use secondary text, not decorative low-contrast text.

Below 1000px, task navigation becomes an overlay, starts closed, traps keyboard focus while open, and closes with Escape or its scrim. Selecting a task closes navigation. At narrow widths, task rows stack workspace and status below the task content, and the secondary monitor is hidden. The task rail and command center continue to expose background activity.

## Boundaries

No new task database, scheduling, fabricated progress percentages, or additional dependencies. The design uses the existing session manager, activity derivation, workspace provider, and streaming events. Archive behavior and persisted conversation data are unchanged.
