# Extension API reference

What an extension gets from `pdo-extension-api` besides its interface (ADR-0081 and its
amendments; the interface is [extension-ui.md](extension-ui.md)). The crate's rustdoc is the
source of truth for signatures; this page says what each part is for and what goes on the wire.

Version: **0.12.0**. In 0.x a minor may break; `OLDEST_COMPATIBLE_API_VERSION` names the oldest
version the daemon still runs (`0.1.0`: no minor has broken the wire so far).

## The hooks

An extension implements `Extension`. Only `manifest` is required; every other hook has a neutral
default, and with no extension the daemon behaves as if none existed.

| Hook | Called | Default |
| --- | --- | --- |
| `manifest` | at registration | required |
| `start(context)` | once, before serving; `context.data_dir`, `context.host` | nothing |
| `identify(request)` | on every request; the first `Some` wins | `None`: the core decides |
| `admit(request)` (0.8) | on a request no extension identified, before any route; the first non-`Admit` wins, a panic refuses | `Admission::Admit` |
| `display_name(actor)` (0.8) | wherever an Actor is shown; the first `Some` wins, a panic is skipped | `None`: shown as without extension |
| `authorize(actor, operation)` | before every mutation of an actor, and every guarded read (0.8, below); the first refusal wins, a panic refuses | allow |
| `on_create(actor, entity)` | after a creation; attributes prefixed `<id>/` | none |
| `validate_create(actor, kind, value)` (0.9) | before a creation carrying a value for this extension, after the authorization; `Err(reason)` refuses with `400`, a panic refuses | accept |
| `on_create_with_data(actor, entity, data)` (0.9) | in place of `on_create`, with the value the creation carried (`None` without one); a Run's before its first Node | `on_create(actor, entity)` |
| `on_event(event)` | off the request path, on the extension's own thread | ignore |
| `routes()` | once; nested under `/ext/<id>/` | none |
| `ui()` | on every `GET /ext` | none |
| `cli()` / `run_cli(invocation)` | `pdo ext <id> …` | no command line |
| `skills()` | once, before `start` | none |
| `node_env(launch)` (0.8) | at every launch of a Node, agent or script, and at an agent's resume; the first registered wins a name | none |
| `node_preamble(launch)` (0.8) | at every launch of an agent; texts follow in registration order | none |
| `trigger_suspension(trigger)` (0.8) | at every read of a Trigger and every due tick; the first `Some` wins, a panic suspends nothing | not suspended |

## The Manifest

```rust
use pdo_extension_api::{Manifest, ResourceDeclaration};

fn manifest(&self) -> Manifest {
    Manifest::new("test", "Test", env!("CARGO_PKG_VERSION"))
        .with_resource(
            ResourceDeclaration::new("note", "Notes", ["create", "update", "delete", "archive"])
                .described("The notes of a folder"),
        )
}
```

- `id`: lowercase ASCII letters, digits and `-`, unique among the loaded extensions. It prefixes
  the extension's routes (`/ext/<id>/`), its attributes (`<id>/<key>`), its Ressources
  (`<id>/<kind>`) and its data folder.
- `api_version`: the version of the API it was compiled against (`requiring_api` to override).
- `resources` (0.8): the Ressources the extension owns, its `ResourceKind::Extension { extension,
  kind }`. Each has a `kind`, a `label` (plural, as the interface shows it), a one-line
  `description` and the names of its `verbs`. The core adds them to the catalogue.

## The Operation

Every mutation is described by an `Operation`: a resource, a verb, a target, and the Projets it is
in. So is every read the core guards (0.8, CORE-5, below).

```json
{ "resource": "run", "verb": { "action": "rename_run" }, "target": "20261005-181858-e816357",
  "projects": ["prj_8f2c"] }
```

- `resource`: the wire name of a `ResourceKind`: one of the core's (`run`, `node`, `project`…, the
  list is `ResourceKind::CORE`) or `<extension>/<kind>` (`test/note`).
  `ResourceKind::from_wire_name` reads one back.
- `verb`: `"read"` (0.8), `"create"`, `"update"`, `"delete"`, or `{"action": "<name>"}`.
  `Verb::name` gives the bare name the catalogue lists (`"fire"`).
