---
name: grill-with-docs
description: Per-story grilling that stress-tests your plan against the project's domain model, sharpens terminology, and writes CONTEXT.md/ADRs inline as decisions crystallise. Run it when designing a business user story before slicing it into work.
disable-model-invocation: true
---

Call the Skill tool twice, for **grilling** (the interview method) and **domain-modeling** (the glossary + ADR discipline). Together they are the session: grill one decision at a time down the design tree, and write `CONTEXT.md`/ADRs inline as terms and decisions land. Everything about *how* to grill and *what* to write lives in those two skills — this skill only adds where the session runs and what comes next.

## Branch placement

This session writes versioned context files (`CONTEXT.md`, ADRs). Before the first file write, land on the **integration branch** for this business user story — not on `develop` or a stray `feature/*`:

- Branch `integration/<business-ref>-<slug>` from up-to-date `develop` (create it if absent). Name it after the **business user story** from the business backlog (its reference + a slug) — you're at the grilling step, *before* `/to-spec`, so the branch is NOT named after the spec/technical ticket (it doesn't exist yet). See `docs/agents/business-backlog.md` for where the business backlog lives.
- The grilling output lands here. `/to-spec` then synthesises the spec; `/to-tickets` pushes this branch and bases the sub-tickets on it; `ready-for-agent` sub-tickets auto-merge back into the integration branch (after a green local check), and the `integration -> develop` merge stays human.

See the `git-flow` skill for the full integration-branch workflow.

## Next step

When shared understanding is reached and the docs are written, signpost the next step: `/to-spec` to synthesise the spec on the technical backlog from this grilling output. Guided, not gated — name the step, don't force it.
