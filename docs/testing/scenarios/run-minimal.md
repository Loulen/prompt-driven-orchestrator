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

5. `tmux capture-pane -p -t maestro-<run_id>-only-iter-1`. The pane content
   must show the **claude TUI**, not just an `echo`/`cat` shell. Acceptable
   first-launch states:
   - The "Quick safety check: Is this a project you created or one you trust?"
     dialog. **First launch in a fresh worktree path always lands here.**
     The agent confirms with `tmux send-keys -t maestro-<run_id>-only-iter-1
     Enter`.
   - After confirmation: the chat view, with the prompt body
     ("Reply with exactly the line …") visible as the **first user message**.

   Take a screenshot of the pane after the trust dialog clears.
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
