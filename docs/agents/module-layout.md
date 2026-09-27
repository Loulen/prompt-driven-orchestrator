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

- **Architecture test.** A `domain/` module imports nothing technical and no `adapters::`: no
  sqlx, no axum, no `tokio::process` (`crates/pdo/tests/architecture.rs`, proven on the fixtures
  of `crates/pdo/tests/fixtures/architecture/`).
- **File size.** A production file has **at most 400 lines**, Rust and TypeScript. Tests are not
  counted (`tests/`, a `#[cfg(test)]` module, `*.test.ts(x)`), nor is `shared/generated/`. Files
  that are still too big are listed with their size in `crates/pdo/tests/file_size_baseline.txt`,
  a list that may only shrink: remove a line once its file is back within the limit, never add
  one (`crates/pdo/tests/file_size.rs`).
- **Typed contract.** Every DTO sent over HTTP or `/ws` derives `ts_rs::TS` with
  `#[ts(export)]`; `make types` writes `frontend/src/shared/generated/`, and `make check` fails
  when it drifts from the Rust types.
- **Route coverage.** Every route of the assembled router has at least one HTTP-level test,
  method by method (`crates/pdo/tests/route_coverage.rs`). It reads the router's
  `.route(…)` calls and the URL literals of `tests/`. Write the call so the method is readable
  (`client.post(format!("{}/runs", …))`). When the router moves, point the test's
  `ROUTER_SOURCES` at its new home.

## During the migration

The legacy crate (`crates/pdo-daemon`, package `pdo-legacy`, library still `pdo_daemon`) is
strangled one context at a time. It only shrinks: new
behaviour goes into the context crate once that crate exists.

Contexts already out of the legacy crate:

- `platform` (#916): `crates/pdo-platform`, `frontend/src/platform/` (page mounts, update,
  audit journal, service unit). The first context out and the template the next ones copy:
  pure rules in `domain/`, `application/ports.rs` plus one use case per file, `adapters/` for
  the routes (`http/`), the CLI (`cli/`) and the implementations of the ports. Its use cases
  are tested against in-memory ports in `crates/pdo-platform/tests/use_cases/` (stage 2), its
  adapters against the real resource in their own `#[cfg(test)]` module (stage 3).
