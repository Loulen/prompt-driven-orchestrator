# Scenario — `edit-and-save`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format below. Asserts that the explicit Save button
> flow (introduced in #35) persists edits to disk and that the daemon's
> pipeline_watcher broadcast does not wipe unsaved in-memory state (the
> regression originally caught by Bug E / Bug F from #17).

## Setup

- Daemon running on the user's repo (default `http://127.0.0.1:5172`).
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- A test pipeline named `edit-and-save-scenario.yaml` seeded in
  `.maestro/pipelines/`. If it isn't already there, the agent creates it
  before driving the UI:

  ```yaml
  name: edit-and-save-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: alpha
      name: alpha
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 200, y: 100 }
    - id: beta
      name: beta
      type: doc-only
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 400, y: 100 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      outputs: []
      view: { x: 600, y: 100 }
  edges: []
  ```

## Steps the agent executes

1. Open the UI, confirm the **`Daemon: connected`** label is visible in the
   status bar.
2. Click the **edit-mode toggle** (pencil icon, top-right). Verify the badge
   switches from `Run` to `Edit`.
3. Click the `edit-and-save-scenario` row in the **Pipelines** sidebar. The
   canvas should render four nodes (`Start`, `alpha`, `beta`, `End`).
4. Click the `alpha` node. The **Node Inspector** opens on the right with a
   `Prompt` textarea (placeholder `Enter the node's role prompt...`).
5. In the prompt textarea, type `MARKER_<timestamp>_SAVE_TEST` and stop typing.
6. Assert the tab title reads **`• edit-and-save-scenario.yaml`** (dirty
   indicator `•` is present) and the **Save** button in the TabBar is
   **enabled** (not greyed out).
7. Click the **Save** button (or press `Ctrl+S` / `Cmd+S`).
8. Assert:
   - The dirty indicator `•` disappears from the tab title (it now reads
     `edit-and-save-scenario.yaml`).
   - **"Saved just now"** (or **"Saved Xs ago"**) text appears near the Save
     button (`data-testid="saved-ago"`).
   - The Save button is now **disabled** (no unsaved changes).
9. Reload the page (`F5` or `navigate_page`). Re-toggle Edit mode, re-open
   the `edit-and-save-scenario` pipeline, click the `alpha` node.
10. Assert the prompt textarea displays **`MARKER_<timestamp>_SAVE_TEST`** —
    the value persisted through save and page reload.
11. Read `.maestro/pipelines/edit-and-save-scenario.prompts/alpha.md` from
    disk. Its content **must equal** the marker value typed in step 5 — proves
    the Save button wrote through to the filesystem.

## Negative path — broadcast race (refs #17)

This section tests that the daemon's pipeline_watcher does not wipe unsaved
in-memory edits. The property originally caught by Bug E / Bug F still holds
under the explicit Save model.

12. With the `alpha` node still selected after step 11, append
    `_THEN_UNSAVED_<timestamp>` to the existing prompt textarea content. The
    full value should now read
    `MARKER_<timestamp>_SAVE_TEST_THEN_UNSAVED_<timestamp>`.
13. **Do NOT click Save.** Wait at least 3 s for the daemon's pipeline_watcher
    debounce window to elapse.
14. Assert the textarea **still contains the full combined value** including
    the unsaved suffix. The watcher's broadcast must not have wiped the
    in-memory state. Take a screenshot.
15. Assert the tab title still shows the dirty indicator `•` — the unsaved
    edit has not been silently discarded or auto-saved.
16. Read `.maestro/pipelines/edit-and-save-scenario.prompts/alpha.md` from
    disk. Its content **must still equal** `MARKER_<timestamp>_SAVE_TEST`
    (from the earlier explicit save in step 7) — proves no auto-save occurred.

## Cleanup

- Delete `.maestro/pipelines/edit-and-save-scenario.yaml`.
- Delete `.maestro/pipelines/edit-and-save-scenario.prompts/` if it was
  created during the run.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: status bar shows 'Daemon: connected'",
    "step 6: tab shows dirty indicator '•', Save button enabled",
    "step 8: dirty indicator cleared, 'Saved Xs ago' visible",
    "step 10: textarea reads MARKER_…_SAVE_TEST after reload",
    "step 11: alpha.md on disk equals MARKER_…_SAVE_TEST",
    "step 14: textarea still reads full value after 3 s wait (no broadcast wipe)",
    "step 15: tab title still shows dirty indicator '•' (no silent auto-save)",
    "step 16: alpha.md on disk unchanged (no auto-save)"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
