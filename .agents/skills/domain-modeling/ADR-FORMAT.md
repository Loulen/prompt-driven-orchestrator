# ADR Format

ADRs live in `docs/adr/` and use sequential numbering: `0001-slug.md`, `0002-slug.md`, etc.

Create the `docs/adr/` directory lazily — only when the first ADR is needed.

## Template

```md
# {Short title of the decision}

{1-3 sentences: what's the context, what did we decide, and why.}
```

That's it. An ADR can be a single paragraph. The value is in recording *that* a decision was made and *why* — not in filling out sections.

## Optional sections

Only include these when they add genuine value. Most ADRs won't need them.

- **Status** frontmatter (`proposed | accepted | deprecated | superseded by ADR-NNNN`) — useful when decisions are revisited
- **Considered Options** — only when the rejected alternatives are worth remembering
- **Consequences** — only when non-obvious downstream effects need to be called out

## What stays out

An ADR records the decision and its why. The implementation lives elsewhere — move to the ticket/PR (or delete):

- **Implementation plans**: step lists, exhaustive case matrices, test inventories, rollout steps. The grilling produced them; the ticket consumes them.
- **Code citations**: function names, `file.rs:42` references, pinned log strings, test names. They rot at the first refactor. Exception: a name that *is* the public contract (a CLI exit code, a wire-format field).
- **Restatements of what the code shows**: payload shapes, table schemas, UI copy.

What *does* belong even when it looks technical: the measurements that killed an alternative ("we tried X on the real data; it failed on the exact case it was meant to save"). No amount of code reading will ever reveal those — they are the treasure an ADR exists to keep.

**Size smell**: past ~100 lines, an ADR is almost certainly absorbing the implementation plan. Cut before committing.

## Amending an ADR

When a later decision revises an existing ADR:

- **Rewrite the body** in the current vocabulary so it reads true today. Never stack a dated addendum on top of a body that has become wrong, and never leave a translation instruction ("read X wherever it says Y").
- Mark the old ADR with a one-line pointer (`superseded by ADR-NNNN`, or `amended by ADR-NNNN: <one clause>`) in its status/preamble.
- Git owns the document's history; the document owns only the current truth. No dated correction trails.

## Numbering

Scan `docs/adr/` for the highest existing number and increment by one.

## When to offer an ADR

All three of these must be true:

1. **Hard to reverse** — the cost of changing your mind later is meaningful
2. **Surprising without context** — a future reader will look at the code and wonder "why on earth did they do it this way?"
3. **The result of a real trade-off** — there were genuine alternatives and you picked one for specific reasons

If a decision is easy to reverse, skip it — you'll just reverse it. If it's not surprising, nobody will wonder why. If there was no real alternative, there's nothing to record beyond "we did the obvious thing."

A deliberate deviation **scoped to a single code site** is better served by a code comment at that site than by an ADR: the constraint sits exactly where the next editor would undo it, with zero drift. Reserve ADRs for decisions that constrain work beyond one site.

### What qualifies

- **Architectural shape.** "We're using a monorepo." "The write model is event-sourced, the read model is projected into Postgres."
- **Integration patterns between contexts.** "Ordering and Billing communicate via domain events, not synchronous HTTP."
- **Technology choices that carry lock-in.** Database, message bus, auth provider, deployment target. Not every library — just the ones that would take a quarter to swap out.
- **Boundary and scope decisions.** "Customer data is owned by the Customer context; other contexts reference it by ID only." The explicit no-s are as valuable as the yes-s.
- **Deliberate deviations from the obvious path.** "We're using manual SQL instead of an ORM because X." Anything where a reasonable reader would assume the opposite. These stop the next engineer from "fixing" something that was deliberate.
- **Constraints not visible in the code.** "We can't use AWS because of compliance requirements." "Response times must be under 200ms because of the partner API contract."
- **Rejected alternatives when the rejection is non-obvious.** If you considered GraphQL and picked REST for subtle reasons, record it — otherwise someone will suggest GraphQL again in six months.
