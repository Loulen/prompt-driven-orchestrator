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
  of `crates/pdo/tests/fixtures/architecture/`). It covers every crate with a `src/domain/`, and
  checks that those are exactly the thirteen contexts.
- **File size.** A production file has **at most 400 lines**, Rust and TypeScript. Tests are not
  counted (`tests/`, a `#[cfg(test)]` module, `*.test.ts(x)`), nor is `shared/generated/`. The
  baseline of the files that were already too big when the rule landed
  (`crates/pdo/tests/file_size_baseline.txt`) is empty: every production file is within the limit,
  and a line may never be added back (`crates/pdo/tests/file_size.rs`). Split a file that grows
  past it by responsibility.
- **Typed contract.** Every DTO sent over HTTP or `/ws` derives `ts_rs::TS` with
  `#[ts(export)]`; `make types` writes `frontend/src/shared/generated/`, and `make check` fails
  when it drifts from the Rust types.
- **Route coverage.** Every route of the assembled router has at least one HTTP-level test,
  method by method (`crates/pdo/tests/route_coverage.rs`). It reads the router's
  `.route(…)` calls and the URL literals of `tests/`. Write the call so the method is readable
  (`client.post(format!("{}/runs", …))`). When the router moves, point the test's
  `ROUTER_SOURCES` at its new home.

## The contexts

What each context holds, and how it reads the others. Every context has the same three modules;
the routes are its `adapters/http/` (`router()`), its tests are stage 2 (use cases against in-memory
ports, `tests/use_cases/`), stage 3 (its real adapters, `tests/adapters/` or `tests/lifecycle/`)
and unit tests beside the code.

- `platform` (`crates/pdo-platform`, `frontend/src/platform/`): the daemon as an installed piece of
  software — page mounts, update, audit journal, service unit. Its CLI (`pdo page`, `pdo service`)
  is `adapters/cli/`.
- `harness` (`crates/pdo-harness`, `frontend/src/harness/`): the harnesses, their registry, probes
  and catalogues. The parsers of each harness's own artefacts (transcripts, turn state, reported
  cost, context peak, steering) live in its `domain/`; cost, stale detection, staging, tmux and
  stats read them from there, never the other way round.
- `sandbox` (`crates/pdo-sandbox`, `frontend/src/sandbox/`): Docker behind the `DockerCli` port
  (`adapters/docker_cli.rs`; its stage-3 tests run against a real daemon when one answers, and say
  they skipped otherwise), the staging of a sandbox's home, the staging profiles. Editing and
  deleting a staging profile ask the Autorisation d'opération; preparing, cleaning up and killing
  inside a Run's sandbox are Run transitions and never do. The Run's side of its sandbox is
  `pdo_run::application::sandbox`.
- `session` (`crates/pdo-session`, `frontend/src/session/`): tmux behind the one `Tmux` port
  (`application/ports.rs`, adapter `adapters/tmux/`) — every tmux call of the daemon goes through
  it, never through `Command::new("tmux")` —, the PTY bridge, the shared terminal, the declared
  wait, the reaper, the library assistant, the Sessions hébergées (ADR-0091: `application/hosted/`,
  their records under `~/.pdo/hosted/` by `adapters/hosted_store.rs`, reached by the extensions'
  Host through `adapters/extension_hosted.rs`; their folder, harness and Cap are ports the
  composition root implements, `crates/pdo/src/ports/session/hosted.rs`). What it reads of the Runs (the declared wait's facts, a
  pane's iteration, the run shell's worktree, the sweep's owners, the status bar's gauges) are
  ports the composition root implements (`crates/pdo/src/ports/session/`).
