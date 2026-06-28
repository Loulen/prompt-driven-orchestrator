---
name: build-factory
description: Entry point of the skeleton. Sets up the agentic software factory in this repo — wires the two backlogs (business + technical), the triage vocabulary, and the domain docs for the engineering skills, scaffolds CONTEXT.md / docs/adr/, then kicks off the design phase (grilling). Run this first, in a fresh repo as well as an existing project.
disable-model-invocation: true
---

# /build-factory

Install guide for the factory. The goal: by the end, the pipeline skills (`grill-with-docs`,
`to-prd`, `to-issues`, `triage`, `tdd`, `agentic-tests`, `git-flow`) know how **this project**
works, and the person knows **where to go next**.

**Guiding principle — wire the *workflow*, not the *stack*.** Never ask about the framework,
package manager, ports, or build/test commands: agents discover those at runtime, the layer
stays tech-agnostic. `/build-factory` handles the process: the backlogs, triage, the domain,
and the kick-off of design.

**Pedagogical principle — no info-dump.** Don't lay out the whole method in one block. Each
decision opens with a **short framing**: which skills it affects and their role in the
pipeline, *then* the question. One decision at a time; wait for the answer before the next. The
person learns the factory by assembling it.

## 0. Frame (short)

Two sentences, no more, to set the scene before diving in:

> "The method separates two backlogs: a **business** one (owned by humans, the user stories)
> and a **technical** one (consumed by agents, the self-contained issues). Design bridges the
> two; that's where the human effort concentrates. We'll wire that for this repo, then start
> design."

## 1. Explore the repo state

Before asking anything, look at what already exists (assume nothing):

- `git remote -v` — GitHub repo? GitLab? Which one? (used to propose the right technical backlog)
- `CLAUDE.md` / `AGENTS.md` at the root — does either exist? Is there already an `## Agent skills` block?
- `CONTEXT.md`, `CONTEXT-MAP.md` at the root — present? (single vs multi-context)
- `docs/agents/`, `docs/adr/` — does a prior config already exist?
- **Is there application code?** (`src/`, `package.json`, etc.) → tells **existing project** vs **from-scratch**. Confirm your verdict with the person, don't decide it silently.

## 2. Decisions, one at a time (with their *why*)

Present each section as: *framing (skills affected + role) → choices → default*. Assume the
person doesn't know the vocabulary yet; explain it along the way. **One section, one answer,
then the next.** Don't dump the four at once.

### A — Business backlog (project management)

> Framing: this is where the **user stories** live, managed by humans (PO/team).
> `grill-with-docs` **picks** the story (or set of stories) to design from here to start a
> design session; at the end of the chain, `to-issues` **posts back** the technical issues it
> created, onto the source item, for traceability. It's the "Understand the need" starting
> point of the pipeline.

Common tools: **Jira**, **Trello**, **Notion**, **Linear**, **GitHub/GitLab Projects**, a
spreadsheet, a markdown file… Ask which one. If it isn't integrable (no CLI/MCP available, or
not chosen yet), record it as `<tool to define>`: the flow still holds, the back-link just
becomes manual. Write the result to `docs/agents/business-backlog.md` (seed:
[business-backlog.md](./business-backlog.md)).

> **Where does your business backlog (project management) live?**

### B — Technical backlog (issues for agents)

> Framing: this is the second backlog, **by and for agents**. `to-prd` writes the PRD here,
> `to-issues` slices the **self-contained issues** (tracer bullets), `triage` labels them,
> `git-flow` frames their merge. The engineering skills read and write *here*. They need to
> know whether to call `gh issue create`, `glab issue create`, write a markdown file under
> `.scratch/`, or follow some other workflow.

Default posture: if a `git remote` points at GitHub → propose **GitHub**; at GitLab →
propose **GitLab**. Otherwise, offer:

- **GitHub** — issues in the repo's GitHub Issues (`gh` CLI). Seed: [issue-tracker-github.md](./issue-tracker-github.md)
- **GitLab** — issues in the repo's GitLab Issues ([`glab`](https://gitlab.com/gitlab-org/cli) CLI). Seed: [issue-tracker-gitlab.md](./issue-tracker-gitlab.md)
- **Local markdown** — issues under `.scratch/<feature>/` (solo, or repo without a remote). Seed: [issue-tracker-local.md](./issue-tracker-local.md)
- **Other** (Jira, Linear…) — ask the person to describe the workflow in one paragraph; write `docs/agents/issue-tracker.md` from that prose.

Write the result to `docs/agents/issue-tracker.md`.

> **Which tool for the technical backlog?**

### C — Triage vocabulary

