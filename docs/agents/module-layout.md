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

- `pipeline` (#922): `crates/pdo-pipeline`, `frontend/src/pipeline/`. The pipeline document
  (model, the one `parse_pipeline`, its references, loop regions and cycles), the migrator, the
  portable document, the workflow import, the node library entry, a pipeline's identity (name ⇄
  stem) and what a Run in flight forbids an edit to touch (`domain/mutation.rs`, over the
  `RunProgress` the Run hands over) live in its `domain/`. The instance registry
  (`adapters/registry/`), the library (`adapters/library_fs.rs`) and the watcher
  (`adapters/watcher/`) are adapters; every daemon write marks its path in `SelfWrites` so the
  watcher does not announce it. The Runs a pipeline has in flight (`ActiveRuns`) and the Stats
  absorption of a rename (`RenameLedger`) are ports the legacy crate implements in
  `pipeline_ports.rs`; the composition root wires the context (`crates/pdo/src/contexts.rs`).
  The legacy crate reads the document through `pdo_pipeline::domain::pipeline` (re-exported as
  `crate::pipeline`). `RUST_LOG=pdo_daemon::pipeline_watcher` becomes
  `RUST_LOG=pdo_pipeline::adapters::watcher`, `pdo_daemon::pipeline_migrator` becomes
  `pdo_pipeline::adapters::fs_migration`.
