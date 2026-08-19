# 006 — Remove the command palette open animation

- **Status**: TODO
- **Commit**: 9df8439
- **Severity**: HIGH
- **Category**: Purpose & frequency / Interruptibility
- **Estimated scope**: 1 file, about 12 lines removed

## Problem

The command palette is opened with `Cmd/Ctrl+K` — the single most frequently
used keyboard shortcut in the app. It currently plays a 250ms enter animation
plus a scrim fade every time it opens, so a keyboard action the user performs
dozens of times a day is gated behind a quarter second of motion. Because both
are `@keyframes` on a conditionally mounted element
(`src/components/CommandPalette.tsx:278` is `if (!open) return null;`), rapid
toggling restarts the animation from zero rather than retargeting.

```css
/* src/styles/global.css:735-742 — current */
.command-palette-scrim {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-palette) - 1);
  background: var(--scrim);
  backdrop-filter: blur(3px);
  animation: fade-in var(--duration-fast) var(--ease-out-quart);
}
```

```css
/* src/styles/global.css:762-763 — current (tail of the .command-palette rule) */
  transform: translateX(-50%);
  animation: palette-enter var(--duration-normal) var(--ease-out-quart);
}
```

```css
/* src/styles/global.css:1457-1465 — current */
@keyframes palette-enter {
  from {
    opacity: 0;
    transform: translate(-50%, -8px);
  }
  to {
    opacity: 1;
    transform: translate(-50%, 0);
  }
}
```

The relevant rule: at 100+ uses per day, and specifically for keyboard
shortcuts and command palette toggles, the correct amount of animation is none.
Raycast's palette has no open/close transition, and that is why it feels
instant.

A prior plan (`animation-plans/004-make-palette-navigation-instant.md`, DONE)
removed the per-result color transitions so arrow-key navigation is instant. It
did not touch the palette's own open animation, which is the remaining cost on
the same interaction.

## Target

The palette and its scrim appear on the frame the shortcut is pressed. No
`animation` declaration on either element, and no orphaned keyframe left behind.

```css
/* target */
.command-palette-scrim {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-palette) - 1);
  background: var(--scrim);
  backdrop-filter: blur(3px);
}
```

```css
/* target — tail of the .command-palette rule */
  transform: translateX(-50%);
}
```

`transform: translateX(-50%)` must stay. It is not animation; it is the
horizontal centering for `left: 50%`. Removing it would shift the palette
280px to the right.

`@keyframes palette-enter` has exactly one consumer (the declaration being
deleted), so it becomes dead and is deleted too.

## Repo conventions to follow

- All CSS lives in `src/styles/global.css`; motion tokens are defined in
  `src/styles/tokens.css` and re-exported into the `@theme` block at
  `src/styles/global.css:62-67`. This plan only removes declarations, so no new
  tokens are needed.
- Exemplar of a correctly instant, non-animated overlay in this codebase:
  `.workspace-switcher-dialog` at `src/styles/global.css:687-705` has no
  `animation` and no `transition`. The palette should match that.

## Steps

1. In `src/styles/global.css`, in the `.command-palette-scrim` rule (starts at
   line 735), delete the line:
   `animation: fade-in var(--duration-fast) var(--ease-out-quart);`
   Leave every other declaration in that rule untouched, including
   `backdrop-filter: blur(3px);`.
2. In `src/styles/global.css`, in the `.command-palette` rule (starts at line
   745), delete the line:
   `animation: palette-enter var(--duration-normal) var(--ease-out-quart);`
   **Keep** the preceding `transform: translateX(-50%);` line.
3. In `src/styles/global.css`, delete the entire `@keyframes palette-enter`
   block at lines 1457-1465 (nine lines, from `@keyframes palette-enter {`
   through its closing `}`).
4. Do NOT delete `@keyframes fade-in` (around line 1437). It still has a live
   consumer through the `--animate-fade-in` theme token at
   `src/styles/global.css:94`, which is used by
   `src/components/SettingsModal.tsx:112`.

## Boundaries

- Do NOT touch `src/components/CommandPalette.tsx`. This is a CSS-only change.
- Do NOT touch `@keyframes fade-in`, `--animate-fade-in`, or
  `src/components/SettingsModal.tsx`.
- Do NOT change the palette's position, size, z-index, border, background,
  `backdrop-filter`, or box-shadow. Motion declarations only.
- Do NOT add new dependencies.
- If a cited excerpt does not match what you find (drift since commit
  `9df8439`), STOP and report rather than improvising.

## Verification

- **Mechanical**:
  - `bun run typecheck` — expected to pass (no TypeScript touched).
  - `bun run lint` — expected to pass.
  - `bun run test` — expected to pass unchanged. `src/components/CommandPalette.test.tsx`
    asserts nothing about animation, so no test updates should be needed. If a
    test fails, STOP and report.
  - `rg -n 'palette-enter' src` — expected to return **no matches** after the
    change.
- **Feel check**: run `bun run dev`, then:
  - Press `Cmd+K`. The palette must be fully visible on the first painted frame
    with no slide and no fade. Press `Esc` and `Cmd+K` again rapidly several
    times; there must be no visible re-animation or flicker on any toggle.
  - Confirm the palette is still horizontally centered in the window. If it is
    offset to the right, `transform: translateX(-50%)` was removed by mistake.
  - In DevTools, open the Animations panel and press `Cmd+K`. No animation
    should be recorded for `.command-palette` or `.command-palette-scrim`.
  - Open the Settings modal (it uses `animate-fade-in` / `animate-slide-up`).
    Its entrance must still animate, confirming `@keyframes fade-in` survived.
- **Done when**: `rg -n 'animation:' src/styles/global.css` no longer lists any
  line inside the `.command-palette` or `.command-palette-scrim` rules,
  `palette-enter` appears nowhere in `src/`, `bun run test` passes, and `Cmd+K`
  is visually instant.
