# Domain docs

Single-context repo: one `CONTEXT.md` at the repo root (business glossary), decisions in `docs/adr/`.

**The authoring rules live in the `grill-with-docs` skill** — they apply to *every* edit of these files, not just grilling sessions:

- [`.claude/skills/grill-with-docs/CONTEXT-FORMAT.md`](../../.claude/skills/grill-with-docs/CONTEXT-FORMAT.md) — glossary-not-spec, no changelog, prune as you go
- [`.claude/skills/grill-with-docs/ADR-FORMAT.md`](../../.claude/skills/grill-with-docs/ADR-FORMAT.md) — what stays out, how to amend, size smell

One-line version: CONTEXT.md = vocabulary + intent (pointer to the ADR for the contract) · ADR = decision + why + the measurements that killed the alternatives · implementation plans = the issue/PR · history = git.
