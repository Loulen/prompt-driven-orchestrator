---
name: agentic-tests
description: Runs the project's agentic tests — by default the current ticket's Feature Path (FP), or the full suite of Happy Paths (HP) when "HP"/"all" is requested or when on an integration branch with an open MR. Use when running agentic tests, validating a ticket before auto-merge, or running the HP suite before an integration→develop PR.
---

# /agentic-tests — runner

Runs the **agentic test** layer (apex of the pyramid): a subagent drives the **real running
system** through its **primary surface** along a journey, **surface-first** — probing internals
only when the surface isn't enough. It sits **above end-to-end tests**: the QA pass, performed by
an agent instead of a human. The *concept* (the two levels, the gates) is in `CLAUDE.md`; the
*format* of journeys is in [SCENARIO-FORMAT.md](./SCENARIO-FORMAT.md), and the HP inventory lives
under `docs/test-scenarios/` (created at the first HP curation). This skill only **executes**.

## Primary surface & driver

The **primary surface** is how a real user reaches the system. Drive it first; probe internals
(store, logs, live state) only when the surface can't show what a step needs. The driver stays
**agnostic** — pick it from the current stack — but a sensible default per system type:

| System type | Primary surface | Recommended driver |
|---|---|---|
| Web app | the UI in a browser | **Playwright CLI** — browser automation for agents (the CLI, **not** the MCP) |
| Cloud / infra / devops | the provider CLI | **AWS CLI** (or the provider's own CLI) |
| Data pipeline / transforms | the warehouse | **dbt** — build the models, then query the tables |
| Interactive CLI / TUI | the command line | **tmux** — drive the live session |

Assume no framework, no ports, no seeding tool: discover how to reach the surface at runtime.

## 1. Pick the mode

| Condition | Mode |
|---|---|
| argument `HP` / `all` (or equivalent) | **HP** — the whole suite |
| else, on an `integration/*` branch with an open integration→develop MR | **HP** |
| else (on a `feature/*`) | **FP** — the current ticket |

When ambiguous (on `develop`/`main`, no argument), ask which mode to run.

## 2. Shared prerequisites

- The **system runs locally** (start it the way the project expects; if you don't know how,
  ask). No running system = no execution.
- You can reach the **primary surface** with a driver of your choice (see the table above).

## 3. FP mode (ticket → integration auto-merge gate)

1. Derive the ticket reference from the branch name `feature/<ticket-ref>-<slug>` and fetch the
   ticket from the technical backlog (see `docs/agents/issue-tracker.md`).
2. Read the **"Acceptance criteria → Feature Path (FP)"** section. It's a *behavioral*
   journey: translate it into concrete actions at runtime.
3. Run the journey against the system through its **primary surface** (probe internals only when
   the surface isn't enough).
4. Report: pass/fail per step **+ findings**.

**Gate** (see `git-flow`): auto-merge only happens if the FP is **green** *and* no **blocking
finding** is raised — on top of green build + tests. On red or a blocking finding: **do not
merge**, report.

## 4. HP mode (integration → develop gate, human decision)

1. Read all `docs/test-scenarios/HP-*.md`.
2. Run each journey **independently** against the system (an HP must run on its own).
3. Apply the **execution rules** (below).
4. Produce a per-scenario report (pass/fail) + a consolidated **Findings** section, ready to
   paste into the integration→develop PR.

The agent **never merges** into `develop`: it runs, reports, and proposes the HP curation
(see `git-flow`). HP red → the human decides.

## 5. Execution rules (agent)

- **Retry on different data before raising a data-related finding.** If a step fails on a
  particular instance, retry with another instance of the same kind; a "data" finding is only
  justified if **all** reasonable instances fail, or if the behavior is structural.
- **Raise all findings**, blocking or not (a warning, surprising behavior, side effect, real
  breakage). You **qualify the severity**; a blocking finding fails the gate.
- **Surface-first.** An internal probe only complements what the surface doesn't show, and only
  when that internal state exists.
- **Data selection by characteristics** (filters, badges, query predicates…), not hard-coded IDs
  (HP mode): if no data satisfies the conditions, that's a legitimate signal, not an excuse to
  bypass the surface.

## 6. Report format

For each journey: `✅ / ❌ <id or ticket> — <title>`, the failing steps, then:

```
## Findings
- [blocking] …
- [non-blocking] …
```
