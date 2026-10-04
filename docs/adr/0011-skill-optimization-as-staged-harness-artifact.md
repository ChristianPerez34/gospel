# ADR 0011: Skill Optimization as a Staged Harness Artifact

## Status

Accepted

## Context

Gospel skills are static plugins (ADR-0003). Auto-match injects name and description only; slash invocation injects the full body. `source_edit` cannot write `.agents/**`, and `write_harness_file` is `.gospel/` only. Trace Log is observability, never agent-readable memory. A Skill Optimization Run therefore cannot treat discovered skills or traces as a live training loop.

The alternative — auto-writing `.agents/skills/` or evolving skills inside a Session Turn — would mix instruction with unverified self-edit and fight version control.

## Decision

Optimize one named skill offline as harness substrate, not as a discovered Skill:

1. Harvest Skill Invocations from Display Transcripts of workspace-affine Sessions (user text plus tool *names*, never tool arguments, results, Model History, or Trace Log).
2. Apply a bounded add/delete/replace patch budget (default 4) to the skill body. Step-level edits cannot overwrite a fenced slow-update region.
3. Accept a candidate only when the Selection Gate sees a strictly higher mean score on held-out Skill Tasks. Ties reject; rejected edits go to the Rejected Edit Buffer.
4. Write accepted candidates under `.gospel/skill-opt/<name>/`. Discovered skills change only through Skill Adoption.
5. Skills remain instruction (ADR-0008). Optimization must not become tool-registration policy.

No optimizer model is invoked at inference time. Student turns that evaluate a candidate inject the full body the same way Skill Invocation does.

## Consequences

- Workspace skills stay human-owned until Skill Adoption.
- Auto-match remains description-only; optimization targets the invoked full body.
- `.gospel/skill-opt/` is local harness state, not a Skill Source.
