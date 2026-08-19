# 007 — Stop animating layout on the activity disclosure

- **Status**: TODO
- **Commit**: 9df8439
- **Severity**: HIGH
- **Category**: Performance / Easing & duration
- **Estimated scope**: 2 files (1 CSS, 1 doc), about 15 lines changed

## Problem

Expanding a tool activity card is the most frequent non-typing interaction in
the app — every agent turn renders several `ActivityStep` rows and the developer
opens them constantly. That interaction currently animates two non-composited
properties for 250ms each.

**1. `grid-template-rows` is animated on the container.** This is the `0fr → 1fr`
height trick. It is strictly worse than animating `height`: every frame re-runs
grid track sizing on the container and re-lays-out the entire expanded subtree,
which contains `<pre>` blocks and nested sections
(`src/components/ActivityStep.tsx:363-378`).

```css
/* src/styles/global.css:1101-1109 — current */
.activity-step-disclosure {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--duration-normal) var(--ease-out-quart);
}

.activity-step-disclosure[data-open="true"] {
  grid-template-rows: 1fr;
}
```

**2. `border-radius` is animated on the trigger** on the same click, forcing a
repaint of the row including its `!important` box-shadow and the three
`::before` traffic-light radial gradients at `src/styles/global.css:1076-1093`.

```css
/* src/styles/global.css:1060-1070 — current (tail of .activity-step-trigger) */
  position: relative;
  transition: border-radius var(--duration-fast) var(--ease-out-quart);
}
```

**3. The body's reveal is chained 250ms behind the container**, so the full
disclosure does not finish until roughly 400ms.

```css
/* src/styles/global.css:1116-1123 and :1135-1139 — current */
.activity-step-body {
  opacity: 0;
  transform: translateY(8px);
  transition:
    opacity var(--duration-fast) var(--ease-out-quart),
    transform var(--duration-fast) var(--ease-out-quart);
  transition-delay: 0ms;
  /* ...visual declarations... */
}

.activity-step-disclosure[data-open="true"] .activity-step-body {
  opacity: 1;
  transform: translateY(0);
  transition-delay: var(--duration-normal);
}
```

This violates `DESIGN.md`'s own rule, in its `## Motion` section:

> **Never animate layout properties** (width, height, top, left). Use transform
> and opacity only.

`DESIGN.md` also contains a contradicting bullet in the same list:

> **Action card expand**: max-height transition 250ms, ease-out-quart. Content
> fades in 150ms after height reaches target.

That bullet is the source of the 400ms chain and of the height animation. The
two bullets cannot both hold. This plan resolves the contradiction in favour of
the general rule (no layout animation) and updates the specific bullet to match,
because there is no way to animate from zero height to content height without
animating layout.

Note: a prior plan (`animation-plans/005-make-disclosures-interruptible.md`,
DONE) converted this disclosure from one-way keyframes to transitions. That
fixed interruptibility and must be preserved — this plan does not reintroduce
keyframes.

## Target

The container height snaps; only `opacity` and `transform` animate. The
disclosure completes within one 150ms window instead of ~400ms.

```css
/* target */
.activity-step-disclosure {
  display: grid;
  grid-template-rows: 0fr;
}

.activity-step-disclosure[data-open="true"] {
  grid-template-rows: 1fr;
}
```

```css
/* target — tail of .activity-step-trigger */
  position: relative;
}
```

```css
/* target */
.activity-step-body {
  opacity: 0;
  transform: translateY(8px);
  transition:
    opacity var(--duration-fast) var(--ease-out-quart),
    transform var(--duration-fast) var(--ease-out-quart);
  /* ...visual declarations unchanged... */
}

.activity-step-disclosure[data-open="true"] .activity-step-body {
  opacity: 1;
  transform: translateY(0);
}
```

Values, all already existing tokens: `--duration-fast` is `150ms`,
`--ease-out-quart` is `cubic-bezier(0.25, 1, 0.5, 1)` (both defined in
`src/styles/tokens.css:57` and `:59`).

The `grid-template-rows: 0fr` / `1fr` pair stays as the mechanism for
collapsing to zero height — it just stops being *transitioned*, so the height
change happens in a single frame with no per-frame relayout.

## Repo conventions to follow

- All CSS lives in `src/styles/global.css`. Motion tokens come from
  `src/styles/tokens.css` via the `@theme` re-export at
  `src/styles/global.css:62-67`. Use `var(--duration-fast)` and
  `var(--ease-out-quart)`, never literal values.
- Exemplar of the correct pattern already in this file: the chevron rotation at
  `src/components/ActivityStep.tsx:338` animates `transform` only
  (`transition-transform duration-150 ease-out-quart`). The disclosure should
  animate the same channels.
- `.activity-step-disclosure`, `.activity-step-disclosure-clip`,
  `.activity-step-body` and the `data-open` attribute are all asserted by tests
  (`src/components/ActivityStep.test.tsx:89,106,114,126-138,145,155` and
  `src/components/ChatView.test.tsx:124,172`). Class names and the attribute
  must not change.

