# Scenario — `run-minimal`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Asserts that #18's
> `spawn_tmux_session` rewrite actually launches `claude` with the right prompt
> and that the tmux session lives long enough for the user to attach.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `claude` available on `PATH` (the daemon shells out to `claude
  --dangerously-skip-permissions "$(cat <prompt>)"`).
- A pipeline `run-minimal-scenario.yaml` exists in `.maestro/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI:

  ```yaml
  name: run-minimal-scenario
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 100 }
    - id: xCsiuWj7
      name: only
      type: doc-only
      inputs:
        - name: in
          side: left
      outputs:
        - name: out
          side: top
      view: { x: 200, y: 100 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 400, y: 100 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: xCsiuWj7, port: in }
    - source: { node: xCsiuWj7, port: out }
      target: { node: end, port: result }
  ```

  And `.maestro/pipelines/run-minimal-scenario.prompts/xCsiuWj7.md`:

  ```
  Reply with exactly the line `MAESTRO_RUN_MINIMAL_OK` and then call the
  `maestro complete` command. Do nothing else.
  ```

## Steps the agent executes

### Step 0b — Start and End nodes visible (refs #30, #39)

After selecting a Run in step 2, assert:

- A **Start node** (green play-button circle, `▶`) is visible to the
  left of the `only` node in the DAG canvas.
- An **End node** (orange circle, `◯`) is visible to the right of the
  `only` node in the DAG canvas.
- Edges connect Start → `only` → End.
- Click the Start node → the right panel swaps to the **StartInspector**:
  - Header reads **"Run start"** with a `runtime` badge.
  - Subtitle shows the start node's id (e.g. `start`).
  - Body shows the user's submitted input (e.g. `hi`) inline as monospace
    `<pre>` text.
  - A **"View as markdown ↗"** link at the bottom opens the
    `MarkdownArtifactModal` on `_input.md`.
- Click the End node → the right panel swaps to the **EndInspector**:
  - Header reads **"Run end"** with a `runtime` badge.
  - Subtitle shows the end node's id (e.g. `end`).
  - "Termination reasons" section lists the `result` port with status
    **"pending"** (no edge has fired yet).

### Step 0c — Start handle connected to first node (refs #49)

After selecting a Run (step 2), assert:

- The edge from Start (`sourceHandle: "user_prompt"`) to `only`
  (`targetHandle: "in"`) is visually rendered — no missing-edge gap between
  the Start circle and the `only` node.
- The Start node uses **TriangleHandle** for its output port (not a plain
  `<Handle>` without an `id`).

### Step 0d — Initial prompt collapsed by default (refs #49)

After selecting the `only` node (step 5):

- The **"Initial Prompt"** section in the right panel is **collapsed** by
  default — only the header row with a **▸** chevron is visible.
- Clicking the header row expands the section (chevron changes to **▾**) and
  reveals the prompt content.
- Clicking again collapses it.

### Step 0e — Terminal preview has no horizontal scroll on overflow (refs #49)

While the node is running or completed:

- Long agent output lines in the terminal preview **wrap** to the panel
  width — no horizontal scrollbar appears.
- ANSI coloring is preserved on wrapped lines.
- Vertical scroll behavior is unchanged (pinned-to-bottom still works).

1. Open the UI; confirm the **`Daemon: connected`** label is visible.
2. Open the **New Run** modal. Pick `run-minimal-scenario`. Provide any input
   string (e.g. `hello`). Click **Launch**. Capture the resulting `run_id`
   from the URL or the run-list panel.
3. Within ~2 s, the DAG node `only` should animate to **`running`**.
4. From a shell on the host, list tmux sessions and assert one matches
   `maestro-<run_id>-only-iter-1`:

   ```bash
   tmux ls | grep "maestro-<run_id>-only-iter-1"
   ```

5. Verify the **terminal preview** in the UI right panel is non-empty and
   contains live content (not the placeholder "Connecting..." string). The
   frontend polls `GET /runs/<run_id>/nodes/only/pane?iter=1` at 1 s cadence
   for running nodes and renders `tmux capture-pane -pe` output with ANSI
   colors via `ansi-to-html`.

5b. Verify the **Initial Prompt** section in the right panel is populated and
   contains `## Inputs` and `## Outputs` headings. These come from the
   deterministic preamble the runtime injects into every NodeRun prompt.
   The frontend fetches `GET /runs/<run_id>/nodes/only/prompt?iter=1` and
   renders the response in a monospace `<pre>` block. If the section shows
   "Loading prompt..." or is empty, the endpoint is not wired or the prompt
   file was not written at spawn time.

   Also verify from the shell:
   `tmux capture-pane -p -t maestro-<run_id>-only-iter-1`. The pane content
   must show the **claude TUI**, not just an `echo`/`cat` shell. Acceptable
   first-launch states:
   - The "Quick safety check: Is this a project you created or one you trust?"
     dialog. **First launch in a fresh worktree path always lands here.**
     The agent confirms with `tmux send-keys -t maestro-<run_id>-only-iter-1
     Enter`.
   - After confirmation: the chat view, with the prompt body
     ("Reply with exactly the line …") visible as the **first user message**.

   Take a screenshot of the pane after the trust dialog clears.
