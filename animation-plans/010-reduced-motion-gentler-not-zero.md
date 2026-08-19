# 010 — Make reduced motion gentler, not zero

- **Status**: TODO
- **Commit**: 9df8439
- **Severity**: MEDIUM
- **Category**: Accessibility
- **Estimated scope**: 5 files, about 60 lines changed

## Problem

Reduced motion is currently a blanket nuke:

```css
/* src/styles/global.css:139-153 — current */
@media (prefers-reduced-motion: reduce) {
  :root {
    --gospel-duration-fast: 0ms;
    --gospel-duration-normal: 0ms;
    --gospel-duration-slow: 0ms;
  }

  *,
  *::before,
  *::after {
    animation-duration: 0ms !important;
    transition-duration: 0ms !important;
    scroll-behavior: auto !important;
  }
}
```

The rule this violates: reduced motion means fewer and gentler animations, **not
zero** — keep transitions that aid comprehension, remove position changes. Keep
opacity and colour, drop movement.

Three concrete consequences:

**1. Feedback is destroyed along with movement.** Every
`transition-colors duration-150` hover and active state across roughly 25
components becomes an instant snap. And the `pulse` keyframe — which animates
`opacity` only (`src/styles/global.css:1367-1375`) and is the *sole* "the agent
is thinking" affordance on `StatusIndicator` (`src/components/StatusIndicator.tsx:17-18`)
— stops entirely. A user who asked for less movement loses state information.

**2. Every `motion-reduce:` utility in the codebase is dead.** The blanket
`animation-duration: 0ms !important` already wins, so these eight declarations
are unverifiable in the running app:

```tsx
// src/components/ChatView.tsx:102 — current
        className="mr-2 mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full bg-accent-action animate-pulse motion-reduce:animate-none"
```

Worse, where they *are* the intended policy they target the wrong channel:
`animate-pulse` is opacity-only, which is exactly what should be kept. Full list
of sites: `src/components/ChatView.tsx:100, 102, 247, 333, 388` and
`src/components/ActivityStep.tsx:294, 304, 338`.

**3. The JS smooth scroll is not covered at all.** `scroll-behavior: auto !important`
is a CSS declaration and does not override an explicit `behavior: "smooth"`
passed in `ScrollToOptions` — the JS argument wins. So a reduced-motion user
still gets an animated scroll here, re-firing on every appended reviewer comment
during a live review:

```tsx
// src/components/ReviewerPanelCard.tsx:22-27 — current
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [r.comments.length]);
```

There is no `useReducedMotion` hook anywhere in `src/`, so there is no seam to
branch on.

## Target

**CSS policy.** Durations stay at their normal values. Transitions are narrowed
to the non-movement channels, and the movement keyframes are redefined inside the
media query as plain fades. Redefining a `@keyframes` with the same name inside
the media query overrides the earlier definition, so consumers need no change.

```css
/* target — replaces src/styles/global.css:139-153 */
@media (prefers-reduced-motion: reduce) {
  /* Reduced motion means gentler, not none: keep opacity and colour feedback,
     drop position and scale changes. */
  *,
  *::before,
  *::after {
    transition-property: opacity, color, background-color, border-color, box-shadow, fill,
      stroke !important;
    scroll-behavior: auto !important;
  }

  /* Movement keyframes degrade to a plain fade; durations are unchanged. */
  @keyframes fadeSlideIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes slide-up {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
}
```

Note what is deliberately **absent** from the target:

- No `--gospel-duration-*: 0ms` overrides. Durations stay real so the surviving
  opacity and colour transitions are still visible.
- No `animation-duration: 0ms`. `pulse` (opacity only) and `spin` (a loading
  affordance) keep running.
- No `toast-in` override — plan 009 replaces that keyframe with a transition,
  which this policy's `transition-property` narrowing already handles by
  stripping its `transform` channel.
- No `palette-enter` override — plan 006 deletes it.

**JS policy.** A hook, modelled on the existing `useThemePreference`:

