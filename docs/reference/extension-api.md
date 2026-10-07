# Extension API reference

What an extension gets from `pdo-extension-api` besides its interface (ADR-0081 and its
amendments; the interface is [extension-ui.md](extension-ui.md)). The crate's rustdoc is the
source of truth for signatures; this page says what each part is for and what goes on the wire.

Version: **0.9.0**. In 0.x a minor may break; `OLDEST_COMPATIBLE_API_VERSION` names the oldest
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
| `GET /repos/recent` (the New Run prefill) | `run` × `read` on the last Run of each repository, in the repository's Projet | the repository is absent |

A read is in the same Projets as the gestures on its entity: a Run's in the Projet of its primary
repository, a Projet's in itself. The actor is the one `identify` found: on `/ws`, the one of the
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
- `publish(ExtensionEvent)`: broadcast on `/ws` and to the other extensions.
- `applied(OperationApplied)`: tell the core that an authorized mutation succeeded.
- `catalogue()` (0.8): the catalogue of the Ressources, below.
- `display_name(actor)` (0.8): how an Actor shows, the first extension that names it (the
  caller's included); `None` when none does.

`NeutralHost` allows everything, publishes nothing and answers the core's catalogue alone: the
host of `ExtensionContext::new`, and a stand-in in an extension's tests.

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
- The interface: the creator of a Run on its row; `useCurrentActor()` and `useActorDisplay(actor)`
  of `@pdo/extension-ui` for the extensions' bundles.

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

The same list comes from `Host::catalogue()` inside the daemon, and `core_resources()` /
`catalogue(manifests)` build it without a daemon. An extension of rights reads it to list what
rights can be set on, without knowing the other extensions.

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
- **Preamble** (`node_preamble`): placed at the very top of an agent's prompt, before
  `# PDO Runtime Preamble`, followed by a blank line. Never asked for a script.

The Run carries who created it on the API: `created_by` on `GET /runs/<id>` and `GET /runs`,
`trigger_editor` on a fired Run (`GET /runs/<id>`); a Trigger carries `last_editor`, set at its
creation and at each `PATCH`. All three are the Actor's JSON, absent when unknown.

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

## Events

`on_event` and `/ws` carry the same JSON. After every authorized and successful mutation, an
`operation_applied`:

```json
{ "type": "operation_applied", "actor": { "kind": "local_operator" },
  "operation": { "resource": "project", "verb": "update", "target": "p1", "projects": ["p1"] },
  "entity": { "kind": "project", "id": "p1" } }
```

An extension's own events arrive as `{"type": "extension_event", "extension", "name", "payload"}`.
