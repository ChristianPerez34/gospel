# 004 — Make palette navigation instant

- **Status**: DONE
- **Commit**: 683c4c1
- **Severity**: HIGH
- **Category**: Purpose & frequency
- **Estimated scope**: 1 file, 3 lines removed

## Problem

The command palette updates `activeIndex` on every ArrowDown or ArrowUp key.
Each change moves `.is-active` to another result, but every result transitions
its color state for 150ms. Repeated key presses therefore make selection
feedback trail behind the keyboard.

```tsx
// src/components/CommandPalette.tsx:298 — current
onKeyDown={(event) => {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    setActiveIndex((current) =>
      Math.min(current + 1, Math.max(filteredResults.length - 1, 0))
    );
    return;
  }
  if (event.key === "ArrowUp") {
    event.preventDefault();
    setActiveIndex((current) => Math.max(current - 1, 0));
    return;
  }
```

```css
/* src/styles/global.css:785 — current */
.command-palette-result {
  display: grid;
  min-height: 44px;
  width: 100%;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--space-2);
  border-radius: var(--radius-sm);
  padding: var(--space-2) var(--space-3);
  text-align: left;
  transition:
    background-color var(--duration-fast) var(--ease-out-quart),
    color var(--duration-fast) var(--ease-out-quart);
}

.command-palette-result:hover,
.command-palette-result.is-active {
  background: color-mix(in srgb, var(--surface-overlay) 82%, var(--surface-elevated) 18%);
}
```

## Target

Palette row feedback is instantaneous for keyboard and pointer navigation.
Remove the transition declaration; preserve every other row style and active
color exactly.

```css
/* src/styles/global.css — target */
.command-palette-result {
  display: grid;
  min-height: 44px;
  width: 100%;
  grid-template-columns: 28px minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--space-2);
  border-radius: var(--radius-sm);
  padding: var(--space-2) var(--space-3);
  text-align: left;
}
```

Do not change the palette surface's documented 250ms entrance. This plan is
only about high-frequency result selection after the palette is open.

## Repo conventions to follow

- The palette is a keyboard-first surface (`Cmd/Ctrl+K`, arrows, Enter, Escape).
- The audit rule for 100+ daily keyboard actions is no animation.
- Preserve `.command-palette-result:hover` and `.is-active` as the single
  shared visual state so mouse and keyboard never disagree.
- `DESIGN.md:171-178` deliberately specifies the palette surface entrance;
  leave `palette-enter` untouched.

## Steps

1. In `src/styles/global.css`, delete only the `transition` declaration from
   `.command-palette-result`.
2. Keep the active/hover background rule and all layout values unchanged.
3. Do not edit `CommandPalette.tsx`; its index and keyboard behavior are already
   correct.

## Boundaries

- Do NOT remove or alter `palette-enter`, `.command-palette`, or
  `.command-palette-scrim`.
- Do NOT add input-modality state, JavaScript timers, or a replacement
  animation.
- Do NOT change result filtering, keyboard indices, focus management, or result
  colors.
- Do NOT add dependencies.
- If the active row no longer uses `.is-active` since commit 683c4c1, STOP and
  report the drift.

## Verification

- **Mechanical**:
  - inspect `.command-palette-result` and confirm it has no `transition` or
    `animation` declaration;
  - `bun run typecheck` exits 0;
  - `bun run test` exits 0.
- **Feel check**: open the palette with Cmd/Ctrl+K:
  - hold ArrowDown, then ArrowUp; the active background stays exactly under the
    current selection without a trailing crossfade;
  - alternate mouse hover and arrow keys; selection feedback remains immediate;
  - Enter still activates the visibly selected row;
  - at 10% DevTools playback, no result-row animation appears;
  - palette entrance motion remains unchanged;
  - `prefers-reduced-motion` behavior remains unchanged.
- **Done when**: every selection change is visible in the same frame as the
  index update and the palette's documented entrance still works.
