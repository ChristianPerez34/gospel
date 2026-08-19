# Plan 030: Add Unmount Safety and Unit Tests to `useMcpServers` Hook

> **Executor instructions**: Follow this plan step by step. Run every verification command and confirm the expected result before moving to the next step. If anything in the "STOP conditions" section occurs, stop and report — do not improvise. When done, update the status row for this plan in `plans/README.md` — unless a reviewer dispatched you and told you they maintain the index.
>
> **Drift check (run first)**: `git diff --stat 9df8439..HEAD -- src/hooks/useMcpServers.ts`
> If any in-scope file changed since this plan was written, compare the "Current state" excerpts against the live code before proceeding; on a mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `9df8439`, 2026-08-02

## Why this matters

The `useMcpServers` React hook fetches and manages MCP server configurations from the Tauri backend via `invoke("list_mcp_servers")`. Currently, when `active` becomes true, `useEffect` triggers `reload()` without checking whether the component remains mounted when the promise resolves. If a user closes the settings modal or switches tabs while an IPC call is in-flight, `setServers`, `setError`, and `setLoading` are called on an unmounted component.

Additionally, `useMcpServers` has zero test coverage in `src/hooks/`. Adding unmount safety guards and a comprehensive Vitest unit test suite prevents memory leaks and state desynchronization.

## Current state

- File: [`src/hooks/useMcpServers.ts`](file:///Users/lifelinelogics/RustProjects/gospel/src/hooks/useMcpServers.ts) (lines 22–38):
```typescript
  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await invoke<McpServer[]>("list_mcp_servers");
      setServers(next);
    } catch (e) {
      setError(`Failed to load MCP servers: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void reload();
  }, [active, reload]);
```
- Conventions: Custom hook unit tests use `@testing-library/react` `renderHook` and `vi.mock("@tauri-apps/api/core")`. See [`src/hooks/useModelAvailability.test.ts`](file:///Users/lifelinelogics/RustProjects/gospel/src/hooks/useModelAvailability.test.ts) as exemplar.

## Commands you will need

| Purpose   | Command                                  | Expected on success |
|-----------|------------------------------------------|---------------------|
| Typecheck | `bun run typecheck`                      | exit 0, no errors   |
| Lint      | `bun run lint`                           | exit 0              |
| Unit Test | `bun run test -- src/hooks/useMcpServers`| all pass            |
| All Tests | `bun run test`                           | all pass            |

## Scope

**In scope**:
- `src/hooks/useMcpServers.ts`
- `src/hooks/useMcpServers.test.ts` (create)

**Out of scope**:
- `src/components/McpSettingsPanel.tsx`
- Any Tauri IPC backend rust code

## Git workflow

- Branch: `advisor/030-unmount-safety-use-mcp-servers`
- Commit message style: `fix(hooks): add unmount cleanup guard and tests for useMcpServers`

## Steps

### Step 1: Add unmount guard to `useMcpServers` `useEffect` and `reload`

In `src/hooks/useMcpServers.ts`:
1. Maintain an `isMounted` ref or local `cancelled` flag inside `useEffect`.
2. Allow `reload` to take an optional `mountedRef?: { current: boolean }` parameter or check component mount status before updating state:

```typescript
  useEffect(() => {
    if (!active) return;
    let mounted = true;
    
    setLoading(true);
    setError(null);
    invoke<McpServer[]>("list_mcp_servers")
      .then((next) => {
        if (mounted) setServers(next);
      })
      .catch((e) => {
        if (mounted) setError(`Failed to load MCP servers: ${e}`);
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });

    return () => {
      mounted = false;
    };
  }, [active]);
```

**Verify**: `bun run typecheck` → exits 0.

### Step 2: Create unit test suite `src/hooks/useMcpServers.test.ts`

Create `src/hooks/useMcpServers.test.ts` modeled after `useModelAvailability.test.ts`. Test:
- Initial state when `active = false` (does not invoke backend).
- Successful load of MCP servers when `active = true`.
- Error handling when `invoke("list_mcp_servers")` rejects.
- Unmount behavior (unmounting during in-flight promise does not update state or throw warning).
- Toggling server enabled state (`setEnabled`).

**Verify**: `bun run test -- src/hooks/useMcpServers` → all tests pass.

### Step 3: Run Biome lint & format checks

Run `bun run lint` and `bun run format:check`.

**Verify**: `bun run check` → exits 0.

## Test plan

- Test cases in `src/hooks/useMcpServers.test.ts`:
  1. `does not fetch servers when active is false`
  2. `fetches servers and sets state when active is true`
  3. `handles IPC rejection gracefully`
  4. `ignores async response if unmounted before promise resolves`
  5. `updates server enabled state on setEnabled call`

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bun run test -- src/hooks/useMcpServers` exits 0 with 5 passing tests
- [ ] `bun run test` passes completely
- [ ] `plans/README.md` status row updated

## STOP conditions

- If `useMcpServers` return type or function signature is changed, stop and report (components consume `servers`, `loading`, `error`, `reload`, `setEnabled`, `trust`, etc.).