- `pipeline` (`crates/pdo-pipeline`, `frontend/src/pipeline/`): the pipeline document (model, the
  one `parse_pipeline`, its references, loop regions and cycles), the migrator, the portable
  document, the workflow import, the node library entry, a pipeline's identity (name ⇄ stem) and
  what a Run in flight forbids an edit to touch (`domain/mutation.rs`). The instance registry
  (`adapters/registry/`), the library (`adapters/library_fs.rs`) and the watcher
  (`adapters/watcher/`) are adapters; every daemon write marks its path in `SelfWrites` so the
  watcher does not announce it. The Runs a pipeline has in flight (`ActiveRuns`) are the run
  context's `adapters/pipeline_runs.rs`, the Stats absorption of a rename (`RenameLedger`) the stats
  context's `adapters/rename_ledger.rs`.
  The shared draft of a pipeline (ADR-0088): its pure rules (the delta, the changes, the presence)
  are `domain/draft/`, the open drafts and their participants `application/drafts/`, the
  replicated document over `yrs` `adapters/draft_doc.rs` (behind `DraftDocuments`), the store
  `adapters/sqlite_drafts.rs`, the socket `adapters/http/draft.rs`, and the follower of the
  written files `adapters/draft_follow.rs` (a write the watcher marks `outside` is a version
  « external edit » first). The library assistant's reads and saves into a draft are
  `application/drafts/assistant.rs`; the notice its participants get, `domain/draft/notice.rs`. The frontend's side is `pipeline/store/draftSession.ts`
  over `yjs`.
  The versions a Save freezes (ADR-0088): their rules (numbering, titles, the name of a kept
  version) are `domain/version.rs`, the use cases `application/versions.rs` and
  `application/save_pipeline.rs`, the store `adapters/sqlite_versions.rs`; the frontend's are
  `pipeline/model/versions.ts`, the « Save… » window, the « Versions » tab and the launch window
  (`pipeline/components/SaveVersionModal.tsx`, `VersionsTab.tsx`, `LaunchDraftModal.tsx`).
