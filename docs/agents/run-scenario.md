# Driving PDO during an agentic test

The **agentic test** layer (ADR-0004, apex of the pyramid) has an agent drive the **real running
app** through a user journey and raise **findings**. This file is the PDO-specific playbook: how to
drive the UI and probe the side-effects the UI doesn't show. It is tech-specific on purpose — the
generic concept and format are elsewhere:

- **Runner:** the `/agentic-tests` skill (mode selection, gates).
- **Format & rules:** [`SCENARIO-FORMAT.md`](../../.claude/skills/agentic-tests/SCENARIO-FORMAT.md).
- **What to run:**
  - **Happy Paths (HP)** — `docs/test-scenarios/HP-*.md` (curated, permanent, ≤ 3).
  - **Feature Paths (FP)** — in the sub-issue's "Acceptance criteria → Feature Path" section
    (throwaway, no file).

> There is no per-scenario PASS/FAIL JSON verdict anymore. A run produces **findings** (blocking or
> not); the agent qualifies severity, and a blocking finding fails the gate (see `SCENARIO-FORMAT.md`
> and `git-flow`).

## 1. Confirm the app is running

No running stack ⇒ no execution. Discover the daemon, don't assume a port:

- Find the daemon's port (e.g. `ss -ltnp | grep pdo`, or ask the user). The default is commonly
  `http://127.0.0.1:5172`, but a local dev daemon may differ.
- Daemon reachable: `curl -fs http://127.0.0.1:<port>/runs` returns JSON.
- Frontend reachable in a browser; the status bar shows the daemon **connected**.

If a precondition the journey lists isn't met, that's a **finding** — report it, don't try to "fix"
the setup.

## 2. Drive the browser

UI-first. Use the **Chrome DevTools MCP** server when available
(`mcp__plugin_chrome-devtools-mcp_chrome-devtools__*`); fall back to **Playwright MCP**
(`mcp__plugin_playwright_playwright__browser_*`) — both expose roughly the same primitives:

- `navigate_page` — open `http://127.0.0.1:<port>`
- `take_snapshot` (semantic DOM) and `take_screenshot` (visual evidence)
- `click`, `fill`, `type_text`, `press_key` — interaction
- `wait_for` — for specific text or selectors
- `list_console_messages` — surface JS errors that must not be silent

Capture a screenshot at each non-trivial transition and reference it in your findings.

**Select data by characteristics** (filters, badges, status), not hard-coded ids. If no data
satisfies the journey's conditions, that's a legitimate finding — not an excuse to bypass the UI.
Before raising a *data* finding, retry on another instance of the same kind.

## 3. Validate side-effects outside the browser

The point of this layer is that the UI alone doesn't tell the full story — verify what the user
*thinks happened*:

- **tmux sessions**: `tmux capture-pane -p -t pdo-<run_id>-<node_id>-iter-<n>`. The session-name
  pattern is fixed; confirm it shows the expected program (e.g. `claude` running, not a bare shell
  that exited). A first launch in a fresh worktree lands on the "Quick safety check" trust dialog —
  confirm it with `tmux send-keys -t <session> Enter`.
- **Filesystem** (read with `cat` / Read):
  - Pipelines: `.pdo/pipelines/<name>.yaml` and the sidecar `.pdo/pipelines/<name>.prompts/<node>.md`
  - Run artifacts: `.pdo/runs/<run_id>/worktree/.pdo/artifacts/...`
- **Daemon state**: `curl http://127.0.0.1:<port>/runs/<run_id>` for the projected run state (incl.
  `sessions_spawned`, `started_at`/`completed_at`, `loc`), and `/runs/<run_id>/events` for the full
  event log.
- **Event log DB** (rare, forensic): `sqlite3 .pdo/pdo.db "SELECT * FROM events WHERE run_id =
  '<run_id>' ORDER BY id"`.

## 4. Report findings

For each journey, report `✅ / ❌ <id> — <title>`, the failing steps with evidence (one observation
per line, screenshot/snapshot refs), then a consolidated **Findings** section:

```
## Findings
- [blocking] …
- [non-blocking] …
```

Rules: a single failed assertion ⇒ the journey fails (don't half-pass); raise **all** findings,
blocking or not, qualifying severity.

## 5. Cleanup

If the journey started a Run, archive it before reporting (`cleanup_run` reaps sessions + worktree):

```bash
curl -X POST http://127.0.0.1:<port>/runs/<run_id>/commands \
  -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'
```

Delete any pipeline the agent seeded. Skip cleanup only if the journey explicitly asks to leave state
behind for human inspection.

## Reference paths

- Happy Paths: `docs/test-scenarios/HP-*.md` (inventory: `docs/test-scenarios/README.md`)
- Layer rationale: `docs/adr/0004-testing-strategy.md`