```ts
// target — src/hooks/useReducedMotion.ts
import { useEffect, useState } from "react";

const QUERY = "(prefers-reduced-motion: reduce)";

function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia(QUERY).matches;
}

export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(prefersReducedMotion);

  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;

    const query = window.matchMedia(QUERY);
    const handleChange = () => setReduced(query.matches);

    handleChange();
    query.addEventListener("change", handleChange);
    return () => query.removeEventListener("change", handleChange);
  }, []);

  return reduced;
}
```

and the scroll branch:

```tsx
// target — src/components/ReviewerPanelCard.tsx
  const reducedMotion = useReducedMotion();

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: reducedMotion ? "auto" : "smooth",
    });
  }, [r.comments.length, reducedMotion]);
```

**Utility cleanup.** `motion-reduce:` declarations are kept only where they drop
*movement*, and removed where they would drop opacity or colour:

| Site | Current | Action | Why |
| --- | --- | --- | --- |
| `ChatView.tsx:100` | `animate-fade-slide-in-fast motion-reduce:animate-none` | remove `motion-reduce:animate-none` | keyframe is now a fade under reduced motion; keep the fade |
| `ChatView.tsx:102` | `animate-pulse motion-reduce:animate-none` | remove `motion-reduce:animate-none` | `pulse` is opacity-only; it is the state signal |
| `ChatView.tsx:247` | `animate-fade-slide-in-fast motion-reduce:animate-none` | remove `motion-reduce:animate-none` | as above |
| `ChatView.tsx:333` | `animate-fade-slide-in motion-reduce:animate-none` | remove `motion-reduce:animate-none` | as above |
| `ChatView.tsx:388` | `animate-fade-slide-in-fast motion-reduce:animate-none` | remove `motion-reduce:animate-none` | as above |
| `ActivityStep.tsx:294` | `transition-colors ... motion-reduce:transition-none` | remove `motion-reduce:transition-none` | colour feedback is the channel to keep |
| `ActivityStep.tsx:304` | `animate-pulse motion-reduce:animate-none` | remove `motion-reduce:animate-none` | opacity-only pulse |
| `ActivityStep.tsx:338` | `transition-transform ... motion-reduce:transition-none` | **KEEP** | this one drops a rotation, which is movement — correct |

## Repo conventions to follow

- Hooks live in `src/hooks/`, one named export per file, no default export.
  Model `useReducedMotion` on `src/hooks/useThemePreference.ts:41-70`: it uses
  `useState` with a lazy initialiser function, guards `typeof window === "undefined" || !window.matchMedia`
  (needed because the test environment is `happy-dom`), calls `handleChange()`
  once before subscribing, and cleans up with `removeEventListener`. Match that
  shape exactly.
- Motion tokens stay in `src/styles/tokens.css`; do not add new ones here.
- Biome formats this repo (`bun run format`); the multi-line
  `transition-property` value in the target is already wrapped the way Biome
  will want it.

## Steps

1. Create `src/hooks/useReducedMotion.ts` with the exact contents shown in the
   Target section.
2. In `src/styles/global.css`, replace the whole
   `@media (prefers-reduced-motion: reduce)` block at lines 139-153 with the CSS
   from the Target section. Delete the `:root` duration overrides entirely.
3. In `src/components/ReviewerPanelCard.tsx`, import `useReducedMotion` from
   `../hooks/useReducedMotion`, call it inside the component next to the existing
   `scrollRef` declaration (line 21), and change the `scrollTo` call (lines
   23-27) to pass `behavior: reducedMotion ? "auto" : "smooth"`. Add
   `reducedMotion` to the effect's dependency array alongside `r.comments.length`.
4. In `src/components/ChatView.tsx`, remove the `motion-reduce:animate-none`
   token from the class strings at lines 100, 102, 247, 333, and 388. Change
   nothing else in those strings — keep every `animate-fade-slide-in*` and
   `animate-pulse` class.
5. In `src/components/ActivityStep.tsx`, remove `motion-reduce:transition-none`
   from the trigger's class string at line 294, and remove
   `motion-reduce:animate-none` from the dot's class string at line 304. **Keep**
   `motion-reduce:transition-none` on the chevron at line 338 — that one
   correctly suppresses a rotation.
