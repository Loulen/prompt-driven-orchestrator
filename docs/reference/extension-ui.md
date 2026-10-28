# Extension UI reference

How an extension adds to PDO's interface (ADR-0081, amendments #29, #30, #31 and #32): a
catalogue of **named locations**, a few **kinds of contribution**, **commands**, **`when`
clauses**, the locations an extension **hosts** in its own interface, and the **design
system** it draws with (ADR-0091). The locations,
what each accepts and the context each hands over are part of the semver of
`pdo-extension-api`.

The Rust types are in `pdo-extension-api` (`UiLocation`, `UiContribution`, `UiElement`,
`UiCommand`, `When`, `UiContributions`). Their TypeScript is generated into
`frontend/src/shared/generated/` (`UiLocation.ts`, `UiContribution.ts`…); the props and contexts
below are `frontend/src/shared/extensions/contract.ts` (`runPeople.ts` for `run/item/people`).

## Declaring contributions

An extension returns its contributions from `Extension::ui`:

```rust
use pdo_extension_api::{UiCommand, UiContribution, UiContributions, UiLocation, When};

fn ui(&self) -> Option<UiContributions> {
    let new_note = UiCommand::new("test.new-note", "New note", "newNote")
        .with_icon("/ext/test/new-note.svg");
    Some(
        UiContributions::bundle("/ext/test/ui.js", 19)
            .contribute(
                UiContribution::page(UiLocation::TopbarEnd, "home", "Notes", "NotesPage")
                    .with_icon("/ext/test/icon.svg"),
            )
            .contribute(
                UiContribution::action(UiLocation::TopbarEnd, "new-note", &new_note)
                    .when(When::has("run")),
            )
            .command(new_note),
    )
}
```

An action goes to any Action location the same way, with a `when` on what that location hands
over:

```rust
let link = UiCommand::new("test.link-note", "Link to a note…", "linkNote");
UiContributions::bundle("/ext/test/ui.js", 19)
    .contribute(
        UiContribution::action(UiLocation::RunItemMenu, "link-note", &link)
            .when(When::not(When::eq("entity.status", "archived"))),
    )
    .command(link)
```

- The bundle (`bundle_url`) is one ES module. It imports `react`, `react/jsx-runtime` and
  `react-dom` as bare specifiers: the front shares its own React through an import map. A bundle
  built for another React major (`react_major`) is never loaded, and a banner says so.
- A component-backed contribution names the component the bundle exports (`NotesPage`); an action
  names one of the extension's commands.
- Contribution ids are unique within the extension; the front keys each element
  `<extension id>:<contribution id>`.
- The daemon leaves out, with a warning in its log, a contribution its location does not accept,
  an action whose command is not declared, and an id already taken.

**A location is a list.** Every extension's contributions add up there: extensions in
registration order, then each extension's contributions in declaration order. No contribution is
exclusive.

## Kinds of contribution

| Kind (`type`) | Constructor | Rendered by |
| --- | --- | --- |
| Page (`page`) | `UiContribution::page(location, id, label, component)`, `.with_icon(url)` | the exported component, full-window |
| Action (`action`) | `UiContribution::action(location, id, &command)` | the core: an entry that calls the command; its label is the command's title |
| Tab (`tab`) | `UiContribution::tab(…)` | the exported component, in a detail's tab |
| Section (`section`) | `UiContribution::section(…)` | the exported component, under a heading |
| Decoration (`decoration`) | `UiContribution::decoration(…)` | the exported component, a small mark on an item (a row, a card, the status bar) |
| List filter (`list_filter`) | `UiContribution::list_filter(…)` | the exported component, above a list |
| Source of completion (`completion`, 0.14) | `UiContribution::completion(location, id, label, trigger, suggest)`, `.rendering(pattern, component)` | the core: a list of the exported function's suggestions, opened by `trigger` in a text field; the exported component draws each match of `pattern` in the rendered text |

Every contribution may carry a `when` clause (`.when(When::…)`): the core renders it only while
the clause holds.

## The catalogue

Every component receives `extension` (`{ id, name }`) and `context`. Every context holds at least
the **selected Run** and **its Projet**, plus `location`:

```ts
interface RunContext { id: string; name: string | null; pipeline: string; status: string; attributes: Attributes }
interface ProjectContext { id: string; name: string; attributes: Attributes }
interface LocationContext { location: UiLocation; run: RunContext | null; project: ProjectContext | null }
```

The Projet is the one the Runs list groups the Run under: the Projet whose members hold the
repository of the Run's root, compared verbatim (a child Run follows its root). `null` without a
selected Run, or when its repository belongs to no Projet.

### `topbar/start`

- **Accepts**: page.
- **Renders**: a text entry per page, after the Acogni mark, before the command field. The page
  opens full-window, with every page of the location on its rail. A long label ends in an ellipsis.
- **Context**: `PageContext` (below).
- **0.1**: the `Navigation` slot.

### `topbar/end`

- **Accepts**: page, action.
- **Renders**: a 24 px icon button per element in the top bar's right-hand group, **before the
  core's icons** (New run, Stats, Settings, the theme), a hairline after them, the label as
  tooltip. With many, the group keeps its icons aligned and is clipped before it overlaps the
  command field. A page's button opens it full-window **without rail**: the page brings its own
  navigation. An action's button calls its command.
- **Icon**: a page's `icon`, or an action's command `icon`, is an SVG the extension serves. It is
  drawn as a mask in the colour of the text around it (hover and accent states apply). Without one,
  or while it loads, the button shows the label's first letter, boxed.
- **Context**: `PageContext` for a page, `LocationContext` for an action.

### `topbar/account`

- **Accepts**: decoration.
- **Renders**: the component as is, at the far right of the top bar, **after the theme**: an avatar
  opening the menu of the signed-in person (Profile, Administration, Sign out), say. The component
  draws its own button and menu (the `DropdownMenu` of its bundle). Nothing without a
  contribution.
