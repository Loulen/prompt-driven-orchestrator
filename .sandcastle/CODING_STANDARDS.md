# Coding Standards — PDO

The reviewer loads this file via `@.sandcastle/CODING_STANDARDS.md`. The implementer reads it as a reference. Keep it tight.

## Source of truth

Before any non-trivial change, read these:

- `CONTEXT.md` at the repo root — project glossary and design decisions. Use its vocabulary in code, tests, and commit messages (`Pipeline`, `Node`, `NodeRun`, `Run`, `Blackboard`, `code-mutating`, `doc-only`, `Cycle` as emergent property, etc.).
- `docs/adr/0001-sharp-tool-not-safe-tool.md` — no prescriptive validation of pipelines.
- `docs/adr/0002-mechanical-conditionals-only.md` — `when:` clauses reference only `iter`, frontmatter fields, and `$<var>`. No LLM-eval, no string-expression, no semantic predicates.
- `docs/adr/0003-stack-rust-react-xyflow.md` — daemon Rust (Tokio + Axum + sqlx + notify + serde_yaml), frontend React + Vite + xyflow + shadcn/ui, single binary with embedded frontend (rust-embed).
- `docs/design/` — visual source of truth (HTML/CSS/JS bundle). Design tokens in `docs/design/project/styles.css`.

If your change touches a domain area not yet captured in the glossary, add an entry; if it sets a new architectural direction, propose an ADR.

## Style

### Rust

- Edition 2021 (or 2024 if Slice 1 has decided on it).
- `cargo fmt` enforced. `cargo clippy -- -D warnings` enforced.
- Prefer `Result<T, E>` over panicking, except in `unreachable!` arms with a clear invariant.
- Use `thiserror` for crate-level error types, `anyhow` for binary entry points.
- One module = one responsibility. Deep modules over shallow wrappers.
- Public APIs of deep modules (`pipeline-parser`, `condition-evaluator`, `prompt-augmenter`, etc.) must be small and documented.

### TypeScript / React

- Strict `tsconfig.json`. No `any` without an explicit comment justifying it.
- Functional components, hooks, no class components.
- Component files PascalCase (`RunsListPanel.tsx`), utility files camelCase.
- shadcn/ui primitives composed; no parallel UI library.
- Tailwind classes consumed via the design tokens copied from `docs/design/project/styles.css`.

## Build / test commands

Run from the repo root unless noted. Slice 1 sets these up — earlier slices may not have all of them yet.

| Purpose | Command |
|---|---|
| Type-check Rust | `cargo check --workspace --all-targets` |
| Test Rust | `cargo test --workspace` |
| Lint Rust | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format Rust | `cargo fmt --all --check` |
| Type-check frontend | `cd frontend && npm run typecheck` |
| Test frontend | `cd frontend && npm run test` |
| Lint frontend | `cd frontend && npm run lint` |
| Build frontend | `cd frontend && npm run build` |

A change touching only the Rust crates needs the Rust commands; only the frontend needs the frontend ones; both need both. The reviewer should run whatever is relevant to the diff.

## Testing

- Tests target **external behavior**, not implementation details. If a refactor changes the implementation but not the public contract, existing tests should still pass.
- Every deep module shipped in this MVP must have unit tests covering the cases enumerated in the parent issue's acceptance criteria. This is non-negotiable per the PRD.
- Side-effectful modules (`git-worktree-manager`, `tmux-supervisor`, `http-server`, `file-watcher`) are NOT integration-tested in v1; they are validated through end-to-end scenarios that come together as features ship.
- Use `pretty_assertions` and `insta` (snapshot testing) where the output is deterministic and complex.

## Architecture

- Deep modules are crates or modules with a small, well-named public API and rich internal logic. Resist the urge to expose internals.
- Side-effectful modules wrap `git`, `tmux`, FS, and HTTP. They are thin and replaceable.
- Event-sourced state: SQLite append-only event log is the source of truth. State is a projection. No `state.yaml` or in-memory snapshot that can drift.
- Sharp-tool philosophy (ADR-0001): the editor never blocks the user. Surface info-only warnings if useful, never errors.
- Mechanical conditionals (ADR-0002): no LLM in the routing layer.