- `target`: the id of the entity acted on, `null` for a creation. A node is `<run>/<node>`, a
  review comment `<run>/<comment>`.
- `projects` (0.8): the ids of the Projets the Operation is in, without duplicates; empty: **out
  of any Projet**.

The core fills `projects` for its own Operations:

| Resource | Projets |
| --- | --- |
| `run`, `node` | the Projet owning the Run's primary repository; a creation: the repository it names (`target_repo`, else `target_repos[0]`), or the parent Run's for a Run created from a node session without a repository |
| `session` | the Projets of the session's Run (node, manager, run shell); the library assistant is in none |
| `review_comment` | the Projets of the Run in its target |
| `project` (targeted) | that Projet |
| `repository` (`fetch`, `fast_forward`) | the Projet owning the repository |
| `trigger` (0.11) | the Projet owning the Trigger's target repository, matched as a Run's; a creation: the repository it names; a Trigger without a target repository is in none. An edit moving it to another Projet's repository is asked, and announced applied, in each |
| everything else | none |

A repository belongs to one Projet at most, so the core's lists hold one id at most. A Run's
repository is matched to a Projet's member verbatim, then without trailing `/` or `.`, then
resolved on disk: `/repo/` or a link to `/repo` stays in the Projet that owns `/repo`. An extension
fills `projects` for its own Ressources (`Operation::in_projects`), with as many Projets as its
entity belongs to. **An extension never works out a Projet from a repository**: it reads
`operation.projects`. When the Projets cannot be read, the core logs it and the Operation is in
none.

An `Operation` also reads back from this wire form (`Deserialize`; `target` and `projects` may be
omitted): the form the question of authorization receives.

## The reads

The core asks `authorize` before it serves these reads (0.8, CORE-5), with the verb `read`:

