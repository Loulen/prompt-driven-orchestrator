---
name: to-us
description: Turns a feature or idea into business User Stories on the business backlog. Runs a senior-PO grilling — the grilling method guided by the existing CONTEXT.md glossary — writes each retained slice to the US template, and publishes backend-agnostically after a mandatory human review. The PO front of the pipeline, upstream of design. Run it when you want to shape a feature into ready-to-pick User Stories.
disable-model-invocation: true
---

# /to-us

**Feature → PO grilling → User Stories on the business backlog.** This is the **PO front** of the
factory: it turns a feature (a name, an idea, source items) into business **User Stories** — the
"what" and "why", owned by humans. Everything technical stays downstream: once a US is picked,
`/grill-with-docs` designs it, then `/to-spec` → `/to-tickets` slice it into technical work.

**Expert in the loop.** The human is the PO here. `/to-us` interrogates, drafts, and proposes;
it **never publishes a User Story without an explicit human go-ahead** (§3).

## 0. Frame

- **Input: the feature.** A name or an idea is enough; if it's missing, ask for it. Then gather the
  raw material: source items already on the business backlog, session notes, dropped files, mockups,
  and the existing `CONTEXT.md` glossary.
- **Branch placement.** A brand-new feature has **no business reference yet** — the User Stories this
  session produces are what *become* referenceable. So `/to-us` does not open an
  `integration/<business-ref>-…` branch (that's the per-story grilling's job, later).
- **Read the context, don't write it.** `/to-us` **reads** `CONTEXT.md` and the ADRs to ground the PO
  grilling; it is not a writer of them. Per ADR-0004 the only writers are the two grilling sessions
  (`/build-factory` context mode and `/grill-with-docs`). If the PO grilling exposes a genuine gap or
  ambiguity in the **business glossary**, surface it and route it to a grilling session — don't amend
  `CONTEXT.md` here.

## 1. PO grilling

Run a **senior-PO grilling** on the feature — the relentless one-question-at-a-time interview method
of the [`grilling`](../grilling/SKILL.md) skill, with a PO posture layered on top and the **existing
context as the guide**:

- **The object grilled is the feature** — its value, its actors, its scope — **not** a technical
  plan. Interrogate the four things a User Story lives or dies on: **value** (the "so that"),
  **actor** (who), **outcome** (the observable result), and **acceptance** (how you'll know it's
  done).
- **Guided by `CONTEXT.md`.** Read the glossary and the ADRs first, then hold the PO to them:
  challenge a term that drifts from the established language (steer to the canonical word, flag an
  `_Avoid_` synonym), and stress-test each capability against the documented decisions. You are
  grounding the PO in the project's own language — reading it, never amending it.
- **Value altitude.** The grilling arbitrates value and behavior only — **no** technology and **no**
  data-model decisions. Those belong to the downstream per-story grilling, not here.
- **Arbitrate scope** — for each capability, decide **in-scope** vs **deferred**. Note deferrals as
  you go; they'll become their own backlog items (§2).

## 2. Write the User Stories

- **Vertical slices.** Cut thin slices through the whole path, admin→user, each demoable on its own.
  Guiding rule: never load data you can't yet show — so an **admin ingestion is a User Story in its
  own right**, not a prerequisite buried inside a user-facing one.
- **Resolve the layout, project-first.** Read `docs/agents/us-format.md` if it exists — a project
  that tailored its own US layout wins. Absent it, fall back to the co-located core default
  [US-FORMAT.md](./US-FORMAT.md).
- **Each retained slice** → one User Story at the resolved layout. **Each deferred item** → a title
  plus a one-line intent (no full template).
- **Completion criterion:** every capability surfaced in the grilling is accounted for — retained as
  a full US or listed as a deferral. Nothing from the grilling is silently dropped.

## 3. Mandatory human review

Present **all** the drafted User Stories in the conversation — retained and deferred — and get an
**explicit go-ahead** before any publish gesture. This is the expert-in-the-loop gate: the human, as
PO, validates the value, the scope calls, and the wording. Nothing reaches the backlog until they
say so.

## 4. Publish — backend-agnostically

Read `docs/agents/business-backlog.md` (the business-backlog port; core-default pattern:
`skills/build-factory/business-backlog.md`) and publish **exactly the way it documents** — the
backend may be GitHub/GitLab Projects, Trello, Jira, Linear, a spreadsheet, or local markdown. Never
hardcode a backend here.

- **Create** each retained User Story as an item on the business backlog, via the port's create
  gesture. Deferred items go wherever the port sends future work (e.g. a backlog column/state).
- **Assignment and labeling are project config** — apply whatever rules the port documents
  (assignee, feature label, a "ready for human review" marker). If the port defines none, publish
  without them; don't invent labels or assignees.
- **Verify, then report.** Re-read one published item through the port to confirm it landed intact,
  then report the created items with their links — deferrals included. If the port is manual
  (`<tool to define>`), hand the human the ready-to-paste User Stories instead.
