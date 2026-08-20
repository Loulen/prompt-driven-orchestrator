---
name: implement
description: Implement a spec or set of tickets by orchestrating a tdd role, an independent code-review role, and an agentic-tests role in a loop until build, tests, and the Feature Path are green with no blocking finding.
disable-model-invocation: true
---

# Implement

Implement the work described in the spec or tickets, looping three roles until the work is **done**: the project **builds**, the **test suite** is green, and the ticket's **Feature Path (FP)** is green — with **no blocking finding** open.

Input: a spec (`/to-spec`) or one or more tickets (`/to-tickets`) on the technical backlog. Read `CONTEXT.md` for domain vocabulary and respect the ADRs in the area you're touching. Work on the branch `git-flow` puts you on for this ticket (a `feature/*` off the story's `integration/*`, or off `develop`).

## The three roles

Use **subagents if available** — each role gets its own context so they don't contaminate each other, and the review role in particular stays **fresh eyes, not the author**. Where subagents aren't available, run the roles sequentially in this context (see *Degrading to one context*).

- **`/tdd`** — builds the lower tiers (unit, component, integration-style) test-first, at **pre-agreed seams**. Agree the seams with the user before the first test; work one vertical slice at a time, red → green.
- **`/code-review`** — an **independent** reviewer. Give it only the diff and the spec/ticket, not your implementation rationale. It runs the Standards and Spec axes and reports findings; a **blocking** finding holds the loop.
- **`/agentic-tests`** — drives the ticket's **Feature Path** through the **real running system** along its primary surface (UI, CLI, or warehouse), reporting pass/fail plus findings. See `skills/agentic-tests/`.

## The loop

1. **Slice.** Pick the next vertical slice of the spec/ticket. Confirm the seams to test with the user.
2. **Build it test-first.** Run `/tdd` for the slice: one failing test at a seam → minimal code to pass → repeat. Run typechecking and the affected test files continuously as you go.
3. **Repeat** slices until the spec/ticket is implemented, then run the **full test suite** once.
4. **Review, independently.** Run `/code-review` on the diff since the branch point, with the spec/ticket as the Spec axis input. Fold non-blocking findings in or note them; a **blocking** finding sends you back to step 2.
5. **Drive the Feature Path.** Run `/agentic-tests` to exercise the FP against the running system. A red FP or a blocking finding sends you back to step 2.
6. **Green gate.** The work is done when, together: the project **builds**, the **test suite** is green, the **FP** is green, and **no blocking finding** is open. Anything red loops back to step 2.

Refactoring belongs to the review stage, not the red → green cycle (see `/tdd`).

## Commit

Once the green gate holds, commit the work to the current branch, referencing the ticket. Don't merge — merging is `git-flow`'s gate (a `ready-for-agent` sub-ticket auto-merges into its `integration/*` on a green local check; `integration -> develop` stays human).

## Degrading to one context

Without subagents, run the same loop sequentially in this context: `/tdd` per slice, then `/code-review` on the diff, then `/agentic-tests` on the FP. The one thing you lose is the reviewer's independence — so at the review step, deliberately set aside your implementation reasoning and review the diff against the spec as an outsider would. This multi-subagent orchestration is the single capability ADR-0002 calls out as non-portable; this sequential run is its documented fallback.

## Next step

When the green gate holds and the work is committed, signpost the handoff: open a PR toward the branch this dev started from and let `git-flow` carry it toward integration. Guided, not gated — name the step, don't force it.
