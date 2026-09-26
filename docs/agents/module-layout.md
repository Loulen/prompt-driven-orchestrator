# Module layout

Where a new file goes. The decision (modular monolith, one crate per bounded context, hexagonal
inside) is [ADR-0079](../adr/0079-pdo-est-un-monolithe-modulaire-un-crate-par-bounded-context-hexagonal-a-l-interieur.md);
the test pyramid that goes with it is [ADR-0080](../adr/0080-pyramide-de-tests-en-six-etages-les-use-cases-se-testent-avec-des-adaptateurs-en-memoire.md).

> "Module layout" here is the physical shape of the source tree. It is **not** the
> `layout`/`semantic` partitioning of pipeline nodes (CONTEXT.md). Say "module layout", never bare
> "layout".

## Two axes

- **Business axis = crate.** One crate per bounded context: `run`, `pipeline`, `review`,
  `session`, `harness`, `sandbox`, `skill`, `trigger`, `stats`, `workspace`, `settings`,
  `platform`. A context only sees what another context makes `pub`. Crates cannot form cycles.
- **Technical axis = module inside the crate.** Every context crate has the same three modules:
  - `domain/` holds the pure rules. No I/O, no tokio process, no sqlx, no axum, no `adapters::`.
  - `application/` holds `ports.rs` (the traits the context needs) and one file per use case.
    Every mutation is a use case that builds an Opération and goes through the Autorisation
    d'opération.
  - `adapters/` holds the driving side (`http/`: routes and DTOs exported to TypeScript) and the
    driven side (SQLite, git, tmux, fs… implementations of the ports).

Shared crates:

- `pdo-kernel` is the shared kernel: Acteur, Opération, authorization, event bus, extension host,
  entity attributes. Keep it small and stable.
- `pdo-extension-api` is the public contract (semver, ADR-0081). It depends on nothing in core.
- `pdo` (bin) is the composition root. It wires adapters onto ports, merges each context's
  `router()`, and holds the CLI and the embedded SPA. It is the only place that knows every
  context.

Frontend: `frontend/src/<context>/{api,model,store,components}/`, plus `shared/` (`ui/` for shadcn,
`http/`, `generated/` for ts-rs types that you never edit by hand, `extensions/`, `lib/`) and
`app/`. No barrel files: import from the file that owns the symbol.

## Where does my change go?

1. Which business subject is it about? That tells you the context crate (or frontend folder).
2. Is it a rule, an orchestration, or a technical detail? That tells you `domain/`,
   `application/` or `adapters/`.
3. Does it need something from another context? Use its public API. If that API doesn't exist,
   add it on the owning side. Never reach into another crate's internals, and never create a
   cycle. A cycle means the concept belongs in the other context or in the kernel.

## Rules checked by the test suite

- **Architecture test.** A `domain/` module imports nothing technical and no `adapters::`.
- **File size.** A production file has **at most 400 lines**. Tests are not counted. Files that
  are still too big are listed in a baseline that may only shrink.
- **Route coverage.** Every route of the assembled router has at least one HTTP-level test.

## During the migration

The legacy `crates/pdo-daemon` crate is strangled one context at a time. It only shrinks: new
behaviour goes into the context crate once that crate exists. While it still exists,
`scripts/layout-ratchet.sh` keeps guarding its flat list. The ratchet disappears along with the
legacy crate.
