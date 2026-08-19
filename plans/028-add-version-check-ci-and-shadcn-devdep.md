# Plan 028: Add Version Check Step to PR CI Workflow and Move shadcn to devDependencies

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md` — unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9df8439..HEAD -- .github/workflows/pr.yml package.json`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: dx
- **Planned at**: commit `9df8439`, 2026-08-02

## Why this matters

1. **Version Sync CI Safety**: Gospel maintains package version alignment between `src-tauri/Cargo.toml`, `package.json`, and `src-tauri/tauri.conf.json` via `scripts/sync-version.py`. However, the PR workflow `.github/workflows/pr.yml` does not execute `bun run version:check`. Mismatched version tags can be merged to `main` without detection, breaking release builds.
2. **Dependency Hygiene**: `shadcn` is listed in `package.json` under `"dependencies"` (runtime production dependencies). Because `shadcn` is a developer component code-generation CLI, it belongs in `"devDependencies"`.

Adding `bun run version:check` to `pr.yml` and moving `shadcn` to `devDependencies` ensures strict release version integrity and clean production metadata.

## Current state

- File: [`.github/workflows/pr.yml`](file:///Users/lifelinelogics/RustProjects/gospel/.github/workflows/pr.yml) (lines 46–63):
```yaml
      - name: Install dependencies
        run: bun install --frozen-lockfile

      - name: Run TypeScript Typecheck
        run: bun run typecheck
```
- File: [`package.json`](file:///Users/lifelinelogics/RustProjects/gospel/package.json) (lines 37, 42–57):
```json
37:     "shadcn": "^4.11.0",
...
42:   "devDependencies": {
```

## Commands you will need

| Purpose         | Command                                | Expected on success |
|-----------------|----------------------------------------|---------------------|
| Version Check   | `bun run version:check`                | exit 0              |
| Typecheck       | `bun run typecheck`                    | exit 0, no errors   |
| Biome Format    | `bun run format:check`                 | exit 0              |
| Biome Check     | `bun run lint`                         | exit 0              |

## Scope

**In scope**:
- `.github/workflows/pr.yml`
- `package.json`

**Out of scope**:
- `scripts/sync-version.py`
- `bun.lock` (re-run `bun install` if lockfile updates are needed)

## Git workflow

- Branch: `advisor/028-add-version-check-ci-and-shadcn-devdep`
- Commit message style: `ci(workflow): add version check to PR workflow and move shadcn to devDependencies`

## Steps

### Step 1: Move `shadcn` to `devDependencies` in `package.json`

In `package.json`:
- Remove `"shadcn": "^4.11.0"` from `"dependencies"`.
- Add `"shadcn": "^4.11.0"` to `"devDependencies"`.

**Verify**: `bun run typecheck` → exits 0.

### Step 2: Add `version:check` step to `.github/workflows/pr.yml`

In `.github/workflows/pr.yml`, add a step immediately after `Install dependencies`:

```yaml
      - name: Verify Version Alignment
        run: bun run version:check
```

**Verify**: `bun run version:check` → exits 0.

### Step 3: Run formatting check and full checks

Run `bun run check` and `bun run format:check` to confirm lockfile and formatting consistency.

**Verify**: `bun run check` → exits 0.

## Test plan

- Test version check step locally:
  - Run `bun run version:check` → confirm exit 0.
  - Temporarily modify version in `package.json` to `0.0.0-test` and run `bun run version:check` → confirm non-zero failure output.
  - Revert temporary version modification.

## Done criteria

- [ ] `bun run version:check` passes cleanly
- [ ] `.github/workflows/pr.yml` contains `bun run version:check` step
- [ ] `package.json` lists `shadcn` under `devDependencies` and not `dependencies`
- [ ] `bun run check` exits 0
- [ ] `plans/README.md` status row updated

## STOP conditions

- If `bun run version:check` fails on clean `main`, fix version sync first before modifying CI workflow.
