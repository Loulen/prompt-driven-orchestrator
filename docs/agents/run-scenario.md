# Run a layer 5 scenario

Layer 5 of the testing pyramid (per `docs/adr/0004-testing-strategy.md`) is the
agentic layer: a scenario in `docs/testing/scenarios/<name>.md` describes a
user-flow, and an agent executes it against the running app and emits a
PASS/FAIL verdict with evidence.

This is **manually triggered**: the user asks "run the `edit-and-save` scenario"
(or similar). There is no automation. The agent's job is to follow the scenario
faithfully, not to invent missing steps.

## What the agent must do

### 1. Read the scenario file

Open `docs/testing/scenarios/<name>.md`. It declares:
- **Setup** — preconditions (daemon up, pipeline file present, etc.)
- **Steps** — numbered actions the agent performs
- **Verdict format** — the JSON shape to emit at the end

If a step is ambiguous, incomplete, or marked `TBD`, do **not** fabricate the
missing detail. Surface it in `anomalies` and proceed only as far as the
written content allows.

### 2. Check setup before driving the UI

Confirm each precondition the scenario lists. Common checks:

- Daemon reachable: `curl -fs http://127.0.0.1:5172/runs` returns JSON
- Pipeline file exists on disk: `cat .pdo/pipelines/<name>.yaml`
- The repo is in a clean state if the scenario assumes that

If a precondition isn't met, the verdict is `FAIL` and the missing precondition
goes in `evidence`. Don't try to "fix" the setup — report and stop.

### 3. Pilot the browser

Drive the UI with the **Chrome DevTools MCP** server when available
(`mcp__plugin_chrome-devtools-mcp_chrome-devtools__*` tools). If only Playwright
MCP is installed, use `mcp__plugin_playwright_playwright__browser_*` — both
expose roughly the same primitives:

- `navigate_page` — open `http://127.0.0.1:5172`
- `take_snapshot` (semantic DOM) and `take_screenshot` (visual evidence)
- `click`, `fill`, `type_text`, `press_key` — interaction
- `wait_for` — for specific text or selectors
- `list_console_messages` — surface JS errors that should not be silent

Capture a screenshot at each non-trivial transition; reference it in the
`evidence` list (path or snapshot id).

### 4. Validate side-effects outside the browser

The whole point of layer 5 is that the UI alone doesn't tell the full story —
verify what the user *thinks happened* by inspecting the side-effects:

- **tmux sessions**: `tmux capture-pane -p -t pdo-<run_id>-<node_id>-iter-<n>`.
  The session name pattern is fixed; check it shows the expected program
  (e.g. `claude` running, not just an `echo` and an exit).
- **Filesystem**: read directly with `cat` / `Read`:
  - Pipelines: `.pdo/pipelines/<name>.yaml` and sidecar
    `.pdo/pipelines/<name>.prompts/<node_id>.md`
  - Run artifacts: `.pdo/runs/<run_id>/worktree/.pdo/artifacts/...`
- **Daemon state**: `curl http://127.0.0.1:5172/runs/<run_id>` for the projected
  run state, and `/runs/<run_id>/events` for the full event log.
- **Event log DB** (rare): `sqlite3 .pdo/pdo.db "SELECT * FROM events
  WHERE run_id = '<run_id>' ORDER BY id"` for low-level forensic checks.

### 5. Emit the verdict

Print the exact JSON shape the scenario declares:

```json
{
  "verdict": "PASS",
  "evidence": [
    "step 1: navigated to http://127.0.0.1:5172, header 'PDO' visible (screenshot 1)",
    "step 4: tmux capture-pane shows 'Welcome to Claude Code' on session pdo-<run_id>-impl-iter-1",
    "step 6: artifact .pdo/runs/<run_id>/worktree/.pdo/artifacts/impl/iter-1/result.md exists, 312 bytes"
  ],
  "anomalies": [
    "step 3: DAG node animation took ~4s instead of expected ~2s — within tolerance, surfacing for awareness"
  ]
}
```

Rules:
- One observation per `evidence` entry, prefixed with the step number.
- A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
- `anomalies` is for surprises that don't fail the verdict (warnings, slow paths,
  unrelated console noise) — keep it factual.

### 6. Cleanup

If the scenario started a Run, archive it before reporting:
`curl -X POST http://127.0.0.1:5172/runs/<run_id>/commands -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'`.

Skip cleanup only if the scenario explicitly asks the agent to leave state
behind for human inspection.

## Reference paths

- Scenario files: `docs/testing/scenarios/<name>.md`
- Layer 5 rationale: `docs/adr/0004-testing-strategy.md`
- Default daemon URL: `http://127.0.0.1:5172`
