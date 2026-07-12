# Plan 001: Configure and Enforce Frontend Vitest Execution

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2e5bd36..HEAD -- package.json AGENTS.md src/test/setup.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: tests
- **Planned at**: commit `2e5bd36`, 2026-07-11

## Why this matters

The project uses Vitest for frontend testing. However, `bun test` is a built-in Bun CLI command that runs Bun's native test runner. This runner does not support Vitest-specific features (such as `vi.mock` setup in `src/test/setup.ts` and `vi.mocked` type assertions), causing 87 frontend tests to fail.
To resolve this, we must ensure the codebase clearly documents, configures, and enforces `bun run test` (which executes `vitest run`) for running the frontend test suite, and add defensive shims to avoid crashes.

## Current state

- `package.json` defines the `test` script:
```json
    "test": "vitest run",
    "test:watch": "vitest"
```
- Running `bun test` runs Bun's native test runner instead of Vitest, leading to 87 test failures due to `vi.mocked` being undefined and modules like `@tauri-apps/api/core` remaining unmocked.
- Running `bun run test` executes `vitest run` and successfully runs all 106 tests with 0 failures.

## Commands you will need

| Purpose   | Command         | Expected on success |
|-----------|-----------------|---------------------|
| Run tests | `bun run test`  | exit 0, all pass    |

## Scope

**In scope**:
- `AGENTS.md` - document that the command to run frontend tests is `bun run test` (not `bun test`).
- `src/test/setup.ts` - add a defensive shim for `vi.mocked` to avoid crashes if tests are run in other environments.

**Out of scope**:
- Migrating the test suite from Vitest to Bun's native test runner.
- Changing any production source files.

## Git workflow

- Branch: `advisor/001-fix-frontend-vitest-harness`
- Commit message style: `test: configure and document vitest execution via bun run test`

## Steps

### Step 1: Update AGENTS.md documentation
Update `AGENTS.md` to specify `bun run test` (which runs Vitest) as the correct command to execute frontend tests.

Add a note in `AGENTS.md` explaining the difference between `bun test` (which runs Bun's native test runner and will fail due to Vitest-specific mocks) and `bun run test`.

**Verify**: Check that `AGENTS.md` contains the correct instructions.

### Step 2: Add defensive shim to test setup
Edit `src/test/setup.ts` to add a defensive fallback shim for `vi.mocked` if it is undefined (e.g. if run in environments where `vi` is shimmed).

```typescript
// Shim vi.mocked for environments where it is missing
if (typeof vi !== 'undefined' && !vi.mocked) {
  (vi as any).mocked = <T>(fn: T): any => fn;
}
```

**Verify**: Run `bun run test` and check that all 106 tests pass.

## Done criteria

- [ ] `bun run test` exits 0 with all 106 tests passing.
- [ ] `AGENTS.md` documents `bun run test` as the standard test execution command.
- [ ] No files outside the in-scope list are modified.

## STOP conditions

- `bun run test` fails to run or fails any of the 106 tests.
- Modifications are needed in out-of-scope files.
