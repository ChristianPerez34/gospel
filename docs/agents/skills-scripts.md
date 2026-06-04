# Skills Script Execution

## Overview

Skills can bundle executable scripts in a `scripts/` directory alongside the `SKILL.md`. The LLM can invoke these scripts via the `run_skill_script` tool.

## Interpreter Detection

The interpreter is determined by reading the first line of the script:

1. **Shebang present**: Use the shebang path (e.g., `#!/bin/bash` → `/bin/bash`).
2. **No shebang, `.mjs` or `.js` extension**: Default to `node`.
3. **No shebang, `.sh` extension**: Default to `bash`.
4. **No shebang, unrecognized extension**: Return an error string ("no executor"). The LLM can recover from this.

## Timeout

- **Default**: 30 seconds.
- **Override**: The skill's `timeout-seconds:` frontmatter field overrides the default.
- Scripts exceeding the timeout are killed and an error is returned to the LLM.

## Output Caps

- **16 KiB** per-stream cap on stdout and stderr.
- If output exceeds the cap, it is truncated and the `truncated` flag in the response is `true`.

## Path Guard

The script path is resolved via `canonicalize()`. The canonical script path must start with the canonical skill directory. This prevents symlink escapes:

```
canonicalize(skill_dir) == canonicalize(script_path.parent())
```

If the check fails, an error string is returned to the LLM (not a panic).

## Symlink Escape Rejection

Symlinked skill directories that point outside the skills root are rejected during discovery. The `canonicalize()` check ensures that even if a symlink is placed in the skills directory, it cannot execute scripts outside the skill's own directory.

## Fail-Soft on Permission Errors

Scripts with permission errors (e.g., unreadable files) are skipped with a `tracing::warn!`. The tool returns an error string to the LLM, not a panic.
