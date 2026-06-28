---
name: to-issues
description: Break a plan, spec, or PRD into independently-grabbable issues on the project issue tracker using tracer-bullet vertical slices. Use when user wants to convert a plan into issues, create implementation tickets, or break down work into issues.
---

# To Issues

Break a plan into independently-grabbable issues using vertical slices (tracer bullets).

The issue tracker and triage label vocabulary should have been provided to you (in `docs/agents/`) — run `/build-factory` if not.

## Process

### 1. Gather context

Work from whatever is already in the conversation context. If the user passes an issue reference (issue number, URL, or path) as an argument, fetch it from the issue tracker and read its full body and comments.

### 2. Explore the codebase (optional)

If you have not already explored the codebase, do so to understand the current state of the code. Issue titles and descriptions should use the project's domain glossary vocabulary, and respect ADRs in the area you're touching.

### 3. Draft vertical slices

Break the plan into **tracer bullet** issues. Each issue is a thin vertical slice that cuts through ALL integration layers end-to-end, NOT a horizontal slice of one layer.

Slices may be 'HITL' or 'AFK'. HITL slices require human interaction, such as an architectural decision or a design review. AFK slices can be implemented and merged without human interaction. Prefer AFK over HITL where possible.

<vertical-slice-rules>
- Each slice delivers a narrow but COMPLETE path through every layer (schema, API, UI, tests)
- A completed slice is demoable or verifiable on its own
- Prefer many thin slices over few thick ones
</vertical-slice-rules>

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For each slice, show:

- **Title**: short descriptive name
- **Type**: HITL / AFK
- **Blocked by**: which other slices (if any) must complete first
- **User stories covered**: which user stories this addresses (if the source material has them)
- **Feature Path (FP)**: the nominal end-to-end journey that will gate this slice's auto-merge — a short behavioral parcours (no UI clicks, no tech). Review it here, alongside the slice itself.

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the dependency relationships correct?
- Should any slices be merged or split further?
- Are the correct slices marked as HITL and AFK?

Iterate until the user approves the breakdown.

### 5. Publish the issues to the issue tracker

For each approved slice, publish a new issue to the issue tracker. Use the issue body template below. These issues are considered ready for AFK agents, so publish them with the correct triage label unless instructed otherwise.

Publish issues in dependency order (blockers first) so you can reference real issue identifiers in the "Blocked by" field.

**Integration branch.** If the plan came from a `grill-with-docs` session, an `integration/<business-ref>-<slug>` branch (named after the business user story from the business backlog, created during grilling) holds the grilling output (`CONTEXT.md`, ADRs). Push that branch, and **state in each issue body that the sub-issue is implemented on the business-story integration branch** — branch from it and merge back into it, NOT `develop`. `ready-for-agent` sub-issues auto-merge into the integration branch after a green local check (the project's build + test commands); the `integration -> develop` merge stays human. See the `git-flow` skill. (No integration branch? Sub-issues base on `develop` as usual.)

<issue-template>
## Parent

A reference to the parent issue on the issue tracker (if the source was an existing issue, otherwise omit this section).

## What to build

A concise description of this vertical slice. Describe the end-to-end behavior, not layer-by-layer implementation.

Avoid specific file paths or code snippets — they go stale fast. Exception: if a prototype produced a snippet that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), inline it here and note briefly that it came from a prototype. Trim to the decision-rich parts — not a working demo, just the important bits.

## Acceptance criteria

The full set of acceptance criteria — including checks NOT observable during a parcours (those are verified by lower-tier unit/component tests or review):

- [ ] Criterion 1
- [ ] Criterion 2

### Feature Path (FP)

The executable nominal journey for this slice — the subset a subagent runs against the running app to gate this sub-issue's auto-merge into the integration branch (see the `agentic-tests` skill and the `git-flow` gate). Behavioral, NOT UI clicks or tech-specific; the implementing agent translates it into concrete actions at runtime. Throwaway: it lives with the issue and dies with it. Error/edge cases stay in lower-tier tests.

1. <user's business action> → <what is observed>
2. …

Verify: UI first (what the user must see / be able to do); probe a backing-store only if one exists and the UI is not enough.

## Blocked by

- A reference to the blocking ticket (if any)

Or "None - can start immediately" if no blockers.

</issue-template>

Do NOT close or modify any parent issue.

### 6. Comment the issues back on the business backlog

If the work originated from the business backlog (a card/story there), add a comment on that item listing the technical issues just created (references + links), for traceability. See `docs/agents/business-backlog.md` for how to reach the business backlog. Skip if there is no originating business item, or if no business backlog is configured.
