# Plan 038: Synchronize Autonomy Policy Across `AppShell` and `SettingsStudio`

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 04a60d3..HEAD -- src/hooks/useAutonomyPolicy.ts src/hooks/useAutonomyPolicy.test.ts src/main.tsx src/components/AppShell.tsx src/components/AppShell.test.tsx`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW — lifting `useAutonomyPolicy` into a shared React context with a standalone fallback mirrors `WorkspacesProvider` and keeps isolated hook/component tests passing unchanged.
- **Depends on**: none
- **Category**: security
- **Planned at**: commit `04a60d3`, 2026-11-25

## Why this matters

`useAutonomyPolicy` currently stores `ApprovalPolicy` in component-local `useState`, and both `AppShell` and `SettingsStudio` call `useAutonomyPolicy()` independently. When a developer switches Autonomy Mode (Ask vs. Auto) or edits allow/deny rules in `SettingsStudio`, the backend `ApprovalBroker` switches immediately — in Auto mode (ADR-0014), Gospel runs every command tool and skill script without approval, including destructive, restricted, and workspace-escaping commands — while `AppShell`'s `TopBar` keeps showing the stale mode ("Ask" even while Auto is live, or vice versa) and toggles from stale state on click. Sharing one policy instance across the app (and refreshing when an `always_allow` approval persists a rule) keeps the visible safety indicator accurate at all times.

## Current state

Relevant files:

- `src/hooks/useAutonomyPolicy.ts` — defines `useAutonomyPolicy()` with local `useState<ApprovalPolicy>(DEFAULT_POLICY)` and IPC wrappers (`get_approval_policy`, `set_autonomy_mode`, `add_approval_allow_rule`, `add_approval_reject_rule`, `remove_approval_rule`, `clear_approval_rules`).
- `src/main.tsx` — mounts root providers (`WorkspacesProvider`) around `<App />`.
- `src/components/AppShell.tsx` — calls `useAutonomyPolicy()` at lines 228–232 to drive `TopBar` (`autonomyMode`, `setAutonomyMode`) and `autonomyPolicy`, and mounts `<SettingsModal>` (which renders `SettingsStudio`).
- `src/components/SettingsStudio.tsx` — calls `useAutonomyPolicy()` at line 216 independently of `AppShell`.
- `src/hooks/useAutonomyPolicy.test.ts` — tests `useAutonomyPolicy()` via `renderHook(() => useAutonomyPolicy())` without a provider.
- `src/components/SettingsStudio.test.tsx` — renders `<SettingsStudio ... />` directly without a provider.
- `src/components/AppShell.test.tsx` — renders `<WorkspacesProvider><AppShell /></WorkspacesProvider>` without `main.tsx`.

Current excerpts:

```ts
// src/hooks/useAutonomyPolicy.ts:23-39
export function useAutonomyPolicy(): UseAutonomyPolicyReturn {
  const [policy, setPolicy] = useState<ApprovalPolicy>(DEFAULT_POLICY);
  const [loading, setLoading] = useState(true);

  const refresh = useCallback(async () => {
    try {
      const current = await invoke<ApprovalPolicy>("get_approval_policy");
      if (current && typeof current === "object" && "mode" in current) {
        setPolicy(current);
      }
    } catch (e) {
      console.error("[useAutonomyPolicy] failed to fetch policy:", e);
    }
  }, []);
```

```tsx
// src/main.tsx:12-18
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <WorkspacesProvider>
      <App />
    </WorkspacesProvider>
  </React.StrictMode>
);
```

```tsx
// src/components/AppShell.tsx:228-232
  const {
    mode: autonomyMode,
    setMode: setAutonomyMode,
    policy: autonomyPolicy,
  } = useAutonomyPolicy();
```

Repo convention exemplar — `src/hooks/useWorkspaces.ts:21-103` uses `createContext` + `createElement(Context.Provider, { value }, children)` in a `.ts` hook file so no `.tsx` rename is needed:

```ts
// src/hooks/useWorkspaces.ts:21-24,102
const WorkspacesContext = createContext<UseWorkspacesReturn | null>(null);

