<!-- PDSF:BEGIN -->
## PDSF skills

The PDSF skills are installed under `.agents/skills/`. Invoke any of them as `/<skill>` (e.g.
`/build-factory`) — your harness loads the skill's markdown into context on demand. This block is
managed by the installer; `/build-factory` refines it with the method constants and the workflow map.

Git flow — every instance must know it:

@.agents/skills/git-flow/SKILL.md

**Next step: run `/build-factory`** to wire this project's backlogs, triage, and domain.
<!-- PDSF:END -->

## Agentic tests (concept)

Apex of the pyramid: a subagent drives the **real running system** through its **primary
surface** — here the PDO web UI, with the daemon's HTTP API and the filesystem as
probes — along a journey, probing internals only when the surface isn't enough. It
sits **above end-to-end tests**: the QA pass, performed by an agent instead of a human. Two levels:

- **Happy Path (HP)**: curated suite (**at most 3 HP**), core value, under
  `docs/test-scenarios/`. Run + reported at the **integration→main** merge, on request (see
  `docs/agents/git-flow.md`).
- **Feature Path (FP)**: **executable** acceptance criteria of a ticket (in the ticket body,
  **throwaway**). Ticket→integration **auto-merge** gate: green FP + no blocking finding, on
  top of build + tests.

Runner: `/agentic-tests`. Format & inventory: the `agentic-tests` skill's `SCENARIO-FORMAT.md`.
Driving PDO during a journey: `docs/agents/run-scenario.md`.

## Dev workflow

For a `tâche` ticket (the `ready-for-agent` role): branch per Git flow, then run **`/implement`** — the three-role
loop. It drives `/tdd` (red → green at the agreed seams) on the lower pyramid tiers, an
**independent** `/code-review` on the diff, and a subagent that runs `/agentic-tests` on the
ticket's Feature Path — driving the running system through its **primary surface** and reporting
findings, so validation is an actual step, not just a suggestion. It iterates until build + tests
+ FP are green with no blocking finding, then merges per Git flow. Refactoring belongs to the
review stage, not the red → green cycle.

Subagents are the **baseline** — `/implement` uses them where available and degrades to a
sequential `/tdd → /code-review → /agentic-tests` run otherwise.

## Git flow

Simplified vanilla git flow, relaxed for a solo repo: **`docs/agents/git-flow.md` overrides the
skill** (no `develop`, `integration/*` → `main`, sanity gate = `make check`).

## Agent skills

### Business backlog
GitHub Issues labelled `US`; the maintainer names the story in the prompt. See `docs/agents/business-backlog.md`.

### Technical backlog (issue tracker)
GitHub Issues of `Loulen/PDO-v2` (`gh`), label `tâche`; extension-only work goes to `Loulen/pdo-extensions`. See `docs/agents/issue-tracker.md`.

### Triage labels
Two labels only: `US` (business story) and `tâche` (technical ticket or spec/PRD, = `ready-for-agent`). See `docs/agents/triage-labels.md`.

### Domain docs
Single-context: `CONTEXT.md` + `docs/adr/`. See `docs/agents/domain.md`; where a new file goes: `docs/agents/module-layout.md`.

### User-story layout
Default layout for business user stories. See `docs/agents/us-format.md`.
