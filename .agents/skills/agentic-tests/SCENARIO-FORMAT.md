# Scenario format — agentic tests (HP / FP)

Reference format for the journeys driven by `/agentic-tests`. The layer's *concept* (the two
levels, the gates) is in `CLAUDE.md` (root). The runner is the `/agentic-tests` skill, which also
maps each system type to its **primary surface** and a recommended driver.

Two levels:

- **Happy Path** (`HP-`): curated, **permanent** suite, core value (paths taken by 80 %+ of
  users). Lives in `docs/test-scenarios/HP-*.md` (created at the first curation). Worth as much
  as **critical-path documentation** as it is a regression suite. Gate `integration -> develop`.
- **Feature Path** (`FP-`): **no file** here. It lives in the acceptance criteria of the
  technical-backlog tickets (see `/to-tickets`) and is **throwaway** — it dies with the
  ticket. If a piece of an FP is worth keeping, **graft it drive-by** onto an HP. Gate ticket
  `-> integration`.

## Vocabulary

| Term | Meaning |
|---|---|
| agentic test | the layer / pyramid tier |
| Happy Path (`HP-`) | curated, permanent suite, core value, gate integration→develop |
| Feature Path (`FP-`) | in-ticket journey, throwaway, gate ticket→integration |
| primary surface | how a real user reaches the system (UI, CLI, warehouse…) — driven first |
| drive-by | a feature validated while crossing the surface during an HP |
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
covers: [domain-term, domain-term]   # flows in CONTEXT.md language
---

# HP-01 — {Title}

## Goal
{1-2 business sentences — what is validated end-to-end.}

## Drive-by
- List of features validated along the way.

## Preconditions
- System running locally, reachable through its primary surface, clean data state.
  (Agnostic: no hard-coded ports / commands / tech.)

## Journey
1. {business action} → {what is observed}
2. …

## Checks
### Surface
- {what the primary surface must show/return after the action}

### Internals (optional)
- {only when the surface isn't enough — in natural language: store, logs, live state}

## Cleanup (best-effort)
- {nothing, or how to get back to a clean state}

## Notes
- {known pitfalls, false positives}
```

## Execution rules

Carried by the `/agentic-tests` runner:

- **Retry on different data** before raising a data-related finding.
- **Raise all findings**, blocking or not.
- **Surface-first**; probe internals only as a complement.
- **Independence**: an HP runs on its own, with no prerequisite from another.

## Adding / curating an HP

1. At the integration→develop MR, the agent runs the suite and **proposes** the curation.
2. Allocate the next free `HP-NN`, create the file under `docs/test-scenarios/`, follow the
   format, update the inventory — within the **max 3** limit.
3. Run the scenario once to confirm it is executable.
