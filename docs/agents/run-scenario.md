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

## 1. Start a demo daemon

Never drive the user's daemon (its port, `~/.pdo`, its tmux sessions): a journey runs on a **demo
daemon** it starts itself, from the branch under test, and stops at the end. No daemon is assumed
to be running.

```bash
make demo-daemon PORT=6491             # this branch's `pdo daemon`
make demo-daemon PORT=6491 TEST_EXT=1  # the demo daemon of the test extension instead
```

Without `TEST_EXT`, the demo daemon is the **vanilla core**: this repository's binary with no
extension loaded, the product published as PDO. The Happy Paths are played on it
(`docs/test-scenarios/README.md` § « The core alone »).

Pick a free port between 6400 and 6999 (another agent may be testing at the same time); the
user's ports (5172, 5272, 6160, 6172…) and the caller's own daemon are refused. The command:

- builds the branch's `pdo` (`cargo build`, incremental: a few seconds when nothing changed), so
  the binary is never older than the branch;
- creates a fresh `HOME` under `/tmp/pdo-demo-<port>/home` (any previous one on that port is wiped)
  and runs the daemon from `~/.pdo/app` there, so its event log, pipelines, profiles and skill bank
  start empty;
- builds its environment from yours, minus every `PDO_*` (a node's `PDO_PORT`, `PDO_TOKEN`,
  `PDO_NODE_ID`: the demo daemon is never in nested mode), `TMUX` and `TMUX_PANE`; it sets `PDO_PORT`,
  `PDO_BIND=127.0.0.1`, `PDO_UPDATE_CHECK=off`, `PDO_PRICE_SYNC=off`, and keeps `RUSTUP_HOME`,
  `CARGO_HOME` and `COREPACK_HOME` on the real toolchains;
- **copies** the harnesses' auth files (`~/.claude/.credentials.json` and the account block of
  `~/.claude.json`, `~/.pi/agent/auth.json`, `~/.local/share/opencode/auth.json`) into the demo
  `HOME`; your own config folders are never linked nor written;
- writes the pidfile `/tmp/pdo-demo-<port>/daemon.pid`, waits for the daemon to answer, then prints
  `PDO_DAEMON_URL=…`, `PDO=…` (the branch's `pdo`, not the one on your `PATH`) and a ready command.

`TEST_EXT=1` starts `crates/pdo/tests/demo_daemon.rs` (the test extension, its routes under
`/ext/test/…`, `pdo ext test echo`); its knobs are `PDO_*` variables, so they are never inherited:
pass them explicitly with `DEMO_ENV='PDO_TEST_INTEROP=1 PDO_TEST_EXTENSION_REFUSE=none'`.
`DEMO_ENV` works for the bare daemon too; it may not set `HOME`, `PATH`, `PDO_PORT`, `PDO_BIND`,
`PDO_TOKEN`, `PDO_NODE_ID`, `PDO_DAEMON_NO_CLEANUP`, `TMUX` or `TMUX_PANE`.

Drive it with the printed `PDO=` binary and `PDO_DAEMON_URL=`. Inside a PDO node, drop the node's
own variables, every `PDO_*`, so the CLI does not act as the node of another daemon (the printed
« Drive it » line already unsets each of them by name):

```bash
env $(env | grep -o '^PDO_[A-Za-z0-9_]*' | sed 's/^/-u /') PDO_DAEMON_URL=http://127.0.0.1:6491 \
  <PDO> run list
curl -fs http://127.0.0.1:6491/runs          # the daemon answers JSON
```

The Runs it launches open their tmux sessions on the socket `pdo-<port>`
(`tmux -L pdo-6491 ls`), never on your default tmux server. Open `http://127.0.0.1:<port>` in the
browser for a UI journey; the status bar shows the daemon **connected**.

If a precondition the journey lists isn't met, that's a **finding**: report it, don't try to "fix"
the setup.

## 2. Drive the browser

UI-first. Use the **Chrome DevTools MCP** server when available
(`mcp__plugin_chrome-devtools-mcp_chrome-devtools__*`); fall back to **Playwright MCP**
(`mcp__plugin_playwright_playwright__browser_*`), or the `playwright-cli` skill from a shell — all
expose roughly the same primitives:

- `navigate_page` — open `http://127.0.0.1:<port>` (the demo daemon's)
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

- **tmux sessions**: `tmux -L pdo-<port> capture-pane -p -t pdo-<run_id>-<node_id>-iter-<n>`. The session-name
  pattern is fixed; confirm it shows the expected program (e.g. `claude` running, not a bare shell
  that exited). A first `claude` launch in a fresh worktree lands on the "Quick safety check" trust
  dialog — confirm it with `tmux -L pdo-<port> send-keys -t <session> Enter`. `copilot`'s equivalent is **not** a
  step to drive: approve the target repository's root once, outside the journey, and it cascades to
  every node sub-worktree (`docs/reference/harnesses.md` § Prerequisites). A `copilot` pane parked on it means the
  prerequisite was never met.
- **Filesystem** (read with `cat` / Read):
  - Pipelines: `.pdo/pipelines/<name>.yaml` of the target repo, or the demo library
    `/tmp/pdo-demo-<port>/home/.pdo/pipelines/`, with the sidecar `<name>.prompts/<node>.md`
  - Run artifacts: `.pdo/runs/<run_id>/worktree/.pdo/artifacts/...`
- **Daemon state**: `curl http://127.0.0.1:<port>/runs/<run_id>` for the projected run state (incl.
  `sessions_spawned`, `started_at`/`completed_at`, `loc`), and `/runs/<run_id>/events` for the full
  event log.
- **Event log DB** (rare, forensic): `sqlite3 /tmp/pdo-demo-<port>/home/.pdo/app/.pdo/pdo.db "SELECT * FROM events WHERE run_id =
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

If the journey started a Run, archive it first (`cleanup_run` reaps its sessions and its worktree,
which live in the target repo, not under `/tmp/pdo-demo-<port>`), and delete any pipeline the agent
seeded in a repo:

```bash
curl -X POST http://127.0.0.1:<port>/runs/<run_id>/commands \
  -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'
```

Then stop the demo daemon you started, whatever the outcome of the journey:

```bash
make demo-daemon-stop PORT=6491          # KEEP=1 keeps /tmp/pdo-demo-6491 (its daemon.log)
```

It kills the pid of the pidfile, runs
`tmux -L pdo-<port> kill-server` without `TMUX`, stops what still runs under the demo `HOME`,
wipes the copied auth files and removes `/tmp/pdo-demo-<port>`. It refuses, and stops nothing, on a
port without a pidfile, one of the user's ports, or a pidfile whose live pid does not run under the
demo `HOME` (a recycled pid). Never `pkill`, `killall` or `tmux kill-server` without `-L`: they
reach the user's daemon and sessions.

Skip the stop only if the journey explicitly asks to leave state behind for human inspection.

## Reference paths

- Happy Paths: `docs/test-scenarios/HP-*.md` (inventory: `docs/test-scenarios/README.md`)
- Layer rationale: `docs/adr/0004-testing-strategy.md`
