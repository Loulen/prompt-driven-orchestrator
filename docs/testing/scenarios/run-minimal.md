# Scenario — `run-minimal`

> Layer 5 (agentic) per ADR 0004. Manual trigger; an agent executes the steps
> and emits the verdict format below.

**Status**: skeleton — content lands with #18 once the tmux/claude spawn primitive
works end-to-end.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`).
- Frontend reachable in a browser controlled by Chrome DevTools MCP or Playwright.
- A pipeline `minimal-run.yaml` exists in `.maestro/pipelines/` (single doc-only
  node, prompt asking the agent to write a one-line artifact).

## Steps the agent executes

1. (TBD with #18) Open the UI; confirm pipeline appears in the New Run picker.
2. Submit a new run; capture the `run_id`.
3. Observe the DAG node animate to "running" within ~2s.
4. `tmux capture-pane -p -t maestro-<run_id>-<node_id>-iter-1` and assert the
   pane shows claude prompting (not just `echo` or `cat`).
5. Wait for the run to reach `completed` (or fail with rationale).
6. Confirm the artifact file exists in `.maestro/runs/<run_id>/worktree/.maestro/artifacts/`.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "<one observation per line, e.g. 'tmux pane shows claude UI'>"
  ],
  "anomalies": [
    "<optional — anything unexpected>"
  ]
}
```
