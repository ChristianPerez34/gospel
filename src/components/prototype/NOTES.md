# UI Prototype: Settings Page Overhaul

> **Question:** How should Gospel's settings surface be overhauled to radically elevate UX (visual hierarchy & ergonomics), DX (developer tools & local configuration), and AX (Agent Experience & harness transparency)?
> **Outcome:** Variant D shipped as the settings surface (`src/components/SettingsStudio.tsx`). Variants A/B/C and the prototype switcher were removed after evaluation; this file remains as the design record.

---

## 1. Research: Current Look & Feel Analysis

### Baseline Architecture
- **Surface**: Centered modal dialog (`w-[680px] max-w-[92vw] max-h-[84vh]`) with a dark scrim backdrop.
- **Navigation**: Flat horizontal tab strip (`General`, `Models`, `MCP`, `Data`) with `border-b-2` indicator lines.
- **Content**:
  - `General`: Theme preference toggle only (`Dark`, `Light`, `System`).
  - `Models`: `Credentialed Providers` header, single "Refresh models" button, and the vertical `ProviderSelector` list.
  - `MCP`: `McpSettingsPanel` containing custom server forms, environment variables, secret headers, and OAuth buttons squeezed into the 680px modal.
  - `Data`: Global and per-workspace archive retention sliders plus a maintenance button.

### Friction & Limitations
1. **UX Bottlenecks**:
   - The 680px modal dialog is cramped for dense developer tooling. Forms like MCP server configuration and JSON imports feel claustrophobic and force nested vertical scrolling.
   - Horizontal tabs don't scale as settings grow; modal blocks background awareness of running agent tasks.
   - No unified search or filter across settings categories.
2. **DX (Developer Experience) Deficits**:
   - **Autonomy Policy (ADR-0014) is completely missing from Settings**: Developers cannot view or modify Ask vs Auto mode, allowed patterns, or User Deny Rules from the settings page.
   - No diagnostic visibility into local config file paths (`~/.config/gospel/credentials.json`), network proxy states, or direct CLI execution flags.
   - Zero live latency testing or health pings for configured model providers.
3. **AX (Agent Experience) Blind Spots**:
   - The agent harness is completely invisible. Developers cannot see which discovered skills are loaded (`.agents/skills/` vs `~/.agents/skills/`), which prompt tokens are consumed, or how prompt match scoring works.
   - No inspection of agent run guards: Loop detection thresholds (3 warnings, 5 controlled stop) and post-response Verification Agent policies are opaque.
   - No way to preview or simulate how a policy or skill change impacts the agent's actual system preamble before running an inference turn.

---

## 2. The Three Alternative Designs

### Variant A: Inset Studio Workbench (Linear / IDE Studio)
- **Concept**: Replaces the cramped 680px floating popover with a widescreen master-detail workbench (matching Linear and IDE preferences) that naturally fits Gospel's Inset Canvas design language.
- **Structure**:
  - **Left Navigation Rail (260px)**: Fast filter search (`Filter settings...`), category list with live badge indicators (`Healthy`, `ASK Mode`, `4 Active`, `14.2 MB`), and system health status.
  - **Center Canvas**: High-density configuration panels for Autonomy Policy, Provider Management, Theme, MCP, and Storage.
  - **Right Live Sidecar ("Agent Impact Inspector", 290px)**: A real-time telemetry panel showing how the selected setting modifies the agent's system preamble, turn overhead (token count), and active tool capabilities.
- **Elevations**:
  - **UX**: Spacious multi-column layout, persistent search, clear status badges, no nested modal claustrophobia.
  - **DX**: Direct rule editor for Autonomy policies (add/remove shell deny patterns) and clear file path attribution.
  - **AX**: Real-time visibility into the exact system preamble clauses and token overhead injected into the agent's runtime.

