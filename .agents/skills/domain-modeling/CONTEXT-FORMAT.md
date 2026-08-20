# CONTEXT.md Format

## Structure

```md
# {Context Name}

{One or two sentence description of what this context is and why it exists.}

## Language

**Order**:
{A one or two sentence description of the term}
_Avoid_: Purchase, transaction

**Invoice**:
A request for payment sent to a customer after delivery.
_Avoid_: Bill, payment request

**Customer**:
A person or organization that places orders.
_Avoid_: Client, buyer, account
```

## What belongs (and where the rest goes)

`CONTEXT.md` is loaded by every agent in every session — each sentence is paid for on every task, so each sentence must earn it. It carries exactly two things:

1. **The language** — terms, `_Avoid_` aliases, and the semantics a *user* of the system must know to use it correctly: the contract of the project's own surface (a DSL, a file format, a CLI or API that users script against). Users won't read the implementation; this is their spec.
2. **The intent** — product principles and the reasoning that constrains features that don't exist yet ("no prescriptive validation", "the tool never acts on its own initiative"). Product reasoning stays: it is the user's will made durable, and it prevents whole classes of wrong proposals.

Everything else lives elsewhere:

- **Implementation reasoning** (why this component works this way, failure-mode narratives, enforcement mechanisms) → the ADR or the ticket. The glossary keeps at most a **decision marker** — "deliberate asymmetry (#268)" — because an agent only follows a pointer when something signals a decision exists; three words and a link buy that.
- **A deliberate deviation local to one code site** → a code comment *at that site*, where the next editor will actually look. Zero drift possible.
- **Descriptions of the system** (screens, endpoints, schemas, stack) → the code shows them; ADRs carry the choices.

**The test for every sentence**: would an agent doing *generic* work in this repo take a wrong decision without it? If it only helps someone working on that exact feature, it belongs behind a pointer.

## Rules

- **Be opinionated.** When multiple words exist for the same concept, pick the best one and list the others under `_Avoid_`.
- **Keep definitions tight.** One or two sentences max. Define what it IS, not what it does.
- **Only include terms specific to this project's context.** General programming concepts (timeouts, error types, utility patterns) don't belong even if the project uses them extensively. Before adding a term, ask: is this a concept unique to this context, or a general programming concept? Only the former belongs.
- **Group terms under subheadings** when natural clusters emerge. If all terms belong to a single cohesive area, a flat list is fine.
- **Pointers, not prose.** An entry that needs a contract points to it (`ADR-NNNN`, `#268`) — the index function is welcome, the inlined contract is not. Tripwire: a function name, an HTTP status table, an exit code, a test name, or a `file:line` means you're writing the wrong document.
- **No changelog.** Git owns history. When a definition changes, rewrite the entry in place; never keep "shipped by #123" bullets, dated corrections, or notes about what the entry used to say wrongly.
- **Prune as you go.** Inline updates are also the pruning mechanism: when you touch an entry that already violates these rules, shrink it in the same edit.

## Single vs multi-context repos

**Single context (most repos):** One `CONTEXT.md` at the repo root.

**Multiple contexts:** A `CONTEXT-MAP.md` at the repo root lists the contexts, where they live, and how they relate to each other:

```md
# Context Map

## Contexts

- [Ordering](./src/ordering/CONTEXT.md) — receives and tracks customer orders
- [Billing](./src/billing/CONTEXT.md) — generates invoices and processes payments
- [Fulfillment](./src/fulfillment/CONTEXT.md) — manages warehouse picking and shipping

## Relationships

- **Ordering → Fulfillment**: Ordering emits `OrderPlaced` events; Fulfillment consumes them to start picking
- **Fulfillment → Billing**: Fulfillment emits `ShipmentDispatched` events; Billing consumes them to generate invoices
- **Ordering ↔ Billing**: Shared types for `CustomerId` and `Money`
```

The skill infers which structure applies:

- If `CONTEXT-MAP.md` exists, read it to find contexts
- If only a root `CONTEXT.md` exists, single context
- If neither exists, create a root `CONTEXT.md` lazily when the first term is resolved

When multiple contexts exist, infer which one the current topic relates to. If unclear, ask.
