# Scenario — `edit-and-save`

> Layer 5 (agentic) per ADR 0004. Manual trigger; an agent executes the steps
> and emits the verdict format below.

**Status**: skeleton — content lands with #17 once the file-watcher self-write
loop is fixed.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`).
- Frontend reachable in a browser (Chrome DevTools MCP / Playwright).
- A pipeline `editable.yaml` exists in `.maestro/pipelines/` — at least one
  node, simple structure.

## Steps the agent executes

1. Open the UI and switch to Edit mode.
2. Modify a node's prompt content (type a recognizable marker like
   `__edit_marker_<timestamp>__`).
3. Add a new node by drag-drop or command.
4. Wait at least 3 seconds (debounce + roundtrip + save).
5. Reload the page.
6. Confirm the marker text and the new node persist after reload.
7. Read `.maestro/pipelines/editable.yaml` from disk and assert the marker is
   present in the YAML or its sidecar prompt file.
8. Negative check: while still in Edit mode, leave the cursor in a node prompt
   for ~5 seconds. The cursor must not jump or lose focus, and the typed text
   must not be erased mid-edit.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "<one observation per line>"
  ],
  "anomalies": [
    "<optional>"
  ]
}
```
