# Extension API reference

What an extension gets from `pdo-extension-api` besides its interface (ADR-0081 and its
amendments; the interface is [extension-ui.md](extension-ui.md)). The crate's rustdoc is the
source of truth for signatures; this page says what each part is for and what goes on the wire.

Version: **0.8.0**. In 0.x a minor may break; `OLDEST_COMPATIBLE_API_VERSION` names the oldest
version the daemon still runs (`0.1.0`: no minor has broken the wire so far).

## The hooks

An extension implements `Extension`. Only `manifest` is required; every other hook has a neutral
default, and with no extension the daemon behaves as if none existed.

| Hook | Called | Default |
| --- | --- | --- |
| `manifest` | at registration | required |
| `start(context)` | once, before serving; `context.data_dir`, `context.host` | nothing |
| `identify(request)` | on every request; the first `Some` wins | `None`: the core decides |
| `authorize(actor, operation)` | before every mutation of an actor, and every guarded read (0.8, below); the first refusal wins, a panic refuses | allow |
| `on_create(actor, entity)` | after a creation; attributes prefixed `<id>/` | none |
| `on_event(event)` | off the request path, on the extension's own thread | ignore |
| `routes()` | once; nested under `/ext/<id>/` | none |
| `ui()` | on every `GET /ext` | none |
| `cli()` / `run_cli(invocation)` | `pdo ext <id> …` | no command line |
| `skills()` | once, before `start` | none |

## The Manifest

```rust
use pdo_extension_api::{Manifest, ResourceDeclaration};

fn manifest(&self) -> Manifest {
    Manifest::new("pm", "Project", env!("CARGO_PKG_VERSION"))
        .with_resource(
            ResourceDeclaration::new("tache", "Tasks", ["create", "update", "delete", "editer"])
                .described("The tasks of a Backlog"),
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
  list is `ResourceKind::CORE`) or `<extension>/<kind>` (`pm/tache`).
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
| `/ws` | `run` × `read`, per Run an event is about, per connection (answer kept 5 s) | the event is not sent to that connection |
| `GET /projects` | `project` × `read`, per Projet | the Projet is absent |
| `GET /projects/{id}` | `project` × `read` | `403` |
| `GET /fs/browse` (the loupe, the explorers) | `repository` × `read`, no target, in no Projet | `403` |

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

`NeutralHost` allows everything, publishes nothing and answers the core's catalogue alone: the
host of `ExtensionContext::new`, and a stand-in in an extension's tests.

## The catalogue of the Ressources

The core's 19 Ressources, then each extension's declarations in registration order:

```http
GET /resources
```

```json
[
  { "resource": "run", "label": "Runs", "description": "See, create, steer, archive and delete Runs",
    "verbs": ["read", "create", "delete", "edit_repos", "…"], "extension": null },
  { "resource": "pm/tache", "label": "Tasks", "description": "The tasks of a Backlog",
    "verbs": ["create", "update", "delete", "editer"], "extension": "pm" }
]
```

The same list comes from `Host::catalogue()` inside the daemon, and `core_resources()` /
`catalogue(manifests)` build it without a daemon. An extension of rights reads it to list what
rights can be set on, without knowing the other extensions.

## Events

`on_event` and `/ws` carry the same JSON. After every authorized and successful mutation, an
`operation_applied`:

```json
{ "type": "operation_applied", "actor": { "kind": "local_operator" },
  "operation": { "resource": "project", "verb": "update", "target": "p1", "projects": ["p1"] },
  "entity": { "kind": "project", "id": "p1" } }
```

An extension's own events arrive as `{"type": "extension_event", "extension", "name", "payload"}`.
