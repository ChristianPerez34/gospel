# Gospel — Design System

> Desktop agent harness for coding tasks. Chat-first, workspace-native, built for solo developers who think in terminals.

## Scene sentence

A developer at 2am in a dark room, headphones on, deep in a codebase. Their screen is the only light. They need the agent to feel like a trusted pair of hands, not a chatbot: fast, quiet, precise, with no decorative noise between intent and result.

Dark is forced by the scene. The interface earns every pixel of brightness.

## Pages

### 1. Splash Window

First launch. Minimal: app logo, version string, loading indicator. Disappears once workspace index loads. Never decorative.

- Logo mark centered on `--surface-base`
- Version number in `--text-muted`, `--text-caption`
- Loading: single thin progress bar, `--accent-action` fill, no spinner
- On first launch: transitions to Onboarding. On subsequent launches: transitions to Chat View (last active workspace and session)

### 2. Onboarding (first-run only)

Short setup wizard. Three steps maximum. No marketing, no illustration, no celebration screens.

**Step 1: Workspace**
- Name input (defaults to directory name)
- Path picker (folder selection native dialog)
- Optional: `.gospel` config file auto-detection notice

**Step 2: Model configuration**
- Provider dropdown (OpenAI, Anthropic, local, etc.)
- API key input (password field, with reveal toggle)
- Model selector (populated based on provider)
- Test connection button with inline success/failure

**Step 3: Preferences**
- Theme toggle (dark default, light available)
- Shell preference (detected, overridable)
- Shortcuts teaser ("Customize shortcuts anytime in Settings")

Each step uses a single-column form, left-aligned labels, `--surface-elevated` card on `--surface-base`. No sidebar, no progress bar visible. A subtle step count ("2 of 3") in `--text-muted` at the bottom right.

### 3. Chat View (primary workspace)

The center of the product. Full-screen chat where the user types a prompt and the agent responds. No permanent sidebar; secondary panels are overlays or togglable drawers.

**Layout:**
- **Top bar** (48px): Current workspace name (clickable to switch), session title (editable inline), model badge pill, status indicator (idle/thinking/acting/error), overflow menu (`...`)
- **Message stream** (flex): Full-width, scrollable. User messages right-aligned with `--surface-elevated` background. Agent messages left-aligned on `--surface-base`. Each message is a block: header (avatar + name + timestamp), body (markdown rendered), footer (action buttons: copy, retry, fork).
- **Agent action cards**: When the agent reads files, runs commands, or edits code, show a collapsible card with:
  - Type icon (file, terminal, diff, search)
  - Summary line (e.g., "Edited 3 files" or "Ran `cargo test`")
  - Expand/collapse toggle
  - Expanded view: inline diff or terminal output with syntax highlighting
- **Error states**: Agent errors render inline as `--status-error` bordered cards with the error message, a retry button, and a "copy error" action. No red screens, no modals.
- **Empty state** (new session): Centered prompt, no illustration. Workspace path shown in `--text-muted`. Suggested starter prompts as ghost-text in the input, not as buttons.
- **Input area** (bottom, 56px min): Multi-line textarea that grows to 200px max. Left: model selector dropdown. Right: send button (`--accent-action`). Supports Shift+Enter for newlines, Enter to send. Context pills shown above input showing what files/directories the agent is aware of.

**Agent streaming:**
- While the agent is thinking: status bar shows "Thinking..." in `--accent-action` with a subtle pulse animation
- While the agent is acting: status bar shows the current action ("Reading src/main.rs...", "Running tests...", "Editing Cargo.toml")
- Action cards appear inline as they happen, each animating in from 0 height

**Session history drawer** (togglable, slides from left, 280px):
- List of sessions grouped by date (Today, Yesterday, This Week, Older)
- Each session: title (auto-generated from first message or edited), time, model badge
- Click to load. Right-click for rename/delete.
- Search bar at top for filtering sessions by content
- New session button at bottom (`+` icon)

**Workspace switcher** (dropdown from top bar):
- List of workspaces with name and path
- Active workspace highlighted with `--accent-action` left border
- Add workspace button at bottom
- Each workspace shows: name, path (truncated), session count

### 4. Diff / Code Review Panel

Slides in from the right (480px default, resizable). Shows code changes proposed by the agent.

