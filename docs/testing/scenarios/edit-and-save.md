# Scenario — `edit-and-save`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format below. Asserts the regression fix landed for
> Bug E (#17) — the daemon-side self-write loop and Bug F (read-triggered
> watcher events) which together caused edits to vanish ~1.5 s after typing.

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
    - id: alpha
      type: doc-only
      prompt_file: edit-and-save-scenario.prompts/alpha.md
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 100, y: 100 }
    - id: beta
      type: doc-only
      prompt_file: edit-and-save-scenario.prompts/beta.md
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 400, y: 100 }
  edges: []
  ```

## Steps the agent executes

1. Open the UI, confirm the **`Daemon: connected`** label is visible in the
   status bar.
2. Click the **edit-mode toggle** (pencil icon, top-right). Verify the badge
   switches from `Run` to `Edit`.
3. Click the `edit-and-save-scenario` row in the **Pipelines** sidebar. The
   canvas should render two nodes (`alpha`, `beta`).
4. Click the `alpha` node. The **Node Inspector** opens on the right with a
   `Prompt` textarea (placeholder `Enter the node's role prompt...`).
5. In the prompt textarea, type `MARKER_<timestamp>_FIRST` and stop typing.
6. **Wait at least 2.5 s** without further interaction. This covers:
    - the 1.5 s frontend save debounce, then
    - the 1 s daemon watcher debounce.
   The textarea **must still display the marker text** during this entire
   window. Take a screenshot when the wait ends.
7. **Without leaving Edit mode**, in the same textarea append
   `_THEN_SECOND_<timestamp>` to the existing content. The full value should
   now read `MARKER_<timestamp>_FIRST_THEN_SECOND_<timestamp>`.
8. Wait another 3 s, then assert the textarea **still contains both markers
   end-to-end**. (Without the fix, the broadcast triggered by step 5's save
   would have wiped the second marker before this point.)
9. Reload the page (`F5` or `navigate_page` again). Re-toggle Edit mode,
   re-open the pipeline, click `alpha`. Assert the textarea **persists the
   full combined value** after reload.
10. Read `.maestro/pipelines/edit-and-save-scenario.prompts/alpha.md` from
    disk. Its content **must equal** the full combined marker value — proves
    the second save reached the backend rather than being eaten by the
    self-write race.

## Negative checks

- During step 6 wait, the cursor must **not** jump to position 0; if a reload
  fires, the textarea remounts and focus is lost. If the cursor still has
  focus and the marker is intact, the broadcast was correctly suppressed.
- During step 8, the daemon log (if visible) should contain
  `(self-write, suppressed)` lines for both the YAML and the prompt sidecar.

## Cleanup

- Delete `.maestro/pipelines/edit-and-save-scenario.yaml`.
- Delete `.maestro/pipelines/edit-and-save-scenario.prompts/` if it was
  created during the run.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: status bar shows 'Daemon: connected' (screenshot 1)",
    "step 6: after 2.5 s wait, textarea still reads MARKER_…_FIRST",
    "step 8: textarea reads MARKER_…_FIRST_THEN_SECOND_… (no wipe)",
    "step 10: alpha.md on disk equals MARKER_…_FIRST_THEN_SECOND_…"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