export function WorkspacesProvider({ children }: { children: ReactNode }) {
  // ...
  return createElement(WorkspacesContext.Provider, { value }, children);
}
```

Documented constraints to honor:

- `CONTEXT.md` (Autonomy Mode / ADR-0014): *"The mode is global. Switching to Auto approves every pending command request in every session except those matching a User Deny Rule; switching to Ask denies pending restricted commands."*
- `AGENTS.md`: Always run frontend tests with `bun run test` (Vitest), never `bun test`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Typecheck | `bun run typecheck` | exit 0, no errors |
| Lint/Format check | `bunx biome check src` | exit 0, no errors (0 new warnings) |
| Targeted tests | `bun run test -- src/hooks/useAutonomyPolicy.test.ts src/components/AppShell.test.tsx src/components/SettingsStudio.test.tsx` | all pass |
| Full frontend tests | `bun run test` | all pass |

## Scope

**In scope** (the only files you should modify):
- `src/hooks/useAutonomyPolicy.ts`
- `src/hooks/useAutonomyPolicy.test.ts`
- `src/main.tsx`
- `src/components/AppShell.tsx` (only if wrapping or refreshing policy on `always_allow` resolution; see Steps 2–3)
- `src/components/AppShell.test.tsx`

**Out of scope** (do NOT touch):
- `src/components/SettingsStudio.tsx` — continues calling `useAutonomyPolicy()`; once it lives inside `AutonomyPolicyProvider`, it reads and mutates the shared policy automatically.
- `src/components/SettingsStudio.test.tsx` — must keep passing without changes because `useAutonomyPolicy()` falls back safely when rendered outside a provider.
- `src-tauri/src/approval_broker.rs` or any Rust file — backend policy is already global and single-instance.

## Git workflow

- Branch: `advisor/038-sync-autonomy-policy-state`
- Commit style: conventional commits matching `git log` (e.g., `fix(autonomy): share autonomy policy state across AppShell and SettingsStudio`)
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Add `AutonomyPolicyProvider` with standalone fallback in `src/hooks/useAutonomyPolicy.ts`

1. Extract the existing hook body in `src/hooks/useAutonomyPolicy.ts` into a private helper `useAutonomyPolicyState(enabled: boolean): UseAutonomyPolicyReturn`.
   - When `enabled` is `false` (meaning a parent `AutonomyPolicyProvider` is already mounted), the `useEffect` must NOT call `refresh()` (no duplicate `get_approval_policy` IPC calls per consumer).
2. Create `const AutonomyPolicyContext = createContext<UseAutonomyPolicyReturn | null>(null);` and export `AutonomyPolicyProvider({ children }: { children: ReactNode })` using `createElement(AutonomyPolicyContext.Provider, { value }, children)` (matching `src/hooks/useWorkspaces.ts`), calling `useAutonomyPolicyState(true)` once at the provider level.
3. Update `export function useAutonomyPolicy(): UseAutonomyPolicyReturn`:
   - Read `const context = useContext(AutonomyPolicyContext);`
   - Call `const fallback = useAutonomyPolicyState(context === null);` unconditionally (respecting React's Rules of Hooks).
   - Return `context ?? fallback`.
4. Ensure `useAutonomyPolicyState(true)` also listens (or exposes `refresh`) cleanly: keep all existing methods (`policy`, `mode`, `loading`, `setMode`, `addAllowRule`, `addRejectRule`, `removeRule`, `clearRules`, `refresh`) and their exact error-handling behavior intact so every existing test in `useAutonomyPolicy.test.ts` continues to pass.

**Verify**: `bun run test -- src/hooks/useAutonomyPolicy.test.ts` → all existing tests pass.

### Step 2: Mount `AutonomyPolicyProvider` at the app root and inside `AppShell`

1. In `src/main.tsx`, wrap `<WorkspacesProvider>` / `<App />` with `<AutonomyPolicyProvider>`:
   ```tsx
   ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
     <React.StrictMode>
       <AutonomyPolicyProvider>
         <WorkspacesProvider>
           <App />
         </WorkspacesProvider>
       </AutonomyPolicyProvider>
     </React.StrictMode>
   );
   ```
2. In `src/components/AppShell.tsx`, make sure `<AppShell />` works identically even when mounted directly in tests without `main.tsx` (as in `src/components/AppShell.test.tsx`):
   - Rename the internal implementation component to `function AppShellContent()` and export:
     ```tsx
     export function AppShell() {
       return (
         <AutonomyPolicyProvider>
           <AppShellContent />
         </AutonomyPolicyProvider>
       );
     }
     ```
   - Wait: to avoid mounting two nested providers in production (`main.tsx` + `AppShell`), have `AutonomyPolicyProvider` check `const parent = useContext(AutonomyPolicyContext); if (parent !== null) return createElement(Fragment, null, children);`! That way `main.tsx` provides one root instance for the whole app, and any standalone `<AppShell />` render in tests automatically gets a shared provider for `AppShell` + `SettingsModal` + `SettingsStudio` with zero duplicate `get_approval_policy` fetches.
3. In `AppShellContent` (`src/components/AppShell.tsx`), inspect where approval resolution (`resolve_approval`) is invoked: when `decision === "always_allow"` (or `"always_deny"` if supported), call `void refreshAutonomyPolicy()` after `invoke("resolve_approval", ...)` succeeds so newly persisted rules show up immediately in `SettingsStudio` without reloading.

**Verify**: `bun run typecheck && bun run test -- src/components/AppShell.test.tsx src/components/SettingsStudio.test.tsx` → exit 0, all tests pass.

### Step 3: Add synchronization tests in `useAutonomyPolicy.test.ts` and `AppShell.test.tsx`

Add the tests described in the Test plan below, then run the full frontend checks.

**Verify**: `bun run typecheck && bunx biome check src && bun run test` → exit 0, all tests pass.

## Test plan

1. **`src/hooks/useAutonomyPolicy.test.ts`** (model after existing `describe("useAutonomyPolicy", ...)` tests):
   - Two consumers calling `useAutonomyPolicy()` inside one `<AutonomyPolicyProvider>`: calling `setMode("auto")` on consumer A immediately updates `mode === "auto"` on consumer B, and `get_approval_policy` is only invoked once on mount (not once per consumer).
   - Calling `addAllowRule` / `addRejectRule` / `removeRule` / `clearRules` on consumer A updates consumer B's `policy`.
   - Nested `<AutonomyPolicyProvider>` reuses the parent context (still only one `get_approval_policy` call and shared state).
   - Calling `useAutonomyPolicy()` without a provider still initializes and updates standalone (preserving backward compatibility).
2. **`src/components/AppShell.test.tsx`** (model after existing `AppShell` tests):
   - Open Settings (`SettingsStudio`), switch Autonomy Mode from Ask to Auto (or toggle in Settings), close Settings (or inspect `TopBar` while open), and assert `TopBar`'s autonomy indicator reflects `"Auto"` and clicking `TopBar` toggles from `"auto"` back to `"ask"` (calling `set_autonomy_mode` with `{ mode: "ask" }`, not `"auto"`).

**Verification**: `bun run test -- src/hooks/useAutonomyPolicy.test.ts src/components/AppShell.test.tsx src/components/SettingsStudio.test.tsx` → all pass, including ≥ 4 new tests.

## Done criteria

- [ ] `bun run typecheck` exits 0
- [ ] `bunx biome check src` exits 0 with no new warnings
- [ ] `bun run test` exits 0; all existing tests and ≥ 4 new synchronization tests pass
- [ ] Only in-scope files are modified (`git status`)
- [ ] `plans/README.md` status row for `038` updated to `DONE`

## STOP conditions

Stop and report back (do not improvise) if:

- `src/hooks/useAutonomyPolicy.ts` already exports a provider or context (codebase drifted).
- Any test in `src/components/AppShell.test.tsx` fails due to unexpected extra `get_approval_policy` calls after wrapping `AppShell` — inspect why nested-provider deduplication didn't run before proceeding.
- A step's verification fails twice after a reasonable fix attempt.
- The fix appears to require modifying `SettingsStudio.tsx` or any Rust file.

## Maintenance notes

- If any future surface (e.g., `CommandPalette` or a background tray control) displays or toggles Autonomy Mode, it should call `useAutonomyPolicy()` inside `AutonomyPolicyProvider` rather than calling `invoke("set_autonomy_mode", ...)` directly.
- If the backend ever changes `ApprovalPolicy` autonomously (outside a frontend IPC call), emit a Tauri event (e.g., `approval-policy-changed`) and subscribe to it once inside `AutonomyPolicyProvider` so every consumer updates automatically.
