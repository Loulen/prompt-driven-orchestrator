---
name: build-factory
description: Entry point of the factory. Two modes — scaffold (wire the two backlogs, triage vocabulary, and domain docs, then lay down CLAUDE.md / CONTEXT.md / docs/adr/) and context (bootstrap CONTEXT.md's glossary + foundational ADRs, the project's first grilling). Detects what's already wired and offers the right mode. Run this first, in a fresh repo as well as an existing project.
disable-model-invocation: true
---

# /build-factory

Sets up the agentic software factory in this repo. It has **two modes**:

- **Scaffold** — wire the ports (the two backlogs, the triage vocabulary, the domain docs) and lay
  down the skeleton (`CLAUDE.md`, an empty `CONTEXT.md`, `docs/adr/`, the default US layout). No
  design yet.
- **Context** — the project-context bootstrap: fill `CONTEXT.md`'s glossary and the foundational
  ADRs. This is the project's **first grilling**, reusing the `grilling` and `domain-modeling`
  support skills. Its output lands on **`develop`**.

The goal: by the end, the pipeline skills (`grill-with-docs`, `to-spec`, `to-tickets`, `triage`,
`tdd`, `agentic-tests`, `git-flow`) know how **this project** works, and the person knows **where
to go next**.

**Guiding principle — wire the *workflow*, not the *stack*.** Never ask about the framework,
package manager, ports, or build/test commands: agents discover those at runtime, the layer stays
tech-agnostic.

**Pedagogical principle — no info-dump.** Don't lay out the whole method in one block. Each
decision opens with a **short framing** (which skills it affects, their role), *then* the question.
One decision at a time; wait for the answer before the next. The person learns the factory by
assembling it.

## 0. Detect state, pick the mode

Before anything, look at what already exists (assume nothing) and self-locate — **guided, not
gated**: name the obvious mode, but let the person override.

Read the repo state:

- `git remote -v` — GitHub? GitLab? neither? (used to propose the right technical backlog)
- `CLAUDE.md` / `AGENTS.md` at the root — present? Is there an `## Agent skills` block?
- `CONTEXT.md`, `CONTEXT-MAP.md` at the root — present? Does `CONTEXT.md` carry real terms, or is
  it still an empty stub?
- `docs/agents/`, `docs/adr/` — do the port docs and any ADRs already exist?
- **Is there application code?** (`src/`, a manifest…) → **existing project** vs **from-scratch**.
  Confirm the verdict with the person; don't decide it silently.

Then map state → mode:

| Repo state | Offer |
|---|---|
| No `docs/agents/` ports, no `## Agent skills` block | **Scaffold** first, then Context |
| Ports wired, `CONTEXT.md` still an empty stub | **Context** (the bootstrap) |
| Ports wired **and** `CONTEXT.md` filled with foundational ADRs | Setup looks complete — recommend `/verify-factory` for a health check; re-run a mode only to switch a port or re-bootstrap |

State the detected mode and why, then proceed — or switch on request.

## Scaffold mode

### Frame (short)

Two sentences before diving in:

> "The method separates two backlogs: a **business** one (owned by humans, the user stories) and a
> **technical** one (consumed by agents, the self-contained tickets). Design bridges the two;
> that's where the human effort concentrates. We'll wire that for this repo, then bootstrap the
> project context."

### Decisions, one at a time (with their *why*)

Present each section as: *framing (skills affected + role) → choices → default*. Assume the person
doesn't know the vocabulary yet; explain it along the way. **One section, one answer, then the
next.** Don't dump the four at once.

#### A — Business backlog (project management)

> Framing: this is where the **user stories** live, managed by humans (PO/team). `grill-with-docs`
> **picks** the story (or set of stories) to design from here; at the end of the chain,
> `to-tickets` **posts back** the technical tickets it created, onto the source item, for
> traceability. It's the "Understand the need" starting point of the pipeline.

Common tools: **Jira**, **Trello**, **Notion**, **Linear**, **GitHub/GitLab Projects**, a
spreadsheet, a markdown file… Ask which one. If it isn't integrable (no CLI/MCP available, or not
chosen yet), record it as `<tool to define>`: the flow still holds, the back-link just becomes
manual. Write the result to `docs/agents/business-backlog.md` (seed:
[business-backlog.md](./business-backlog.md)).

> **Where does your business backlog (project management) live?**

#### B — Technical backlog (tickets for agents)

> Framing: this is the second backlog, **by and for agents**. `to-spec` writes the spec here,
> `to-tickets` slices the **self-contained tickets** (vertical slices), `triage` labels them,
> `git-flow` frames their merge. The engineering skills read and write *here*. They need to know
> whether to call `gh issue create`, `glab issue create`, write a markdown file under `.scratch/`,
> or follow some other workflow.

Default posture: if a `git remote` points at GitHub → propose **GitHub**; at GitLab → propose
**GitLab**. Otherwise, offer:

