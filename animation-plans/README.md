# Animation improvement plans

Implementation specifications produced by read-only motion audits. Each plan is
self-contained: an executor with no prior context should be able to run it
without making taste decisions.

Plans 001-005 came from the audit at commit `683c4c1` and have all been applied.
Plans 006-010 come from a second audit at commit `9df8439`.

| # | Plan | Severity | Status | Dependencies |
| --- | --- | --- | --- | --- |
| 001 | [Quiet the production status motion](001-quiet-production-status-motion.md) | HIGH | DONE | None |
| 002 | [Narrow the shared button transitions](002-narrow-shared-button-transitions.md) | HIGH | DONE | None |
| 003 | [Move review progress to transforms](003-move-review-progress-to-transforms.md) | HIGH | DONE | After 001 |
| 004 | [Make palette navigation instant](004-make-palette-navigation-instant.md) | HIGH | DONE | None |
| 005 | [Make reversible disclosures interruptible](005-make-disclosures-interruptible.md) | MEDIUM | DONE | After 003 |
| 006 | [Remove the command palette open animation](006-remove-palette-open-animation.md) | HIGH | TODO | None |
| 007 | [Stop animating layout on the activity disclosure](007-stop-animating-disclosure-layout.md) | HIGH | TODO | After 005 (landed) |
| 008 | [Coalesce the splitter drag off the React commit path](008-coalesce-splitter-drag.md) | HIGH | TODO | None |
| 009 | [Give toasts an interruptible enter and a real exit](009-toast-enter-exit-lifecycle.md) | MEDIUM | TODO | None |
| 010 | [Make reduced motion gentler, not zero](010-reduced-motion-gentler-not-zero.md) | MEDIUM | TODO | **Blocked by 006 and 009** |

## Recommended execution order

Run 006-010 in numeric order. The order is chosen so that the global
reduced-motion policy is written last, over a settled motion surface.

1. **006 — Remove the command palette open animation.** Smallest, highest
   leverage, CSS-only. Also deletes the `palette-enter` keyframe that plan 010
   would otherwise have to special-case.
2. **007 — Stop animating layout on the activity disclosure.** CSS plus one
   `DESIGN.md` line. Removes the largest per-interaction layout cost in the chat
   stream.
3. **008 — Coalesce the splitter drag.** The only TypeScript-heavy plan in this
   batch and the only one that touches `WorkbenchLayout.tsx` /
   `ConstellationCanvas.tsx`, so it can run in parallel with the CSS plans if
   needed.
4. **009 — Toast enter/exit lifecycle.** Introduces `@starting-style` and the
   `.toast-item` transition, and deletes the `toast-in` keyframe that plan 010
   would otherwise have to special-case.
5. **010 — Reduced motion, gentler not zero.** Must run last. It rewrites the
   global `prefers-reduced-motion` block and its boundaries explicitly assume
   `palette-enter` (006) and `toast-in` (009) are already gone.

## Dependency and execution notes

- **010 is hard-blocked by 006 and 009.** Its Boundaries section instructs the
  executor to stop if `palette-enter` or `toast-in` still exist.
- Plans 006, 007, 009, and 010 all touch `src/styles/global.css`. Execute them
  sequentially rather than in parallel to avoid merge conflicts.
- Plan 008 is the only plan touching `src/components/WorkbenchLayout.tsx` and
  `src/components/ConstellationCanvas.tsx`, and the only one that does not touch
  `global.css`, so it is safe to run concurrently with the others.
- Plan 007 is the only plan that edits `DESIGN.md` (one bullet in `## Motion`,
  resolving a contradiction between that document's general "never animate
  layout properties" rule and its specific "Action card expand: max-height
  transition" bullet).
- Plans 008 and 010 are the only ones expected to require test edits. 010's
  required edit is spelled out verbatim in its step 6; 008's is conditional and
  its verification section says to stop rather than weaken an assertion.
- Each executor must drift-check the stamped commit before editing. If a cited
  excerpt no longer matches, stop and report rather than improvising.
- After a plan lands, update its `Status` here and in the plan file from `TODO`
  to `DONE`.

## Audited and deliberately not planned

From the `9df8439` audit, these were confirmed as findings but not selected for
this batch. They remain valid if picked up later.

| Severity | Location | Finding |
| --- | --- | --- |
| MEDIUM | `WorkspaceSwitcher.tsx:46-57`, `InputBar.tsx:377,466`, `SlashCommandMenu.tsx:49,72,90` | Three trigger-anchored dropdowns teleport in with no entrance and no `transform-origin` |
| MEDIUM | No `:active` rule anywhere in `src/styles/` | Only `ui/button.tsx` has press feedback; every raw `<button>` is `transition-colors` only |
| MEDIUM | `global.css:2002-2013` + `ConstellationCanvas.tsx:239` | 600×600 `filter: blur(20px)` layer repositioned by `left`/`top` instead of `transform` |
| MEDIUM | `global.css:88-95` | The seven `--animate-*` utilities hand-type the curve and duration instead of referencing the tokens; `fade-in` runs at two different curves |
| MEDIUM | `global.css:371, 387, 757` | `backdrop-filter` sits on the elements being transformed/faded, so the backdrop re-samples every frame (needs a WebKit profile to size the win) |
| LOW | `global.css:1912, 2577` | Progress fills use `ease-in-out` where constant progress motion wants `linear` |
| LOW | `global.css:282-299, 1388-1404, 1446-1455, 95, 1097` | Dead motion: unused `.chat-column` transition, three consumerless keyframes, `animate-pulse` on a `display: none` element |
| LOW | `McpSettingsPanel.tsx:690` | `transition-transform` with no duration or easing |

Three missed opportunities were also identified: the involuntary Reviewers-tab
swap (`WorkbenchLayout.tsx:73-79`), the instant session/workspace stream
replacement (`AppShell.tsx:892-896`), and the Settings modal's teleport exit
(`SettingsModal.tsx:109`).

Findings explicitly **rejected** during vetting, because `DESIGN.md`'s `## Motion`
section documents them as deliberate: the agent-thinking pulse, the message and
turn entrance motion, and the disclosure's staged content fade. Also confirmed
clean across the codebase: no `ease-in`, no `transition: all`, no `scale(0)`, no
ungated `:hover` movement, no parent-variable-driven child transforms, and no
animation rAF loops.
