# Scenario — `canvas-undo-redo`

> Layer 5 (agentic) per ADR-0004. Manual trigger: an agent drives a real browser
> and emits the verdict format below. Asserts #226 (ADR-0014): Ctrl/Cmd+Z and
> Ctrl/Cmd+Shift+Z / Ctrl+Y, plus toolbar Undo/Redo buttons, revert and reapply
> **structural** canvas edits (edge delete, node move, pipeline rename) to/from
> previous states. Also asserts the non-obvious behaviors: a typed run is ONE
> undo step (coalescing), Ctrl+Z while a text field is focused yields to native
> field undo (no canvas undo), undo history survives a Save but is cleared by a
> clean external hot-reload, and the undone state persists to disk on Save.
>
> **Save the canonical copy to** `docs/testing/scenarios/canvas-undo-redo.md`
> (this file IS that scenario).
>
> **Note for the PDO auto-implement tester node:** to validate THIS branch's
> build (not the installed/main app), build the frontend (`npm --prefix frontend
> ci && npm --prefix frontend run build`) and run a daemon/vite against the
> sub-worktree on a free port (see the `worktree-test-env` and
> `tester-frontend-visual-validation` playbooks). Otherwise run against the
> already-running app.

## Setup

- Daemon running. Default `http://127.0.0.1:5172`; if not reachable, discover the
  active port (`ss -ltnp | grep -i pdo`, then try `6172` dev / `6160` prod) and
  use it consistently for every `curl`/navigation below.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright MCP
  is an acceptable fallback (same primitives).
