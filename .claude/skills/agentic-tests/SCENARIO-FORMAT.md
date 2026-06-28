# Scenario format — agentic tests (HP / FP)

Reference format for the journeys driven by `/agentic-tests`. The layer's *concept* (the two
levels, the gates) is in `CLAUDE.md` (root). The runner is the `/agentic-tests` skill.

Two levels:

- **Happy Path** (`HP-`): curated, **permanent** suite, core value (paths taken by 80 %+ of
  users). Lives in `docs/test-scenarios/HP-*.md` (created at the first curation). Worth as much
  as **critical-path documentation** as it is a regression suite. Gate `integration -> develop`.
- **Feature Path** (`FP-`): **no file** here. It lives in the acceptance criteria of the
  technical-backlog sub-issues (see `/to-issues`) and is **throwaway** — it dies with the
  issue. If a piece of an FP is worth keeping, **graft it drive-by** onto an HP. Gate sub-issue
  `-> integration`.

## Vocabulary

| Term | Meaning |
|---|---|
| agentic test | the layer / pyramid tier |
| Happy Path (`HP-`) | curated, permanent suite, core value, gate integration→develop |
| Feature Path (`FP-`) | in-issue journey, throwaway, gate sub-issue→integration |
| drive-by | a feature validated while crossing a screen during an HP |
| finding | a signal raised at the end of a run, blocking or not |

Journeys strictly use the **domain terms** (see `CONTEXT.md` at the root).

## HP inventory

The inventory lives in `docs/test-scenarios/README.md`, created with the first HP. Suggested
format:

| ID | Title | Covers | Status |
|---|---|---|---|
| HP-01 | … | … | … |

**At most 3 HP.** The HP is the core value, not a catch-all. To add a 4th: merge two journeys,
drop a non-critical one, or graft drive-by onto an existing HP. Curation is a **human decision**,
at the integration→develop MR (see `git-flow`).

## HP format

```md
---
id: HP-01
covers: [domain-term, domain-term]   # screens/flows in CONTEXT.md language
---

# HP-01 — {Title}

## Goal
{1-2 business sentences — what is validated end-to-end.}

## Drive-by
- List of features validated along the way.

## Preconditions
- App running locally, clean data state.
  (Agnostic: no hard-coded ports / commands / tech.)

## Journey
1. {business action} → {what is observed}
2. …

## Checks
### UI
- {what the screen must show after the action}

### Backing store (optional)
- {only if a store exists and the UI is not enough — in natural language}

## Cleanup (best-effort)
- {nothing, or how to get back to a clean state}

## Notes
- {known pitfalls, false positives}
```

## Execution rules

Carried by the `/agentic-tests` runner:

- **Retry on different data** before raising a data-related finding.
- **Raise all findings**, blocking or not.
- **UI-first**; probe the backing store only as a complement.
- **Independence**: an HP runs on its own, with no prerequisite from another.

## Adding / curating an HP

1. At the integration→develop MR, the agent runs the suite and **proposes** the curation.
2. Allocate the next free `HP-NN`, create the file under `docs/test-scenarios/`, follow the
   format, update the inventory — within the **max 3** limit.
3. Run the scenario once to confirm it is executable.
