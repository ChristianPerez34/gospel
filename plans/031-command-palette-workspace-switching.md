# Plan 031: Direct Workspace Search and Switching in Command Palette (`Cmd+K`)

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md` — unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9df8439..HEAD -- src/components/CommandPalette.tsx src/components/CommandPalette.test.tsx`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: direction
- **Planned at**: commit `9df8439`, 2026-08-02

## Why this matters

`PRODUCT.md` specifies that Gospel is built for solo developers working in deep focus sessions who rely on quick keyboard navigation. `DESIGN.md` outlines the Command Palette (`Cmd+K`) as the central navigation hub.

Currently, `CommandPalette.tsx` includes a static action entry ("Switch workspace...") that opens the TopBar dropdown menu. However, developers cannot type a workspace name (e.g. `gospel`, `frontend`) directly in `Cmd+K` to search and switch to that workspace. Integrating workspace items into the command palette search results allows instant keyboard-driven workspace switching without taking hands off the keyboard.

## Current state

- File: [`src/components/CommandPalette.tsx`](file:///Users/lifelinelogics/RustProjects/gospel/src/components/CommandPalette.tsx) (lines 5–16):
```typescript
type CommandGroup = "Sessions" | "Files / context" | "Settings" | "Variants" | "Commands";
```
- File: [`src/components/CommandPalette.test.tsx`](file:///Users/lifelinelogics/RustProjects/gospel/src/components/CommandPalette.test.tsx) — tests palette filtering and selection.

## Commands you will need

| Purpose   | Command                                      | Expected on success |
|-----------|----------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                          | exit 0, no errors   |
| Component Test | `bun run test -- CommandPalette`        | all pass            |
| All Tests | `bun run test`                               | all pass            |

## Scope

**In scope**:
- `src/components/CommandPalette.tsx`
- `src/components/CommandPalette.test.tsx`
- `src/components/AppShell.tsx` (wire workspace list and selection handler if missing)

**Out of scope**:
- TopBar layout or workspace selection IPC commands

## Git workflow

- Branch: `advisor/031-command-palette-workspace-switching`
- Commit message style: `feat(ui): add direct workspace searching and switching to command palette`

## Steps

### Step 1: Add "Workspaces" to `CommandGroup` and accept `recentWorkspaces` / `onSelectWorkspace` props

In `src/components/CommandPalette.tsx`:
1. Update `CommandGroup` type definition:
```typescript
type CommandGroup = "Workspaces" | "Sessions" | "Files / context" | "Settings" | "Variants" | "Commands";
```
2. Add optional props to `CommandPaletteProps`:
```typescript
  recentWorkspaces?: Workspace[];
  onSelectWorkspace?: (workspace: Workspace) => void;
```

**Verify**: `bun run typecheck` → exits 0.

### Step 2: Include workspace results in `allResults` list

In `src/components/CommandPalette.tsx`:
In `allResults` memo, map `recentWorkspaces` to `PaletteResult` entries under group `"Workspaces"`:
- `id`: `workspace-${ws.id}`
- `group`: `"Workspaces"`
- `icon`: `"Folder"` (or `FolderGit2` / lucide icon)
- `label`: `ws.name`
- `detail`: `ws.path`
- `keywords`: `workspace folder ${ws.name} ${ws.path}`
- `action`: `closeAfter(() => onSelectWorkspace?.(ws))`

Update `groupResults` ordering to place `"Workspaces"` at top or after `"Sessions"`.

**Verify**: `bun run typecheck` → exits 0.

### Step 3: Wire props in `AppShell.tsx` and add component tests in `CommandPalette.test.tsx`

1. Pass `recentWorkspaces={workspaces}` and `onSelectWorkspace={handleSwitchWorkspace}` in `AppShell.tsx`.
2. Update `src/components/CommandPalette.test.tsx` to add a test case verifying searching for a workspace name filters palette results and selecting it triggers `onSelectWorkspace`.

**Verify**: `bun run test -- CommandPalette` → passes.

## Test plan

- Test in `CommandPalette.test.tsx`:
  - Render `CommandPalette` with `recentWorkspaces = [{ id: "w1", name: "gospel-app", path: "/path/gospel" }]`.
  - Type `"gospel"` into search input.
  - Assert workspace item appears under "Workspaces" group.
  - Simulate click / Enter key on workspace item → assert `onSelectWorkspace` called with `w1`.

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bun run test -- CommandPalette` passes with new test
- [ ] `bun run test` passes completely
- [ ] Typing a workspace name in `Cmd+K` shows matching workspace items
- [ ] `plans/README.md` status row updated

## STOP conditions

- If `Workspace` type does not have `id` or `name`, inspect `src/types.ts` before proceeding.
