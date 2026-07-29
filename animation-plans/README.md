# Animation improvement plans

These plans were produced from the read-only motion audit at commit `683c4c1`.
They are implementation specifications; all source changes have been applied.

| # | Plan | Severity | Status | Dependencies |
| --- | --- | --- | --- | --- |
| 001 | [Quiet the production status motion](001-quiet-production-status-motion.md) | HIGH | DONE | None |
| 002 | [Narrow the shared button transitions](002-narrow-shared-button-transitions.md) | HIGH | DONE | None |
| 003 | [Move review progress to transforms](003-move-review-progress-to-transforms.md) | HIGH | DONE | Execute after 001 |
| 004 | [Make palette navigation instant](004-make-palette-navigation-instant.md) | HIGH | DONE | None |
| 005 | [Make reversible disclosures interruptible](005-make-disclosures-interruptible.md) | MEDIUM | DONE | Execute after 003 |

## Recommended execution order

1. **001 — Quiet the production status motion.** Establish the product's motion
   budget first and remove the largest continuous layout workload.
2. **003 — Move review progress to transforms.** It touches
   `ConstellationCanvas.tsx` and `global.css`, so applying it after 001 avoids
   parallel merge conflicts and leaves the remaining review motion
   compositor-safe.
3. **002 — Narrow the shared button transitions.** This is isolated to the
   shared primitive and can be reviewed independently after ambient motion is
   quiet.
4. **004 — Make palette navigation instant.** A small, isolated keyboard
   responsiveness correction.
5. **005 — Make reversible disclosures interruptible.** This has the broadest
   interaction and accessibility verification surface; execute after the
   simpler global CSS plans have settled.

## Dependency and execution notes

- Plans 001, 003, 004, and 005 all touch `src/styles/global.css`; execute them
  sequentially in the order above rather than in parallel.
- Plans 001 and 003 both touch `src/components/ConstellationCanvas.tsx`.
- Plan 002 has no semantic dependency and may be executed independently, but
  its shared-button reach warrants a full app feel-check.
- Each executor must drift-check the stamped commit before editing. If a cited
  excerpt no longer matches, stop and report rather than improvising.
- After a plan lands, update its `Status` here and in the plan file from `TODO`
  to `DONE`.