### Step 5e — Output validation 409 on Mark complete (refs #36, refs #1)

Before the node completes on its own, test the output validation guard:

1. If the node is still `running`, it will have no output file yet. Click
   **"Mark complete"** in the right panel.
2. The daemon returns **HTTP 409** with
   `{"error":"missing_outputs","missing":["out"]}`.
3. Assert: an inline **sub-banner** appears below the Mark complete button
   reading **"Missing outputs: out"** in red/mono styling.
4. Write the output file manually:

   ```bash
   mkdir -p .maestro/runs/<run_id>/worktree/.maestro/artifacts/only/iter-1
   echo '# Out' > .maestro/runs/<run_id>/worktree/.maestro/artifacts/only/iter-1/out.md
   ```

5. Click **"Mark complete"** again. This time the daemon accepts it (200 OK).
6. Assert: the sub-banner disappears and the node transitions to **Completed**.

If the node has already completed by the time the agent reaches this step,
skip it — the validation path is already covered by the Layer 3b e2e test
`failed-node.spec.ts`.

### Step 6a — Pin-to-bottom + chevron resume (refs #34)

While the node is still **running** (before `maestro complete` fires):

1. Scroll **up** in the terminal preview `<pre>` pane. Assert:
   - The terminal content **freezes** — subsequent poll responses are not
     rendered (the `<pre>` innerHTML does not change even though polling
     continues in the background).
   - A small floating **chevron `↓`** button (`.pin-bottom-chevron`) appears
     at the bottom-right of the terminal preview.
2. Click the chevron button. Assert:
   - The `<pre>` scrolls to the bottom.
   - The chevron disappears.
   - Rendering **resumes** — the next poll response is rendered into the
     terminal preview.
3. Scroll up again, then manually scroll back to the bottom (within 8 px of
   `scrollHeight - clientHeight`). Assert:
   - The chevron disappears without clicking it.
   - Rendering resumes.

6. Wait up to ~30 s for claude to reply with `MAESTRO_RUN_MINIMAL_OK` and call
   `maestro complete`. Re-capture the pane and assert the literal string
   `MAESTRO_RUN_MINIMAL_OK` is present.
7. Refresh the run view in the UI; the node `only` should now read
   **`completed`**.

### Step 7b — Prompt endpoint accessible after completion (refs #32)

After the node completes (step 7), assert that the prompt endpoint still
returns the full augmented prompt:

```bash
curl -sf "http://127.0.0.1:5172/runs/<run_id>/nodes/only/prompt?iter=1"
```

- HTTP status must be **200**.
- Response body must be **non-empty** and contain `## Inputs` and `## Outputs`
  headings from the deterministic preamble.

This verifies that sub-worktrees (for `code-mutating` nodes) and prompt files
survive node completion — they are only removed by `cleanup_run`.

### Step 7c — End node shows received after completion (refs #39)

After the run completes (step 7):

1. Click the **End node** (orange circle) in the DAG canvas.
2. The right panel swaps to the **EndInspector**:
   - Header reads **"Run end"** with a `runtime` badge.
   - Subtitle shows the end node's id (e.g. `end`).
   - "Termination reasons" section lists the `result` port with status
     **"received"** (green dot, no reason text since this is a normal
     completion — not a halt).