> Framing: `triage` moves an issue through a small state machine (to evaluate, waiting on the
> reporter, ready for an agent, for a human, won't fix). Above all, `to-prd`/`to-issues` apply
> `ready-for-agent` to signal **"an agent can pick this up with no human context"** — that's
> the label that unlocks autonomy. These roles must map to strings actually used in your
> tracker.

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

### D — Domain docs (layout)

> Framing: `grill-with-docs` **writes** `CONTEXT.md` (the domain glossary) and the **ADRs**
> (structural decisions) as design progresses; `to-prd`, `to-issues`, `tdd`, `triage` **read**
> them to speak the project's language and respect its decisions. We need to know whether there
> is a single context or several (monorepo).

- **Single-context** — one `CONTEXT.md` + `docs/adr/` at the root. Most repos.
- **Multi-context** — a `CONTEXT-MAP.md` at the root pointing to one `CONTEXT.md` per context
  (typically a monorepo).

Seed: [domain.md](./domain.md). Write `docs/agents/domain.md`.

> **Single-context or multi-context?**

## 3. Confirm, then write

Show a draft before writing; let them edit. Then:

1. **Ensure the project's agent file carries the method.** Edit `CLAUDE.md` (or `AGENTS.md`
   if that's the file the repo uses; if neither exists, ask which to create — don't pick for
   them). It must hold two things; create what's missing, update in place if present, never
   append a duplicate.

   **(a) The method constants** — the skills assume these are always in context:

   ````markdown
   ## Agentic tests (concept)

   Apex of the pyramid: a subagent validates the **real running app** through tech-agnostic
   scenarios, **UI-first**. Two levels:

   - **Happy Path (HP)**: curated suite (**at most 3 HP**), core value, under
     `docs/test-scenarios/`. Run + reported at the **integration→develop** MR (human decision).
   - **Feature Path (FP)**: **executable** acceptance criteria of a sub-issue (in the issue
     body, **throwaway**). Sub-issue→integration **auto-merge** gate: green FP + no blocking
     finding, on top of build + tests.

   Runner: `/agentic-tests`. Format & inventory: the `agentic-tests` skill's `SCENARIO-FORMAT.md`.

   ## Dev workflow

   For a `ready-for-agent` issue: branch per Git flow, then implement with `/tdd`
   (red → green → refactor) on the lower pyramid tiers. **After a `/tdd` implementation, propose
   to the user to spawn a subagent that runs `/agentic-tests`** on the issue's Feature Path — the
   subagent drives the running app (UI-first) and reports findings, so validation is an actual
   step, not just a suggestion. Iterate `/tdd` ↔ `/agentic-tests` until build + tests + FP are
   green with no blocking finding, then merge per Git flow.

   This subagent step is the **baseline** — it leverages Claude Code subagents. Building richer
   orchestrations on top (parallel/adversarial reviewers, several FPs, dedicated workflow tooling)
   is encouraged.

   ## Git flow

   Simplified vanilla git flow (`main`/`develop`/`integration/*`/`feature/*`/`hotfix/*`, no
   `release` until pre-prod). Every Claude instance must know it:

   @.claude/skills/git-flow/SKILL.md
   ````

   > The `@.claude/skills/git-flow/SKILL.md` import is valid when the skills were vendored into
   > the repo (e.g. `npx skills add …` installs them under `.claude/skills/`). If the skills are
   > only available as a globally-installed plugin, drop that `@import` line — `git-flow` then
   > loads on demand as a skill.

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
   ````

2. **The four docs** `docs/agents/{business-backlog,issue-tracker,triage-labels,domain}.md`
   from this folder's seeds (for an "Other" backlog, write from the prose).

3. **Scaffold**: create `docs/adr/` (empty) and a **`CONTEXT.md` stub** at the root — a few
   section headers, no content (grilling fills it). The exact format is in
   `grill-with-docs/CONTEXT-FORMAT.md`; don't copy it, keep the stub minimal.

## 4. Kick off design (existing vs from-scratch)

`CONTEXT.md` and the ADRs are **the project's reference** for every agent. The design phase
produces them. Branch on the context detected in step 1:

### **From-scratch** project (greenfield)

There's no code to interrogate yet: start from the vision.

1. Ask for a **pitch** of the project — written, or pasted from a transcript (if the person has
   a recording, suggest transcribing it first; don't depend on any specific tool).
2. **Rough it out**: extract the vision, the problem, the first domain terms, the actors. Ask
   the questions needed to make it hold together.
3. **Seed `CONTEXT.md`** with that first glossary and the vision (format:
   `grill-with-docs/CONTEXT-FORMAT.md`). State clearly that **these files will be THE
   reference**: the whole pipeline relies on them.

### **Existing** project

The domain is in the code and in people's heads; it has to be extracted.

- **Warn that it will be a long grilling**: budget **half a day**. The goal is to rebuild
  `CONTEXT.md` (glossary) and the ADRs (decisions already made, sometimes implicit) by
  cross-referencing the business vision and the existing code.

## 5. Handoff — recommend grilling (don't launch it)

Don't launch anything automatically (a long session is started once the slot is blocked). End
with a clear recommendation, stressing the pivotal role of `grill-with-docs`:

> "Setup done. **The next step is design — it carries everything else.** That's where the
> business and technical visions align with the AI, and where `CONTEXT.md` + the ADRs become the
> shared reference. Once you've blocked the slot (~half a day for an existing project), run
> `/grill-with-docs` on the story to design. Then the pipeline chains: `/to-prd` → `/to-issues`
> → implementation in `/tdd` + `/agentic-tests`, framed by `git-flow`."

Remind them that `docs/agents/*.md` can be hand-edited later; re-running `/build-factory` is
only useful to switch backlogs or start over.
