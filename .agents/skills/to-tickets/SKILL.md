---
name: to-tickets
description: Break a plan, spec, or the current conversation into vertical-slice tickets on the technical backlog, each declaring its blocking edges — text in one file per ticket locally, or native blocking links on GitHub/GitLab.
disable-model-invocation: true
---

# To Tickets

Break a plan, spec, or conversation into a set of **tickets** — vertical slices, each declaring the tickets that **block** it.

The issue tracker and triage label vocabulary should have been provided to you (in `docs/agents/`) — run `/build-factory` if not.

## Process

### 1. Gather context

Work from whatever is already in the conversation context. If the user passes a reference (a spec path, an issue number or URL) as an argument, fetch it and read its full body and comments.

### 2. Explore the codebase (optional)

If you have not already explored the codebase, do so to understand the current state of the code. Ticket titles and descriptions should use the project's domain glossary vocabulary, and respect ADRs in the area you're touching.

Look for opportunities to prefactor the code to make the implementation easier. "Make the change easy, then make the easy change."

### 3. Draft vertical slices

Break the work into **vertical slice** tickets.

<vertical-slice-rules>

- Each slice cuts a narrow but COMPLETE path through every layer end-to-end — vertical, NOT a horizontal slice of one layer
- A completed slice is demoable or verifiable on its own
- Each slice is sized to fit in a single fresh context window
- Any prefactoring should be done first

</vertical-slice-rules>

Give each ticket its **blocking edges** — the other tickets that must complete before it can start. A ticket with no blockers can start immediately.

**Wide refactors are the exception to vertical slicing.** A **wide refactor** is one mechanical change — rename a column, retype a shared symbol — whose **blast radius** fans across the whole codebase, so a single edit breaks thousands of call sites at once and no vertical slice can land green. Don't force it into a vertical slice; sequence it as **expand–contract**. First expand: add the new form beside the old so nothing breaks. Then migrate the call sites over in batches sized by blast radius (per package, per directory), each batch its own ticket blocked by the expand, keeping CI green batch to batch because the old form still exists. Finally contract: delete the old form once no caller remains, in a ticket blocked by every migrate batch. When even the batches can't stay green alone, keep the sequence but let them share an integration branch that all block a final integrate-and-verify ticket — green is promised only there.

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For each ticket, show:

- **Title**: short descriptive name
- **Blocked by**: which other tickets (if any) must complete first
- **What it delivers**: the end-to-end behaviour this ticket makes work
- **Feature Path (FP)**: the nominal end-to-end journey that will gate this ticket's auto-merge — a short behavioral journey (no clicks, no tech). Review it here, alongside the ticket itself.

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the blocking edges correct — does each ticket only depend on tickets that genuinely gate it?
- Should any tickets be merged or split further?

Iterate until the user approves the breakdown.

### 5. Publish the tickets to the configured tracker

Publish the approved tickets. **How** depends on the tracker `/build-factory` configured — the tickets are the same either way, only the shape of the blocking edges changes:

- **Local files** → write one file per ticket under `.scratch/<feature-slug>/tickets/<NN>-<slug>.md`, numbered from `01` in dependency order (blockers first). Each file's "Blocked by" lists the numbers/titles it depends on. Use the per-ticket file template below — one ticket per file, never a single combined file.
- **A real issue tracker (GitHub, GitLab)** → publish one issue per ticket in dependency order (blockers first) so each ticket's blocking edges can reference real identifiers. Use the platform's native blocking / sub-issue relationship where it has one; otherwise set each ticket's "Blocked by" to the blocking issues. Apply the `ready-for-agent` triage label unless instructed otherwise — the tickets are agent-grabbable by construction.

Work the **frontier**: any ticket whose blockers are all done. For a purely linear chain that means top to bottom.

**Integration branch.** If the plan came from a `grill-with-docs` session, an `integration/<business-ref>-<slug>` branch (named after the business user story from the business backlog, created during grilling) holds the grilling output (`CONTEXT.md`, ADRs). Push that branch, and **state in each ticket body that the ticket is implemented on the business-story integration branch** — branch from it and merge back into it, NOT `develop`. `ready-for-agent` tickets auto-merge into the integration branch after a green local check (the project's build + test commands + green FP); the `integration -> develop` merge stays human. See the `git-flow` skill. (No integration branch? Tickets base on `develop` as usual.)

Do NOT close or modify any parent issue.

<local-ticket-template>

# <NN> — <Ticket title>

**What to build:** the end-to-end behaviour this ticket makes work, from the user's perspective — not a layer-by-layer implementation list.

**Blocked by:** the numbers/titles of the tickets that gate this one, or "None — can start immediately".

**Status:** ready-for-agent

- [ ] Acceptance criterion 1
- [ ] Acceptance criterion 2

### Feature Path (FP)

The executable nominal journey for this ticket — the subset a subagent runs against the running system to gate this ticket's auto-merge into the integration branch (see the `agentic-tests` skill and the `git-flow` gate). Behavioral, NOT clicks or tech-specific; the implementing agent translates it into concrete actions at runtime. Throwaway: it lives with the ticket and dies with it. Error/edge cases stay in lower-tier tests.

1. <user's business action> → <what is observed>
2. …

Verify: through the ticket's primary surface (what the user must see / be able to do); probe a backing-store only if one exists and the surface is not enough.

</local-ticket-template>

<issue-template>

## Parent

A reference to the parent issue on the tracker (if the source was an existing issue, otherwise omit this section).

## What to build

The end-to-end behaviour this ticket makes work, from the user's perspective — not layer-by-layer implementation.

## Acceptance criteria

The full set of acceptance criteria — including checks NOT observable during a journey (those are verified by lower-tier unit/component tests or review):

- [ ] Criterion 1
- [ ] Criterion 2

### Feature Path (FP)

The executable nominal journey for this ticket — the subset a subagent runs against the running system to gate this ticket's auto-merge into the integration branch (see the `agentic-tests` skill and the `git-flow` gate). Behavioral, NOT clicks or tech-specific; the implementing agent translates it into concrete actions at runtime. Throwaway: it lives with the ticket and dies with it. Error/edge cases stay in lower-tier tests.

1. <user's business action> → <what is observed>
2. …

Verify: through the ticket's primary surface (what the user must see / be able to do); probe a backing-store only if one exists and the surface is not enough.

## Blocked by

- A reference to each blocking ticket, or "None — can start immediately".

</issue-template>

In either form, avoid specific file paths or code snippets — they go stale fast. Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it and note briefly that it came from a prototype. Trim to the decision-rich parts — not a working demo, just the important bits.

### 6. Comment the tickets back on the business backlog

If the work originated from the business backlog (a card/story there), add a comment on that item listing the technical tickets just created (references + links), for traceability. See `docs/agents/business-backlog.md` for how to reach the business backlog. Skip if there is no originating business item, or if no business backlog is configured.
