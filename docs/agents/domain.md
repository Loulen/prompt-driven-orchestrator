# Domain docs

Single-context repo: one `CONTEXT.md` at the repo root (business glossary), decisions in `docs/adr/`.

## Read, don't write

`CONTEXT.md` and the ADRs have a **single writer: the grilling session** (`/grill-with-docs`) — that's the point of grilling: externalizing business context and technical decisions into one deliberate step. Implementation, triage, and review **read** them; they never amend them. A discovery that invalidates an ADR (a mechanism measured broken, a decision that didn't survive contact) goes **in the issue/PR** with its measurements; the doc amendment happens in the next grilling session.

## Reading rules

- Use the glossary's vocabulary in your outputs; don't drift to the `_Éviter_` synonyms. A missing concept is a signal for the next grilling, not a licence to invent language.
- If your output contradicts an ADR, surface it explicitly ("Contradicts ADR-NNNN — worth reopening because…") rather than silently overriding.

## Authoring rules (for grilling sessions)

The formats live in the skill: [`CONTEXT-FORMAT.md`](../../.claude/skills/grill-with-docs/CONTEXT-FORMAT.md) (glossary-not-spec, no changelog, prune as you go) and [`ADR-FORMAT.md`](../../.claude/skills/grill-with-docs/ADR-FORMAT.md) (what stays out, how to amend, size smell). One-line version: CONTEXT.md = vocabulary + intent (pointer to the ADR for the contract) · ADR = decision + why + the measurements that killed the alternatives · implementation plans = the issue/PR · history = git.
