# 008 — Coalesce the splitter drag off the React commit path

- **Status**: TODO
- **Commit**: 9df8439
- **Severity**: HIGH
- **Category**: Performance
- **Estimated scope**: 2 files, about 45 lines changed

## Problem

Dragging the workbench splitter is direct-manipulation motion — the user's hand
is on it, so every dropped frame is felt. It is currently the most expensive
interaction in the app.

The `mousemove` handler is unthrottled and calls `setState` on every event:

```tsx
// src/components/WorkbenchLayout.tsx:82-108 — current
  // Draggable splitter
  const draggingRef = useRef(false);
  const onSplitterDown = useCallback(() => {
    draggingRef.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!draggingRef.current) return;
      const w = Math.max(280, Math.min(640, e.clientX));
      setColW(w);
    };
    const onUp = () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, []);
```

That state is written straight into an inline layout property:

```tsx
// src/components/WorkbenchLayout.tsx:220 — current
      <aside className="workbench-left-column" style={{ width: colW }}>
```

The `width` write is only the start. Because `.workbench-canvas-wrap` is the
flex sibling that absorbs the remaining space
(`src/styles/global.css:1986-1991`), resizing the column resizes the canvas,
whose `ResizeObserver` fires:

```tsx
// src/components/ConstellationCanvas.tsx:84-90 — current
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
```

`setSize` recomputes `cx`/`cy` (`ConstellationCanvas.tsx:92-93`) which are used
to rewrite the inline `left`/`top` of every node
(`ConstellationCanvas.tsx:239, 363, 396, 492, 701`), the SVG's `width`/`height`
(`:242`), and every edge `path`'s `d` attribute (`:243-260`).

So one mouse event triggers: a React re-render of `WorkbenchLayout`, a layout
pass, a `ResizeObserver` callback, a second React re-render of the whole
constellation, and a full rewrite of every node's layout properties. All of it
on the main thread, all of it repeated at mouse-event rate rather than frame
rate.

The relevant rules: animate `transform` and `opacity` only — `width`, `top`, and
`left` trigger layout, paint, and composite; and CSS beats rAF-based JS under
load, so per-event JS work should at minimum be coalesced to one write per
frame. `DESIGN.md`'s `## Motion` section states the same rule: "Never animate
layout properties (width, height, top, left)."

A splitter genuinely must change a width — that part is unavoidable. What is
avoidable is doing it through React state on every mouse event.

## Target

Two changes, both of which keep the visual result identical:

**1. During the drag, write the width directly to the DOM inside a single
`requestAnimationFrame`, bypassing React entirely.** React state is updated once,
on `mouseup`, so the committed width still survives re-renders.

**2. Coalesce the `ResizeObserver` callback to one `setSize` per frame**, so the
constellation re-render cannot run more than once per frame even if the observer
fires more often.

```tsx
// target — src/components/WorkbenchLayout.tsx
  const columnRef = useRef<HTMLElement>(null);
  const draggingRef = useRef(false);
  const frameRef = useRef<number | null>(null);
  const pendingWidthRef = useRef(380);

  const onSplitterDown = useCallback(() => {
    draggingRef.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!draggingRef.current) return;
      pendingWidthRef.current = Math.max(280, Math.min(640, e.clientX));
      if (frameRef.current !== null) return;
      frameRef.current = requestAnimationFrame(() => {
        frameRef.current = null;
        if (columnRef.current) {
          columnRef.current.style.width = `${pendingWidthRef.current}px`;
        }
      });
    };
    const onUp = () => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      if (frameRef.current !== null) {
        cancelAnimationFrame(frameRef.current);
        frameRef.current = null;
      }
      setColW(pendingWidthRef.current);
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    };
  }, []);
```

```tsx
// target — src/components/ConstellationCanvas.tsx
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    let frame: number | null = null;
    const ro = new ResizeObserver(() => {
      if (frame !== null) return;
      frame = requestAnimationFrame(() => {
        frame = null;
        setSize({ w: el.clientWidth, h: el.clientHeight });
      });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
    };
  }, []);
```

## Repo conventions to follow

- The codebase already uses exactly this rAF-coalescing pattern for a
  high-frequency stream of updates. Imitate it:
  `src/hooks/useChatStream.ts:121-124` coalesces streamed tokens into one React
  commit per frame. Match its shape (a ref holding the pending value, a ref
  holding the frame id, an early return when a frame is already scheduled).
- Refs and callbacks in `WorkbenchLayout.tsx` use `useRef` / `useCallback`
  imported at `src/components/WorkbenchLayout.tsx:2`; `requestAnimationFrame`
  is used unwrapped elsewhere (`ConstellationCanvas.tsx:231`,
  `useFocusTrap.ts:67`), so no helper is needed.
- Keep the `colW` state variable (`WorkbenchLayout.tsx:59`) and its initial
  value of `380`. It remains the source of truth between drags.

## Steps