6. Update `src/components/ChatView.test.tsx`. Lines 129-132 currently assert the
   two declarations being removed:
   ```tsx
       expect(screen.getByTestId("agent-turn-turn-running").className).toContain(
         "motion-reduce:animate-none"
       );
       expect(readRow.className).toContain("motion-reduce:transition-none");
   ```
   Replace both assertions with ones that pin the new policy instead:
   ```tsx
       expect(screen.getByTestId("agent-turn-turn-running").className).toContain(
         "animate-fade-slide-in-fast"
       );
       expect(readRow.className).toContain("transition-colors");
   ```
   Do not delete the surrounding assertions about `aria-expanded` or
   `aria-hidden`.
7. Run `bun run format` so the rewritten CSS block matches Biome's output.

## Boundaries

- Do NOT remove `motion-reduce:transition-none` from
  `src/components/ActivityStep.tsx:338`.
- Do NOT add `transform` to the reduced-motion `transition-property` allow-list.
  Stripping movement is the entire point of the block.
- Do NOT override the `pulse` or `spin` keyframes. `pulse` is opacity-only and
  `spin` is a loading affordance; both stay.
- Do NOT add a `toast-in` or `palette-enter` override to the media query. Plan
  009 removes the first and plan 006 removes the second; if either name still
  exists when you run this plan, STOP and report that the prerequisite plan has
  not landed.
- Do NOT change `--gospel-duration-slow` or any other token value.
- Do NOT wire `useReducedMotion` into any component other than
  `ReviewerPanelCard.tsx`. Other call sites can adopt it later.
- Do NOT change the `ChatView.tsx:306-309` scroll helper — it already assigns
  `scrollTop` directly with no smooth behaviour, which is correct.
- Do NOT add new dependencies.
- If a cited excerpt does not match what you find (drift since commit
  `9df8439`), STOP and report rather than improvising.

## Verification

- **Mechanical**:
  - `bun run typecheck` — expected to pass.
  - `bun run lint` — expected to pass. Biome will flag a missing `reducedMotion`
    dependency if step 3 is incomplete.
  - `bun run test` — expected to pass **after** the step 6 edits, and only those.
    If any other test fails, read the failure before editing it; a test asserting
    a removed `motion-reduce:` class is a legitimate update, a test failing for
    any other reason is a signal to STOP and report.
  - `rg -n 'motion-reduce' src` — expected to return exactly one match:
    `src/components/ActivityStep.tsx:338`.
  - `rg -n 'behavior: "smooth"' src` — expected to return no unguarded matches.
- **Feel check**: run `bun run dev`, then in DevTools open the Rendering panel
  and set "Emulate CSS media feature prefers-reduced-motion" to `reduce`. With it
  on:
  - Start a turn. The thinking dot must **still pulse** (opacity 0.3 → 1, 2s
    cycle). If it freezes, the block is still killing animations.
  - Hover the session rows, the topbar buttons, and a tool activity row. Colour
    and background must still ease in over 150ms, not snap. This is the main
    thing the old block broke.
  - Expand a tool activity row. Content must fade in with **no** 8px vertical
    slide.
  - Open the session drawer. It must appear with no horizontal slide, while the
    scrim still fades.
  - Open the Settings modal. It must fade in with no upward slide.
  - Trigger a toast. It must fade with no vertical movement.
  - Start a review and let reviewer comments accumulate. The reviewer panel must
    jump directly to the bottom with no animated scroll — this is the fix that
    CSS alone could not deliver.
  - Click the activity chevron. It must still rotate to indicate state but with
    no transition (instant flip).
  - Then set the emulation back to "no-preference" and confirm all the movement
    returns: drawer slides, modal rises, disclosure body slides up 8px, reviewer
    panel scrolls smoothly.
- **Done when**: `rg -n 'motion-reduce' src` returns only
  `ActivityStep.tsx:338`, the reduced-motion media block contains no
  `animation-duration` or duration-token override, the thinking pulse and all
  hover colour transitions survive under `prefers-reduced-motion: reduce`, the
  reviewer panel scroll is instant under it, and `bun run test` passes with only
  the step 6 test edits.