**Layout:**
- **Header**: File path breadcrumb, file stats (lines added/removed), accept/reject buttons
- **Diff view**: Side-by-side or unified toggle. Syntax highlighting using `--code-*` tokens. Added lines in `--status-success` tint. Removed lines in `--status-error` tint. Unchanged lines in `--text-muted`
- **File tabs** (if multiple files changed): Horizontal tab bar below header, scrollable
- **Actions**: Accept All, Reject All, Accept Current File, Reject Current File. Primary CTAs use `--accent-action`; destructive uses `--status-error`

When the agent proposes changes, the diff panel opens automatically. The user can continue chatting while reviewing. Actions in the diff panel update the agent's context.

### 5. File Context Panel

Slides in from the right (320px). Shows what files and directories the agent currently has loaded in context.

- **File tree**: Collapsible tree rooted at the workspace directory. Files in context highlighted with `--accent-action` dot. Files not in context in `--text-muted`.
- **Context weight**: Each file shows its approximate token count. Total context usage shown at top as a thin bar (`--surface-elevated` track, `--accent-action` fill).
- **Actions**: Add file (via path input or file picker), remove from context, refresh file content
- This panel is informational. Adding/removing files adjusts the agent's context for subsequent messages.

### 6. Session History View

Full-page view (navigated to from session drawer's "See all sessions" link).

- **Search bar** at top, full-width
- **Filter pills**: By model, by date range, by status (has errors, has diffs, completed)
- **Session list**: Virtualized for performance. Each row: session title, preview of first message, model badge, timestamp, status dot (idle/active/error), file count
- **Batch actions**: Select multiple sessions, delete or export

### 7. Settings

Separate window or modal, not a drawer. Structured with a left tab navigation.

**Tabs:**

**General**
- Theme: Dark/Light/System toggle
- Font size: Slider (12-20px, default 14)
- Font family: Dropdown (system, mono options)
- Shell path: Input with auto-detect
- Default model: Dropdown populated from configured providers

**Models**
- List of configured providers with their models
- Each provider card: name, API key (masked), status dot (connected/error), model list
- Add provider button
- Test connection per provider
- Model parameters: temperature slider (0-2, default 0), max tokens input, context window display

**Workspaces**
- List of all workspaces
- Each: name, path, session count, last active
- Edit (rename/resize path), remove, archive
- Add workspace button

**Shortcuts**
- Full keyboard shortcut remapping table
- Search/filter input
- Reset to defaults button
- Categories: Navigation, Chat, Agent, Panels

**Advanced**
- Agent config: system prompt override (textarea), allowed commands (comma-separated), auto-approve threshold (ask/always/never for file edits, command runs)
- Context: max context tokens, automatic file inclusion rules
- Logging: log level dropdown, log file path
- Data: export conversations, clear all data, cache location

### 8. Multi-Agent View (progressive, power feature)

Visible only when multi-agent is enabled (settings toggle or when a second agent is spawned). Replaces the single chat stream with a dashboard.

**Layout:**
- **Agent cards** (grid, 2-column): Each card shows agent name/id, current task summary, status (idle/thinking/acting/error), model badge, session link. Cards use `--surface-elevated` with left border color-coded by agent.
- **Activity feed** (below cards): Chronological log of all agent actions across sessions, filterable by agent
- **Drill-in**: Click a card to open that agent's session in the Chat View. The workspace switcher also shows which agent is active per session.

Each agent gets a distinct `--agent-*` color so the user can track them visually. Supported agent colors: `--agent-cyan`, `--agent-violet`, `--agent-amber`, `--agent-rose`.

### 9. Error and Status States

**Agent error**: Inline card in the chat stream. `--status-error` left border (2px, not a side-stripe). Error message in `--text-primary`. Action buttons: Retry, Copy Error, Report (opens GitHub issue template). No red background, no full-screen error.

**Connection error**: Top bar shows a yellow warning bar ("Connection lost. Retrying..."). Chat input is disabled. Auto-reconnect with exponential backoff. Success removes bar with a 2-second fade.

**Rate limit**: Status indicator in top bar turns `--status-warning` with tooltip showing rate limit details. Chat input shows remaining time estimate.

**Empty workspace**: Centered text on `--surface-base`. "Open a workspace to get started." with a primary button "Open Directory" that triggers native file picker.

**Empty session**: The Chat View empty state (described above). No illustration, no empty-state graphic.

**Loading states**: Agent thinking shows a thin `--accent-action` pulse bar at the top of the message area. File loads show skeleton lines in `--surface-elevated`. No spinners except initial app load.

### 10. Command Palette (Cmd+K / Ctrl+K)

Global command palette overlay. Appears at center-top of the window, entering from top with ease-out-quart.

- **Search input** at top
- **Result groups**: Sessions, Files, Settings, Commands (switch workspace, new session, change model, toggle panel, etc.)
- **Keyboard navigation**: Arrow keys, Enter to select, Esc to dismiss
- **Recent items** shown when search is empty
- Background dims with `--scrim` overlay

## Color

OKLCH. Full palette strategy: 4 named roles, each deliberate.

### Palette

| Token | OKLCH | Hex (approx) | Role |
|-------|-------|-------------|------|
| `--surface-base` | oklch(0.13 0.005 260) | #131318 | Page background, deepest layer |
| `--surface-elevated` | oklch(0.18 0.006 260) | #1e1e24 | Cards, panels, elevated surfaces |
| `--surface-overlay` | oklch(0.22 0.007 260) | #28282f | Hover states, inline blocks |
| `--scrim` | oklch(0.10 0.000 260 / 0.60) | rgba(0,0,0,0.6) | Overlay dimming |

| Token | OKLCH | Hex (approx) | Role |
|-------|-------|-------------|------|
| `--text-primary` | oklch(0.93 0.005 260) | #ececf0 | Headings, primary text |
| `--text-secondary` | oklch(0.70 0.008 260) | #a0a0ab | Body text, descriptions |
| `--text-muted` | oklch(0.50 0.008 260) | #6b6b76 | Timestamps, placeholders, disabled |
| `--text-inverse` | oklch(0.15 0.005 260) | #1a1a1f | Text on bright backgrounds |

| Token | OKLCH | Hex (approx) | Role |
|-------|-------|-------------|------|
| `--accent-action` | oklch(0.75 0.18 175) | #4fe0c0 | Primary interactive element: send button, active states, links, status "thinking" |
| `--accent-structure` | oklch(0.72 0.14 280) | #a78bfa | Structural accent: session highlights, context indicators, file tree active markers |
| `--accent-signal` | oklch(0.78 0.16 75) | #f0b040 | Attention signal: warnings, agent interventions, non-breaking highlights |
| `--accent-data` | oklch(0.70 0.13 30) | #e8705a | Data and diff marker: removed lines, error counts, token consumption |

| Token | OKLCH | Hex (approx) | Role |
|-------|-------|-------------|------|
| `--status-error` | oklch(0.65 0.20 25) | #e5484d | Error states |
| `--status-warning` | oklch(0.75 0.15 85) | #f0b429 | Warnings, rate limits |
| `--status-success` | oklch(0.72 0.19 155) | #46c98a | Success, diff added lines, connected |

| Token | OKLCH | Hex (approx) | Role |
|-------|-------|-------------|------|
| `--agent-cyan` | oklch(0.75 0.12 195) | #3dc9d6 | Agent 1 |
| `--agent-violet` | oklch(0.65 0.18 300) | #9771da | Agent 2 |
| `--agent-amber` | oklch(0.72 0.15 80) | #d49e3e | Agent 3 |
| `--agent-rose` | oklch(0.65 0.18 10) | #db4d68 | Agent 4 |

### Color rules

- Never use pure `#000` or `#fff`. All neutrals are tinted toward the brand hue (260 on the OKLCH hue wheel, a cool blue-violet).
- `--accent-action` carries most interactive weight. Used for exactly: send button, active tab, thinking pulse, links, focus rings.
- `--accent-structure` marks navigational and organizational elements. It colors the session drawer active item, the context file tree active dot, and the workspace switcher highlight.
- `--accent-signal` is for attention without alarm. Agent intervention warnings, cost alerts, non-breaking feedback. Never for primary actions.
- `--accent-data` marks data, deltas, and destructive hints. Diff removed lines, error counts, token consumption bars. Never for primary interactive elements.
- Status colors (`--status-error`, `--status-warning`, `--status-success`) are reserved for their semantic roles. Never repurpose them as decorative fills.
- Agent colors are only used in multi-agent view to visually distinguish agents in cards, activity feeds, and the session drawer. Never in single-agent mode.

## Theme

**Dark is the default.** Light mode is available as a preference but is secondary.

The scene sentence forces dark: a solo developer in extended focus sessions, ambient light low, screen the primary light source. Dark reduces eye strain and lets the accent colors do their work with minimal effort.

Light theme inverts the surface stack but preserves the same accent tokens. Surfaces go from dark to light, but accent hues remain unchanged (they're already high-chroma enough to read on white). Text colors invert proportionally. The palette should feel like the same instrument played in a different register.

### Light mode palette adjustments

| Token | Dark | Light |
|-------|------|-------|
| `--surface-base` | oklch(0.13 0.005 260) | oklch(0.98 0.003 260) |
| `--surface-elevated` | oklch(0.18 0.006 260) | oklch(1.00 0.002 260) |
| `--surface-overlay` | oklch(0.22 0.007 260) | oklch(0.95 0.004 260) |
| `--text-primary` | oklch(0.93 0.005 260) | oklch(0.18 0.010 260) |
| `--text-secondary` | oklch(0.70 0.008 260) | oklch(0.35 0.010 260) |
| `--text-muted` | oklch(0.50 0.008 260) | oklch(0.55 0.008 260) |

## Typography

### Fonts

| Token | Family | Role |
|-------|--------|------|
| `--font-display` | "Matter", "Inter", system-ui | Headings, session titles, workspace names |
| `--font-body` | "Inter", system-ui | Body text, descriptions, form labels, buttons |
| `--font-mono` | "JetBrains Mono", "Fira Code", "Menlo", monospace | Code blocks, terminal output, path strings, tokens |

### Type scale

| Token | Size | Line-height | Letter-spacing | Weight | Usage |
|-------|------|-------------|----------------|--------|-------|
| `--text-display` | 36px | 1.1 | -0.02em | 500 | Onboarding titles, splash logo word |
| `--text-heading-lg` | 24px | 1.2 | -0.01em | 500 | Settings section headings |
| `--text-heading` | 18px | 1.3 | 0 | 500 | Tab labels, panel titles |
| `--text-heading-sm` | 16px | 1.3 | 0 | 500 | Card headers, inline section titles |
| `--text-body` | 14px | 1.5 | 0 | 400 | Primary body text, chat messages |
| `--text-body-sm` | 13px | 1.5 | 0.005em | 400 | Timestamps, secondary info, labels |
| `--text-caption` | 11px | 1.4 | 0.02em | 500 | Badges, keyboard hints, micro labels |
| `--text-mono` | 13px | 1.5 | 0 | 400 | Code inline |
| `--text-mono-lg` | 14px | 1.6 | 0 | 400 | Code blocks, terminal output |

Scale ratio between steps is at least 1.25 for heading levels and 1.08 for body levels. No flat scales.

Line length for body text caps at 65ch. Chat messages render in a centered column of max 720px, regardless of window width.

## Spacing

| Token | Value | Usage |
|-------|-------|-------|
| `--space-1` | 4px | Inline gaps, icon padding |
| `--space-2` | 8px | Tight element spacing |
| `--space-3` | 12px | Compact card padding |
| `--space-4` | 16px | Standard element gap |
| `--space-5` | 24px | Section padding |
| `--space-6` | 32px | Section gap |
| `--space-7` | 48px | Major section separation |
| `--space-8` | 64px | Page-level breathing room |

Vary spacing for rhythm. Same padding everywhere is monotony. Use `--space-4` as default internal padding, `--space-6` between sections, `--space-7` between major views.

## Layout

### Window structure (Tauri desktop app)

- **Title bar**: 36px. Custom, not native. App name left, window controls right (close/minimize/maximize). `--surface-elevated` background.
- **Top bar**: 48px. Fixed.
- **Main content area**: Fills remaining vertical space.
- **Panels**: Overlay-style, slide from right. Not permanent fixtures.
- **Session drawer**: Overlay from left, 280px.

Minimum window size: 800x600. Optimal: 1280x800+.

### Grid

The chat area uses a simple centered column layout:
- Max content width: 720px for message text
- Full width for action cards and diffs (with max-width: 960px)
- Responsive: below 800px width, session drawer becomes a full-screen overlay

### Z-index stack

| Layer | Z-index | Content |
|-------|---------|---------|
| Base | 0 | Chat messages, background |
| Sticky input | 10 | Bottom input bar |
| Side panels | 20 | File context, diff reviewer |
| Drawers | 30 | Session history |
| Dropdowns | 40 | Menus, selects |
| Command palette | 50 | Cmd+K overlay |
| Scrim | 45 | Dim overlay behind palette |
| Toast | 60 | Notifications |
| Dialog | 70 | Confirmations, modals |

## Elevation

No shadows except command palette and modal dialogs. Elevation is communicated through surface color alone.

| Level | Token | Meaning |
|-------|-------|---------|
| 0 | `--surface-base` | Background, chat area |
| 1 | `--surface-elevated` | Cards, panels, input fields |
| 2 | `--surface-overlay` | Hover states, active items |
| 3 | Native OS chrome | Dialogs, native file pickers |

## Shapes

| Token | Radius | Usage |
|-------|--------|-------|
| `--radius-sm` | 4px | Buttons, inputs, inline badges |
| `--radius-md` | 8px | Cards, panels, code blocks |
| `--radius-lg` | 12px | Modal containers, dialog overlays |
| `--radius-full` | 9999px | Status dots, agent color indicators, avatar circles |

No rounded cards inside rounded cards. If a card is `--radius-md`, its children are `--radius-sm`.

## Motion

- **Duration**: 150ms for micro-interactions (hover, focus), 250ms for layout changes (drawer open, panel slide), 400ms for page transitions
- **Easing**: ease-out-quart (`cubic-bezier(0.25, 1, 0.5, 1)`) for entering elements. ease-in-out for exiting elements.
- **No bounce, no elastic, no spring.** Ever.
- **Never animate layout properties** (width, height, top, left). Use transform and opacity only.
- **Agent thinking**: single `--accent-action` pulse bar, 2s cycle, opacity 0.3 to 1.0. No spinner.
- **Message appear**: fade-in 150ms + translateY 8px, ease-out-quart.
- **Action card expand**: max-height transition 250ms, ease-out-quart. Content fades in 150ms after height reaches target.
- **Drawer/panel**: translateX slide 250ms, ease-out-quart. Scrim opacity 200ms.

## Components

### Input bar

Fixed to bottom. Background `--surface-elevated` with 1px `--surface-overlay` top border. Contains:
- Multi-line textarea, `--font-body`, `--text-body` size, placeholder in `--text-muted`
- Model selector dropdown (left, inside input)
- Send button (`--accent-action` background, `--text-inverse` icon), 36x36px
- Context pills scroll row above textarea (if context files are attached)
- Grows vertically from 56px to 256px, then scrolls

### Message block

Each message in the stream:
- **Header**: Avatar (14px circle, agent color or user initial), display name, timestamp (`--text-caption`, `--text-muted`)
- **Body**: Markdown-rendered content. Code blocks use `--surface-overlay` background with `--font-mono-lg`. Inline code uses `--surface-overlay` background with `--radius-sm`.
- **Footer**: Action row (copy, retry, fork) in `--text-muted`, revealed on hover with fade 150ms
- **User message alignment**: right side, `--surface-overlay` background
- **Agent message alignment**: left side, `--surface-base` background

### Action card

Collapsible card inside the agent message stream:
- **Collapsed**: Type icon + summary line. `--surface-elevated` background, `--radius-md`. Left accent: 2px `--accent-action` for reading, `--accent-structure` for editing, `--accent-data` for running commands.
- **Expanded**: Full content below the summary. Terminal output in `--font-mono-lg`. Diffs with syntax highlighting.
- Height: collapsed ~36px. Expanded: natural height, max 400px with scroll.

### Diff viewer

Side-by-side (default) or unified (toggle) code diff:
- File header: filename, `+N/-N` stats in `--status-success`/`--accent-data`
- Added lines: `--status-success` background tint at 10% opacity
- Removed lines: `--accent-data` background tint at 10% opacity
- Line numbers in `--text-muted`
- Action bar: Accept, Reject, Accept All, Reject All

### Command palette

Full-width overlay (max 560px) centered at top of viewport, offset 20% from top:
- Search input: `--font-body`, `--text-body` size, full-width, no border, `--surface-elevated` background
- Results: grouped by type (Sessions, Files, Settings, Commands)
- Each result: icon + primary text + secondary text (path or description) + keyboard shortcut if applicable
- Active result: `--surface-overlay` background
- Keyboard navigation: arrow keys, Enter, Esc

### Settings modal

- 600px wide, centered, `--surface-elevated` background, `--radius-lg`, subtle `--scrim` backdrop
- Left tab navigation: 160px, `--surface-elevated` with active tab using `--accent-action` indicator
- Right content area: scrollable, form fields stacked vertically
- Full-width inputs, `--radius-sm`, `--surface-overlay` background
- Save/Discard actions bottom-right, outside scroll area

### Workspace switcher

Dropdown from top bar workspace name:
- 320px wide, `--surface-elevated`, `--radius-md`
- Each workspace row: Name (`--text-primary`), Path (`--text-muted`, `--font-mono`, truncated), Session count badge
- Active workspace: `--accent-action` left border 2px
- Add workspace button at bottom with `+` icon
- Dismissible by clicking outside or pressing Esc

### Status indicator

Small element (8px dot) in top bar:
- Idle: `--text-muted` (dimmed gray)
- Thinking: `--accent-action` with pulse animation
- Acting: `--accent-structure` solid
- Error: `--status-error` solid
- Connected: `--status-success` solid

### Keyboard shortcut hint

Inline display next to action names:
- `--font-mono`, `--text-caption` size
- Background: `--surface-overlay`, `--radius-sm`, 2px vertical padding, 6px horizontal
- Key combination: `Ctrl+K`, `Cmd+Shift+N`, etc.

### Context pill

Small removable tag above the input bar:
- `--surface-overlay` background, `--radius-md`
- Filename or directory in `--font-mono`, `--text-body-sm`
- Remove icon (x) in `--text-muted`, revealed on hover
- Horizontal scroll if many files

## Responsive behavior

- **1280px+**: Full layout. Panels can be open alongside chat.
- **800-1279px**: Drawer overlays chat when open. Panels still overlay.
- **Below 800px**: Everything overlays. Single column only. Top bar simplifies to icon-only buttons.

## Accessibility

- All interactive elements reachable by keyboard. Tab order follows visual flow.
- Focus rings: `--accent-action` 2px outline, 2px offset. No `outline: none` without replacement.
- Color is never the sole indicator of state. Always pair with icon, text label, or pattern.
- Minimum contrast: `--text-primary` on `--surface-base` exceeds 7:1. `--text-secondary` on `--surface-base` exceeds 4.5:1.
- `--text-muted` is decorative only. Never used for meaningful text alone.
- Reduced motion: all animations and transitions set to `0ms` when `prefers-reduced-motion: reduce`.
- Screen reader: messages are in an ARIA live region. Action cards are collapsible sections with proper heading levels.

## Anti-patterns (banned)

- Side-stripe borders wider than 1px (use 2px only for agent color indicators, never decorative)
- Gradient text (`background-clip: text` with gradient)
- Glassmorphism (blurs, glass cards)
- Hero-metric template (big number + small label + supporting stats)
- Identical card grids (same-sized cards with icon + heading + text)
- Modal as first thought (always prefer inline solutions)
- AI-generated look: no symmetric hero sections, no floating gradient orbs, no stock illustration placeholders

## Dark mode (default)

All tokens above are specified for dark mode. Dark is the canonical theme.

## Light mode

Implemented by swapping the surface and text tokens as specified in the Light mode palette adjustments section. Accent tokens remain unchanged. Code blocks in light mode use `oklch(0.97 0.003 260)` instead of `--surface-overlay` for their background.

## Reference lock

**Primary foundation**: Warp's precision dark workbench, with its Matter-typography-tight, low-chrome, information-dense approach.

**Borrowed details**:
- From Trigger.dev: the multi-accent discipline (spring green, violet, coral, amber each with a role).
- From Factory: the collapsible action card pattern for agent actions (read/edit/run) in a chat stream.

**Preserved traits**: Dark base with tinted neutrals. Four accent roles with strict semantic assignment. Monospace respect. No decorative elements between intent and result.

**Rejected**: Light-first themes, marketing-style landing pages, SaaS cream palettes, gradient hero sections, card-heavy dashboards.