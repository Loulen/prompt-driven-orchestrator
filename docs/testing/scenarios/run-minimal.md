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
    - id: only
      type: doc-only
      prompt_file: run-minimal-scenario.prompts/only.md
      inputs:
        - name: in
      outputs:
        - name: out
      view: { x: 100, y: 100 }
  edges: []
  ```

  And `.maestro/pipelines/run-minimal-scenario.prompts/only.md`:

  ```
  Reply with exactly the line `MAESTRO_RUN_MINIMAL_OK` and then call the
  `maestro complete` command. Do nothing else.
  ```

## Steps the agent executes

### Step 0b — Run start pseudo-node visible (refs #30)

After selecting a Run in step 2, assert:

- A **Start pseudo-node** (green play-button circle, `▶`) is visible to the
  left of the `only` node in the DAG canvas.
- Synthetic edges connect the Start node to each entry node (in this case,
  just `only`).
- Click the Start node → the right panel swaps to the **StartInspector**:
  - Header reads **"Run start"** with subtitle **"runtime · pseudo-node"**
    and a `runtime` badge.
  - Body shows the user's submitted input (e.g. `hi`) inline as monospace
    `<pre>` text.
  - A **"View as markdown ↗"** link at the bottom opens the
    `MarkdownArtifactModal` on `_input.md`.

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

### Step 5d — Click output → modal contains MAESTRO_RUN_MINIMAL_OK (refs #27)

Once step 6 confirms the node completed and the artifact exists:

1. Click the **"open ↗"** link on the `out` output port row.
2. Assert the **MarkdownArtifactModal** opens (`.artifact-markdown` visible).
3. The modal body must contain the string **`MAESTRO_RUN_MINIMAL_OK`**.
4. If the output file has YAML frontmatter, a frontmatter card is displayed
   above the markdown body.
5. Close the modal via the **X** button, **Escape** key, or backdrop click.

### Step 1c — Edit-this-run toggle (refs #28)

9. With the Run still visible (any status), click the **"Edit this run"**
   button on the run overlay. Assert:
   - The **AddPalette** appears (buttons to add `code-mutating` / `doc-only`
     nodes are visible).
   - The right panel swaps to the **editor inspector** (node inspector form
     instead of terminal preview).
   - A footnote reading **"Editing run-scoped copy · template unchanged"**
     is visible beneath the run overlay.
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