## Steps

1. In `src/styles/global.css`, in the `.activity-step-trigger` rule, delete the
   line `transition: border-radius var(--duration-fast) var(--ease-out-quart);`
   (line 1069). Keep `position: relative;` and every other declaration,
   including the `[aria-expanded="true"]` radius override at lines 1072-1074 —
   the radius should still change, just instantly.
2. In `src/styles/global.css`, in the `.activity-step-disclosure` rule (line
   1101), delete the line
   `transition: grid-template-rows var(--duration-normal) var(--ease-out-quart);`.
   Keep `display: grid;` and `grid-template-rows: 0fr;`.
3. Leave `.activity-step-disclosure[data-open="true"] { grid-template-rows: 1fr; }`
   (lines 1107-1109) exactly as it is.
4. Leave `.activity-step-disclosure-clip` (lines 1111-1114) exactly as it is —
   `min-height: 0; overflow: hidden;` is what makes the `0fr` collapse clip its
   content.
5. In `src/styles/global.css`, in the `.activity-step-body` rule, delete the
   line `transition-delay: 0ms;` (line 1122). Keep the `opacity`, `transform`,
   and `transition` declarations, and keep every `!important` visual
   declaration below them unchanged.
6. In `src/styles/global.css`, in the
   `.activity-step-disclosure[data-open="true"] .activity-step-body` rule
   (lines 1135-1139), delete the line
   `transition-delay: var(--duration-normal);`. Keep `opacity: 1;` and
   `transform: translateY(0);`.
7. In `DESIGN.md`, in the `## Motion` section, replace the bullet:
   `- **Action card expand**: max-height transition 250ms, ease-out-quart. Content fades in 150ms after height reaches target.`
   with:
   `- **Action card expand**: height snaps in one frame (never animate layout). Content fades in 150ms with translateY 8px, ease-out-quart.`
   This is the only change to `DESIGN.md`; do not reword any other bullet.

## Boundaries

- Do NOT touch `src/components/ActivityStep.tsx`. This is a CSS change plus one
  documentation line.
- Do NOT rename or remove the `.activity-step-disclosure`,
  `.activity-step-disclosure-clip`, or `.activity-step-body` classes, and do NOT
  change the `data-open` attribute — tests depend on all four.
- Do NOT attempt a JS height measurement, a `max-height` animation, a
  `clip-path` reveal, or a `scaleY` reveal. Any of those either reintroduces
  layout animation or distorts the text. The height snap is the intended target.
- Do NOT touch the `.activity-step-dot { display: none !important; }` rule at
  lines 1096-1099 (handled by a separate finding).
- Do NOT change any `!important` visual declaration on `.activity-step-body`.
- Do NOT add new dependencies.
- If a cited excerpt does not match what you find (drift since commit
  `9df8439`), STOP and report rather than improvising.

## Verification

- **Mechanical**:
  - `bun run typecheck` — expected to pass.
  - `bun run lint` — expected to pass.
  - `bun run test` — expected to pass with **no test edits**. The tests assert
    class names, `data-open`, and `aria-hidden`, none of which change. If a test
    fails, STOP and report.
  - `rg -n 'grid-template-rows|border-radius var' src/styles/global.css` —
    should show `grid-template-rows` only as plain declarations (no
    `transition:` line) and no `border-radius` transition.
- **Feel check**: run `bun run dev`, send a prompt that produces tool activity
  (or use an existing session with tool cards), then:
  - Click a tool row. The panel height must appear immediately, with the body
    content fading and sliding up 8px over 150ms on top of it. The whole reveal
    should be done in about a sixth of a second, not a slow unfurl.
  - Confirm the collapsed state still takes zero vertical space — rows must sit
    flush against each other when closed. If a closed row leaves a gap, step 4
    was violated.
  - Spam-click one tool row open and closed rapidly. The body must retarget its
    opacity from wherever it currently is, never jump back to fully transparent
    and restart. (This is the behaviour plan 005 established; confirm it
    survived.)
  - In DevTools Animations panel, set playback speed to 10% and expand a row.
    Only `opacity` and `transform` animations should be recorded — no
    `grid-template-rows`, no `border-radius`.
  - In DevTools Performance panel, record while expanding a row that contains a
    large `<pre>` block. Compare against `main`: the 250ms band of repeated
    "Layout" work should be gone.
  - Toggle `prefers-reduced-motion: reduce` in the DevTools Rendering panel and
    expand a row. Behaviour is currently governed by the global block at
    `src/styles/global.css:139-153`; just confirm nothing throws or renders
    broken. (Reduced-motion policy itself is plan 009.)
- **Done when**: no `transition` in `src/styles/global.css` names
  `grid-template-rows` or `border-radius`, no `transition-delay` remains in the
  `.activity-step-body` rules, `bun run test` passes unmodified, and the
  `DESIGN.md` bullet reads as specified in step 7.