- `run`, first slice (#923): `crates/pdo-run`, `frontend/src/run/`. A Run's events
  (`domain/event/`), its state (`domain/state/`) and the pure projection that folds one into the
  other (`domain/projection/`, one file per family of events) live in its `domain/`, with the
  rules a Run follows: the graph it walks (`graph_resolver`, `loop_region`, `edge_router`,
  `switch_router`, `condition`, `variable_resolver`, `input_resolution`, `blackboard`), what it
  allows (`transition_guard`, `admission`, `completion_refusal`, `retry_verdict`,
  `restart_verdict`, `reap_policy`, `auto_fail`, `child_awaiting`) and the review comments it
  folds. The event log sits behind the `EventStore` port (`adapters/sqlite_event_store.rs`).
  Every event enters it through one use case, `AppendEvent` (transition guard, store, then
  publication on the kernel's bus). It is a transition, so it never asks the Autorisation
  d'opération (D10). The bus carries exactly what `/ws` sends (D20): a Run event as
  `{"type": "event", "event": …}`, and the announcements other contexts still send on the bus's raw
  channel (`AppState::bus_tx`). `/ws` relays the bus and nothing else. The context serves `/ws`,
  `GET /runs/{run_id}` and `GET /runs/{run_id}/events`; the verdicts' HTTP projection
  (`refusal_response`, `retry_response`, `restart_response`) is its `adapters/http/verdicts/`.
  What a Run's detail reads from disk (LOC, cost, the run-scoped pipeline) is the `LiveFacts`
  port the legacy crate implements in `run_ports.rs`. The composition root wires the context;
  the legacy router merges its `router()` so the legacy crate's in-crate tests keep driving it.
  `RUST_LOG=pdo_daemon` (append, `/ws`) becomes `RUST_LOG=pdo_run::application::append_event`
  and `pdo_run::adapters::http::ws`; `pdo_daemon::event_log` becomes `pdo_run::domain::projection`.
- `run`, second slice (#924): the scheduler and the node lifecycle join `crates/pdo-run`. The
  three layers of ADR-0009 are the three hexagonal layers: the scheduler's pure evaluators
  (`domain/scheduler/`, with `dispatcher`, `run_stall`, `recovery`, `merge_action`), the
  primitives and the drivers (`application/node/` — spawn, primitives, prompt, inputs and
  outputs, reap, reattach — and `application/scheduler/`: the one interpreter of the
  evaluators' actions, a Run's advance, a node's completion, the retry of waiting nodes, the
  run-level stall), and the gestures. Every gesture is a use case that receives the Acteur
  `identify` set (`Agent{run, node}` for the agent CLI) and asks the Autorisation d'opération
  first: `CompleteNode`, `ReleaseCompletion`, `FailNode`, `SkipNode`, `StartNode`, `StopNode`,
  `RetryNode` (one file each, an Operation `node` × action on `<run>/<node>`), and `RunCommand`
  (`application/commands/`, one file per family of commands; a node command asks about its
  node, every other one about the Run, after the forgotten-run gate and the parse). The
  scheduler's own transitions (a spawn, a completion's tail, `RunCompleted`, boot recovery, the
  stale sweep, the sandbox preparation) never build an Operation (D10); a stage-2 test
  (`tests/use_cases/scheduler.rs`) and a stage-4 one (`extension_authorization.rs`) prove an
  extension refusing everything blocks no Run. These use cases answer the HTTP response
  themselves (the ADR-0009 addendum: the command surface forwards it verbatim), so they reach
  the verdicts' projection in `adapters/http/verdicts/`. The context reads the other contexts
  through narrow ports (`application/ports.rs`): tmux and the declared wait (session), the
  Projets (workspace), the bank and the delivery (skill), the staging profiles (sandbox),
  wired by `adapters/contexts.rs`; the instance defaults and the agentic profiles through
  `InstanceSettings` (implemented by the settings context since #929); and what is still
  legacy through `RunSurfaces` (archive and `retry_all`, whose answers the commands forward
  verbatim), implemented in `run_ports.rs`. The node routes,
  `/runs/{run_id}/commands` and `/stale/health` are in its `router()`; the agent CLI (`pdo
  complete`, `fail`, `skip`, `wait-user`, `run wait`) is `adapters/cli/`. Tests: the use cases against in-memory ports
  (`tests/use_cases/`, stage 2); the context over its real adapters — SQLite, git in a
  temporary repository, tmux on its own socket — driven through its router (`tests/lifecycle/`,
  the legacy crate's in-crate tests of this code, moved); the unit tests beside their code.
  `RUST_LOG` targets move with the code: `pdo_daemon::scheduler_interpreter` becomes
  `pdo_run::application::scheduler::interpreter`, `pdo_daemon::run_advance`
  `pdo_run::application::scheduler::advance`, `pdo_daemon::run_command`
  `pdo_run::application::commands`, `pdo_daemon::node_spawn` `pdo_run::application::node::spawn`,
  `pdo_daemon::node_primitives` `pdo_run::application::node::primitives`,
  `pdo_daemon::stale_detector` `pdo_run::application::stale`, `pdo_daemon::boot_recovery`
  `pdo_run::application::boot_recovery`, `pdo_daemon::run_sandbox`
  `pdo_run::application::sandbox`, and the node handlers of `pdo_daemon` become
  `pdo_run::application::{complete_node, fail_node, skip_node, start_node, stop_node,
  retry_node, release_completion, merge_resolver}`.
- `run`, third slice (#925): the Run's surfaces join `crates/pdo-run`, and the legacy crate
  holds no Run handler any more (the review of a Run, its diffs and its comments, moved to
  its own context with #926). Every mutation is a use case on `Runs` that receives the Acteur and asks the
  Autorisation d'opération before anything is written: `create_run` (`run` × `create`, in
  `application/create_run/`: the repositories, the pipeline, the provisioning and the
  isolation frozen, the `RunStarted` payload, the Run on disk, its start — one file per step),
  `forget_run` (`run` × `delete`), `edit_repos` (`run` × `edit_repos`), `save_run_pipeline`
  (`run` × `edit_pipeline`, `application/run_pipeline/`), `start_manager` / `stop_manager`
  (`run` × `start_manager` / `stop_manager`, `application/manager/`). `start_run` is the
  creation without the gate, for a caller that has passed its own: the `retry_all` command, a
  Trigger fire (the trigger context gates it, #927). The archive (`application/archive/`) and
  `retry_all` are what an authorized command triggers, so the `RunSurfaces` port is gone. The
  reads are use cases too: the list, the reapable Runs and the children (`application/list/`),
  an artifact, the refs, a file at a ref and the source drift (`application/refs/`). A Run's
  cost moved with them: the contributions and a transcript's usage in `domain/run_cost/`, the
  price table's tiers and their parsing in `domain/prices/`, the harnesses' files in
  `adapters/run_cost/`, the price files in `adapters/price_files.rs`, and what a Run's detail
  reads from disk (LOC, cost, the run-scoped graph) in `adapters/disk_live_facts.rs`, which now
  implements `LiveFacts`. The price sync (the egress to models.dev) moved to the stats
  context with #928. The git reads a Run's refs and the review share (`rev_parse`, the
  snapshot of a worktree, a file at a ref, the ref/path guards) live in the workspace's
  `adapters/git/revisions.rs`. New narrow ports: `RunPages` (the platform's page mounts an
  archive releases, implemented in `run_ports.rs` while the state owns the platform),
  `RunDirWatch` (the pipeline watcher, `adapters/run_dir_watch.rs`), the pipeline context's
  `SkillBank` (the skills sidecar of a Run's pipeline) and `SelfWrites`; `EventStore::forget`,
  `NodeSkills::withdraw`, `Projects::provisioning_rules` and the instance defaults a create
  reads (attachment budget, default sandbox, auto-naming, manager) extend the existing ones.
  The routes are in the context's `router()`; `pdo run create` and `pdo reap` join the CLI in
  `adapters/cli/` (with `RunAction`). What the downstream contexts consume is `pub` and stays
  put: `Runs::start_run` and `CreateRunRequest` (trigger), `adapters::run_cost` and
  `domain::prices` (stats), `application::refs`, `application::manager::ensure_run_manager`
  and `adapters::disk_live_facts::{compute_run_loc, run_diff_base}` (review). Tests: the
  surfaces' use cases against in-memory ports (`tests/use_cases/surfaces.rs`, stage 2), the
  legacy crate's in-crate tests of this code moved to `tests/lifecycle/surfaces/` (stage 3,
  with `Build { on_disk: true }` so a Run's detail reads the disk as the daemon does), the
  unit tests beside their code. `RUST_LOG=pdo_daemon` (create, archive, forget, repos, the
  run-scoped pipeline, the manager) becomes `RUST_LOG=pdo_run::application::{create_run,
  archive, forget_run, edit_repos, run_pipeline, manager, retry_all}`, `pdo_daemon::price_table`
  becomes `pdo_run::domain::prices`. Front: the Runs' API (`run/api/{runs, createRun,
  artifacts, runPipeline, manager}`), the launch modal (`run/components/NewRunModal.tsx` and
  `newRun/`), the run list (`run/components/runsList/`, rendered by the left panel's Runs tab),
  a node's detail (`run/components/NodeDetailPanel.tsx` and `nodeDetail/`), the artifacts
  (`run/components/MarkdownArtifactModal.tsx` and `artifacts/`), the children
  (`OrchestrationTab`), a Run's repositories, and their models and stores.
- `review` (#926): `crates/pdo-review`, `frontend/src/review/`. A Run's diff as data (files →
  hunks → lines) and the parser of the unified patch (`domain/diff/`), and the rules of the
  review comments (`domain/comments/`: the `rc-NNN` ids, the excerpt, the author of a reply, the
  list's filter, the re-map of an anchor onto another pair, what a reply or a decision changes,
  the one message a batch hands the Pipeline Manager) live in its `domain/`. The comments
  themselves are Run events: their projection stays the run context's
  (`pdo_run::domain::review_comment`). The use cases (`application/`: `run_diff`,
  `structured_diff`, `list_comments`, `send_comments/`, `reply`, `decide`) read the Run through
  the `ReviewRuns` port (`adapters/runs.rs`, over `pdo_run::Runs`: the log, the projection, the
  Run's repository and worktree, the manager on demand, the paste into its session), git through
  `ReviewGit` (`adapters/git.rs`, over the workspace's `adapters/git/revisions.rs`) and the
  `review_agent_can_resolve` instance setting through `ReviewSettings` (implemented by the
  settings context since #929). Every gesture — send, reply,
  resolve, reopen — receives the Acteur and asks the Autorisation d'opération before it reads
  the Run (`review_comment` × `create`, or × `reply` / `resolve` / `reopen` on
  `<run>/<comment>`); the author of a reply is the Acteur's node. The use cases answer a
  `Failure` (status + the body the route always sent) that the HTTP adapter turns into the
  response. The routes are its `router()`, merged by the composition root
  (`crates/pdo/src/contexts.rs`); `pdo review list` / `reply` are `adapters/cli.rs`. Tests: the
  use cases against in-memory ports (`tests/use_cases/`, stage 2), the context over the run
  context's real log and git in temporary repositories (`tests/lifecycle/`, stage 3, the legacy
  crate's in-crate tests of this code, moved; its harness reuses the run context's), the unit
  tests beside their code. `RUST_LOG=pdo_daemon` (the review handlers) becomes
  `RUST_LOG=pdo_review::application::{run_diff, structured_diff, list_comments, send_comments,
  reply, decide, comment_events}`, and `pdo_review::adapters::runs` for the paste into the
  manager. Front: the API (`review/api/{diff, comments}`), the drafts, the merged list, a
  comment's status and the unread replies (`review/model/{drafts, entries, commentStatus,
  seen}`), the Review page's stores (`review/store/useReview{Run, Diff, Live, Layout,
  CommentState, Entries, Gestures}`), the page and its parts, the file cards, the comment cards
  and the Diff tab (`review/components/`).
- `trigger` (#927): `crates/pdo-trigger`, `frontend/src/trigger/`. A Trigger and its fire
  record, the cron schedule, when it fires next (`domain/schedule.rs`, the one
  `recompute_next_fire`), the firing decision and the plan of a tick (`fire_decision`,
  `tick_plan`), the guard's bounds and what a configuration must satisfy (`rules`) are its
  `domain/`. Every gesture is a use case on `Triggers` that asks the Autorisation d'opération
  first: `create`, `update`, `delete` (`trigger` × `create` / `update` / `delete`), `pause`
  (`trigger` × `pause`), `test_guard` (`trigger` × `test_guard`: the dry-run runs an arbitrary
  command), and a fire (`trigger` × `fire` on the Trigger): "Run now" (`application/fire/run_now.rs`)
  asks as the caller `identify` set, the heartbeat's tick (`application/fire/tick.rs`) asks as
  `Actor::Trigger` for each due Trigger. Deciding that a Trigger is due asks no one; a refused
  cron fire starts no Run, spends the slot and records an `error` fire with the refusal. The
  heartbeat is a primary adapter (`adapters/heartbeat.rs`, the clock); the Triggers are over
  SQLite (`adapters/sqlite/`), the guard is run by `sh` (`adapters/guard_shell.rs`). What the
  context reads of the others sits behind its ports (`application/ports.rs`), implemented in
  `adapters/contexts/`: the Runs a fire starts (`Runs::start_run`, the run context's
  creation without its gate), the pipelines of the library, the repositories (the write-boundary
  rule and the pre-cut fetch through the workspace's git), the staging profiles, the platform's
  audit journal. The pause and the guard timeout are the instance settings', implemented by
  the settings context since #929. The legacy crate reads the Triggers
  through `pdo_trigger::adapters::sqlite::list` (the referents of a skill, a staging profile, an
  agentic profile) and creates their tables through its `init`. `GET /stale/health` is the Run's
  stale sweep and stays in `pdo-run`. Tests: the use cases against in-memory ports
  (`tests/use_cases/`, stage 2, replacing the legacy router `oneshot` tests), the SQLite store
  and the pre-cut fetch against the real resource (`tests/adapters/`, stage 3), the pure rules
  and `sh` beside their code; a stage-4 test (`extension_trigger_fire.rs`) proves a cron fire
  asks as the Trigger. `RUST_LOG=pdo_daemon` (the Trigger handlers, the scheduler's tick and
  fire) becomes `RUST_LOG=pdo_trigger::application` (`::fire`, `::fire::tick`, `::create_trigger`,
  `::update_trigger`…), `pdo_daemon::guard_runner` becomes `pdo_trigger::adapters::guard_shell`.
  Front: the API (`trigger/api/{triggers, guard}`), the Triggers tab (`TriggersListPanel` and
  `triggersList/`), the detail panel (`TriggerDetailPanel` and `triggerDetail/`), the guard's
  output and verdict, the cron presets and the canvas reconciliation (`trigger/model/`), and
  the stores of the list, the pause, the fire history and the guard's dry-run. The New Run
  modal's Trigger mode stays in `run/components/newRun/` and uses them.
- `stats` (#928): `crates/pdo-stats`, `frontend/src/stats/`. The three sections' folds
  (`domain/overview/`, `domain/cost/`, `domain/performance/`), the R-7 distribution, the
  absorptions (`domain/absorption/`: keys, the resolver and its order of resolution, the Run
  identity, the tallies, the frozen scope and the cascade of a removal) and the price sync's
  normalisation and diff live in its `domain/`. The folds read the Runs' events through the run
  context's ports, never the table: `EventStore` for one Run, and `EventWindows` for the reads
  across Runs (counts per period, the cohort of a window, its Node starts, a Pipeline's
  history), which `SqliteEventStore` implements (`pdo-run/src/adapters/sqlite_event_windows.rs`);
  the context's own `RunEvents` port wraps both (`adapters/run_events.rs`). The absorptions'
  store (`adapters/sqlite_absorptions/`) never reads `events`: a write reads the stored
  absorptions and the Pipelines' history first, then writes in one transaction, under the one
  lock of the absorption writes. The Performance fold reads the transcripts through
  `SessionTranscripts` and the Runs' files through `RunFiles`; its memo belongs to the context
  instance. Combine, Uncombine and the price sync are use cases that ask the Autorisation
  d'opération (`stats_absorption` × `create` / `delete`, `price_table` × `sync`); the boot
  refresh of the prices and the absorption a Pipeline rename leaves behind (the pipeline
  context's `RenameLedger`, implemented by `adapters/rename_ledger.rs`) are transitions and never
  ask. What the context reads of the contexts still legacy is a port the legacy crate
  implements in `stats_ports.rs` (the Triggers' fires); the composition root wires the context.
  Tests: the use cases against in-memory ports (`tests/use_cases/`, stage 2), the context over
  its real adapters driven through its router (`tests/adapters/`, stage 3, the legacy crate's
  in-crate tests of this code, moved), the unit tests beside their code.
  `RUST_LOG=pdo_daemon` (the price sync and refresh) becomes
  `RUST_LOG=pdo_stats::application::prices`. Front: the Stats modal and its tabs
  (`stats/components/`, `StatsCharts` split by tab under `components/charts/`), the absorptions,
  their hooks (`stats/store/`), models and API (`stats/api/`).
- `settings` (#929): `crates/pdo-settings`, `frontend/src/settings/`. The last context of the
  map: it depends on the others, never the other way round. The `instance_config` hub is
  dissolved: **each context owns its instance defaults** — their env var, their built-in
  value, their `stored → env → default` resolver, and how `GET /settings` discloses them, in
  its own `adapters/instance_defaults.rs` (`pdo_run`: the cap, the auto-completion, the
  auto-naming, `auto_fail`, the manager's auto-start and pin, the attachment budget;
  `pdo_session`: the reaper's TTLs, the default model and harness; `pdo_trigger`: the guard
  timeout; `pdo_review`: `review_agent_can_resolve`; `pdo_platform`: `update_check`;
  `pdo_sandbox`: the default sandbox, the staging profiles' names, Docker), with the shape
  `pdo_kernel::instance_default::Disclosed`. The settings context keeps the stored tier (one
  singleton row, `adapters/sqlite/`) and assembles the page: each context is one
  `SettingsSection` (`adapters/sections/`), the page is their union plus the instance tiers it
  owns (the agentic profile and the skills selection). Its `domain/` holds the stored row and
  the edit a `PUT` may make of it (`edit.rs`: the retired fields, the bounds), an agentic
  profile (the draft, the name clash), who names one. Every mutation is a use case that asks
  the Autorisation d'opération first: the edit of the settings (`instance_settings` ×
  `update`), the create, edit and delete of an agentic profile (`agent_profile` × `create` /
  `update` / `delete` on the profile). The boot's warning about the retired sandbox settings
  and the catalogues' warm-up are transitions and never ask. The ports the upstream contexts
  read their settings through (`InstanceSettings`, `ReviewSettings`, `TriggerSettings`,
  `UpdateCheckSetting`) are implemented by `adapters/contexts/`, and the composition root wires
  them over the context's stores before the contexts that read them. The legacy crate still
  reads the stored row through `pdo_settings::adapters::sqlite::instance_config` (the sweep's
  TTLs, the referents of a skill and of a staging profile) and creates the tables through its
  `init`. Tests: the use cases against in-memory ports (`tests/use_cases/`, stage 2, replacing
  the legacy router `oneshot` tests of `/settings`), the stored tier, the profiles, the page
  through the context's router and the upstream ports over the real SQLite (`tests/adapters/`,
  stage 3, the legacy crate's in-crate tests of this code, moved), the unit tests beside their
  code. `RUST_LOG=pdo_daemon` (the settings handlers, the boot warning) becomes
  `RUST_LOG=pdo_settings::application` (`::boot`, `::agent_profiles::read`…). Front: the API
  (`settings/api/{settings, agentProfiles}`), the draft, the source notes, the categories and
  the agentic profiles' helpers (`settings/model/`), the stores of the page, of the form and
  of the profiles (`settings/store/`), the surface (`SettingsSurface`, split by section under
  `components/settingsSurface/`), `AgentProfilesPanel` and `AgentProfileModal`.
  The legacy crate's router declares no route any more: route coverage reads the context
  routers only.

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