1. In `src/components/WorkbenchLayout.tsx`, add three refs next to the existing
   `draggingRef` (line 83): `columnRef` typed `useRef<HTMLElement>(null)`,
   `frameRef` typed `useRef<number | null>(null)`, and `pendingWidthRef` typed
   `useRef(380)`.
2. Replace the body of the `onMove` handler (lines 92-96) so it stores the
   clamped width in `pendingWidthRef.current`, returns early if
   `frameRef.current !== null`, and otherwise schedules one
   `requestAnimationFrame` that clears `frameRef.current` and writes
   `columnRef.current.style.width = \`${pendingWidthRef.current}px\``. Keep the
   existing clamp `Math.max(280, Math.min(640, e.clientX))` byte-for-byte. Do
   not call `setColW` here.
3. In the `onUp` handler (lines 97-102), after `draggingRef.current = false;`,
   cancel any pending frame (`cancelAnimationFrame(frameRef.current)` guarded by
   a null check, then set `frameRef.current = null`) and call
   `setColW(pendingWidthRef.current)` exactly once. Keep the two `document.body`
   style resets.
4. In the effect's cleanup return (lines 103-107), add a guarded
   `cancelAnimationFrame(frameRef.current)` alongside the two
   `removeEventListener` calls.
5. Keep `pendingWidthRef` in sync when the drag is not the source of the width:
   immediately after the `useState` for `colW`, no change is needed, but inside
   `onSplitterDown` (lines 84-88) set `pendingWidthRef.current = colW;` as the
   first statement so a drag starts from the committed value. Add `colW` to that
   `useCallback`'s dependency array.
6. At `src/components/WorkbenchLayout.tsx:220`, attach the ref to the existing
   element, keeping the inline width so the committed value still applies on
   re-render:
   `<aside ref={columnRef} className="workbench-left-column" style={{ width: colW }}>`
7. In `src/components/ConstellationCanvas.tsx`, replace the `ResizeObserver`
   effect at lines 84-90 with the rAF-coalesced version shown in the Target
   section. Keep `setSize({ w: el.clientWidth, h: el.clientHeight })` as the
   only state write and keep `ro.disconnect()` in the cleanup.

## Boundaries

- Do NOT convert the node positions in `ConstellationCanvas.tsx` from
  `left`/`top` to `transform`. That is tracked as a separate finding and touching
  it here would make this diff unreviewable.
- Do NOT change the `constellation-nebula` positioning at
  `ConstellationCanvas.tsx:239` (separate finding).
- Do NOT change the clamp bounds (280 / 640), the initial column width (380), or
  any `.workbench-*` CSS.
- Do NOT switch from mouse events to pointer events, and do NOT add pointer
  capture. Keep the existing event model.
- Do NOT add a CSS transition to `.workbench-left-column`. A splitter must track
  the cursor with zero lag; a transition would add latency.
- Do NOT add new dependencies.
- If a cited excerpt does not match what you find (drift since commit
  `9df8439`), STOP and report rather than improvising.

## Verification

- **Mechanical**:
  - `bun run typecheck` — expected to pass. `columnRef` must be typed
    `useRef<HTMLElement>(null)` to match `<aside>`; if TypeScript complains
    about the ref type, fix the ref type, not the JSX.
  - `bun run lint` — expected to pass. Biome will flag a missing `colW`
    dependency on the `onSplitterDown` callback if step 5 is done incorrectly.
  - `bun run test` — expected to pass. `src/components/WorkbenchLayout.test.tsx`
    and `src/components/ConstellationCanvas.test.tsx` exist; if either fails,
    read the failure before changing it. `ResizeObserver` and
    `requestAnimationFrame` behaviour under `happy-dom` may mean the canvas size
    now settles a frame later — if a test asserts a size synchronously, wrap the
    assertion in the existing suite's async helper rather than reverting the rAF
    coalescing. If you cannot make it pass without weakening the assertion,
    STOP and report.
- **Feel check**: run `bun run dev`, open a session with an active review so the
  constellation has many nodes and edges, then:
  - Drag the splitter left and right continuously for several seconds. The
    column edge must track the cursor with no perceptible lag and no stutter,
    and the constellation nodes must keep up rather than trailing behind.
  - Release the mouse. The column must stay at the released width — if it snaps
    back to 380px or to the pre-drag width, step 3 or step 6 is wrong.
  - Drag to both extremes and hold. The column must stop at its bounds without
    jitter.
  - In DevTools Performance panel, record a 3-second continuous drag on `main`
    and again on the branch. Compare: the number of React commits ("Function
    Call" / render work) per second should drop to at most one per frame, and
    the long-task bars should shorten. This is the primary evidence the fix
    worked.
  - Switch the left column to the Reviewers tab and drag again, to confirm the
    ref-based write still applies when the column's children change.
- **Done when**: `bun run test` passes, a continuous drag produces at most one
  React commit per frame in a Performance recording, the released width
  persists, and no CSS transition was added to `.workbench-left-column`.