### Variant B: Unified Command & Control Matrix (Dataflow / HUD Density)
- **Concept**: Engineered like a Datadog/Raycast engineering HUD or terminal control center for developers who value maximum information density and zero-latency diagnostics.
- **Structure**:
  - **Top Visual Pipeline Ribbon**: A horizontal dataflow graph showing the full execution chain:  
    `[01. Credentials]` ➔ `[02. Router]` ➔ `[03. Live Tools & MCP]` ➔ `[04. Autonomy Policy]` ➔ `[05. Storage & Traces]`  
    Clicking any node instantly filters to that specific tier.
  - **High-Density Tabular Matrices**:
    - **Provider Benchmark Table**: Lists all providers with active model counts, credential state, and a 1-click live "Ping" button measuring round-trip latency in milliseconds.
    - **Tool & Shell Permission Matrix**: Dual tables for auto-approved workspace commands vs blocked/denied patterns.
    - **Storage Gauge**: Visual capacity bar for trace log storage (14.2 MB / 250 MB cap).
  - **Raw JSON / CLI Mode**: One-click toggle between graphical UI and the raw configuration projection (`~/.config/gospel/credentials.json`).
- **Elevations**:
  - **UX**: Comprehensive bird's-eye view of all subsystems on a single screen without deep paging.
  - **DX**: Instant provider latency benchmarks, 1-click ping tests, and raw JSON export/inspection.
  - **AX**: Clear mapping of where agent guardrails sit along the execution pipeline before inference or tool dispatch.

### Variant C: Dual-Persona Adaptive Sheet (Split Lens & Harness Simulator)
- **Concept**: Solves the fundamental tension in an AI development environment by explicitly separating human developer preferences from agent runtime instructions, supplemented by an interactive sandbox.
- **Structure**:
  - **Top 3-Way Lens Switcher**:
    - `[👤 Developer Environment]`: Theme, workspace paths, archive retention, and local API keys.
    - `[🤖 Agent Harness & Safety]`: Autonomy mode, loop detection thresholds (3/5), verification sub-agent toggles, and skill match rules.
    - `[⚡ Harness Sandbox Simulator]`: Interactive testing environment.
  - **Harness Sandbox Simulator (Split Screen)**:
    - **Left - Live Command Classifier**: Type any command (e.g., `rm -rf dist`, `git push --force`, `cargo test`) to see how the Gospel policy engine classifies it (Allowed, Prompt Required, or Blocked by Deny Rule).
    - **Left - Skill Match Tester**: Type a sample user prompt to test the fuzzy token matcher against discovered skills in real time.
    - **Right - Live Compiled Preamble**: Real-time compiled view of the exact system prompt injected into the LLM.
- **Elevations**:
  - **UX**: Clean separation of concerns eliminates confusion between human settings and agent configuration.
  - **DX**: Immediate interactive feedback on command policies without having to start a session or risk running dangerous commands.
  - **AX**: The ultimate harness playground: test prompt matchers, verify system preambles, and observe safety boundaries before runtime.

---