- **GitHub** — tickets in the repo's GitHub Issues (`gh` CLI). Seed: [issue-tracker-github.md](./issue-tracker-github.md)
- **GitLab** — tickets in the repo's GitLab Issues ([`glab`](https://gitlab.com/gitlab-org/cli) CLI). Seed: [issue-tracker-gitlab.md](./issue-tracker-gitlab.md)
- **Local markdown** — tickets under `.scratch/<feature>/` (solo, or repo without a remote). Seed: [issue-tracker-local.md](./issue-tracker-local.md)
- **Other** (Jira, Linear…) — ask the person to describe the workflow in one paragraph; write `docs/agents/issue-tracker.md` from that prose.

Write the result to `docs/agents/issue-tracker.md`.

> **Which tool for the technical backlog?**

#### C — Triage vocabulary

> Framing: `triage` moves a ticket through a small state machine (to evaluate, waiting on the
> reporter, ready for an agent, for a human, won't fix). Above all, `to-spec`/`to-tickets` apply
> `ready-for-agent` to signal **"an agent can pick this up with no human context"** — that's the
> label that unlocks autonomy. These roles must map to strings actually used in your tracker.

The five canonical roles (default: the string = the name):

- `needs-triage` — a human must evaluate
- `needs-info` — waiting on the reporter
- `ready-for-agent` — fully specified, ready for an autonomous agent
- `ready-for-human` — needs human implementation
- `wontfix` — will not be actioned

Ask whether the roles should **map** to existing labels (e.g. `bug:triage` instead of
`needs-triage`). Otherwise the defaults are fine. Seed: [triage-labels.md](./triage-labels.md).
Write `docs/agents/triage-labels.md`.

> **Existing labels to map, or keep the default names?**

#### D — Domain docs (layout)

> Framing: the grilling sessions **write** `CONTEXT.md` (the domain glossary) and the **ADRs**
> (structural decisions); `to-spec`, `to-tickets`, `tdd`, `triage` **read** them to speak the
> project's language and respect its decisions. We need to know whether there is a single context
> or several (monorepo).

- **Single-context** — one `CONTEXT.md` + `docs/adr/` at the root. Most repos.
- **Multi-context** — a `CONTEXT-MAP.md` at the root pointing to one `CONTEXT.md` per context
  (typically a monorepo).

Seed: [domain.md](./domain.md). Write `docs/agents/domain.md`.

> **Single-context or multi-context?**

### Confirm, then write

Show a draft before writing; let them edit. Then:

1. **Ensure the project's agent file carries the method.** Edit `CLAUDE.md` (or `AGENTS.md` if
   that's the file the repo uses; if neither exists, ask which to create — don't pick for them). It
   must hold two things; create what's missing, update in place if present, never append a
   duplicate.

   **(a) The method constants** — the skills assume these are always in context:

   ````markdown
   ## Agentic tests (concept)

   Apex of the pyramid: a subagent drives the **real running system** through its **primary
   surface** — the UI for a web app, the cloud CLI for infra, the warehouse for a data pipeline —
   along a journey, probing internals only when the surface isn't enough. It sits **above
   end-to-end tests**: the QA pass, performed by an agent instead of a human. Two levels:

   - **Happy Path (HP)**: curated suite (**at most 3 HP**), core value, under
     `docs/test-scenarios/`. Run + reported at the **integration→develop** MR (human decision).
   - **Feature Path (FP)**: **executable** acceptance criteria of a ticket (in the ticket body,
     **throwaway**). Ticket→integration **auto-merge** gate: green FP + no blocking finding, on
     top of build + tests.

   Runner: `/agentic-tests`. Format & inventory: the `agentic-tests` skill's `SCENARIO-FORMAT.md`.

   ## Dev workflow

   For a `ready-for-agent` ticket: branch per Git flow, then run **`/implement`** — the three-role
   loop. It drives `/tdd` (red → green at the agreed seams) on the lower pyramid tiers, an
   **independent** `/code-review` on the diff, and a subagent that runs `/agentic-tests` on the
   ticket's Feature Path — driving the running system through its **primary surface** and reporting
   findings, so validation is an actual step, not just a suggestion. It iterates until build + tests
   + FP are green with no blocking finding, then merges per Git flow. Refactoring belongs to the
   review stage, not the red → green cycle.

   Subagents are the **baseline** — `/implement` uses them where available and degrades to a
   sequential `/tdd → /code-review → /agentic-tests` run otherwise. Building richer orchestrations on
   top (parallel/adversarial reviewers, several FPs, dedicated workflow tooling) is encouraged.

   ## Git flow

   Simplified vanilla git flow (`main`/`develop`/`integration/*`/`feature/*`/`hotfix/*`, no
   `release` until pre-prod). Every Claude instance must know it:

   @.claude/skills/git-flow/SKILL.md
   ````

   > The `@.claude/skills/git-flow/SKILL.md` import is valid when the skills were vendored into the
   > repo (e.g. `npx skills add …` installs them under `.claude/skills/`). If the skills are only
   > available as a globally-installed plugin, drop that `@import` line — `git-flow` then loads on
   > demand as a skill.

   **(b) The `## Agent skills` block** — project-specific, from the decisions above:

   ````markdown
   ## Agent skills

   ### Business backlog
   [one-line summary]. See `docs/agents/business-backlog.md`.

   ### Technical backlog (issue tracker)
   [one-line summary]. See `docs/agents/issue-tracker.md`.

   ### Triage labels
   [one-line summary]. See `docs/agents/triage-labels.md`.

   ### Domain docs
   [single- or multi-context]. See `docs/agents/domain.md`.

   ### User-story layout
   Default layout for business user stories. See `docs/agents/us-format.md`.
   ````

