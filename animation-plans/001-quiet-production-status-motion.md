# 001 — Quiet the production status motion

- **Status**: DONE
- **Commit**: 683c4c1
- **Severity**: HIGH
- **Category**: Purpose & frequency / Performance / Cohesion
- **Estimated scope**: 3 files, about 40 lines removed and 6 lines adjusted

## Problem

Gospel is documented as a precise, quiet workbench. `DESIGN.md:354` permits one
subtle agent-thinking pulse, but production currently runs a 24-bar equalizer,
an agent pulse ring, reviewer pulse rings, a reviewer pulse dot, and blinking
typing text during long jobs.

The equalizer is also a continuous layout animation. Every streaming frame asks
24 elements to animate `height`, which requires layout and paint rather than
compositor-only transform/opacity work.

```tsx
// src/components/ConstellationCanvas.tsx:224 — current
{/* Equalizer */}
<div className="constellation-equalizer" style={{ opacity: agentRunning ? 1 : 0.3 }}>
  {Array.from({ length: 24 }).map((_, i) => (
    <span
      key={i}
      className="constellation-eq-bar"
      style={{
        background: [
          "var(--gospel-agent-cyan)",
          "var(--gospel-agent-violet)",
          "var(--gospel-agent-amber)",
          "var(--gospel-agent-rose)",
        ][i % 4],
        height: agentRunning ? `${30 + Math.abs(Math.sin(i * 0.7)) * 70}%` : "20%",
        animation: agentRunning
          ? `proto-eq ${0.6 + (i % 5) * 0.1}s ease-in-out infinite alternate`
          : "none",
        animationDelay: `${i * 0.05}s`,
      }}
    />
  ))}
</div>
```

```tsx
// src/components/ConstellationCanvas.tsx:283 — current
<div
  className="constellation-agent-ring"
  style={{
    borderColor: running ? "var(--gospel-accent-action)" : "var(--gospel-surface-line)",
    animation: running ? "proto-pulse-ring 1.8s ease-out infinite" : "none",
  }}
/>
```

```tsx
// src/components/ConstellationCanvas.tsx:609 — current
<span
  className="constellation-reviewer-ring"
  style={{
    borderColor: color,
    opacity: r.status === "done" ? 0 : r.status === "idle" ? 0.2 : 1,
    animation: isActive ? "proto-pulse-ring 1.6s ease-out infinite" : "none",
  }}
/>
```

```css
/* src/styles/global.css:361 — current */
.topbar-compute-graph.is-active span {
  background: var(--status-success);
  animation: equalize 0.9s ease-in-out infinite alternate;
  box-shadow: 0 0 8px color-mix(in srgb, var(--status-success) 42%, transparent);
}

/* src/styles/global.css:1882 — current */
.reviewer-panel-pulse-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  animation: proto-pulse 1s infinite;
  flex-shrink: 0;
}

/* src/styles/global.css:1916 — current */
.reviewer-panel-typing {
  font-size: 9px;
  font-family: var(--font-mono);
  animation: proto-blink 1.2s infinite;
}
```

## Target

The existing `StatusIndicator` opacity pulse remains the only perpetual global
agent-status animation. Constellation and reviewer state remains legible through
static color, opacity, glow, text, and progress.

```tsx
// src/components/ConstellationCanvas.tsx — target agent ring
<div
  className="constellation-agent-ring"
  style={{
    borderColor: running ? "var(--gospel-accent-action)" : "var(--gospel-surface-line)",
  }}
/>

// src/components/ConstellationCanvas.tsx — target reviewer ring
<span
  className="constellation-reviewer-ring"
  style={{
    borderColor: color,
    opacity: r.status === "done" ? 0 : r.status === "idle" ? 0.2 : 1,
  }}
/>
```

Delete the complete production equalizer JSX block. The top-bar compute graph,
reviewer pulse dot, and reviewer typing label keep their static visual styles
but lose `animation` and obsolete `animation-delay` declarations.

Do not delete the `@keyframes proto-*` definitions: the query-gated files under
`src/prototype/harness/` still consume them. The production tree under
`src/components/` must no longer reference those prototype keyframes.

## Repo conventions to follow

- `src/components/StatusIndicator.tsx:15-18` is the retained exemplar:
  `thinking` and `acting` use the shared `animate-pulse` utility.
- `src/styles/global.css:89` defines that pulse as exactly
  `pulse 2s ease-in-out infinite`, matching `DESIGN.md:354`.
- Static active state already uses semantic colors and text. Preserve
  `borderColor`, `opacity`, `boxShadow`, `analyzing…`, and `typing…`.
- Reduced motion remains the repository-wide zero-duration policy in
  `src/styles/global.css:139-153`; do not alter it.

## Steps

1. In `src/components/ConstellationCanvas.tsx`, delete the entire
   `constellation-equalizer` block at current lines 224-245.
2. In `AgentNode`, remove only the inline `animation` property from
   `constellation-agent-ring`; retain the state-dependent border color.
3. In `ReviewerNode`, remove only the inline `animation` property from
   `constellation-reviewer-ring`; retain border color and status opacity.
4. In `src/styles/global.css`, remove `animation: equalize ...` and the four
   `.topbar-compute-graph.is-active span:nth-child(...)` delay rules. Preserve
   the active color and glow.
5. In `src/styles/global.css`, remove the animation declarations from
   `.reviewer-panel-pulse-dot` and `.reviewer-panel-typing`, keeping their
   dimensions, typography, and colors.
6. Remove the now-dead `.constellation-equalizer` and
   `.constellation-eq-bar` style blocks. Leave all `@keyframes proto-*`
   definitions intact for the dev-only prototype harness.

## Boundaries

- Do NOT change `src/components/StatusIndicator.tsx` or its 2-second opacity
  pulse.
- Do NOT edit anything under `src/prototype/harness/`.
- Do NOT delete the `@keyframes proto-*` definitions while the prototype uses
  them.
- Do NOT change review progress behavior; plan 003 owns progress motion.
- Do NOT add dependencies or replace static status text with a new animation.
- If the cited production references have moved or gained a documented purpose
  since commit 683c4c1, STOP and report the drift instead of improvising.

## Verification

- **Mechanical**:
  - `rg -n "proto-(eq|pulse|pulse-ring|blink)" src/components` returns no
    production component references.
  - `rg -n "animation: equalize" src/styles/global.css` returns no matches.
  - `bun run typecheck` exits 0.
  - `bun run test` exits 0.
- **Feel check**: run the app, start a normal agent turn, then start a
  multi-review and confirm:
  - the top-bar status dot is the only perpetual global agent-status motion;
  - constellation rings communicate active state through static color/glow;
  - reviewer cards still communicate `running`, `analyzing…`, `typing…`, and
    progress without blinking or scaling;
  - no 24-bar equalizer is visible;
  - DevTools Performance shows no repeating layout work from `proto-eq`;
  - at 10% playback speed, no hidden secondary loop remains;
  - with `prefers-reduced-motion: reduce`, state remains readable.
- **Done when**: production has one quiet status pulse, no component consumes a
  `proto-*` animation, and the dev-only harness remains intact.
