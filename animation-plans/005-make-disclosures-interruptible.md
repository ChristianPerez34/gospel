# 005 — Make reversible disclosures interruptible

- **Status**: DONE
- **Commit**: 683c4c1
- **Severity**: MEDIUM
- **Category**: Interruptibility
- **Estimated scope**: 4 files, about 45 lines plus focused test updates

## Problem

The high-traffic activity disclosure conditionally mounts a one-way entry
keyframe. Closing unmounts immediately; reopening restarts from
`opacity: 0; translateY(8px)` rather than retargeting from the current state.

```tsx
// src/components/ActivityStep.tsx:270 and 350 — current
const [expanded, setExpanded] = useState(card.expanded ?? false);

{expanded && hasBody && (
  <div className="activity-step-body ml-6 grid max-h-[520px] gap-3 overflow-y-auto rounded-sm p-3 animate-fade-slide-in-fast motion-reduce:animate-none">
    {/* detail content */}
  </div>
)}
```

Provider visibility repeats the keyframe pattern on an occasional settings
toggle:

```tsx
// src/components/ProviderSelector.tsx:360 and 382 — current
<button
  className={`hit-target relative w-9 h-5 border-none rounded-full cursor-pointer p-0 transition-colors duration-150 ease-out-quart shrink-0 ${
    provider.visible ? "bg-accent-action" : "bg-surface-overlay"
  }`}
  onClick={() => handleToggle(provider.id)}
  aria-pressed={provider.visible}
  type="button"
>

{(provider.visible || !provider.credentialed) && (
  <div className="px-4 pb-3 flex flex-col gap-2 animate-appear-body">
    {/* provider details */}
  </div>
)}
```

## Target

Activity details remain mounted while the card exists, become inert and hidden
from assistive technology when collapsed, and use CSS transitions that can
reverse from their current state.

```tsx
// src/components/ActivityStep.tsx — target structure
const bodyRef = useRef<HTMLDivElement>(null);

useEffect(() => {
  if (bodyRef.current) bodyRef.current.inert = !expanded;
}, [expanded]);

{hasBody && (
  <div
    ref={bodyRef}
    className="activity-step-disclosure"
    data-open={expanded ? "true" : "false"}
    aria-hidden={!expanded}
  >
    <div className="activity-step-disclosure-clip">
      <div className="activity-step-body ml-6 grid max-h-[520px] gap-3 overflow-y-auto rounded-sm p-3">
        {/* existing detail content, unchanged */}
      </div>
    </div>
  </div>
)}
```

Use the documented disclosure timing. The grid-track transition is the narrow
exception needed to reclaim document space for unknown-height content; all
visible content motion remains transform/opacity.

```css
/* src/styles/global.css — target */
.activity-step-disclosure {
  display: grid;
  grid-template-rows: 0fr;
  transition: grid-template-rows var(--duration-normal) var(--ease-out-quart);
}

.activity-step-disclosure[data-open="true"] {
  grid-template-rows: 1fr;
}

.activity-step-disclosure-clip {
  min-height: 0;
  overflow: hidden;
}

.activity-step-body {
  opacity: 0;
  transform: translateY(8px);
  transition:
    opacity var(--duration-fast) var(--ease-out-quart),
    transform var(--duration-fast) var(--ease-out-quart);
  transition-delay: 0ms;
}

.activity-step-disclosure[data-open="true"] .activity-step-body {
  opacity: 1;
  transform: translateY(0);
  transition-delay: var(--duration-normal);
}
```

Exact timing:

- disclosure space: 250ms via `--duration-normal`;
- content opacity/transform: 150ms via `--duration-fast`;
- entrance offset: the existing repository convention, `translateY(8px)`;
- easing: the established
  `cubic-bezier(0.25, 1, 0.5, 1)` ease-out-quart.

Provider visibility does not need decorative motion. Keep its conditional mount
and remove only `animate-appear-body` so the reversible settings toggle is
instant and cannot restart a keyframe:

