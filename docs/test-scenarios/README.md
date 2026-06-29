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
| [HP-02](HP-02-run-to-completion.md) | Launch a run to completion | run lifecycle, dataflow, artifacts, stats | active |
| HP-03 | *(reserved — free slot)* | candidate: Triggers, once it is core | — |

The 3rd slot is intentionally free. To add it: allocate `HP-03`, follow `SCENARIO-FORMAT.md`, update
this table, and run it once to confirm it's executable — within the **max 3** limit (otherwise merge
two journeys, drop a non-critical one, or graft drive-by).
