---
name: agentic-tests
description: Runs the project's agentic tests — by default the current sub-issue's Feature Path (FP), or the full suite of Happy Paths (HP) when "HP"/"all" is requested or when on an integration branch with an open MR. Use when running agentic tests, validating a sub-issue before auto-merge, or running the HP suite before an integration→develop PR.
---

# /agentic-tests — runner

Runs the **agentic test** layer (apex of the pyramid): a subagent drives the **real running
app** and validates a journey, **UI-first**. The *concept* (the two levels, the gates) is in
`CLAUDE.md`; the *format* of journeys is in [SCENARIO-FORMAT.md](./SCENARIO-FORMAT.md), and the
HP inventory lives under `docs/test-scenarios/` (created at the first HP curation). This skill
only **executes**.

> Tech-agnostic: pick your driver (browser or other) based on the current stack. Assume no
> framework, no ports, no seeding tool.

## 1. Pick the mode

| Condition | Mode |
|---|---|
| argument `HP` / `all` (or equivalent) | **HP** — the whole suite |
| else, on an `integration/*` branch with an open integration→develop MR | **HP** |
| else (on a `feature/*`) | **FP** — the current sub-issue |

When ambiguous (on `develop`/`main`, no argument), ask which mode to run.

## 2. Shared prerequisites

- The **app runs locally** (start it the way the project expects; if you don't know how,
  ask). No running stack = no execution.
- You know how to drive the app (driver of your choice).

## 3. FP mode (sub-issue → integration auto-merge gate)

1. Derive the issue reference from the branch name `feature/<issue-ref>-<slug>` and fetch the
   issue from the technical backlog (see `docs/agents/issue-tracker.md`).
2. Read the **"Acceptance criteria → Feature Path (FP)"** section. It's a *behavioral*
   journey: translate it into concrete actions at runtime.
3. Run the journey against the app, UI-first (probe the backing store only if one exists and
   the UI is not enough).
4. Report: pass/fail per step **+ findings**.

**Gate** (see `git-flow`): auto-merge only happens if the FP is **green** *and* no **blocking
finding** is raised — on top of green build + tests. On red or a blocking finding: **do not
merge**, report.

## 4. HP mode (integration → develop gate, human decision)

1. Read all `docs/test-scenarios/HP-*.md`.
2. Run each journey **independently** against the app (an HP must run on its own).
3. Apply the **execution rules** (below).
4. Produce a per-scenario report (pass/fail) + a consolidated **Findings** section, ready to
   paste into the integration→develop PR.

The agent **never merges** into `develop`: it runs, reports, and proposes the HP curation
(see `git-flow`). HP red → the human decides.

## 5. Execution rules (agent)

- **Retry on different data before raising a data-related finding.** If a step fails on a
  particular instance, retry with another instance of the same kind; a "data" finding is only
  justified if **all** reasonable instances fail, or if the behavior is structural.
- **Raise all findings**, blocking or not (console warning, surprising behavior, side effect,
  real breakage). You **qualify the severity**; a blocking finding fails the gate.
- **UI-first.** A backing-store probe only complements what the UI doesn't show, and only if a
  store exists.
- **Data selection by characteristics** (filters, badges…), not hard-coded IDs (HP mode): if
  no data satisfies the conditions, that's a legitimate signal, not an excuse to bypass the UI.

## 6. Report format

For each journey: `✅ / ❌ <id or issue> — <title>`, the failing steps, then:

```
## Findings
- [blocking] …
- [non-blocking] …
```