```tsx
// src/components/ProviderSelector.tsx — target
{(provider.visible || !provider.credentialed) && (
  <div className="px-4 pb-3 flex flex-col gap-2">
    {/* existing provider details, unchanged */}
  </div>
)}
```

## Repo conventions to follow

- `DESIGN.md:356` specifies 250ms ease-out-quart for action-card expansion and
  a 150ms content fade after expansion.
- `src/styles/tokens.css:56-59` supplies those exact shared values.
- `src/styles/global.css:1367-1375` establishes the existing 8px message-entry
  offset; reuse it rather than inventing another distance.
- `src/components/AppShell.tsx:782-789` shows the repository's imperative
  `.inert` pattern. Use the same DOM property to prevent focus entering
  collapsed content.
- The global reduced-motion rule sets both shared duration tokens to 0ms, so
  the delay also becomes 0ms.

## Steps

1. In `src/components/ActivityStep.tsx`, expand the React import to
   `useEffect, useRef, useState`.
2. Add `bodyRef` and an effect that sets `bodyRef.current.inert = !expanded`.
3. Replace the conditional `expanded && hasBody` mount with the always-present
   `hasBody` disclosure wrapper shown above. Keep every existing detail child
   and rendering branch unchanged.
4. Remove `animate-fade-slide-in-fast motion-reduce:animate-none` from the
   activity body; the new CSS owns motion and the global reduced-motion policy
   owns the override.
5. Add the four disclosure style rules to `src/styles/global.css` beside the
   activity-step/chat component styles, using the exact values above.
6. In `src/components/ProviderSelector.tsx`, remove only
   `animate-appear-body` from the conditionally mounted provider-details class.
7. Update `src/components/ActivityStep.test.tsx`:
   - initial collapsed content is now present in the DOM, so replace
     `queryByText(...).toBeNull()` assertions with checks that the nearest
     `.activity-step-disclosure` has `aria-hidden="true"`;
   - after clicking, assert `aria-expanded="true"` on the trigger and
     `aria-hidden="false"` on the disclosure;
   - add a rapid open-close-open test and assert the same wrapper changes
     `data-open` `false → true → false → true` without remounting.
8. Preserve all existing grouped-pass, raw JSON, diff, and accessibility tests.

## Boundaries

- Do NOT alter activity content, expansion defaults, ARIA labels, or the
  chevron's existing 150ms rotation.
- Do NOT leave collapsed interactive content focusable; the `.inert` effect and
  `aria-hidden` state are mandatory.
- Do NOT keep hidden provider credential inputs mounted. Provider details must
  retain their existing conditional mount; only remove their entry keyframe.
- Do NOT introduce keyframes, timers, springs, or dependencies.
- Do NOT change the global reduced-motion policy.
- If `HTMLElement.inert` is unavailable in the configured DOM types after
  commit 683c4c1, STOP and report the type error instead of replacing it with
  `tabIndex` hacks.

## Verification

- **Mechanical**:
  - `rg -n "animate-fade-slide-in-fast|animate-appear-body" src/components/ActivityStep.tsx src/components/ProviderSelector.tsx`
    returns no matches.
  - `bun run typecheck` exits 0.
  - `bun run test` exits 0, including the new rapid-toggle test.
- **Feel check**:
  - expand, collapse, and immediately re-expand an activity card repeatedly;
    the wrapper retargets smoothly and never restarts from an unrelated frame;
  - at 10% playback, space expands for 250ms and content follows with the
    documented 150ms fade/8px translation;
  - collapse during the delayed fade; content reverses without flashing;
  - Tab through the page while the card is collapsed; focus never enters its
    hidden buttons, links, or disclosures;
  - toggle provider visibility rapidly; details now appear/disappear instantly
    with no stale animation;
  - with `prefers-reduced-motion: reduce`, expansion, content, and delay all
    become immediate.
- **Done when**: activity disclosure motion is reversible at every point,
  collapsed content is inaccessible to focus/assistive technology, and provider
  visibility has no restartable keyframe.
