# 003 — Move review progress to transforms

- **Status**: DONE
- **Commit**: 683c4c1
- **Severity**: HIGH
- **Category**: Easing & duration / Performance
- **Estimated scope**: 4 files, about 20 lines

## Problem

Both live review progress surfaces write progress into `width` and transition
that layout property for 600ms. The update is slower than the 300ms UI budget,
forces layout/paint, and uses entrance-oriented ease-out for an element that is
morphing while already on screen.

```tsx
// src/components/ReviewerPanelCard.tsx:66 — current
<div className="reviewer-panel-progress-track">
  <div
    className="reviewer-panel-progress-fill"
    style={{ width: `${r.progress * 100}%`, background: color }}
  />
</div>
```

```tsx
// src/components/ConstellationCanvas.tsx:664 — current
<div className="constellation-pop-progress">
  <div
    className="constellation-pop-progress-fill"
    style={{ width: `${r.progress * 100}%`, background: color }}
  />
</div>
```

```css
/* src/styles/global.css:1867 and 2495 — current */
.reviewer-panel-progress-fill {
  height: 100%;
  transition: width 600ms var(--ease-out-quart);
}

.constellation-pop-progress-fill {
  height: 100%;
  transition: width 600ms var(--ease-out-quart);
}
```

## Target

Add one shared strong ease-in-out token for on-screen movement:

```css
/* src/styles/tokens.css — target */
--gospel-ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);
--ease-in-out: var(--gospel-ease-in-out);
```

Expose the same semantic alias beside the current motion aliases in
`src/styles/global.css`:

```css
/* src/styles/global.css @theme — target */
--ease-out-quart: var(--gospel-ease-out-quart);
--ease-in-out: var(--gospel-ease-in-out);
--duration-fast: var(--gospel-duration-fast);
```

Each fill owns the track's full width and scales from the left:

```css
/* target for both progress-fill selectors */
height: 100%;
width: 100%;
transform-origin: left center;
transition: transform var(--duration-normal) var(--ease-in-out);
```

Set transform directly on each fill, not through a parent CSS variable:

```tsx
// target for both React progress fills
style={{ transform: `scaleX(${r.progress})`, background: color }}
```

The exact duration is 250ms via `--duration-normal`. The exact curve is
`cubic-bezier(0.77, 0, 0.175, 1)`.

## Repo conventions to follow

- Primitive motion values live in `src/styles/tokens.css`; semantic aliases are
  grouped beside `--ease-out-quart`.
- `src/styles/global.css:63-67` exposes token aliases to Tailwind/CSS.
- `src/styles/global.css:403-409` demonstrates transform-based state motion with
  a shared duration and curve.
- The progress tracks already have `overflow: hidden`; retain it so the
  full-width fill is clipped correctly.
- Reduced motion sets shared durations to `0ms`; using
  `var(--duration-normal)` preserves that policy automatically.

## Steps

1. In `src/styles/tokens.css`, add
   `--gospel-ease-in-out: cubic-bezier(0.77, 0, 0.175, 1);` immediately after
   `--gospel-ease-out-quart`, then add
   `--ease-in-out: var(--gospel-ease-in-out);` beside the semantic easing alias.
2. In the `@theme` motion aliases in `src/styles/global.css`, expose
   `--ease-in-out: var(--gospel-ease-in-out);`.
3. Change `.reviewer-panel-progress-fill` to full width, left-center transform
   origin, and a 250ms transform transition using `var(--ease-in-out)`.
4. Make the identical CSS change to `.constellation-pop-progress-fill`.
5. In `src/components/ReviewerPanelCard.tsx`, replace the inline width
   percentage with `transform: \`scaleX(${r.progress})\``.
6. In `src/components/ConstellationCanvas.tsx`, make the same inline transform
   change to the reviewer popover progress fill.
7. Do not round or reinterpret `r.progress`; preserve the existing 0-to-1
   progress value.

## Boundaries

- Do NOT animate `width`, `height`, `left`, or another layout property.
- Do NOT use a CSS variable on a parent to drive the child transform.
- Do NOT change progress calculation in `src/hooks/useConstellation.ts`.
- Do NOT alter track size, colors, reviewer status, or review event handling.
- Do NOT replace the curve with the built-in `ease-in-out`; use exactly
  `cubic-bezier(0.77, 0, 0.175, 1)`.
- Do NOT add dependencies.
- If progress is no longer normalized to 0-1 since commit 683c4c1, STOP and
  report the drift instead of inventing a clamp.

## Verification

- **Mechanical**:
  - `rg -n "transition:\\s*width 600ms|style=\\{\\{ width:.*progress" src`
    returns no progress-motion matches.
  - `rg -n "gospel-ease-in-out|--ease-in-out" src/styles/tokens.css src/styles/global.css`
    shows the primitive and both aliases.
  - `bun run typecheck` exits 0.
  - `bun run test` exits 0.
- **Feel check**: run a review that emits several progress updates:
  - both the reviewer card and constellation popover move for exactly 250ms;
  - the fill grows from its left edge and never appears to resize from center;
  - a new update arriving mid-transition retargets from the current scale;
  - at 10% DevTools playback, the fill does not overshoot or expose its
    full-width box outside the track;
  - the Performance panel shows transform/composite work, not repeated layout;
  - with `prefers-reduced-motion: reduce`, progress snaps to each new value.
- **Done when**: the two progress surfaces remain visually synchronized, use
  only transform motion, and never take longer than 250ms to catch up.