- `harness` (#919): `crates/pdo-harness`, `frontend/src/harness/`. The parsers of each
  harness's own artefacts (transcripts, turn state, reported cost, context peak, steering)
  live in its `domain/`; cost, stale detection, staging, tmux and stats read them from
  there. `RUST_LOG=pdo_daemon::harness_registry` becomes
  `RUST_LOG=pdo_harness::application::load_registry`.
- `sandbox` (#921): `crates/pdo-sandbox`, `frontend/src/sandbox/`. Docker sits behind the
  `DockerCli` port (`adapters/docker_cli.rs` runs the real binary; its stage-3 tests run
  against a real daemon when one answers, and say they skipped otherwise). Editing and
  deleting a staging profile are use cases that ask the Autorisation d'opération; preparing,
  cleaning up and killing inside a Run's sandbox are Run transitions and never do. The
  legacy `run_sandbox.rs` is the Run's side (it builds a Run's sandbox context from its
  state) and leaves with the run context. `RUST_LOG=pdo_daemon::sandbox_staging` becomes
  `RUST_LOG=pdo_sandbox::adapters::staging`, `pdo_daemon::sandbox_container` becomes
  `pdo_sandbox::application::container`, `pdo_daemon::sandbox_run` becomes
  `pdo_sandbox::application::prepare` (and `pdo_daemon::run_sandbox` for the Run's side).
- `session` (#920): `crates/pdo-session`, `frontend/src/session/`. tmux sits behind the one
  `Tmux` port (`application/ports.rs`, adapter `adapters/tmux/`): the legacy crate, the PTY
  bridge and the shared terminal reach tmux through it, never through `Command::new("tmux")`.
  What the context reads of the Runs (the declared wait's facts, a pane's iteration, the run
  shell's worktree, the sweep's owners) are ports the legacy crate implements in
  `session_ports/`, wired by the composition root (`crates/pdo/src/contexts.rs`) over the
  `ContextResources` the legacy daemon opens (`contexts.rs`). `RUST_LOG=pdo_daemon::tmux_session_manager`
  becomes `RUST_LOG=pdo_session::adapters::tmux` (and `…::application::launch` for the
  spawns), `pdo_daemon::pty_bridge` becomes `pdo_session::adapters::pty`.

While it still exists,
`scripts/layout-ratchet.sh` keeps guarding its flat list. The ratchet disappears along with the
legacy crate. The `pdo` binary lives in the composition root (`crates/pdo`, package
`pdo-daemon`: dist names the installer after it), which exposes `serve(config, extensions)` and
delegates to the legacy crate. The legacy daemon still opens the database and binds the
listener; it hands the composition root its `ContextResources` (the pool, the per-daemon
knobs, and the ports a context needs from what is still legacy, such as the Runs), and
`crates/pdo/src/contexts.rs` wires each extracted context's adapters onto its ports and
merges its `router()` before `identify` wraps everything. Route coverage reads each
context's `router()` too (`ROUTER_SOURCES`). The daemon's integration tests (stage 4) live there too, in
`crates/pdo/tests/`, and every test daemon goes through the same `identify` head as `pdo daemon`.

Extracted so far: `workspace` (`crates/pdo-workspace`, `frontend/src/workspace/`) and `skill`
(`crates/pdo-skill`, `frontend/src/skill/`). While the legacy crate still assembles the daemon,
it wires an extracted context's adapters onto its ports (`workspace_context`, `skill_context` in
`lib.rs`) and `.merge(…)`s its `router()` into `build_router`; list every
merged router in `ROUTER_SOURCES` of `route_coverage.rs`. The legacy crate may call an extracted
context's public API (its use cases, its adapters' free functions, re-exported at the root of
`lib.rs` for the modules that still read them), never the other way round.

Two patterns the workspace set for the contexts that follow:

- **A port for a whole technology.** Every `git` call of the workspace lives in
  `adapters/git/`, behind the `Git` port for the workspace's own use cases. A context that still
  lives in the legacy crate calls the adapter's functions directly until it migrates onto the
  port.
- **Settings a downstream context owns.** A Projet carries the agentic profile and the skills
  selection, which belong to contexts downstream of the workspace. The workspace stores them as
  opaque JSON (`StoredSetting`) and never reads them; its HTTP adapter is generic over a
  `ProjectSettings` trait that names their real types, so the wire keeps its shape and the
  generated TypeScript type is `Project<A, S>` (the front aliases it in
  `workspace/model/project.ts`).

Every mutation of a context goes through the Autorisation d'opération (`pdo_kernel::
authorization::authorize`) inside its use case, never in the route, and never in a transition
the daemon makes on its own (scheduler, periodic check, archive). A refusal maps to the
kernel's `Denied::into_response()`: `403` and `{"error": "operation_refused", "message",
"reason"}`, which the front's `ApiError` already shows. Do not write a context-specific refusal
body. `crates/pdo/tests/extension_mutation_routes.rs` reads the mutation routes of every router
listed in `ROUTER_SOURCES` and checks that each one answers that `403` under an extension that
refuses everything. When you extract a context, add a request for each of its mutation routes
there (or list a `POST` that mutates nothing, with the reason). The extension it uses lives in
`crates/pdo/tests/test_extension/`; `demo_daemon.rs` next to it serves `pdo daemon` with that
extension for the Feature Paths (see its header).

Two more the skill context set:

- **Consume another context's technology through your own port.** The skill context never runs
  `git`: its delivery port (`SkillDelivery`) and its Source scanner are implemented by adapters
  that call the workspace's git adapter (`index`, `info/exclude`, the provisioning manifest, the
  shallow clone). What the workspace did not offer yet was added on the workspace's side.
- **Reads a downstream context answers.** Who selects a skill (instance, Projets, Triggers,
  pipelines, Runs) lives downstream of the skill context. It asks through a port
  (`SkillReferents`) that the legacy crate implements until those contexts move out. A
  transition of the daemon or of the scheduler (the seed at startup, the delivery into a
  worktree) is a use case too, but it does not ask the Autorisation d'opération (D10).

The HTTP and WebSocket contract stays frozen for the whole migration. A temporary suite,
`crates/pdo/tests/snapshots_temporary/`, plays eight business scenarios against the real
daemon and freezes every response and every `/ws` message as a masked `insta` snapshot. A move
that changes one field turns it red. It is deleted with the legacy crate (#932). Refresh it only
for a deliberate contract change: `INSTA_UPDATE=always cargo nextest run -E
'test(/snapshots_temporary/)'`, then review the diff.