| Read | Operation | Refused |
| --- | --- | --- |
| `GET /runs`, `/runs/reapable`, `/runs/{id}/children` | `run` × `read`, targeted, per Run listed | the Run is absent |
| any `GET /runs/{id}…` (detail, events, nodes, artifacts, diff, review comments, a node's pane…) | `run` × `read` on `{id}` | `403` `operation_refused` |
| `/ws` | `run` × `read`, per Run a message is about (its events, `run_forgotten`, `operation_applied` on it or its nodes, its creation included), per connection (answer kept 5 s) | the message is not sent to that connection; a `trigger_fired` is sent without its `run_id` |
| `GET /projects` | `project` × `read`, per Projet | the Projet is absent |
| `GET /projects/{id}` | `project` × `read` | `403` |
| `GET /fs/browse` (the loupe, the explorers) | `repository` × `read`, no target, in no Projet | `403` |
| `GET /runs/presences` (0.12) | `run` × `read`, targeted, per Run listed | the Run is absent |
| `/ws`, a Presence (0.12) | `run` × `read` on the Run a `presence_arrived` or `presence_left` is about, per connection (answer kept 5 s) | the message is not sent to that connection |
| `GET /repos/recent` (the New Run prefill) | `run` × `read` on the last Run of each repository, in the repository's Projet | the repository is absent |
| `GET /triggers` (0.11) | `trigger` × `read`, targeted, per Trigger listed, in the Projet of its target repository | the Trigger is absent |
| `GET /triggers/{id}`, `/triggers/{id}/fires` (0.11) | `trigger` × `read` on `{id}` | `403` `operation_refused` |
| `/ws`, about a Trigger (0.11) | `trigger` × `read` on the Trigger a message is about (`trigger_created`, `trigger_updated`, `trigger_deleted`, `trigger_fired`, `operation_applied` on it), in its Projet, per connection (answer kept 5 s) | the message is not sent to that connection |
| `/ws`, an extension's event guarded by a read (0.11) | the read the event carries (`ExtensionEvent::guarded_by`), per connection (answer kept 5 s) | the event is not sent to that connection |
| `/ws`, an `operation_applied` on an extension's Ressource (0.11) | `<extension>/<kind>` × `read` on the entity it is about (its target, or the entity a creation made), in the Projets of its Operation, per connection (answer kept 5 s) | the message is not sent to that connection |

A read is in the same Projets as the gestures on its entity: a Run's in the Projet of its primary
repository, a Trigger's in the Projet of its target repository, a Projet's in itself. The actor is the one `identify` found: on `/ws`, the one of the
upgrade request. A read is never announced as `operation_applied`, and a refusal is logged at
`debug` only. Without extension nothing is asked: the lists are served as before.

**Refuse a read on its resource and verb.** An extension that refused every Operation whatever its
verb now hides the Runs and the Projets too.

### The question of authorization

```http
POST /authorize
{ "operations": [
    { "resource": "repository", "verb": "read" },
    { "resource": "run", "verb": "create", "projects": ["prj_8f2c"] },
    { "resource": "run", "verb": "read", "target": "20261005-181858-e816357" } ] }
```

```json
{ "decisions": [
    { "decision": "deny", "reason": "only an administrator browses the disk" },
    { "decision": "allow" },
    { "decision": "allow" } ] }
```

Whether the caller may run each Operation, in order, **without running any**: nothing changes,
nothing is announced. An Operation that names no Projet is placed where the core places its own: a
targeted `run` or `node` in the Projet of its Run, a targeted `project` in itself. At most 500
Operations per question; an unreadable one answers `400` `invalid_question`. Without extension,
every decision is `allow`. The core's UI asks it to hide the loupe and the Projets a Run may not be
created in (`extension-ui.md`, `run/new`); an extension's UI may ask it too.

## The Host

`context.host` (an `Arc<dyn Host>`) is how an extension runs its **own** mutations through the
core's mechanisms (amendment #34):

- `authorize(actor, operation)`: every extension's `authorize`, the caller's included; answer a
  refusal with `Authorization::into_result()?`, a `403` `operation_refused`.
- `on_create(actor, entity)`: the attributes the other extensions stamp; the extension stores them.
- `publish(ExtensionEvent)`: broadcast on `/ws` and to the other extensions. An event
  `guarded_by(read)` (0.11) reaches only the `/ws` connections whose Actor may run that read
  (below); the other extensions receive it whatever the guard.
- `applied(OperationApplied)`: tell the core that an authorized mutation succeeded.
- `catalogue()` (0.8): the catalogue of the Ressources, below.
- `display_name(actor)` (0.8): how an Actor shows, the first extension that names it (the
  caller's included); `None` when none does.
- `create_run(actor, NewRun)` (0.9): create a Run on behalf of `actor`, as `POST /runs` does for
  that caller (below); answers its id or the refusal (`RunNotCreated`: status, message, body).
- `project_name(project_id)` (0.11): the name of a Projet (`"P1"`), or `None` for an id the core
  does not know. Synchronous and quick: call it from your own `authorize` to name the Projet in a
  refusal's reason. The core reads the Projets once before it serves and again at each change, so
  the name follows a renaming before the change is announced; a deleted Projet has none.
- `presences(run_id)` (0.12): the Actors who have the Run open in a browser right now, by order
  of arrival ([Presences](#presences-012)); empty for a Run nobody has open or the core does not
  know. Synchronous and quick.
- `run_cost(run_id)` (0.12): the cost of the Run, the one its Info tab shows, read on demand from
  its sessions: `RunCost { usd, partial, unpriced_models, uncosted_harnesses }`, `usd` `None`
  when unknown (show « — », never `0`), `partial` when a model without a price was left out (a
  lower bound), `uncosted_harnesses` the harnesses without a cost source that make it unknown.
  `None` for a Run the core does not know; the breakdown by harness the Info tab may add is not
  part of it. Await it from a route or a task. An extension that
  aggregates costs reads them so, without reading the transcripts.
- `triggers_changed()` (0.11): the suspensions of the Triggers changed for a cause outside a
  Trigger's mutation, one the extension holds itself. The core announces
  `{"type": "trigger_updated"}`, without `trigger_id`, and the list of the Triggers reads them all
  again (below). The interface also asks again which gestures on the Runs it may show: call it
  when an Actor's rights change, so that an open view follows without reloading.

`NeutralHost` allows everything, publishes nothing, answers the core's catalogue alone, creates no
Run (`503`), names no Projet, signals nothing, sees nobody on a Run and knows no cost: the host of `ExtensionContext::new`, and a stand-in in an extension's
tests.

## Admission of a request

`identify` names the caller; when **no** extension does, the core asks every extension's
`admit(request)` (method, URI, headers) before any route runs:

| `Admission` | Answer |
| --- | --- |
| `Admit` | served, as without extension |
| `Refuse { reason }` | `401` `{"error": "unauthenticated", "message": "authentication required: <reason>", "reason"}` |
| `Redirect { to }` | the same `401`, plus `"redirect": "<to>"`: the page to sign in on |

Every `401` carries `WWW-Authenticate: Bearer`. The admission guards every route: the API, `/ws`,
the terminals' PTY (`/sessions/<id>/pty`), `/ext-cli/<id>`, `/actors/*` and every extension's
routes under `/ext/<id>/`, fallbacks included. It never guards the SPA's own files: the
interface always loads, asks `GET /actors/me`, and follows a `redirect` with `next=<path, query
and hash>` appended ([extension-ui.md](extension-ui.md#signing-in)). So an extension that refuses
anonymous callers **admits its own sign-in page by its path** (and whatever that page needs).

The CLI sends `PDO_TOKEN`, when set, as `Authorization: Bearer <token>` on every call to the
daemon (`pdo ext …`, `pdo complete`, `pdo run …`, the node sessions' hooks): `identify` reads it.

## Display names

```rust
use pdo_extension_api::{Actor, ActorDisplay};

fn display_name(&self, actor: &Actor) -> Option<ActorDisplay> {
    match actor {
        Actor::Identified { namespace, id } if namespace == "test" => {
            self.users.name_of(id).map(|name| ActorDisplay::new(name).with_avatar(format!("/ext/test/avatars/{id}")))
        }
        _ => None,
    }
}
```

`ActorDisplay` is `{ "name": "Alice", "avatar_url": "/ext/test/avatars/alice" | null }`. Where it
shows:

- `GET /actors/me`: `{ "actor": <Actor>, "display": <ActorDisplay> | null }`, the caller.
- `POST /actors/display` `{ "actors": [<Actor>…] }`: `{ "displays": [<ActorDisplay> | null…] }`,
  in order.
- `GET /audit`: each row's `actor` (the Actor of the request that made the mutation, absent on
  older rows and outside a request) and `actor_display`.
- The interface: `useCurrentActor()` and `useActorDisplay(actor)` of `@pdo/extension-ui` for the
  extensions' bundles. The core draws no person on a Run (0.12): neither on its row nor in its
  Info tab; an extension does (`run/item/people`, `run/info`).

An `Actor` on the wire: `{"kind": "local_operator"}`, `{"kind": "agent", "run_id", "node_id"}`,
`{"kind": "trigger", "trigger_id"}`, `{"kind": "identified", "namespace", "id"}`.

## The catalogue of the Ressources

The core's 19 Ressources, then each extension's declarations in registration order:

```http
GET /resources
```

```json
[
  { "resource": "run", "label": "Runs", "description": "See, create, steer, archive and delete Runs",
    "verbs": ["read", "create", "delete", "edit_repos", "…"], "extension": null },
  { "resource": "test/note", "label": "Notes", "description": "The notes of a folder",
    "verbs": ["create", "update", "delete", "archive"], "extension": "test" }
]
```

Since 0.11, `node` lists `import_files` (the files a user imports into a node with a live session)
and `send_text` (a text written into its terminal input, without Enter): an extension of rights
that maps the node verbs maps these two as well, or they follow its default.

The same list comes from `Host::catalogue()` inside the daemon, and `core_resources()` /
`catalogue(manifests)` build it without a daemon. An extension of rights reads it to list what
rights can be set on, without knowing the other extensions.

`pipeline` × `validate` (0.11) sets the Projets a Pipeline is offered to by the launch forms
(`PUT /pipelines/{id}/validation`, CONTEXT.md « Pipeline validé »). Targeted on the Pipeline's id,
in no Projet: a Pipeline belongs to the instance. An extension of rights that maps the Pipeline
verbs maps this one too, or the Validate button is refused.

## The launch of a Node (0.8)

`node_env` and `node_preamble` receive a `NodeLaunch`: `run_id`, `node_id`, `iter`, `kind`
(`agent` or `script`), `created_by` (the Actor of `run` × `create`: the caller of `POST /runs`,
or `{"kind": "trigger", …}` for a fired Run), `trigger` (`{id, editor}`: the Trigger that fired
the Run and its last editor then), `repository` (the Run's primary repository) and `project`
(the id of the Projet owning it).

- **Variables** (`node_env`): exported before the Node's command on the host, passed as
  `-e NAME=value` on the `docker exec` in a sandbox. A name the core sets is **never
  overwritten**: dropped with a warning (`PATH`, `HOME`, `PDO_RUN_ID`, `PDO_NODE_ID`,
  `PDO_NODE_ITER`, `PDO_DAEMON_URL`, `PDO_SBX_SESSION`, the harness's variables, a script's
  `PDO_INPUT_*`/`PDO_OUTPUT_*`/…, the sandbox profile's env). A name that is no shell identifier
  (`[A-Za-z_][A-Za-z0-9_]*`) is refused. Other `PDO_*` names are allowed (`PDO_TOKEN`).
- **`CLAUDE_CONFIG_DIR`**: when a Node gets one, it is frozen on its `NodeStarted`
  (`claude_config_dir`), and the core reads that Node's Claude Code transcripts, for its cost
  and its liveness, under `$CLAUDE_CONFIG_DIR/projects/`.
- **Git identity** (`GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME`,
  `GIT_COMMITTER_EMAIL`): the ones a Node gets are frozen on its `NodeStarted` (`git_identity`,
  these four only: the rest of the environment may hold secrets and is never written to the
  event log). They sign every commit of the Node's delivery: `<node> iter-<N>: completed`, the
  commit in an isolated Node's sub-worktree, its merge into the Run's worktree and the
  resolution commit in the Node's favour. A daemon with no git identity of its own then
  delivers, provided all four are given (a missing or empty one falls back to the daemon's).
  Without them, the delivery keeps the daemon's identity.
- **Preamble** (`node_preamble`): placed at the very top of an agent's prompt, before
  `# PDO Runtime Preamble`, followed by a blank line. Never asked for a script.

The Run carries who created it on the API: `created_by` on `GET /runs/<id>` and `GET /runs`,
`trigger_editor` on a fired Run (`GET /runs/<id>`); a Trigger carries `last_editor`, set at its
creation and at each `PATCH`. All three are the Actor's JSON, absent when unknown.

## The creation of a Run (0.9)

A request that creates a Run may carry a value per extension: `POST /runs` (JSON, or a multipart
`extensions` field holding the JSON) takes `"extensions": {"<id>": <any JSON>}`. The core never
interprets a value; in order, before any effect:

1. an `<id>` no installed extension has is refused like an unknown field: `400`
   `{"error": "unknown extension `<id>` in `extensions`; installed extensions: …"}`;
2. the Autorisation d'opération (`run` × `create`) is asked;
3. each extension whose id is present checks its value: `validate_create(actor, ResourceKind::Run,
   value)`. A refusal answers `400` `{"error": "extension `<id>` refused the creation: <reason>",
   "extension", "reason"}`; no Run, no event.

Then the Run is created: the data is frozen in `RunStarted` (`"extensions"`, only when
non-empty), and each extension receives its value in `on_create_with_data(actor, entity, data)`
**before the Run's first Node is launched** (its `node_env` comes after), whatever the path: a
gesture, a Trigger's fire (no data), a `retry_all` (the twin carries the original's data, not
checked again). An extension that only overrides `on_create` keeps working: the default of
`on_create_with_data` calls it.

The surfaces that fill `extensions`:

- `pdo run create … --ext <id>.<key>=<value>` (repeatable): `extensions.<id>.<key>` = the string
  `<value>` (split at the first `=`; the same key twice is refused);
- the New Run form: a `run/new` section that `provide`s a value
  ([extension-ui.md](extension-ui.md#sections));
- `Host::create_run(actor, run)`: `NewRun::new(pipeline, input).in_repo(RunRepo::new(path))
  .named(name).with_extension(id, value)`; the first repository is the primary (its
  `from_branch` is the source branch), the others are secondaries. The Run is created as `POST
  /runs` creates it for `actor`: the same authorization (a refused Actor gets `403`
  `operation_refused`), the same checks, the same stamps, `created_by` = `actor`, a root Run. Await
  it from a route or a task; never from a hook of the creation it starts.

## Presences (0.12)

The core knows which Actors have a Run open in a browser. Each tab of the interface says on its
event stream which Run it shows (`{"type": "show_run", "run_id": "<id>" | null}` on `/ws`,
again at each reopening); a Run its Actor may not read, or that does not exist, counts as none.
An Actor is **present** on a Run while one of its tabs at least shows it: it arrives with its first
tab, and leaves when its last one shows another Run, none, or closes (its connection ends).

- `Host::presences(run_id)`: the Run's Presences now, `[{ "actor": <Actor>, "since": "<RFC
  3339>" }]`, by order of arrival.
- `GET /runs/presences`: `{"runs": [{"run_id", "presences": […]}]}`, every Run someone has open
  that the caller may read.
- The events, on `/ws` (to the connections that may read the Run) and in `on_event`:

  ```json
  { "type": "presence_arrived", "run_id": "20261009-101500-a1b2c3d",
    "actor": { "kind": "identified", "namespace": "test", "id": "bob" },
    "presences": [ { "actor": { "kind": "identified", "namespace": "test", "id": "alice" },
                     "since": "2026-10-09T10:15:02.120Z" },
                   { "actor": { "kind": "identified", "namespace": "test", "id": "bob" },
                     "since": "2026-10-09T10:16:40.031Z" } ] }
  ```

  `presence_left` has the same shape; `presences` is the Run's Presences once the change
  happened. The front hands them to `run/item/people` ([extension-ui.md](extension-ui.md#runitempeople)).

Without extension every tab is the local operator's: one Presence at most per Run.

## A suspended Trigger (0.8)

`trigger_suspension` receives a `TriggerRef` (`id`, `name`, `last_editor`) and answers the
reason, or `None`. The answer is never stored: the suspension lifts as soon as the extension
stops giving one, and the Trigger fires again at its next slot. While suspended:

- a due tick spends the slot without firing and records a fire `skipped-suspended` with the
  reason;
- `POST /triggers/<id>/fire` answers `409` `{"error": "trigger '<name>' is suspended: <reason>"}`;
- `GET /triggers`, `GET /triggers/<id>`, and the answers of a create or an edit carry
  `"suspension": {"extension": "<id>", "reason": "…"}`; the interface shows the Trigger switched
  off with an amber « suspended » pill, the reason in its tooltip and in the Trigger's detail.

The suspension is asked again at each read. When its cause changes outside any mutation of a
Trigger, call `Host::triggers_changed()` (0.11): an interface open on the Triggers then shows the
pill appear or go without reloading.

## Events

`on_event` and `/ws` carry the same JSON. After every authorized and successful mutation, an
`operation_applied`:

```json
{ "type": "operation_applied", "actor": { "kind": "local_operator" },
  "operation": { "resource": "project", "verb": "update", "target": "p1", "projects": ["p1"] },
  "entity": { "kind": "project", "id": "p1" } }
```

An extension's own events arrive as `{"type": "extension_event", "extension", "name", "payload"}`.
Who opens and leaves a Run arrives as `presence_arrived` and `presence_left` (0.12,
[Presences](#presences-012)).

**Guard what a Reader may not read (0.11).** An event about an entity of a Projet goes, by
default, to every `/ws` connection. Attach the read that protects it, and the core sends it only to
the connections whose Actor may run that read (a Reader of another Projet receives nothing of it):

```rust
let read = Operation::new(ResourceKind::extension("notes", "note"), Verb::Read)
    .on(&note.id)
    .in_projects([note.project.clone()]);
host.publish(ExtensionEvent::new("note_changed", payload).guarded_by(read));
```

The core asks `authorize` once per connection and per read (the answer kept 5 s, as for a Run),
and sends the event without the guard: the envelope does not change. A guard that is no read
refuses every connection. Without an extension that restricts the reads, every connection
receives it. An `operation_applied` on an extension's Ressource needs no guard: the core guards it
with the read of its entity, in the Projets of its Operation, so fill those Projets when you build
the Operation, and its `entity` for a creation (without either, the message reaches every
connection). Your own `authorize` then answers that read: refuse it where the Actor may not read
the entity. For a deletion, the entity is gone when the read is asked: decide on the Projets of
the Operation, not by looking the entity up, or no connection learns of the deletion.
