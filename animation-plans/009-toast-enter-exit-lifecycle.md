# 009 — Give toasts an interruptible enter and a real exit

- **Status**: TODO
- **Commit**: 9df8439
- **Severity**: MEDIUM
- **Category**: Interruptibility / Missed opportunity
- **Estimated scope**: 2 files, about 50 lines changed

## Problem

Toasts stack. `ToastContainer` maps a list and `addToast` appends, so several can
be on screen at once — the textbook case where a fixed-duration keyframe is the
wrong tool, because a keyframe restarts from zero instead of retargeting from
whatever state the element is currently in.

```tsx
// src/components/Toast.tsx:53-57 — current
    <div
      className={`flex items-center gap-2.5 py-2.5 px-3.5 bg-surface-elevated border rounded-md shadow-[var(--shadow-floating)] pointer-events-auto max-w-[380px] animate-toast-in transition-opacity duration-200 ${TYPE_STYLES[toast.type]}`}
      role="alert"
    >
```

```css
/* src/styles/global.css:92 — current */
  --animate-toast-in: toast-in 0.2s ease;
```

```css
/* src/styles/global.css:1415-1424 — current */
@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
```

Worse, the `transition-opacity duration-200` in that class string is **dead
code**. It can never run, because dismissal removes the node from the array in
the same tick:

```tsx
// src/components/Toast.tsx:171-173 — current
  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);
```

Every error toast is created with `autoDismissMs: 5000`
(`src/components/Toast.tsx:180-184`), so this fires unattended: a toast the user
may still be reading blinks out of existence with no exit phase at all. The
result is an animate-in / teleport-out asymmetry on the one surface in the app
whose whole job is to be noticed and then to leave gracefully.

Two rules apply. Anything triggered rapidly or reversible mid-motion — toasts
stacking is the named example — must use transitions or springs, not keyframes;
entry without JS is done with `@starting-style`. And a state change that
teleports, where a brief transition would prevent a jarring change, is a missed
opportunity.

## Target

A single `.toast-item` class carrying a transition (not a keyframe), with
`@starting-style` supplying the entry-from state and a `data-exiting` attribute
supplying the exit-to state. Both directions retarget, because both are
transitions on the same properties.

```css
/* target — add to src/styles/global.css */
.toast-item {
  opacity: 1;
  transform: translateY(0);
  transition:
    opacity var(--duration-fast) var(--ease-out-quart),
    transform var(--duration-fast) var(--ease-out-quart);
}

@starting-style {
  .toast-item {
    opacity: 0;
    transform: translateY(25%);
  }
}

.toast-item[data-exiting="true"] {
  opacity: 0;
  transform: translateY(25%);
}
```

`--duration-fast` is `150ms` and `--ease-out-quart` is
`cubic-bezier(0.25, 1, 0.5, 1)`, both already defined in
`src/styles/tokens.css:57` and `:59`. This also retires the off-scale `0.2s`
that the keyframe token used.

`translateY(25%)` is a percentage of the toast's **own height**, not a hardcoded
pixel offset — percentage translates are the correct tool here because toasts
vary in height with message length and action buttons.

The dismissal path holds the node for exactly one exit phase before unmounting:

```tsx
// target — src/components/Toast.tsx
  const dismissToast = useCallback((id: string) => {
    setToasts((prev) => prev.map((t) => (t.id === id ? { ...t, exiting: true } : t)));
    const timer = setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
      exitTimersRef.current.delete(timer);
    }, TOAST_EXIT_MS);
    exitTimersRef.current.add(timer);
  }, []);
```

with `const TOAST_EXIT_MS = 150;` as a module constant so it stays in lockstep
with `--duration-fast`.

On `@starting-style` support: this app ships in a macOS WKWebView, where
`@starting-style` is available from Safari 17.5. On an older webview the entry
simply has no transition and the toast appears instantly — an acceptable
degradation for a transient notification, and strictly better than today's
non-interruptible keyframe. Do not add a JS `data-mounted` fallback; it is not
worth the complexity here.

## Repo conventions to follow

- Semantic class names live in `src/styles/global.css`; Tailwind utilities are
  composed inline in the component. Put the new `.toast-item` rules in
  `global.css` next to the other component rules and reference the class from the
  component's existing template string.
- Motion values come from tokens (`var(--duration-fast)`,
  `var(--ease-out-quart)`), never literals. Exemplar of the correct
  transition-plus-attribute-state pattern already in this codebase:
  `.session-drawer` at `src/styles/global.css:385-395`, which transitions
  `transform` and toggles the end state with an `.is-open` class rather than a
  keyframe.
- Timer cleanup convention: `src/components/Toast.tsx:39-47` already uses
  `setTimeout` with a `clearTimeout` cleanup inside `useEffect`. The new exit
  timers need equivalent cleanup, held in a ref so `useToasts` can clear them if
  the host unmounts mid-exit.

## Steps

1. In `src/styles/global.css`, add the three rules from the Target section
   (`.toast-item`, the `@starting-style` block, and
   `.toast-item[data-exiting="true"]`). Place them immediately before the
   `.input-bar` rule (currently around line 1163) so they sit with the other
   component-level rules.