### Variant D: Studio Harness (The A + C Synthesis Iteration — Mobbin & Emil Kowalski Enhanced)
* **File**: [SettingsStudio.tsx](file:///Users/lifelinelogics/RustProjects/gospel/src/components/SettingsStudio.tsx)
* **Concept**: Fuses Variant A’s spacious master-detail studio layout with Variant C’s persona clarity and interactive simulator, refined using real UI pattern research from **Mobbin / Refero** (analyzing Linear, Anthropic Console, Raycast, Resend, and Parallel) and tuned with **Emil Kowalski's Design Engineering craft philosophy**.
* **Key Craft Enhancements (Emil Kowalski Polish)**:
  * **Border Fatigue Elimination**: Stripped all heavy 1px wireframe borders from inner cards, inputs, buttons, and preset chips. Hierarchy is established through Gospel's **Inset Canvas tonal elevation** (`#0b0c0f` sunken base ➔ `#131418` canvas ➔ `#1a1b21` elevated cards with soft ambient shadow `shadow-[0_2px_12px_rgba(0,0,0,0.25)]`). Hairline borders are reduced to whisper-quiet 3–7% opacity.
  * **Adaptive & Collapsible Sidebars ("Focus Mode")**: Neither sidebar is forced on screen at all times:
    * **Left Navigation Rail**: Collapsible from 240px master-detail list to a 58px icon rail (toggle via `[` or header icon), expanding canvas width on demand.
    * **Right Agent Inspector Sidecar**: Slideable 310px live tester that can be toggled on/off via `]` or `⌘J`. When closed, a compact status badge in the header shows live autonomy status (`Auto · 2 Skills`), keeping the main canvas uncluttered.
  * **Micro-interactions & Tactility**:
    * `:active` press scale feedback (`active:scale-[0.97] transition-all duration-150 ease-out`) across all buttons, filter tabs, preset chips, and toggles.
    * Smooth width and fade transitions (`transition-[width] duration-200 ease-out`).
* **Mobbin Design Patterns Applied**:
  * **Linear Breadcrumb Navigation**: Replaced arbitrary modal headers with Linear's clean breadcrumb: `Gospel › Settings Lens › {Category}`, paired with a live Git branch badge (`⎇ main · clean`).
  * **Anthropic Console & Parallel API Key Controls**: Masked secrets (`sk-ant-••••••••01`), interactive `Eye`/`EyeOff` toggle, 1-click clipboard copy, and inline key update forms directly on provider cards.
  * **Raycast-Style Quick Chips**: 1-click test chips for both shell commands (`rm -rf dist`, `git push --force`, `cargo check`) and prompt matchers (`Diagnose bug`, `Prototype UI`, `TDD`), plus 1-click preset additions for common deny rules (`+ rm -rf *`, `+ git push *--force*`, `+ drop table*`).
  * **Linear & Resend Integrations**: MCP server cards with connection status indicators, active tool count badges, toggle switches, and expandable tool capability lists.
  * **Raycast Sticky Bottom Shortcut Bar**: Anchored status bar with breadcrumb context, keyboard shortcuts (`[ [ ] Nav Rail`, `[ ] ] Inspector`, `[⌘F] Search`, `[1-3] Lenses`, `[↵] Apply`, `[Esc] Close`), and live engine sync status (`● Synced · 14ms latency`).
* **Elevates**:
  * **UX**: Seamless focus mode with collapsible sidebars, calm border-free tonal surfaces, and responsive micro-interactions.
  * **DX**: Immediate, zero-risk verification of command safety rules while editing policies; masked key management and quick keyboard shortcuts (`[`, `]`, `⌘J`, `1-3`).
  * **AX**: Full visibility into prompt token impacts, system preambles, and tool dispatch permissions without cluttering the screen during developer tasks.

---

## 3. Comparison Matrix

| Criteria | Baseline (Modal) | Variant A (Studio) | Variant B (Matrix) | Variant C (Dual Lens) | Variant D (A+C Synthesis) |
|---|---|---|---|---|---|
| **Width & Space** | Cramped (680px) | Spacious (1040px) | Wide HUD (1100px) | Focused (1020px) | Balanced (1120px) |
| **Persona Filtering** | ❌ None | ❌ Flat list | ⚠️ Flow steps | ✅ Full split tabs | ✅ Unified Studio + Filter |
| **Autonomy Control** | ❌ None | ✅ Direct rule builder | ✅ Inline toggle | ✅ Full policy cards | ✅ Mode cards + Rule editor |
| **Agent Transparency** | ❌ Invisible | ✅ Passive Sidecar | ✅ Flow Graph | ✅ Sandbox & Preamble | ✅ Active Sidecar + Sandbox |
| **Interactive Tester** | ❌ None | ❌ None | ⚠️ Ping only | ✅ Separate view | ✅ Persistent sidecar + Full view |
| **Ideal Persona** | Minimal config | Studio developer | Systems architect | Prompt engineer | The Complete Developer-Agent Suite |

---

## 4. How to Test & Evaluate

1. Start Gospel dev server: `bun run dev`
2. Open settings via the **Settings** gear in the Top Bar — Variant D (Studio Harness) is the shipped surface.
3. Toggle the navigation rail with `[` and the Agent Inspector with `]` or `⌘J`.
