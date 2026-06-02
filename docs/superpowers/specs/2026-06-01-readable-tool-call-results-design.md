# Readable Tool Call Results Design

## Status

Approved in conversation. Browser mockup selected `structured-tool-card`.

## Goal

Make Gospel tool-call activity readable at a glance without hiding the raw data developers may need for debugging.

The current UI turns tool arguments and results into one raw text block. That is technically complete, but it makes every tool result feel like a JSON dump. The new design should show the useful facts first, preserve the full payload, and keep the interface quiet enough for long coding sessions.

## Register And Scene

Register: product.

Scene: a developer in a dark room during a focused coding session watches the agent inspect the workspace; they need to understand what happened in seconds without parsing JSON by hand.

This keeps the existing dark, restrained product language. Color remains semantic only: action, structure, signal, data, status.

## Decision

Implement a frontend formatter layer for action cards.

Known tools receive specific, structured renderers. Unknown tools fall back to a polished generic JSON view. The backend event contract remains unchanged for this pass.

This is preferred because:

- it is the smallest correct change
- backend tool payloads already include enough structured data
- it avoids coupling Rust tool contracts to presentation details
- it can improve live cards and stored transcript cards through the same path

## UX Shape

Each tool card has three levels of information.

1. Header
   Shows icon, human label, one compact argument summary, and status.

2. Structured body
   Shows the most relevant fields for the tool: counts, path, pattern, truncation state, preview rows, or report excerpts.

3. Raw payload
   A secondary `Show raw JSON` disclosure that preserves arguments and results exactly as received.

The default expanded view should never start with raw JSON when the tool is recognized.

## Tool-Specific Renderers

### `read_file`

Header summary:

- `path`
- optional line range

Body:

- path
- line range
- total lines
- size
- truncated state
- content preview in monospace

Failure body:

- reason
- message

### `search_code`

Header summary:

- pattern
- optional path
- optional include glob

Body:

- match count
- scanned file count
- skipped file count
- truncated state
- first matches as rows with path, line, and text preview

Failure body:

- reason
- message, especially invalid regex feedback

### `find_files`

Header summary:

- glob
- optional path

Body:

- file count
- scanned entry count
- truncated state
- first files as monospace path rows

### `list_directory`

Header summary:

- path or workspace root
- depth

Body:

- entry count
- visited entry count
- truncated state
- rows grouped visually by directory and file using labels, not color alone

### `delegate_exploration`

Header summary:

- task preview

Body:

- success state
- truncated state
- tools used count or list
- report excerpt with fixed sections preserved

### Corpus tools

Render `corpus_summary`, `corpus_query`, and `corpus_neighbors` with the same system:

- show structural counts first
- show node names and relation counts as rows
- keep raw JSON behind the disclosure

## Generic Fallback

Unknown tools use a generic structured fallback:

- header label from title-cased tool name
- arguments section if present
- result section if present
- pretty-printed JSON with indentation
- non-JSON strings shown as monospace text

If parsing fails, the card still renders safely as text.

## Component Design

Keep `ActionCard` as the rendering component, but change its data contract from one `content` string to structured sections.

Expected shape:

- `summary`: card title
- `detail`: compact header detail
- `sections`: display sections for key-values, rows, prose, code, or raw text
- `rawPayload`: optional exact payload text
- `status`: calling or completed

The card remains keyboard accessible:

- main button toggles expanded state
- raw JSON disclosure is a separate button when available
- `aria-expanded` reflects each disclosure
- text labels accompany status color

## Visual Rules

- No decorative glass, gradients, or side-stripe accents.
- Borders are full 1px borders with semantic icon color, not thick side accents.
- Monospace is used for paths, patterns, and output previews.
- Body rows use surface contrast, not heavy color.
- Running status remains visible in text and color.
- Mobile and narrow windows collapse metric fields into one column.

## Non-Goals

- No backend event schema changes in this pass.
- No full diff viewer for tool output.
- No copy, retry, or open-file actions unless already supported by existing card actions.
- No virtualized JSON tree. Payloads are already capped by backend tools.

## Verification

- TypeScript build passes.
- Known tool result samples render without throwing.
- Unknown tool payloads still render through fallback.
- Reduced motion behavior remains unchanged.
- Manual check in the app confirms live action cards and completed transcript cards use the same readable format.