- A test pipeline `undo-redo-scenario.yaml` seeded in `.pdo/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI:

  ```yaml
  # Inputs are emergent (#149): work nodes declare OUTPUTS only; inputs derive
  # from incoming edges. Only End keeps a declared `result` input. Three edges
  # form a line start→alpha→beta→end; the alpha→beta edge is the deletion target.
  name: undo-redo-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      outputs:
        - name: user_prompt
      view: { x: 0, y: 200 }
    - id: alpha
      name: alpha
      type: doc-only
      outputs:
        - name: out
      view: { x: 240, y: 120 }
    - id: beta
      name: beta
      type: doc-only
      outputs:
        - name: out
      view: { x: 240, y: 360 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
      view: { x: 520, y: 220 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: alpha, port: out }
    - source: { node: alpha, port: out }
      target: { node: beta, port: out }
    - source: { node: beta, port: out }
      target: { node: end, port: result }
  ```

## Steps the agent executes

### A. Open & baseline

1. Open the UI; confirm the **`Daemon: connected`** label is visible in the
   status bar.
2. Click the `undo-redo-scenario` row in the **Library** sidebar. The canvas
   renders four nodes (`Start`, `alpha`, `beta`, `End`) and **three edges**.
3. Record the baseline: the edge count (`3`), the on-canvas position of `alpha`
   (read `.react-flow__node[data-id="alpha"]`'s `transform: translate(Xpx, Ypx)`
   — flow units mirroring the stored `view`; call it `alpha0`), and the tab
   title (no dirty `•`).
4. Assert that with a freshly-opened (unedited) pipeline, **both toolbar buttons
   are disabled**: `[data-testid="toolbar-undo"]` and `[data-testid="toolbar-redo"]`
   both have the `disabled` attribute (empty history).

### B. Edge delete → undo → redo (keyboard)

5. **Right-click the `alpha → beta` edge** to open the context menu, click
   **"Delete edge"**. Assert: edge count is now **2**, the alpha→beta edge is
   gone, the tab shows the dirty `•`, and `toolbar-undo` is now **enabled** while
   `toolbar-redo` is still **disabled**.
6. Press **Ctrl+Z** (Cmd+Z on macOS) with focus on the canvas/body (NOT a text
   field). Assert: edge count is back to **3**, the alpha→beta edge is restored,
   `toolbar-redo` is now **enabled**.
7. Press **Ctrl+Shift+Z** (then separately confirm **Ctrl+Y** does the same on a
   re-deleted edge if you want both bindings covered). Assert: edge count is **2**
   again (the delete was reapplied).
8. Press **Ctrl+Z** once more to restore the edge (count **3**) — leaving the
   graph topologically identical to baseline for the next sections.

### C. Toolbar buttons + disabled states

9. Click `[data-testid="toolbar-undo"]`. Assert it triggers an undo (the most
   recent change reverts; if the stack is now empty, the button becomes
   `disabled`). Click `[data-testid="toolbar-redo"]`; assert it reapplies.
10. Undo repeatedly (button or Ctrl+Z) until `toolbar-undo` is `disabled`
    (history bottom). Assert no further undo changes the canvas (no-op at the
    bottom). Then redo back up until `toolbar-redo` is `disabled` (history top).
    Leave the canvas restored to the 3-edge baseline.

### D. Node move → undo → redo (real drag)

11. Click `alpha` to select it (accent ring), then **drag it** by a clear delta
    (e.g. ~+150px x, ~-50px y on screen). Use an **incremental** drag (several
    small mouse moves with a short wait between, then release) — xyflow's
    d3-drag ignores a single atomic move. Release. Record `alpha1`. Assert
    `alpha1 ≠ alpha0` and the tab is dirty.
12. Press **Ctrl+Z**. Assert `alpha` returns to **`alpha0`** (±1 for rounding) —
    the move was undone.
13. Press **Ctrl+Y** (or Ctrl+Shift+Z). Assert `alpha` is back at **`alpha1`** —
    the move was reapplied. Then Ctrl+Z once to leave it at `alpha0`.

### E. Coalescing — a typed run is ONE undo step

14. Click empty canvas (pane) so nothing is selected; the right pane shows the
    **Pipeline** inspector with a **Name** field. Record the current name
    (`undo-redo-scenario`).
15. Focus the Name field and **type several characters quickly** (e.g. append
    `-X9` so the name becomes `undo-redo-scenario-X9`), staying within ~half a
    second between keystrokes. Click the pane/body to blur.
16. Press **Ctrl+Z exactly once** (focus NOT in the field). Assert the name
    reverts to the **full original** `undo-redo-scenario` in a **single** undo —
    NOT character-by-character. (Per-keystroke `updatePipelineMeta({name})` calls
    were coalesced into one history entry, ADR-0014 / #226.)

### F. Input-focus guard — Ctrl+Z yields to native field undo

17. Focus the Name field again and type `ZZZ`. **With the field still focused**,
    press **Ctrl+Z**. Assert the **canvas structural state is unchanged** by this
    keypress: edge count still **3**, `alpha` still at `alpha0`. (The handler is
    inert while an INPUT/TEXTAREA/SELECT/contenteditable is focused — it does not
    fire a canvas undo. Whether the browser performs a native field-text undo is
    a soft observation; note it in `anomalies`, do not fail on it.) Blur, then
    fix the name back to `undo-redo-scenario` (clear the field and retype, or
    Ctrl+Z on the canvas after blur) so the next section starts clean.

### G. Invalidation — Save keeps history, clean reload clears it

18. With the canvas at a known dirty state (e.g. delete the alpha→beta edge so
    count is **2**), press **Ctrl+S** (or click Save). Assert the dirty `•`
    clears and **"Saved …"** appears (`data-testid="saved-ago"`).
19. Press **Ctrl+Z**. Assert the edge is restored (count **3**) — **undo still
    works across a Save** (history is kept). The tab is dirty again.
20. Save again (Ctrl+S) so the tab is clean (count 3 on disk).
21. **External edit (clean hot-reload):** from a shell, change the on-disk file so
    a re-parse replaces the pipeline — e.g. rename `beta` to `beta2` in
    `.pdo/pipelines/undo-redo-scenario.yaml` (a `name:` value edit). Wait for the
    UI to re-render the change (the daemon's file watcher emits `pipeline_changed`;
    the node label updates to `beta2`).
22. Assert the **undo history is now cleared**: `toolbar-undo` and `toolbar-redo`
    are both **disabled**, and pressing **Ctrl+Z** does **nothing** (the canvas
    keeps the externally-loaded `beta2` state — undo cannot cross the reload
    boundary, ADR-0014).

### H. Persistence of an undone state

23. Make and undo an edit to reach a specific intended state, then **Save**, then
    **reload the page** (`F5` / `navigate_page`) and reopen `undo-redo-scenario`.
24. Read `.pdo/pipelines/undo-redo-scenario.yaml` from disk and assert it matches
    the **post-undo** state that was on screen at Save time (e.g. if you deleted an
    edge then undid it then saved, the file has all 3 edges; if you saved with the
    edge deleted, the file has 2). This proves undo mutates the same buffer that
    Save serializes.

### I. Console hygiene (throughout)

25. At the end, call `list_console_messages`. Assert there were **no uncaught JS
    errors** during the run (warnings/info are fine; surface any error in
    `anomalies` or fail if it coincides with a broken assertion).

## Optional (best-effort) — live-run 409 backstop

> Stage only if a run can be launched cheaply; otherwise skip and note it. This
> asserts that an undo/redo producing an illegal mid-run mutation is caught by
> the daemon, not by the client.

26. Launch a run on `undo-redo-scenario`, open the run-scoped tab. While a node is
    `running`/`awaiting_user`, delete a **pending** node (allowed), then attempt an
    undo/redo sequence and a Save that would reach an illegal state for the live
    node. Assert the daemon rejects the save with the **SaveError modal**
    (HTTP 409), the on-disk run snapshot is **unchanged**, and no tmux session was
    orphaned (`tmux ls` shows the live node's session intact). Cleanup the run
    afterwards (`cleanup_run`).

## Cleanup

- Delete `.pdo/pipelines/undo-redo-scenario.yaml` and, if created,
  `.pdo/pipelines/undo-redo-scenario.prompts/`.
- If the optional run was launched, archive it:
  `curl -X POST <daemon>/runs/<run_id>/commands -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'`.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: status bar shows 'Daemon: connected'",
    "step 4: fresh pipeline — both toolbar-undo and toolbar-redo disabled",
    "step 5: right-click → Delete edge dropped alpha→beta (3→2 edges), tab dirty, undo enabled",
    "step 6: Ctrl+Z restored the edge (2→3), redo enabled",
    "step 7: Ctrl+Shift+Z / Ctrl+Y reapplied the delete (3→2)",
    "step 11-13: drag moved alpha to alpha1; Ctrl+Z → alpha0; Ctrl+Y → alpha1",
    "step 16: one Ctrl+Z reverted the whole typed name run to 'undo-redo-scenario' (coalesced)",
    "step 17: Ctrl+Z while Name field focused left canvas structure unchanged (3 edges, alpha at alpha0)",
    "step 19: Ctrl+Z works after Save (history kept across save)",
    "step 22: after external clean reload, both toolbar buttons disabled and Ctrl+Z is a no-op (history cleared)",
    "step 24: on-disk YAML matches the post-undo state saved",
    "step 25: no uncaught JS errors in console"
  ],
  "anomalies": [
    "<optional — e.g. native field-text undo at step 17 did/did not visibly revert; mid-drag coalescing split; external-edit conflict dialog>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. If a step is
ambiguous or the build under test lacks the feature, record it in `anomalies`
and proceed only as far as the written content allows (do not fabricate).