- **Context**: `DecorationContext`, `entity` = `null`. Who is signed in: `useCurrentActor()` of
  `@pdo/extension-ui` ([below](#the-actors-names)).
- **Since**: 0.8.

### `settings`

- **Accepts**: section.
- **Renders**: the Extensions page of the Settings, one section per contribution, titled with its
  label. A section saves the way its extension decides, never with the instance form.
- **Context**: `LocationContext`.
- **0.1**: the `SettingsPanel` slot.

### `settings/page`

- **Accepts**: page.
- **Renders**: a rail entry of the Settings per page, labelled with its label, after the core's
  categories and the Extensions entry (which keeps listing the `settings` sections). The entry
  opens the page in the Settings' content pane, behind its error boundary; the Settings header
  shows its breadcrumb and its actions (`shell.setBreadcrumb`, `shell.setActions`), `shell.close()`
  closes the Settings. The page saves the way its extension decides, never with the instance
  form: the Settings' Save footer is hidden while the page is on screen. Nothing without a
  contribution.
- **Context**: `PageContext` ([Pages](#pages)); `navigate(path)` changes `context.path`.
- **Deep link**: `#/ext/<extension>/settings/page/<path>` opens the Settings on the page. As
  for the other page locations, the link names the extension's **first** page there: an
  extension with two Settings pages reaches the second from its rail entry, not by link.
- **Since**: 0.9.

### `stats/tab`

- **Accepts**: tab.
- **Renders**: a rail entry of the Stats per tab, labelled with its label, **after Performance**.
  The tab renders in the Stats frame: its header (period presets, « Computed », Refresh, ✕) stays
  the core's, the component fills the content pane, behind its error boundary (a component that
  throws shows « This element of the extension … failed » in its tab, the other tabs, the
  extension's included, still open).
  Nothing without a contribution.
- **Context**: `StatsTabContext`:

  ```ts
  interface StatsPeriod { preset: "7d" | "30d" | "90d" | "all"; from: string; to: string; bucket: "day" | "week" | "month" }
  interface StatsTabContext extends LocationContext { period: StatsPeriod; refresh: number }
  interface StatsTabProps { extension: ExtensionRef; context: StatsTabContext }
  ```

  `period` is the window the header shows (ISO instants, `to` excluded, whole UTC days) and the
  bucket the core's charts fold by; `refresh` grows by one at each click on Refresh: a tab reloads
  what it shows when it changes. Like every Stats setting, the period is back to 30 days at each
  open.
- **Since**: 0.9.

### `runs/filter`

- **Accepts**: list filter.
- **Renders**: a row under the Runs list's own filters, one control per contribution.
- **Context**: `ListFilterContext` = `LocationContext` + `resource` (`"run"`) + `filter`. The
  component also receives `resource`, `filter` (prefixed key → value) and `setFilter(next)`; the
  list keeps the Runs carrying every attribute of the filter, as `?attr.<id>/<key>=<value>` does.
- **0.1**: the `ListFilter { resource: Run }` slot.

### `run/detail`

- **Accepts**: tab.
- **Renders**: a tab of the Run's detail panel, after the core's tabs.
- **Context**: `TabContext` = `LocationContext` + `entity` (`{ kind: "run", id, attributes }`).
  The component also receives `entity`.
- **0.1**: the `DetailTab { resource: Run }` slot.

### Tabs

A tab location renders one tab per contribution after the core's tabs, titled with its label.
Without a contribution the detail shows no extra tab (and no tab bar where the core has a single
view). The component receives `entity` and `context`: `TabContext` = `LocationContext` + `entity`
(`{ kind, id, attributes, name? }`).

### `node/detail`

- **Accepts**: tab.
- **Renders**: a tab of the lower pane of a node's detail (the node of a Run), after I/O and,
  for an orchestrator node, Orchestration.
- **Context**: `TabContext`, `entity` = `{ kind: "node", id: <node id>, attributes: {}, name? }`.
  `context.run` is the Run the node belongs to.
- **0.1**: the `DetailTab { resource: Node }` slot.

### `trigger/detail`

- **Accepts**: tab.
- **Renders**: a tab of a Trigger's detail, after the core's « Details ».
- **Context**: `TabContext`, `entity` = `{ kind: "trigger", id, name, attributes }`.
- **0.1**: the `DetailTab { resource: Trigger }` slot.

### `skill/detail`

- **Accepts**: tab.
- **Renders**: a tab of a skill of the bank, after « SKILL.md » and « Files ».
- **Context**: `TabContext`, `entity` = `{ kind: "skill", id, name, attributes: {} }`.
- **0.1**: the `DetailTab { resource: Skill }` slot.

### Sections

A section location renders one section per contribution, after the core's own content, headed
the way its host heads its sections (the contribution's label). The component receives
`context`, `entity` (the entity the host shows or edits, as in `context.entity`) and, where the
host has a Save and a Cancel, `form`:

```ts
interface SectionForm {
  onSave(handler: (saved: EntityContext) => unknown): () => void; // returns its unregistration
  onCancel(handler: () => unknown): () => void;
  provide?(handler: () => unknown): () => void; // 0.9, `run/new` only
}
interface SectionProps { extension: ExtensionRef; context: LocationContext; entity?: EditedEntity | EntityContext; form?: SectionForm }
```

**A section keeps its changes as a draft and applies them in its `onSave` handler**: nothing is
applied before the host's Save. The host saves its own fields first, then calls every handler in
registration order, each awaited, with the entity it saved, whose id is known by then (a Projet
created by the editor has no id before). A throw or a rejection keeps the form open and shows the
message; what the core saved stays saved. `onCancel` handlers run when the form is cancelled or
closed without saving. A section's handlers are dropped when it unmounts: register them in an
effect and return the unregistration.

**A section may provide a value to the creation** (0.9, `provide`): the host calls every
`provide` handler just before it submits, in the sections' order, each awaited, and sends what it
answers under the id of the section's extension (`POST /runs` `extensions.<id>`); `undefined`
provides nothing, and the objects two sections of one extension provide are merged key by key.
The daemon hands the value to that extension's `validate_create`, then to its
`on_create_with_data` before the Run's first Node ([extension-api.md](extension-api.md#the-creation-of-a-run-09)).
A throw or a rejection of the handler, like the extension's refusal of the value (`400` with its
reason), keeps the form open and shows the message; nothing is created. Only `run/new` calls it,
and only when it launches a Run (not a Trigger); without a providing section, the request is
unchanged.

### `project/edit`

- **Accepts**: section.
- **Renders**: the Projet editor (the group-header pencil of the Runs and Triggers lists), under
  the core's fields, one section per contribution, a rule above, titled with its label and the
  extension's name. At creation as at edition.
- **Context**: `ProjectEditContext` = `LocationContext` + `entity` (`{ kind: "project", id: string
  | null, name, attributes }`, `id` is `null` at creation; `name` follows the Name field) +
  `creating`. `form` is present: see *Sections*.
- **0.1**: the `DetailTab { resource: Project }` slot (a Projet has no detail panel: its tab is a
  section of its editor, and its component receives the Projet as `entity`).

### `run/new`

- **Accepts**: section.
- **Renders**: the New Run form, after the variable overrides, one section per contribution, in
  both of its modes (a Run, a Trigger).
- **Context**: `RunNewContext` = `LocationContext` + `mode` (`"run"` or `"trigger"`) + `pipeline`
  (the id picked, or `null`) + `repository` (the target repository, as typed) + `targetProject`
  (the Projet owning that repository, compared verbatim, or `null`; CORE-5) + `input` (the
  prompt, as typed; 0.9) + `name` (the Run's name as typed, the Trigger's in `trigger` mode, `""`
  for none; 0.9) + `trigger` (the id of the Trigger being edited, or `null`). `form` is present;
  `provide` (0.9) sends a value with the Run being launched, see *Sections*; `onSave` receives the Run launched
  (`{ kind: "run", id, name?, attributes }`, read back so it carries the Attributes the
  extensions stamped on the create) or the Trigger saved (`{ kind: "trigger", id, name, attributes }`). The
  Run or the Trigger exists whatever the handlers answer: a failure is an error toast, and the form
  closes.
- **Rights** (CORE-5): the form asks the question of authorization (`POST /authorize`) at each
  open. When the disk may not be browsed (`repository` × `read`), the loupe is gone; when a Run
  may not be created somewhere, the field offers the repositories of the Projets that allow it,
  grouped by Projet, and says how many Projets it hides. « New Run » is disabled, with its reason,
  when no Projet allows a Run.

### `node/config`

- **Accepts**: section.
- **Renders**: the inspector of a node of the pipeline editor, after Outputs.
- **Context**: `NodeConfigContext` = `LocationContext` + `entity` (`{ kind: "node", id, name?,
  attributes: {} }`) + `pipeline` (`{ id, name }`) + `readOnly` (the canvas shows a Run). No
  `form`: the inspector saves as it edits, and so does a section, the way its extension decides.

### `pipeline/config`

- **Accepts**: section.
- **Renders**: the Info tab of Pipeline info (the toolbar's `i`) on a template, after Stats. A Run's
  Info tab shows `run/info` instead. Pipeline info is the only surface of a pipeline's settings: an
  empty canvas selection shows nothing on the right.
- **Context**: `PipelineConfigContext` = `LocationContext` + `entity` (`{ kind: "pipeline", id,
  name, attributes }`). No `form`.

### `run/info`

- **Accepts**: section.
- **Renders**: the Info tab of a Run's panel, after its stats, before the description.
- **Context**: `RunInfoContext` = `LocationContext` + `entity` (`{ kind: "run", id, name?,
  attributes, created_by, trigger_editor }`): the Actor that created the Run and, for a fired
  Run, the Trigger's last editor, `null` when unknown (API 0.8). No `form`.

### Decorations

A decoration location renders the marks the extensions put on one item, side by side, each behind
its own error boundary (a failing one becomes a small « ! » whose tooltip names the extension).
**Without a contribution nothing is rendered at all**: the item keeps its layout. Several wrap
rather than push the item's own text out. The component receives `entity` and `context`:
`DecorationContext` = `LocationContext` + `entity` (`EntityContext`, `null` on the status bar).
Its `when` clause sees the entity: `When::has("entity.attributes.test/note")` marks only the items
that carry the attribute.

A decoration sits inside its item, often a button: keep it small (a badge of one line), and call
`event.stopPropagation()` in a clickable one, or the click also selects the item.

### `run/item`

- **Accepts**: decoration.
- **Renders**: a row of the Runs list, in its right-hand column, on line 2 (the line of the child
  counts, before them), so a badge never truncates the pipeline's name.
- **Context**: `DecorationContext`, `entity` = `{ kind: "run", id, name?, attributes }`.

### `run/item/people`

- **Accepts**: decoration.
- **Renders**: a row of the Runs list, in its left-hand column, on line 2 after the pipeline's
  name (and its trigger badge): the people of the Run. **The core draws no person there**:
  nothing without a contribution, neither the creator nor anyone else.
- **Context**: `RunPeopleContext` = `DecorationContext` + `presences`, `entity` = `{ kind: "run",
  id, name?, attributes, created_by }`:

  ```ts
  interface Presence { actor: Actor; since: string } // since: the arrival of the Actor's first tab
  interface RunPeopleEntity extends EntityContext { kind: "run"; created_by: Actor | null }
  interface RunPeopleContext extends DecorationContext { entity: RunPeopleEntity; presences: Presence[] }
  ```

  `presences` are the Actors who have the Run open in a browser, by order of arrival, one per
  Actor whatever its count of tabs, **live**: the context changes at each arrival and each
  departure ([extension-api.md](extension-api.md#presences-012)). Name them with
  `useActorDisplay(actor)`.
- **Since**: 0.12.

### `trigger/item`

- **Accepts**: decoration.
- **Renders**: a row of the Triggers list, on a third line of its own under the pipeline and the
  schedule, so a badge never truncates the pipeline's name; several wrap.
- **Context**: `DecorationContext`, `entity` = a `TriggerEntity` (`{ kind: "trigger", id, name,
  pipeline, enabled, attributes, last_editor, suspension }`): its last editor and, when an
  extension suspends it, `{ extension, reason }` (API 0.8).

### `pipeline/item`

- **Accepts**: decoration.
- **Renders**: a row of the Pipelines list, at the end of its second line (after the node count
  and the date).
- **Context**: `DecorationContext`, `entity` = `{ kind: "pipeline", id, name, attributes }`.

### `canvas/node`

- **Accepts**: decoration.
- **Renders**: a node card of the canvas (editor and Run view), on a line of its own under the
  title; the card grows that line only when there is a decoration.
- **Context**: `CanvasNodeContext` = `DecorationContext` + `node` (`{ type, status }`), `entity` =
  `{ kind: "node", id, name, attributes: {} }`.

### `port/item`

- **Accepts**: decoration.
- **Renders**: a port row of a node's detail (Inputs and Outputs), after the port's name and
  badges.
- **Context**: `PortItemContext` = `DecorationContext` + `port` (`{ run, node, name, direction:
  "input" | "output", type }`), `entity` = `{ kind: "port", id: "<node>/<port>", name, attributes:
  {} }`.

### `review/file`

Accepts decorations as well as actions: see [`review/file`](#reviewfile) among the Action
locations.

### `review/composer`

- **Accepts**: source of completion (0.14), the one location that takes it
  ([Sources of completion](#sources-of-completion)).
- **Renders**: the comment editor of the Review page. Typing a source's trigger opens a list
  under it, in the style of the ref picker, without header nor keyboard hint; the render draws
  what was inserted in the editor's Preview, the comment cards (draft and sent) and the replies.
  Without a contribution, the editor and the comments are unchanged.
- **Context**: `ReviewComposerContext` = `ReviewContext` + `anchor` (`{ path, side, line }`, the
  line the comment is written on); `run` is the Run under review (and `project` its Projet). A
  render receives the location's context without `review` nor `anchor` (a comment card does not
  know which pair the editor had).
- **Since**: 0.14.

### `statusbar`

- **Accepts**: decoration.
- **Renders**: the 22 px status bar, right-hand group, before the service health and the version,
  on one line (clipped, never wrapped). The bar shows icons and numbers: draw an icon and a short
  number, the name in a tooltip (`Tooltip` of `@pdo/extension-ui`).
- **Context**: `DecorationContext`, `entity` = `null`.

### `runs/empty`

- **Accepts**: decoration.
- **Renders**: the empty state of the Runs list, shown while the list holds no Run at all (not
  while a filter hides every Run), under the core's « No runs yet », which stays. The decorations
  stack one under the other, each behind its error boundary: say why the list is empty (« No
  Project yet », and whom to ask). Nothing without a contribution.
- **Context**: `DecorationContext`, `entity` = `null`.
- **Since**: 0.11.

### List filters

A list-filter location renders a row under the list's own filters, one control per contribution.
The component receives `resource`, `label` (the contribution's declared label, its caption),
`filter` (prefixed key → value) and `setFilter(next)`, and
`context` = `ListFilterContext` (`LocationContext` + `resource` + `filter`). The list keeps what
carries every attribute of the filter, as `?attr.<id>/<key>=<value>` does on the list endpoint.

### `triggers/filter`

- **Accepts**: list filter.
- **Renders**: above the Triggers list; `resource` is `"trigger"`. The list keeps the Triggers
  carrying every attribute of the filter (`GET /triggers?attr.…`).
- **0.1**: the `ListFilter { resource: Trigger }` slot.

### `pipelines/filter`

- **Accepts**: list filter.
- **Renders**: above the Pipelines list; `resource` is `"pipeline"`. The list keeps the pipelines
  carrying every attribute of the filter (`GET /pipelines?attr.…`).
- **0.1**: the `ListFilter { resource: Pipeline }` slot.

### `projects/filter`

- **Accepts**: list filter.
- **Renders**: above the Runs list and above the Triggers list, which both group by Projet (each
  list keeps its own filter); `resource` is `"project"`. A list keeps the items whose Projet
  carries every attribute of the filter (`GET /projects?attr.…`), by the grouping's rule: the
  Projet whose members hold the repository (of the Run's root, of the Trigger). An item outside
  any Projet is left out while the filter is active. The row opens on a « Project » tag, so its
  controls never read as the list's own.
- **0.1**: the `ListFilter { resource: Project }` slot.
### The Action locations

Every location below accepts **actions only** (amendment #30), except `review/file`, which
also takes decorations (amendment #31). The core renders each action and
calls its command with the location's context; the `when` clause reads that same context.

- **In a bar**: an icon button per action, before the core's buttons: the command's `icon`, or the
  label's first letter, boxed; the label as tooltip and accessible name.
- **In a menu**: a group per extension, titled with the extension's name, after the core's entries
  and **before the destructive entry** (Delete, Cleanup…). Each entry shows the command's icon and
  the label.
- **Without a contribution**, a bar or a menu looks exactly as it did: no button, no group, no
  separator.
- **A command that fails** (a throw, a rejection) shows an error toast; the bar or the menu keeps
  working. Every element sits in its own error boundary.

The contexts they hand over, on top of `LocationContext`:

```ts
interface EntityContext { kind: string; id: string; attributes: Attributes; name?: string | null }
// The Actor of the extension API, as the daemon serializes it (API 0.8).
type Actor = { kind: "local_operator" } | { kind: "agent"; run_id: string; node_id: string }
  | { kind: "trigger"; trigger_id: string } | { kind: "identified"; namespace: string; id: string };
interface RunEntity extends EntityContext { kind: "run"; name: string | null; pipeline: string; status: string }
interface TriggerEntity extends EntityContext {
  kind: "trigger"; name: string; pipeline: string; enabled: boolean;
  last_editor: Actor | null;                                  // API 0.8
  suspension: { extension: string; reason: string } | null;   // API 0.8
}
interface PipelineEntity extends EntityContext { kind: "pipeline"; name: string }
interface NodeEntity extends EntityContext { kind: "node"; name: string | null; type: string | null; status: string | null }
interface EdgeEntity extends EntityContext { kind: "edge"; from: string; to: string }   // id: "<from>.<port>-><to>.<port>"
interface FileEntity extends EntityContext { kind: "file"; path: string; status: string } // id: the path
interface SkillEntity extends EntityContext { kind: "skill" | "skill_folder"; name: string }

interface ItemContext<E> extends LocationContext { entity: E }
interface SelectionContext extends LocationContext { selection: { kind: "run"; ids: string[] } }
interface CanvasItemContext<E> extends ItemContext<E> { pipeline: { id: string; name: string; run: string | null } }
interface ReviewContext extends LocationContext { review: { from: string; to: string } }
interface ReviewFileContext extends ReviewContext { entity: FileEntity; file: { run: string; path: string; status: string } }
```

`attributes` is `{}` for an entity that carries none. A command receives this context plus
`extension` and `contribution` (see *Commands*).

### `runs/header`

- **Accepts**: action.
- **Renders**: an icon button per action in the Runs tab's title bar, before « Project » and
  « New Run ».
- **Context**: `LocationContext`.

### `run/item/menu`

- **Accepts**: action.
- **Renders**: the « ⋯ » menu of a Run's row (a `MoreHorizontal` trigger after the row's hover
  icons): Rename, Pause / Resume, Retry all, Open shell, Copy run id as the Run allows, the
  extensions' groups, then Cleanup (Stop and archive) or Forget.
- **Context**: `ItemContext<RunEntity>`: the row's Run, whatever the selection. Hide an action
  on an archived Run with `When::not(When::eq("entity.status", "archived"))`.

### `runs/bulk`

- **Accepts**: action.
- **Renders**: an entry per action in the bar of the Runs multi-selection, after Cleanup, Retry
  and Pause.
- **Context**: `SelectionContext`: the ids of the selected Runs.

### `run/toolbar`

- **Accepts**: action.
- **Renders**: an icon button per action in the toolbar of a Run's canvas, after its finished-run
  group and before Review and Pipeline info. Not on a pipeline template's canvas.
- **Context**: `LocationContext`, whose `run` is the Run on the canvas.

### `canvas/node/menu`

- **Accepts**: action.
- **Renders**: the right-click menu of a node of the canvas (template or Run, not archived):
  Duplicate, Fan out, Export as YAML…, the extensions' groups, then Delete.
- **Context**: `CanvasItemContext<NodeEntity>`: the node (`status` on a Run's canvas, else
  `null`) and the pipeline of the tab.

### `canvas/edge/menu`

- **Accepts**: action.
- **Renders**: the right-click menu of an edge of the canvas: the extensions' groups, then
  Delete edge.
- **Context**: `CanvasItemContext<EdgeEntity>`.

### `node/toolbar`

- **Accepts**: action.
- **Renders**: an icon button per action at the right of the header of a node's detail, in a
  Run.
- **Context**: `ItemContext<NodeEntity>`: the node (`type` is `null` there) and, as `run`, the
  selected Run.

### `trigger/item/menu`

- **Accepts**: action.
- **Renders**: the « ⋯ » menu of a Trigger's row: Run now, Edit, Enable / Disable, Copy trigger
  id, the extensions' groups, then Delete.
- **Context**: `ItemContext<TriggerEntity>`.

### `pipeline/item/menu`

- **Accepts**: action.
- **Renders**: the « ⋯ » menu of a row of the Pipelines tab: Open, Rename, Duplicate, the
  extensions' groups, then Delete pipeline.
- **Context**: `ItemContext<PipelineEntity>`.

### `pipelines/header`

- **Accepts**: action.
- **Renders**: an icon button per action in the Pipelines tab's title bar, before « Import a
  workflow » and « New pipeline ».
- **Context**: `LocationContext`.

### `tab/menu`

- **Accepts**: action.
- **Renders**: the right-click menu of an editor tab: Close, Close others, Close to the right,
  Close all, then the extensions' groups.
- **Context**: `ItemContext<PipelineEntity>`: the tab's pipeline.

### `review/toolbar`

- **Accepts**: action.
- **Renders**: an icon button per action in the Review page's top bar, before « open in a new
  tab ».
- **Context**: `ReviewContext`, whose `run` is the Run under review (and `project` its Projet).

### `review/file`

- **Accepts**: action and decoration (amendments #30 and #31), the one Action location that takes
  something else.
- **Renders**: in the header of a file of the Review page, an icon button per action before
  « Copy path » (clicking it does not fold the file), and the decorations before its comment
  counts.
- **Context**: `ReviewFileContext`, for both: `entity` = `{ kind: "file", id: <path>, name:
  <basename>, path, status, attributes: {} }`, `file` = `{ run, path, status }`, `review` the
  compared pair; `run` is the Run under review (and `project` its Projet).

### `skill/menu`

- **Accepts**: action.
- **Renders**: the « ⋮ » menu of a skill or a folder of the skill bank: its verbs, the
  extensions' groups, then its delete (or « Managed by PDO » on a locked skill).
- **Context**: `ItemContext<SkillEntity>`.

## Sources of completion

A source of completion (0.14) lets an extension complete what a person types in a text field
of the core, today the Review comment editor (`review/composer`). The core never interprets
what is inserted: it is text of the comment, sent and stored as typed.

```rust
use pdo_extension_api::{UiContribution, UiContributions, UiLocation};

UiContributions::bundle("/ext/test/ui.js", 19).contribute(
    UiContribution::completion(UiLocation::ReviewComposer, "tags", "Tags", '#', "suggestTags")
        .rendering("#[a-z][a-z0-9-]*", "TagChip"),
)
```

```ts
interface CompletionSuggestion {
  id: string;            // unique within the answer
  label: string;         // the row's main text
  insert: string;        // what replaces the trigger and the query
  hint?: string | null;  // after the label, monospace (an id)
  detail?: string | null; // at the right of the row (a role)
  avatar?: { name: string; avatar_url?: string | null } | null; // an image, or the initials of name
}
type CompletionFunction = (query: string, context: ReviewComposerContext)
  => CompletionSuggestion[] | Promise<CompletionSuggestion[]>;
interface CompletionRenderProps { extension: ExtensionRef; context: LocationContext; text: string }
```

- **Trigger**: one character. The list opens when the word under the caret starts with it, at the
  start of the text or after a whitespace (`a#b` opens nothing), and closes at the first
  whitespace.
- **`suggest`**: the function the bundle exports under that name, called at each keystroke with
  the text typed after the trigger (`""` right after it) and the location's context. An answer
  that arrives after a newer one is dropped; a throw or a rejection shows no suggestion. The
  sources of the same trigger add up in the list, in contribution order. An empty answer closes
  the list.
- **Pick**: ↑ and ↓ move, Enter or Tab picks, Escape closes the list (a second Escape cancels the
  editor as before), a click picks. The trigger and the query are replaced by `insert`, followed
  by a space.
- **Render** (optional): each match of `pattern`, a JavaScript regular expression, in the plain
  text of a rendered comment (not in code nor in a link) is drawn by the exported `component`,
  which receives the match as `text`. A component that throws shows the plain text. Without a
  render, the inserted text shows as markdown.
- A source whose bundle does not export `suggest` is left out; a render whose component is not
  exported, or whose pattern is not a valid regular expression, draws nothing.

## Pages

```ts
interface PageContext extends LocationContext { path: string }
interface PageShell {
  setBreadcrumb(items: string[]): void; // crumbs after the page's title, in the shell's header
  setActions(node: ReactNode): void;    // right-aligned header content, before ✕
  close(): void;
}
interface PageProps { extension: ExtensionRef; context: PageContext; navigate(path: string): void; shell: PageShell }
```

A page of `topbar/start` or `topbar/end` shows in the full-window shell Settings and Stats use
(a page of `settings/page` shows inside the Settings, [above](#settingspage)): its header (icon, title, breadcrumb,
actions, ✕) and its Escape handling. `shell` keeps its identity for the page's lifetime, so it can
sit in an effect's dependencies.

**Escape** closes the page. A page that handles Escape itself (a dialog, a drawer) listens on
`document` or on its own elements and calls `event.preventDefault()`: the shell listens on
`window`, so it sees the event last and leaves the page open.

## Deep links

`#/ext/<extension>/<location>/<path>` opens the extension's first page at `<location>` (in
declaration order, whatever its `when`) and hands `<path>` to it as `context.path`.

- Opening a page from its entry writes its deep link to the address bar; `navigate(path)` writes
  the new one; closing the page leaves it. « Back » and « Forward » follow.
- A deep link is clickable in every markdown render of the core: artifacts, Review comments,
  `SKILL.md`, the changelog. Written as a markdown link whose target is the deep link, or as
  plain text (`#/ext/test/topbar/end/note/note-12`); text in code is left alone. From the Review page,
  the link loads the app on the page.
- A link toward an extension that is not installed, whose interface is not loaded, or that has no
  page at that location shows an error toast and opens nothing.

### Toward a Run

`#/runs/<run_id>` is the core's own deep link toward a Run. Following it closes the open
extension page (and any artifact viewer), selects the Run in the list and shows its detail, as a
click on its row would; the link then leaves the address bar.

- It works however it is followed: `CommandHost.open("#/runs/<id>")`, a page setting
  `window.location.hash`, a markdown link whose target is `#/runs/<id>`, or the address bar on load.
  From the Review page, the link loads the app on the Run.
- A Run the daemon does not know shows an error toast; the selection, the open page and its deep
  link stay as they were.
- Only the written form is recognised as a link: plain text `#/runs/<id>` in a markdown render is
  left as text.

## Commands

```ts
interface CommandContext extends LocationContext { extension: ExtensionRef; contribution: string }
interface CommandHost {
  notify(message: string, kind?: "info" | "error"): void; // a toast
  open(link: string): void;                               // follow a deep link: #/ext/… or #/runs/<id>
}
type CommandFunction = (context: CommandContext, host: CommandHost) => unknown;
```

`UiCommand::new(id, title, export)` declares a command the bundle exports under `export`. The
core calls it with the context of the location of the action that was clicked (`contribution` is
that action's id). It may return a promise. A throw or a rejection reaches the operator in an
error toast. An action whose command the bundle does not export is not rendered.

## `when` clauses

A declarative condition on the location's context, evaluated by the core. A key is a dotted path:
`run`, `run.status`, `project.name`, `run.attributes.<extension>/<key>`, `entity.kind`,
`entity.status`, `selection.ids`, `review.to`, `path`. An absent key reads as nothing.

| Rust | Wire | Holds when |
| --- | --- | --- |
| `When::has(key)` | `{"has": "run"}` | the key holds something: not absent, `null`, `false` nor `""` |
| `When::eq(key, value)` | `{"eq": ["run.status", "running"]}` | the key's value, as text, equals `value` |
| `When::not(c)` | `{"not": …}` | `c` is false |
| `When::all([…])` | `{"all": […]}` | every clause holds (true when empty) |
| `When::any([…])` | `{"any": […]}` | one clause holds (false when empty) |

An operator the front does not know is false: the element stays hidden.

## Hosted locations

An extension may host locations in its own interface (amendment #32): the other extensions
contribute there exactly as to the core's catalogue, and the host renders their elements with the
shared module `@pdo/extension-ui`. The core's test extension `a` hosts `a/panneau` (sections, under
each item of its page) and `a/actions` (actions, in its page's header); a host's own documentation
lists what each of its locations hands over.

### Declaring one

```rust
use pdo_extension_api::{HostedLocation, UiContributions, UiElementKind};

pub const PANEL: &str = "a/panneau";

fn ui(&self) -> Option<UiContributions> {
    Some(
        UiContributions::bundle("/ext/a/ui.js", 19)
            .importing_ui_module()
            .host(
                HostedLocation::new(PANEL, UiElementKind::Section)
                    .describe("Under each item of A's page, one section per contribution.")
                    .context_key("item", "The item the section sits under: { id, title }"),
            ),
    )
}
```

- The id is `<extension id>/<name>`, the name in lowercase ASCII letters, digits and `-`. It is a
  `&'static str`: a constant of the host's API.
- A hosted location accepts **one** kind of element (`UiElementKind`): action, tab, section,
  decoration or list filter. Never a page, which belongs to the core's shell.
- `context_key` documents what the location adds to the context every location hands over.
- The daemon leaves out, with a warning in its log, a declaration whose id does not start with
  the host's id and `/`, that takes the id of a location of the core or one already declared, or
  that accepts pages.
- `GET /ext` lists the declarations in `ui.locations`: `{ id, accepts, description, context: [{
  key, description }] }`.

### Contributing to one

```rust
use pdo_extension_api::{UiContribution, UiLocation};

UiContributions::bundle("/ext/audit/ui.js", 19)
    .contribute(UiContribution::section(UiLocation::Hosted("a/panneau"), "audit", "Audit", "AuditSection"))
```

- The element's kind must be the one the host accepts; the daemon leaves out, with a warning,
  any other.
- A contribution to the location of an extension that is not loaded, or that does not declare
  it, is **ignored without error**: the host is not installed, nothing shows.
- The rule of the list holds: every contribution adds up, in registration order, then in
  declaration order.

### Rendering one: `@pdo/extension-ui`

The front declares the shared module in its import map next to React (`/shared/pdo-extension-ui.js`).
A bundle imports it as a bare specifier and never embeds it: with Vite, list it in
`build.rollupOptions.external` with `react`, `react/jsx-runtime` and `react-dom`. Its types are
`frontend/public/shared/pdo-extension-ui.d.ts` (served as `/shared/pdo-extension-ui.d.ts`).

```ts
import { HostedContribution, HostedContributions, useHostedContributions } from "@pdo/extension-ui";

// Every element, in order; nothing at all without contribution.
<HostedContributions location="a/panneau" context={context} />

// Or the list, to lay it out yourself (tab headers, then the selected tab's body).
const tabs = useHostedContributions("a/tabs", context);
tabs.map((tab) => <button key={tab.key} onClick={() => select(tab.key)}>{tab.label}</button>);
<HostedContribution element={selected} />
```

| Export | What it does |
| --- | --- |
| `useHostedContributions(location, context?)` | The elements at `location` (`{ key, extension, id, label, type, context }`), `when` clauses applied on `context`. Empty without contribution, and for a location no loaded extension hosts |
| `HostedContribution({ element, props? })` | One element: its component with `extension`, `context` and `props`, inside an error boundary; an action as a button (its command's icon and title) that calls it |
| `HostedContributions({ location, context?, props?, wrap? })` | Every element through `HostedContribution`, each wrapped by `wrap(element, rendered)` when given; `null` without element |
| `runHostedCommand(element)` | Call an action's command, as its button does |
| `useCurrentActor()`, `useActorDisplay(actor)` | The Actors' names, since 0.8 ([The Actors' names](#the-actors-names)) |
| `openProbedSocket(path, options?)` | A WebSocket to the daemon, opened through the probe, since 0.10; its delay of silence since 0.11 ([WebSockets](#websockets)) |
| `copyToClipboard(text)` | Put `text` on the clipboard from a user gesture, as every Copy button of the core does: the Clipboard API, or outside a secure context (an instance served over plain http) `execCommand("copy")` on a hidden textarea. Resolves to whether the text was written, never rejects. Since 0.11 |
| `Button`, `IconButton`, `Select`, `Menu`, `Modal`, `toast`… | The primitives of the design system, since 0.17 ([The design system](#the-design-system)) |
| `version`, `major` | The version of `pdo-extension-api` the module comes with, and its major |

- **Context**: an element receives the host's `context`, merged over the context every location
  hands over (the selected Run, its Projet) and `location` (the hosted id). Pass a stable object
  (`useMemo`): the list is recomputed when it changes. A `when` clause reads it by dotted keys
  (`page.kind`).
- **Errors**: a component that throws takes down its own element only; a command that throws or
  rejects shows an error toast.
- **No layout shift**: without an element, nothing is rendered, no wrapper box either.
- **Version**: a bundle that imports the module declares it (`importing_ui_module()`, on the wire
  `ui.ui_module_major`). A bundle built against another major than the front's is never loaded,
  and a banner says so, as for React. The module's version is `pdo-extension-api`'s.

## The design system

Since 0.17 the core serves its design system to the extensions' interfaces (ADR-0091): its tokens
and the primitives it draws its own controls with. An extension's interface draws with them and
ships no copy of either: a change of style of the core reaches it without a rebuild. They are
part of the semver of `@pdo/extension-ui`: removing or breaking a primitive, or a token, is a
new major.

### Tokens

The tokens are custom properties, defined by `/shared/tokens.css`, a stylesheet at a stable
address served by the binary. The interface loads it; a page an extension serves outside the
interface (a sign-in page) links it:

```html
<html data-theme="dark">
  <head><link rel="stylesheet" href="/shared/tokens.css" /></head>
  <body style="margin: 0; background: var(--surface-0); color: var(--text); font-family: var(--font-ui)">
```

| Tokens | What they are |
| --- | --- |
| `--surface-0`, `--surface-1`, `--surface-2` | The editor and dialogs, the side bars and panels, the top bar and the popovers |
| `--hover`, `--active`, `--selected` | Translucent fills of a hovered, pressed and selected row or control |
| `--text`, `--text-2`, `--text-3` | Primary, secondary and muted ink; every one holds 4.5:1 on every surface |
| `--line`, `--line-soft`, `--line-strong` | Hairlines |
| `--primary`, `--primary-hi`, `--primary-ink` | The ruby of the one primary action of a view and of the selection, its hover, the ink on it |
| `--selected-edge`, `--focus-ring`, `--scrim`, `--shadow`, `--dim`, `--dim-soft` | The selection's edge, the focus ring, a dialog's backdrop, a floating layer's shadow, a tour's dim |
| `--color-st-running`, `-await`, `-done`, `-blocked`, `-failed`, `-skipped` | The six status colours (a status is shown as colour and icon) |
| `--fs-2xs` … `--fs-xl`, `--lh-ui`, `--w-regular`, `--w-medium`, `--w-strong` | The type scale, 10 to 16 px, 13 px for the body text; line height; weights |
| `--h-row`, `--h-row-2`, `--h-control`, `--h-control-sm`, `--h-bar`, `--h-topbar`, `--h-statusbar` | Heights: rows 24 and 40 px, controls 24 and 20 px, bars 32 px, top bar 36 px, status bar 22 px |
| `--g-1` … `--g-7`, `--pad-x`, `--gap` | Spacing, on a 4 px grid |
| `--radius-ui`, `--radius-pop` | 2 px corners; 6 px for the floating toolbar of a row only |
| `--font-ui`, `--font-code` | Archivo and Geist Mono, served by the binary under `/shared/fonts/` with their licence (SIL OFL 1.1) |

Dark is the default, `data-theme="light"` switches to the light theme. The attribute applies to
any element: a container carrying the other theme repaints what is under it. The interface sets
it on `<html>`; an extension reads nothing, its interface follows a switch at once.

The interface is drawn at the **UI scale** (Settings › Interface, 100, 110 or 120 %, 120 by
default), a CSS `zoom` on its root. A length in pixels needs nothing. A position computed from
the screen (`getBoundingClientRect`, `clientX`, `innerWidth`) is divided by the zoom before it is
written to a style: the element's `currentCSSZoom`. The primitives place their floating layers
so.

The classes of the design system are there for an extension's own markup: `d-mono` (code), `d-meta`
(11 px muted), `d-caps` (a caps header), `d-row` (a 24 px row with its hover and selection),
`d-bar` (a 32 px bar).

Version 0.17 renamed every token, with no alias. An extension built against an earlier version
migrates its styles:

| Before 0.17 | Since 0.17 |
| --- | --- |
| `--color-bg-0`, `--color-bg-1` | `--surface-0` |
| `--color-bg-2` | `--surface-1` |
| `--color-bg-3` | `--surface-2` (a fill under a pointer: `--hover`) |
| `--color-bg-4`, `--color-bg-5` | `--hover`, `--active` |
| `--color-fg`, `--color-fg-2` | `--text`, `--text-2` |
| `--color-fg-3`, `--color-fg-4`, `--color-fg-5` | `--text-3` |
| `--color-line`, `--color-line-strong`, `--color-line-soft` | `--line`, `--line-strong`, `--line-soft` |
| `--color-acc`, `--color-acc-hi`, `--color-acc-dim` | `--primary`, `--primary-hi` |
| `--color-acc-bg` | `--selected` |
| `--color-on-acc` | `--primary-ink` |
| `--color-st-<status>-bg` | `color-mix(in oklab, var(--color-st-<status>) 14%, transparent)` |
| `--color-st-stopped`, `-archived`, `-stale`, `-pending` | `--color-st-skipped` |
| `--color-st-interrupted`, `--color-st-paused` | `--color-st-await`, `--color-st-blocked` |
| `--color-edge-selected` | `--selected-edge` |
| `--font-sans`, `--font-mono` | `--font-ui`, `--font-code` |

### Primitives

Dense (24 px controls, 13 px text), in both themes, keyboard and screen reader included. Their
props are in `pdo-extension-ui.d.ts`; an `icon` prop takes a `lucide-react` icon or any component
taking `size`, `strokeWidth` and `className`.

| Export | What it draws |
| --- | --- |
| `Button` | A text button, hairline: `tone` `primary` (ruby, the one main action), `ghost`, `danger`; `size` `sm` (20 px); an `icon` |
| `IconButton` | An icon-only button, its `label` and `kbd` in a tooltip; `pressed`, a `badge` count |
| `Tooltip` | Wraps one control: « Name (shortcut) », one `description` line at most |
| `Kbd` | A key or a chord |
| `TextInput`, `TextArea` | Inputs; `invalid`, `mono`, `small` |
| `Field` | A form row: the label in a 96 px column (`help` in its tooltip), the control, the `error` at the row end |
| `Select` | A single value out of `options` (icons, hints, groups, a search box past 8) |
| `Segmented` | Two to four exclusive choices, arrow keys |
| `Switch`, `Checkbox` | On/off, ruby when on; a box with its text, `mixed` |
| `Popover` | A non-modal panel under its `trigger`; Escape and a click outside close it |
| `Menu` | A menu under its `trigger`: check column, icon, label, shortcut; separators, labels, submenus, `danger` |
| `Modal` | A modal dialog: title bar, body, footer (`actions`, `footerNote`); Escape closes, Ctrl+Enter runs `onSubmit` |
| `ConfirmDialog` | A confirmation: the action and its object, the consequence, the mono target; Enter confirms |
| `toast(text, kind?, options?)` | A toast in the core's stack: `info`, `success`, `warning`, `error`, a `title`, one action |
| `Tabs` | A tab strip, the live tab underlined in ruby, a count |
| `Section` | A section: caps head with a chevron and a count, actions on hover; folds |
| `KeyValue` | A read-only property grid |
| `Banner` | A tinted line at the top of a pane: status icon, fact, detail, actions |
| `EmptyState` | An icon, two to four words, one action; loading and error alike |
| `Chip` | A neutral chip, a status `tone` as its icon; removable |
| `StatusIcon` | A Run's or a node's status as the core draws it, or a `tone` with its name |
| `Avatar` | A neutral square with the initials; a robot for an agent, a bolt for a trigger |

The test extension `kit` (`crates/pdo/tests/test_extension/kit.js`) is a page drawn with nothing
else.

## The Actors' names

Since 0.8 an extension may name the Actors (`Extension::display_name`, a name and an avatar),
and every extension's interface shows those names without knowing the extension that gave
them. The shared module `@pdo/extension-ui` exports:

| Export | What it does |
| --- | --- |
| `useCurrentActor()` | The Actor using the interface and its display name, `{ actor, display }` (`display` is `null` when no extension names it); `null` until known |
| `useActorDisplay(actor)` | The display name of any `Actor` (`{ name, avatar_url }`); `null` while unknown, or when no extension names it. Batched and cached: call it per row |

The core shows them too: the `actor_display` of each row of the audit journal (`GET /audit`).
It draws no person on a Run (since 0.12): an extension does, at `run/item/people` and `run/info`.

Over HTTP: `GET /actors/me` (`{actor, display}`) and `POST /actors/display` (`{actors: [Actor]}`
→ `{displays: [ActorDisplay | null]}`, in order).

## WebSockets

An extension's interface opens its WebSockets to the daemon with `openProbedSocket` of
`@pdo/extension-ui` (since 0.10, ADR-0083), never with `new WebSocket` and its own reconnection
loop. A browser admits one connecting WebSocket per host, every tab and port together, and delays
the attempts towards an endpoint that failed: a socket retried blindly towards a stopped daemon
holds the sockets of every other tab of the host.

```ts
import { openProbedSocket } from "@pdo/extension-ui";

const socket = openProbedSocket("/ext/a/stream");
const stopState = socket.onState((state) => setIndicator(state)); // connexion | ouvert | hors_ligne
const stopMessages = socket.onMessage((data) => apply(data)); // string, or ArrayBuffer
socket.send(JSON.stringify({ hello: true })); // false while not open
socket.close(); // 1000
```

- **The probe**: before every opening, `GET /actors/me` (2 s timeout). While the daemon does not
  answer (or answers `5xx`), the probe repeats, never the socket: about 1 s, then twice as long
  each time up to about 30 s, a random half taken off. An opening resets the wait.
- **A hidden tab** (`document.visibilityState`) does not probe; it probes at once when it shows
  again. An open socket stays open whatever the visibility.
- **A refused session** (`401`) goes to the sign-in page the refusal names, as every request does
  ([Signing in](#signing-in)); no socket is opened.
- **The state**: `connexion` until the first opening or the first failure, `ouvert` while open
  (each opening fires it again: the state to read again after a cut), `hors_ligne` while down.
- **Messages**: text as a `string`, binary as an `ArrayBuffer` (`binaryType = "arraybuffer"`).
- **`reconnect: false`**: probed before its one opening, never reopened once it ended.
- **`reconnect: "after-drop"`**: reopened (through the probe) only when it was open and dropped
  without the daemon's Close (`1006`, or `1001`): the daemon stopped or restarted. A Close from
  the daemon, or an upgrade refused, ends it for good. The core's terminals open their PTY so.
- **`silenceMs`** (since 0.11, ADR-0083 §5): the delay of silence. An open socket that received
  nothing (a message, or the server's beat) for that long is closed and counts as a drop: it goes
  through the probe again, `"after-drop"` included, unless `reconnect` is `false`. A browser that
  goes offline drops it at once, and a browser back online probes at once. A half-open connection
  (sleep, network change, a proxy that cut it) fires no `close`: without the delay, the interface
  stays frozen. Beat the socket on the server side more often than the delay: the core's `/ws`
  beats every 5 s (delay 15 s), a terminal every 10 s (delay 30 s). Without the option, nothing
  changes.
- **The server side** answers the client's Close frame before ending the connection: a closing
  without it is a `1006`, which the browser counts as a failure. An extension serving a WebSocket
  route keeps reading after the Close it received until the answer is flushed.

The core's own event stream and terminals open through the same function. The test extension `test`
opens `/ws` through it on its page and shows the state.

## Signing in

An extension may refuse a request no extension identified (`Extension::admit`, 0.8): `401`
`{"error": "unauthenticated", "message", "reason"}`, plus `"redirect": "<its sign-in page>"`
when it redirects. The interface's files always load; before rendering anything it asks
`GET /actors/me`, and any call refused with a `redirect` sends the browser there, once, with
`next=<path, query and hash>` appended so the sign-in page brings the person back to the deep
link they opened. `/ws` and a terminal's PTY are refused at their upgrade; their sockets are
opened through the probe (`GET /actors/me`, [WebSockets](#websockets)), which follows the
redirection the same way.

The sign-in page is the extension's: a route under `/ext/<id>/` it admits itself (by its path),
served as a full HTML page outside the SPA.

## Version 0.1

An extension written for 0.1 keeps working: the daemon accepts an extension requiring any API
version from `OLDEST_COMPATIBLE_API_VERSION` (0.1.0) to its own, and maps each `UiSlot` onto its
location (`UiSlot::contribution`). The components receive the props they received in 0.1
(`entity`, `filter`, `setFilter`, `resource`) plus `context`. A 0.1 list filter of the Triggers,
the pipelines or the Projets, and a detail tab of a node, a Trigger, a skill or a Projet, map onto
the locations #31 added (0.1 declared them, never rendered them); another resource maps onto
nothing.

## Adding a location

A location of the core is added to the core; an extension adds its own as a hosted location
(above):

1. A variant of `UiLocation` (its id in `serde(rename)`), its line in `UiLocation::ALL`,
   `UiLocation::id` and `UiLocation::accepts`, then `make types`.
2. Its section on this page (a test fails while a location of `UiLocation::ALL` has none).
3. Its rendering in the front: a component that builds the location's context with
   `useLocationContext("<id>", extra)` and renders `useContributions("<id>", context)`.
4. A contribution of the internal test extension (`crates/pdo/tests/test_extension/`), and a
   minor bump of `pdo-extension-api`.