8. Confirm the artifact file exists:

   ```bash
   ls .maestro/runs/<run_id>/worktree/.maestro/artifacts/
   ```

   At minimum `_input.md` is present; depending on what claude wrote, an
   `only.md` artifact may also be there.

### Step 5c — Inputs/Outputs sections visible (refs #27)

After selecting the running (or completed) node `only` in the DAG, the right
panel should show **Inputs** and **Outputs** sections below the terminal
preview.  Assert:

- **Inputs** section lists port `in` with a status dot (green if the file
  exists, grey if not).
- **Outputs** section lists port `out`.  Once the node completes and writes
  `only/iter-1/out.md`, the status dot turns green and a file-size badge
  appears.
- Each port row displays a truncated artifact path.

### Step 5d — Click output port card → modal contains MAESTRO_RUN_MINIMAL_OK (refs #27 #33)

Once step 6 confirms the node completed and the artifact exists:

1. Click **anywhere on the `out` output port row** (`button.port-row`) — not
   just the "↗" icon. The entire card is the click target when files exist.
2. Assert the **MarkdownArtifactModal** opens (`.artifact-markdown` visible).
3. The modal body must contain the string **`MAESTRO_RUN_MINIMAL_OK`**.
4. If the output file has YAML frontmatter, a frontmatter card is displayed
   above the markdown body.
5. Close the modal via the **X** button, **Escape** key, or backdrop click.
6. Verify that the `in` input port row (no files exist) renders as a
   non-interactive `<div>` — no pointer cursor, no hover effect.

### Step 1c — Edit-this-run toggle + save indicator (refs #28 #35 #39)

9. With the Run still visible (any status), click the **"Edit this run"**
   button on the run overlay. Assert:
   - The **AddPalette** appears (buttons to add `code-mutating` / `doc-only`
     nodes are visible).
   - The right panel swaps to the **editor inspector** (node inspector form
     instead of terminal preview).
   - A footnote reading **"Editing run-scoped copy · template unchanged"**
     is visible beneath the run overlay.
   - The **TabBar** is visible above the canvas with the run-scoped tab.
   - Both **Start** and **End** nodes remain visible on the edit canvas
     (they are non-deletable and always present).
9b. Make any edit (e.g. change the node prompt). Assert:
   - The tab title is prefixed with **`•`** (dirty indicator).
   - The **Save** button in the TabBar is enabled.
   Click **Save**. Assert:
   - The **`•`** prefix disappears from the tab title.
   - **"Saved Xs ago"** text appears near the Save button.
10. Click **"Stop editing"** (or the same toggle again). Assert:
    - The **AddPalette** disappears.
    - The right panel returns to the **NodeDetailPanel** (terminal preview).
    - The **"Edit this run"** button is visible again on the overlay.

## Negative checks

- **Tmux session must persist** until `maestro complete` is called. If the
  session dies within a few seconds of launch (the original Bug A symptom),
  capture the pane *before* it dies (via repeated `tmux capture-pane` polls)
  and report the failure — that is the regression this scenario exists to
  catch.
- **No `echo Maestro NodeRun: …` banner** — that string was the old broken
  command. If you see it, you're on the pre-#18 build.
- **Daemon stderr (visible if the agent is running `maestro daemon` in the
  foreground) should contain INFO lines** like `Spawned tmux session: …`. If
  it's silent, Bug C is back.

## Cleanup

- Kill the tmux session if still alive:
  `tmux kill-session -t maestro-<run_id>-only-iter-1`.
- Delete the worktree:
  `git worktree remove --force .maestro/runs/<run_id>/worktree`.
- Delete `.maestro/pipelines/run-minimal-scenario.yaml` and its `.prompts/`
  dir if the agent created them in step 0.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 0c: Start→only edge visually rendered with TriangleHandle",
    "step 0d: Initial Prompt section collapsed by default, toggles on click",
    "step 0e: terminal preview wraps long lines without horizontal scroll",
    "step 4: tmux session 'maestro-<run_id>-only-iter-1' present",
    "step 5: pane shows claude TUI (trust dialog → chat view)",
    "step 6: pane contains 'MAESTRO_RUN_MINIMAL_OK'",
    "step 7: UI shows node 'only' as completed"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
