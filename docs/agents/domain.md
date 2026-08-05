# Domain docs

Single-context repo: one `CONTEXT.md` at the repo root, decisions in `docs/adr/`.

## Where things live

| Content | Home |
|---|---|
| Vocabulary, product intent, domain semantics | `CONTEXT.md` — definition, the non-obvious *why*, `_Éviter_` aliases, pointer to the ADR that fixed the contract |
| Decisions: the why, rejected alternatives, the measurements that killed them | `docs/adr/` — one decision per ADR, sequential numbering |
| Implementation plans, case matrices, test inventories | The issue / PR that implements them — never the docs |
| History (what a doc used to say, when it changed) | Git — the docs carry only the current truth, no changelog bullets, no dated corrections |

## Rules that keep the docs small

- **CONTEXT.md is a glossary and nothing else.** An entry that needs the full contract points to its ADR (`ADR-NNNN`); it never inlines the contract. Tripwire: a function name, an HTTP status table, an exit-code table, a test name, or a `file:line` in CONTEXT.md means you're writing the wrong document.
- **An ADR records the decision, not the plan.** Keep the measurements that killed an alternative (they're invisible in the code); drop step lists, code citations, and pinned log strings. Past ~100 lines, an ADR is absorbing the implementation plan.
- **Amend by rewriting.** When a decision revises an earlier ADR, rewrite that ADR's body so it reads true today, and leave a one-line pointer (`amended by ADR-NNNN: <clause>`). Never stack dated addenda on a body that has become wrong, never leave translation instructions ("read X wherever it says Y").
- **Prune as you go.** Any edit that touches an entry already violating these rules shrinks it in the same edit.

The `grill-with-docs` skill (`.claude/skills/grill-with-docs/`) enforces these formats during grilling sessions — see its `CONTEXT-FORMAT.md` and `ADR-FORMAT.md`.