- `run` (`crates/pdo-run`, `frontend/src/run/`): a Run's events (`domain/event/`), its state
  (`domain/state/`) and the pure projection that folds one into the other (`domain/projection/`),
  with the rules a Run follows — the graph it walks, what it allows, its verdicts — and the
  scheduler's pure evaluators (`domain/scheduler/`). The event log sits behind the `EventStore`
  port (`adapters/sqlite_event_store.rs`); every event enters it through one use case,
  `AppendEvent` (transition guard, store, then publication on the kernel's bus), and `/ws` relays
  the bus and nothing else. The three layers of ADR-0009 are the three hexagonal layers: the
  evaluators in `domain/`, the primitives and the drivers in `application/node/` and
  `application/scheduler/`, the gestures as use cases (`CompleteNode`, `FailNode`, `RunCommand`,
  `create_run`, `forget_run`…) that receive the Acteur and ask the Autorisation d'opération first.
  The scheduler's own transitions never build an Operation (D10). The context reads the others
  through narrow ports (`application/ports.rs`, wired by `adapters/contexts.rs` and the
  composition root). The agent CLI (`pdo complete`, `fail`, `skip`, `wait-user`, `run`, `reap`) is
  `adapters/cli/`.
- `review` (`crates/pdo-review`, `frontend/src/review/`): a Run's diff as data and the parser of the
  unified patch (`domain/diff/`), the rules of the review comments (`domain/comments/`). The
  comments themselves are Run events: their projection is the run context's
  (`pdo_run::domain::review_comment`). It reads the Run through `ReviewRuns` (`adapters/runs.rs`),
  git through `ReviewGit` (`adapters/git.rs`) and its instance setting through `ReviewSettings`. Its
  CLI (`pdo review`) is `adapters/cli.rs`.
- `skill` (`crates/pdo-skill`, `frontend/src/skill/`): the Banque de skills, its folders, its
  Sources, the import and the delivery into a worktree. Who selects a skill is read through
  `SkillReferents`, which the settings context answers (`adapters/contexts/skill_referents.rs`).
- `trigger` (`crates/pdo-trigger`, `frontend/src/trigger/`): a Trigger and its fire record, the cron
  schedule and the one `recompute_next_fire`, the firing decision and the plan of a tick. Every
  gesture asks the Autorisation d'opération, a fire included: "Run now" as the caller `identify`
  set, the heartbeat's tick (`adapters/heartbeat.rs`) as `Actor::Trigger`. A refused cron fire
  starts no Run and records an `error` fire with the refusal. What it reads of the others sits
  behind its ports, implemented in `adapters/contexts/`.
- `stats` (`crates/pdo-stats`, `frontend/src/stats/`): the three sections' folds, the R-7
  distribution, the absorptions and the price sync. The folds read the Runs' events through the
  run context's ports (`EventStore`, `EventWindows`), never the table; the Triggers' fires through
  `TriggerFires`, which the composition root answers over the trigger context's tables
  (`crates/pdo/src/ports/trigger_fires.rs`).
- `workspace` (`crates/pdo-workspace`, `frontend/src/workspace/`): the Projets, the repositories,
  the recent ones, git (`adapters/git/`, behind the `Git` port), the worktrees, the provisioning.
- `telemetry` (`crates/pdo-telemetry`, `frontend/src/settings/` for its page): the instance's
  metrics and traces pushed over OTLP (ADR-0090). The pure rules (the setting, the attributes, a
  repository named by its remote and never by a path, the Run lifecycle read off the events) are
  `domain/`; the hub that rebuilds its providers when the setting changes, the instruments and the
  recorder of the Runs (a subscriber of the kernel's bus) are `application/`; the OTLP exporters,
  the routes and the layer that measures every request are `adapters/`. A Run's facts and cost are
  read through `RunFactsSource` and `RunCosts`, which the composition root answers
  (`crates/pdo/src/ports/telemetry_runs.rs`). The Actor is never an attribute.
- `settings` (`crates/pdo-settings`, `frontend/src/settings/`): the last context of the map, it
  depends on the others and never the other way round. **Each context owns its instance
  defaults** — their env var, their built-in value, their `stored → env → default` resolver and how
  `GET /settings` discloses them, in its own `adapters/instance_defaults.rs`. The settings context
  keeps the stored tier (one singleton row, `adapters/sqlite/`), assembles the page (each context
  is one `SettingsSection`, `adapters/sections/`) and owns the agentic profiles. The ports the
  upstream contexts read their settings through are implemented by `adapters/contexts/`, which
  also reads, for the skill and sandbox contexts, who names a skill or a staging profile, and
  names the real types of what a Projet carries (`ProjectTiers`).

## The composition root

`crates/pdo` (package `pdo-daemon`: dist names the installer after it) is the only crate that
knows every context. It exposes `serve(config, extensions)` (ADR-0081), with
`Config::with_release_channel` for a distribution to declare how it updates (#24), and:

- opens the database and creates every context's tables (`src/daemon/storage.rs`), runs the boot's
  fix-ups, binds the listener and starts the background loops (`src/daemon/`);
- wires each context's adapters onto its ports, one file per context (`src/contexts/`), and
  merges each context's `router()` in front of the embedded SPA (`src/spa.rs`) before `identify`
  wraps everything (`src/identify.rs`);
- implements the ports no single context can answer, because they read several
  (`src/ports/`): the session context's Run-side facts, the page mounts an archive releases, the
  Runs as the platform sees them, the Triggers' fires the Stats count. A context wired after the
  port that reads it is reached through a `Late` handle, bound once everything is wired;
- carries the CLI (`src/cli/`, each subcommand implemented by its context) and the extensions'
  routes and index (`src/extensions.rs`).

The daemon's integration tests (stage 4) live in `crates/pdo/tests/`, and every test daemon goes
through the same `identify` head as `pdo daemon`. Its own unit tests run the contexts wired as the
daemon wires them, over an in-memory database (`contexts::testing`).

The logs of each context and the `RUST_LOG` target of each module are in the
[runbook](../reference/runbook.md).

## Patterns the contexts share

- **A port for a whole technology.** Every `git` call lives in the workspace's `adapters/git/`,
  every `tmux` call behind the session's `Tmux` port, every `docker` call behind the sandbox's
  `DockerCli`. A context that needs one of them consumes it through its own port, implemented by an
  adapter that calls the owning context's adapter (the skill context never runs `git`: its
  delivery port and its Source scanner call the workspace's git adapter). What the owner did not
  offer yet is added on the owner's side.
- **Settings a downstream context owns.** A Projet carries the agentic profile and the skills
  selection, which belong to contexts downstream of the workspace. The workspace stores them as
  opaque JSON (`StoredSetting`) and never reads them; its HTTP adapter is generic over a
  `ProjectSettings` trait that names their real types, so the wire keeps its shape and the
  generated TypeScript type is `Project<A, S>` (the front aliases it in
  `workspace/model/project.ts`).
- **Reads a downstream context answers.** Who selects a skill (instance, Projets, Triggers,
  pipelines, Runs) lives downstream of the skill context. It asks through a port
  (`SkillReferents`) that a downstream context answers. A transition of the daemon or of the
  scheduler (the seed at startup, the delivery into a worktree) is a use case too, but it does not
  ask the Autorisation d'opération (D10).

## Mutations and the extension API

Every mutation of a context goes through the Autorisation d'opération (`pdo_kernel::
authorization::authorize`) inside its use case, never in the route, and never in a transition
the daemon makes on its own (scheduler, periodic check, archive). A refusal maps to the
kernel's `Denied::into_response()`: `403` and `{"error": "operation_refused", "message",
"reason"}`, which the front's `ApiError` already shows. Do not write a context-specific refusal
body. `crates/pdo/tests/extension_mutation_routes.rs` reads the mutation routes of every router
listed in `ROUTER_SOURCES` and checks that each one answers that `403` under an extension that
refuses everything. When you add a mutation route, add a request for it there (or list a `POST`
that mutates nothing, with the reason). The extension it uses lives in
`crates/pdo/tests/test_extension/`; `demo_daemon.rs` next to it serves `pdo daemon` with that
extension for the Feature Paths (see its header).

The other hooks of the extension API (#931) follow the same rule: generic, in the use case,
never context-specific.

- **Entity attributes.** A use case that creates a Run, a Projet, an agentic profile, a
  Trigger or a pipeline of the library calls `pdo_kernel::attributes::stamp` once the entity
  exists: the extensions answer `on_create`, the kernel prefixes each key with the extension's
  id and stores the result through the `AttributeStore` port (`entity_attributes`, SQLite,
  in the composition root). The entity's DTO carries `attributes`, skipped when empty so the
  wire stays byte-identical without extension; the list endpoint reads `?attr.<id>/<key>=
  <value>` through `AttributeFilter` / `AttributedList`. Never put attributes in the event log.
- **`OperationApplied`.** Nothing to write in a use case: `authorize` records every allowed
  Operation in the request's scope (`pdo_kernel::applied`), and the `identify` middleware
  publishes them once the response is a success. A primary adapter outside HTTP (the Triggers'
  cron tick) publishes its own. Deleting an entity drops its attributes there too.
- **Events.** The extension host subscribes to the kernel's bus: an extension's `on_event`
  receives the JSON `/ws` relays, on the extension's own thread, through a bounded queue that
  never makes the publisher wait.
- **Routes and UI.** `crates/pdo/src/extensions.rs` nests each extension's router under
  `/ext/<id>` and serves `GET /ext`, the index the front reads. The front's UI slots live in
  `frontend/src/shared/extensions/` (navigation entry in the top bar, settings panel on the
  Settings rail, list filter under the Runs filters, detail tab in the Run's info panel); each
  bundle is imported at runtime, with React shared through the import map of `index.html` and
  the shims of `frontend/public/shared/`. A bundle declaring another React major is never
  imported, and a banner says so.

## The HTTP and WebSocket contract

The wire is a contract: the generated TypeScript types (`make check`) guard its shape, and the
stage-4 tests of `crates/pdo/tests/` (at least one per route, `route_coverage.rs`) its behaviour.
A new top-level path prefix also needs its entry in the Vite dev proxy (`frontend/vite.config.ts`).