2. In `src/styles/global.css`, delete the `--animate-toast-in: toast-in 0.2s ease;`
   line at line 92, and delete the entire `@keyframes toast-in` block at lines
   1415-1424. Both become dead once step 4 lands. Do NOT delete any other
   `--animate-*` token or keyframe.
3. In `src/components/Toast.tsx`, add `exiting?: boolean;` to the `ToastData`
   interface (currently lines 4-17), documented as internal-only state set by
   `dismissToast`.
4. In `src/components/Toast.tsx:53-57`, change the wrapper element: replace
   `animate-toast-in transition-opacity duration-200` in the class string with
   `toast-item`, and add `data-exiting={toast.exiting ? "true" : "false"}`. Keep
   `role="alert"`, every other utility class, and the `${TYPE_STYLES[toast.type]}`
   interpolation exactly as they are. The resulting className is:
   `` `flex items-center gap-2.5 py-2.5 px-3.5 bg-surface-elevated border rounded-md shadow-[var(--shadow-floating)] pointer-events-auto max-w-[380px] toast-item ${TYPE_STYLES[toast.type]}` ``
5. In `src/components/Toast.tsx`, add a module-level `const TOAST_EXIT_MS = 150;`
   near the other module constants (`toastIdCounter` / `SESSION_ID`, lines
   158-159), with a brief comment noting it must match `--duration-fast`.
6. In `useToasts` (`src/components/Toast.tsx:161`), add
   `const exitTimersRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set());`
   and replace `dismissToast` (lines 171-173) with the two-phase version from the
   Target section: mark the toast `exiting: true`, then remove it after
   `TOAST_EXIT_MS`.
7. In `useToasts`, add an unmount cleanup effect that clears every timer still in
   `exitTimersRef.current` and empties the set, so a host unmounting mid-exit
   leaves no pending timer.
8. Guard against double-dismiss: in `dismissToast`, return early if the toast is
   already `exiting` (look it up in the `setToasts` updater, or track dismissed
   ids in a ref). A user clicking the × twice, or an auto-dismiss firing while
   the user clicks ×, must not schedule two removals.
9. Add `useRef` to the React import at `src/components/Toast.tsx:1` if it is not
   already there.

## Boundaries

- Do NOT change `ToastContainer`'s layout (`src/components/Toast.tsx:148-156`).
  Making the surviving toasts slide smoothly into a freed slot requires FLIP
  measurement and is explicitly OUT of scope for this plan. The exit phase alone
  removes the "blinks out of existence" jarring change; the remaining sibling
  snap is acceptable and is deliberately left.
- Do NOT change the auto-dismiss duration (`autoDismissMs: 5000`) or the
  conditions under which auto-dismiss applies (`src/components/Toast.tsx:39-47`).
- Do NOT change `addToast`, `showError`, or any other public method of
  `useToasts` beyond `dismissToast`.
- Do NOT change `role="alert"` or any accessibility attribute. A screen reader
  must still announce the toast when it appears.
- Do NOT add a `data-mounted` / `useEffect` mount-flag fallback for
  `@starting-style`.
- Do NOT add new dependencies.
- If a cited excerpt does not match what you find (drift since commit
  `9df8439`), STOP and report rather than improvising.

## Verification

- **Mechanical**:
  - `bun run typecheck` — expected to pass. The new `exiting?: boolean` field is
    optional, so no existing `ToastData` construction site needs updating.
  - `bun run lint` — expected to pass.
  - `bun run test` — expected to pass. There is no `Toast.test.tsx`, but
    `src/components/AppShell.test.tsx` renders the app shell and may assert on
    toast text; if it fails, read the failure before editing it.
  - `rg -n 'toast-in|animate-toast-in' src` — expected to return **no matches**.
- **Feel check**: run `bun run dev` and trigger a real error toast (for example
  attempt an action with no model configured), then:
  - Confirm the toast fades and rises into place over about 150ms rather than
    appearing instantly. If it appears with no motion at all, the webview may
    lack `@starting-style` — verify with
    `CSS.supports('at-rule', '@starting-style')` in the console before treating
    it as a bug.
  - Trigger three toasts in quick succession. Each must animate in
    independently; none may visibly restart or flicker when the next arrives.
  - Click × on a toast. It must fade and drop out over 150ms **before**
    disappearing, not vanish instantly.
  - Let an error toast auto-dismiss after 5s without touching it. Same exit
    phase must play.
  - Click × twice rapidly on the same toast. It must exit once and must not
    throw or double-remove.
  - In DevTools Animations panel at 10% playback, dismiss a toast: only
    `opacity` and `transform` transitions should be recorded, with no keyframe
    animation.
  - Trigger two toasts, then dismiss the older one while the newer is still
    animating in. The newer toast must continue smoothly from its current
    position rather than jumping.
- **Done when**: `toast-in` appears nowhere in `src/`, dismissing a toast plays a
  visible 150ms exit before the node leaves the DOM, `bun run test` passes, and
  the DevTools Animations panel records transitions rather than an animation.
