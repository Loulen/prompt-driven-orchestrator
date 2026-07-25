# Agentic tests — Happy Path inventory

The **agentic test** layer (apex of the testing pyramid, ADR-0004): a subagent drives the **real
running app** and validates a user journey, UI-first, raising **findings** rather than a binary
verdict.

- **Runner:** the `/agentic-tests` skill (mode selection + gates).
- **Format:** [`SCENARIO-FORMAT.md`](../../.claude/skills/agentic-tests/SCENARIO-FORMAT.md).
- **Driving PDO:** [`docs/agents/run-scenario.md`](../agents/run-scenario.md) — how an agent drives
  the app and probes side-effects (browser MCP, `tmux capture-pane`, daemon HTTP, filesystem).

Two levels:

- **Happy Path (`HP-`)** — curated, **permanent** suite (the paths taken by 80 %+ of users). Worth
  as much as critical-path documentation as it is a regression suite. Lives here. Gate
  `integration → develop`. **At most 3.** Curation is a human decision (see `git-flow`).
- **Feature Path (`FP-`)** — **no file here.** It lives in the "Acceptance criteria → Feature Path"
  section of a technical-backlog sub-issue and is **throwaway** (it dies with the issue). If a piece
  of an FP is worth keeping, graft it **drive-by** onto an HP.

> **Resilience / adversity is not a Happy Path.** It is edge-case robustness, expensive to drive,
> and covered **permanently by the layer-3 automated tests** (`tests/process_lifecycle.rs`,
> `crates/pdo-daemon/tests/`, `frontend/e2e/`). It does not earn an HP slot.

## Inventory

| ID | Title | Covers | Status |
|---|---|---|---|
| [HP-01](HP-01-author-and-save.md) | Author & save a pipeline | pipeline authoring, library, unified canvas | active |
| [HP-02](HP-02-run-to-completion.md) | Launch a run to completion | run lifecycle, dataflow, artifacts, stats, **sandbox A/B (`full` vs `off`)** | active |
| HP-03 | *(reserved — free slot)* | candidate: Triggers, once it is core | — |

The 3rd slot is intentionally free. To add it: allocate `HP-03`, follow `SCENARIO-FORMAT.md`, update
this table, and run it once to confirm it's executable — within the **max 3** limit (otherwise merge
two journeys, drop a non-critical one, or graft drive-by).

**Sandbox (PRD #403) is grafted onto HP-02 as an A/B drive-by rather than given its own slot.** The
pair `full` + `off` is what makes it meaningful: the `off` twin is the control (a Run that silently
fell back to the host path also looks green), and `full` — not `minimal` — is what makes the
"no node before the sandbox is ready" guarantee testable at all. Two consequences worth knowing
before running the suite: each execution copies ~1 GB of staging and spends tens of seconds
preparing, and the journey must **stay on the Run** during preparation (see HP-02's notes for why
that is load-bearing). The day the instance default stops being `off` — a VPS deployment, where
most Runs will be sandboxed — this drive-by is the natural candidate to be promoted to `HP-03`.
