# 002 — Narrow the shared button transitions

- **Status**: DONE
- **Commit**: 683c4c1
- **Severity**: HIGH
- **Category**: Performance
- **Estimated scope**: 1 file, one class-string replacement

## Problem

Every shared button inherits `transition-all`. The primitive is used by composer
controls, approval actions, toast actions, settings, review controls, and
activity tools, so an unsafe transition default is multiplied across the app.
It may animate unintended layout, spacing, ring, or size changes.

```tsx
// src/components/ui/button.tsx:6 — current
const buttonVariants = cva(
  "group/button inline-flex shrink-0 items-center justify-center rounded-lg border border-transparent bg-clip-padding text-sm font-medium whitespace-nowrap transition-all outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 active:not-aria-[haspopup]:translate-y-px disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
```

The same string contains state changes that actually need animation:

- background and text color on hover;
- border color and box shadow for focus/invalid rings;
- opacity when disabled;
- transform for the one-pixel active press.

## Target

Replace `transition-all` with an explicit property list, the repository's
150ms micro-interaction duration, and its established ease-out-quart curve.

```tsx
// src/components/ui/button.tsx — target excerpt
const buttonVariants = cva(
  "group/button inline-flex shrink-0 items-center justify-center rounded-lg border border-transparent bg-clip-padding text-sm font-medium whitespace-nowrap transition-[color,background-color,border-color,box-shadow,opacity,transform] duration-150 ease-out-quart outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 active:not-aria-[haspopup]:translate-y-px disabled:pointer-events-none disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
```

The target animates only:

`color`, `background-color`, `border-color`, `box-shadow`, `opacity`, and
`transform`.

It must not animate width, height, padding, margin, position, or other layout
properties.

## Repo conventions to follow

- `src/components/ActivityStep.tsx:289` uses the existing
  `duration-150 ease-out-quart` micro-interaction convention.
- `src/styles/tokens.css:56-59` defines the same 150ms duration and
  `cubic-bezier(0.25, 1, 0.5, 1)` curve.
- Preserve `active:not-aria-[haspopup]:translate-y-px`; it is the existing
  subtle press feedback and falls inside the 100-160ms button budget.
- Keep all Base UI, CVA, variant, focus, invalid, and dark-mode behavior
  unchanged.

## Steps

1. In the base CVA class string in `src/components/ui/button.tsx`, replace only
   `transition-all` with:
   `transition-[color,background-color,border-color,box-shadow,opacity,transform] duration-150 ease-out-quart`.
2. Do not add transition utilities to individual variants. The safe transition
   policy belongs on the shared primitive.
3. Do not change the active translation or any variant/size classes.

## Boundaries

- Do NOT edit button call sites.
- Do NOT change button geometry, variants, focus rings, active feedback, or
  disabled semantics.
- Do NOT use `transition-all`, `transition-property: all`, or a wildcard
  equivalent.
- Do NOT add dependencies.
- If Tailwind no longer accepts the arbitrary transition-property syntax at
  commit 683c4c1, STOP and report the compiler output; do not fall back to
  `transition-all`.

## Verification

- **Mechanical**:
  - `rg -n "transition-all|transition-property:\\s*all" src/components/ui/button.tsx`
    returns no matches.
  - `bun run typecheck` exits 0.
  - `bun run test` exits 0.
- **Feel check**: run the app and inspect default, outline, secondary, ghost,
  destructive, and icon buttons:
  - hover/focus colors and rings still transition over 150ms;
  - press feedback still moves down exactly one pixel;
  - rapidly press and release a send, approval, and settings button; the
    transform retargets cleanly from its current state;
  - in DevTools at 10% playback speed, padding and dimensions never tween;
  - toggle `prefers-reduced-motion: reduce`; transitions become immediate under
    the existing global policy.
- **Done when**: every shared button preserves its intended feedback and no
  button can animate an unspecified property.
