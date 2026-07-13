# Plan 002: Add Biome Lint, Format, and Typecheck Configuration

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 2e5bd36..HEAD -- package.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: S
- **Risk**: LOW
- **Depends on**: plans/001-fix-frontend-vitest-harness.md
- **Category**: tech-debt
- **Planned at**: commit `2e5bd36`, 2026-07-11
- **Updated at**: 2026-07-12 — Biome chosen as the single lint/format tool instead of ESLint and Prettier.

## Why this matters

The codebase lacks static analysis and automated formatting commands. Developers and automated agents have no standardized linting, formatting, or standalone type-checking checks. This plan adds Biome for linting and formatting and exposes TypeScript's existing `noEmit` check through a package script.

Using Biome keeps lint and formatting rules in one configuration and avoids maintaining overlapping ESLint and Prettier dependency and configuration stacks.

## Current state

- `package.json` has Vitest scripts but no lint, format, or typecheck scripts.
- `tsconfig.json` has `strict` and `noEmit` configured.
- There is no committed root lint or formatter configuration.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Install dependency | `bun add -d @biomejs/biome` | exit 0 |
| Lint check | `bun run lint` | exit 0, no errors |
| Format write | `bun run format` | exit 0 |
| Format check | `bun run format:check` | exit 0, no unformatted files |
| Typecheck | `bun run typecheck` | exit 0, no errors |
| Frontend tests | `bun run test` | exit 0, all pass |

## Scope

**In scope**:
- `package.json` and `bun.lock` — add Biome and the quality scripts.
- `biome.json` — configure linting, formatting, import organization, and generated-file exclusions.
- Formatting/fixing frontend and repository files reported by Biome.

**Out of scope**:
- Adding Stylelint or a CI pipeline.
- Adding ESLint or Prettier alongside Biome.
- Changing application behavior to resolve a lint or formatting diagnostic.

## Git workflow

- Branch: `advisor/002-add-biome-typecheck`
- Commit message style: `chore: add biome lint, format, and typecheck configuration`

## Steps

### Step 1: Install Biome

Run:

```bash
bun add -d @biomejs/biome
```

**Verify**: `package.json` and `bun.lock` include `@biomejs/biome`.

### Step 2: Configure Biome

Create `biome.json` at the repository root with:

- Git ignore integration.
- Exclusions for dependency, build, generated schema, Rust target, corpus, and agent-tooling directories.
- Two-space indentation, 100-character lines, double quotes, semicolons, and ES5 trailing commas.
- Tailwind CSS v4 directive parsing.
- The recommended lint preset, with project-specific warnings for pre-existing unused-code, hooks, accessibility, type-only import, and explicit-`any` debt.
- Source import organization enabled through Biome's assist configuration.

**Verify**: `bunx biome lint .` parses the configuration without errors or deprecation warnings.

### Step 3: Configure package scripts

Add:

```json
"typecheck": "tsc --noEmit",
"lint": "biome lint .",
"format": "biome format --write .",
"format:check": "biome format ."
```

**Verify**: all four scripts are present in `package.json`.

### Step 4: Run checks and apply formatting

Run:

```bash
bun run typecheck
bun run format
bun run lint
bun run format:check
bun run test
```

Resolve diagnostics only when doing so is mechanical and does not change application behavior.

## Done criteria

- [x] `bun run typecheck`, `bun run lint`, and `bun run format:check` exit 0.
- [x] `bun run test` exits 0.
- [x] `biome.json` is valid for the installed Biome version without configuration deprecations.
- [x] Biome is the only added lint/format dependency.
- [x] No application behavior changes are made to satisfy static analysis.

## STOP conditions

- Package installation fails due to dependency conflicts.
- Fixing a diagnostic requires changing application behavior or unrelated Rust source.
- Biome cannot cover the repository's TypeScript/React lint and formatting needs without adding another tool.
