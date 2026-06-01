# ADR 0002: Workspace-Scoped Live Exploration

## Status

Accepted

## Context

Gospel's chat Agent previously depended on the corpus for workspace-aware codebase help. That made corpus availability a hard dependency for streaming chat and left no safe path for direct file inspection, scoped search, or directory discovery. Corpus tools also accepted workspace overrides and could fall back to the process current directory, which was too loose for chat-time exploration.

## Decision

Add live workspace exploration tools that are scoped to the active Gospel workspace and governed by one shared safety policy.

The live tools:

- read safe text files with line, byte, and display caps
- search safe text files with deterministic result caps
- discover files and directories with deterministic traversal and broad-tool ignore rules
- reject path escapes, including symlink escapes
- block secret-like, risky hidden, binary, and oversized targets

Corpus remains useful as indexed structural knowledge, but it is no longer the source of truth for file contents and it is no longer required for streaming chat to inspect a workspace.

When corpus auto-build fails during streaming chat, Gospel emits the existing failure event and continues the turn with live workspace tools only.

## Consequences

- Workspace-aware chat no longer falls back to the process current directory.
- The active Gospel workspace becomes the only root for chat-time codebase exploration.
- Source-of-truth reads come from live workspace tools, while corpus remains an optional orientation layer.
- Broad investigations can be delegated to a backend Exploration Agent that uses the same workspace and safety policy without recursive delegation.