2. **The four port docs** `docs/agents/{business-backlog,issue-tracker,triage-labels,domain}.md`
   from this folder's seeds (for an "Other" backlog, write from the prose).

3. **The default US layout** `docs/agents/us-format.md` — **copy `skills/to-us/US-FORMAT.md`
   verbatim** (it is the single source of truth for the default). It needs no interview: lay it down
   as-is; the PO tailors it later if they want. `/verify-factory` compares the two byte-for-byte to
   report whether the layout is still the default or has been tailored.

4. **Scaffold**: create `docs/adr/` (empty) and a **`CONTEXT.md` stub** at the root — a few section
   headers, no content (the context bootstrap fills it). The exact format is in
   `domain-modeling/CONTEXT-FORMAT.md`; don't copy it, keep the stub minimal.

## Context mode — the project-context bootstrap

`CONTEXT.md` and the ADRs are **the project's reference** for every agent (see ADR-0004). This mode
produces the *baseline*: the first glossary and the foundational structural decisions. It is the
project's **first grilling** — an end in itself, with **no business story and no integration
branch** (that's what makes it a `build-factory` mode and not `/grill-with-docs`, which is always
per-story). Its output lands on **`develop`**, so every later `integration/*` branch inherits it.

Reuse the support skills rather than re-inventing the interview:

- **`grilling`** — the interview engine: resolve one decision at a time, write it down, don't move
  on until it's settled.
- **`domain-modeling`** — extract the domain terms, actors, and relationships that seed the
  glossary.

The context bootstrap is a **writer** of `CONTEXT.md` and the ADRs (the only writers are the two
grilling sessions — this mode and `/grill-with-docs`; see `domain.md`, *Read, don't write*). Write
inline as decisions crystallise — never batch. `CONTEXT.md` stays a glossary + product intent;
implementation detail belongs in an ADR or a ticket. Formats: the `domain-modeling` skill's
`CONTEXT-FORMAT.md` and `ADR-FORMAT.md`.

Branch on the state detected in step 0:

### From-scratch project (greenfield)

No code to interrogate yet — start from the vision.

1. Ask for a **pitch** (written, or pasted from a transcript — suggest transcribing a recording
   first; depend on no specific tool).
2. **Rough it out** with `domain-modeling`: extract the vision, the problem, the first domain
   terms, the actors. Grill the questions needed to make it hold together.
3. **Seed `CONTEXT.md`** with that first glossary and the vision, and open the foundational ADRs
   for the decisions that already have a real alternative. State clearly that **these files are THE
   reference**: the whole pipeline relies on them.

### Existing project

The domain is in the code and in people's heads; extract it.

- **Warn that it will be a long grilling**: budget **half a day**. The goal is to rebuild
  `CONTEXT.md` (glossary) and the foundational ADRs (decisions already made, sometimes implicit) by
  cross-referencing the business vision with the existing code, driven by `domain-modeling`.

## Validate the wiring

Before handing off, confirm the ports actually work — a quick smoke check, not the full health
check:

- **External sources reachable**: the technical backlog answers (e.g. `gh`/`glab` authed and the
  repo resolves; a local backend's directory is writable), and the business backlog is reachable or
  explicitly recorded as `<tool to define>`.
- **Agentic tests can run**: the primary-surface driver launches (see `agentic-tests` for the
  per-system-type default — Playwright CLI / AWS CLI / dbt / tmux). You don't run a scenario here;
  you confirm an agent *could* drive the system.

For the full, re-runnable health check — both backlogs reachable, triage labels present, tooling
auth, driver launches, scaffold present, context filled, git-flow health, US template
default/tailored — hand off to **`/verify-factory`**. Don't duplicate its checklist here.

## Handoff — signpost, don't launch

Close by reporting what was done and the sensible next step — never block, never auto-start a long
session (a slot has to be blocked first).

- **After scaffold** → "Ports wired and the skeleton is down. Next: **`/build-factory` context
  mode** to bootstrap `CONTEXT.md` + the foundational ADRs — the design that carries everything
  else. Block the slot first (~half a day for an existing project)."
- **After context** → "Baseline `CONTEXT.md` + foundational ADRs are on `develop`. Run
  `/verify-factory` to confirm the wiring, then the pipeline chains per business story:
  `/grill-with-docs` → `/to-spec` → `/to-tickets` → implementation via `/tdd` + `/agentic-tests`,
  framed by `git-flow`."

`docs/agents/*.md` can be hand-edited later; re-running `/build-factory` is only needed to switch a
port or re-bootstrap.
