# Extension UI reference

How an extension adds to PDO's interface (ADR-0081, amendments #29, #30, #31 and #32): a
catalogue of **named locations**, a few **kinds of contribution**, **commands**, **`when`
clauses**, and the locations an extension **hosts** in its own interface. The locations,
what each accepts and the context each hands over are part of the semver of
`pdo-extension-api`.

The Rust types are in `pdo-extension-api` (`UiLocation`, `UiContribution`, `UiElement`,
`UiCommand`, `When`, `UiContributions`). Their TypeScript is generated into
`frontend/src/shared/generated/` (`UiLocation.ts`, `UiContribution.ts`…); the props and contexts
below are `frontend/src/shared/extensions/contract.ts`.

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
- **Renders**: a text entry per page, after the PDO mark. The page opens full-window, with every
  page of the location on its rail.
- **Context**: `PageContext` (below).
- **0.1**: the `Navigation` slot.

### `topbar/end`

- **Accepts**: page, action.
- **Renders**: an icon button per element in the top bar's right-hand group, **before Stats**,
  with the label as tooltip. A page's button opens it full-window **without rail**: the page
  brings its own navigation. An action's button calls its command.
- **Icon**: a page's `icon`, or an action's command `icon`, is an SVG the extension serves. It is
  drawn as a mask in the colour of the text around it (hover and accent states apply). Without one,
  or while it loads, the button shows the label's first letter, boxed.
- **Context**: `PageContext` for a page, `LocationContext` for an action.

### `topbar/account`

- **Accepts**: decoration.
- **Renders**: the component as is, at the far right of the top bar, **after Settings**: an avatar
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
  (the Projet owning that repository, compared verbatim, or `null`; CORE-5) + `trigger` (the id
  of the Trigger being edited, or `null`). `form` is present; `onSave` receives the Run launched
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

### `statusbar`

- **Accepts**: decoration.
- **Renders**: the status bar, right-hand group, before the service health.
- **Context**: `DecorationContext`, `entity` = `null`.

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

## The Actors' names

Since 0.8 an extension may name the Actors (`Extension::display_name`, a name and an avatar),
and every extension's interface shows those names without knowing the extension that gave
them. The shared module `@pdo/extension-ui` exports:

| Export | What it does |
| --- | --- |
| `useCurrentActor()` | The Actor using the interface and its display name, `{ actor, display }` (`display` is `null` when no extension names it); `null` until known |
| `useActorDisplay(actor)` | The display name of any `Actor` (`{ name, avatar_url }`); `null` while unknown, or when no extension names it. Batched and cached: call it per row |

The core shows them too: the creator of a Run on its row (only when an extension names it),
the `actor_display` of each row of the audit journal (`GET /audit`).

Over HTTP: `GET /actors/me` (`{actor, display}`) and `POST /actors/display` (`{actors: [Actor]}`
→ `{displays: [ActorDisplay | null]}`, in order).

## Signing in

An extension may refuse a request no extension identified (`Extension::admit`, 0.8): `401`
`{"error": "unauthenticated", "message", "reason"}`, plus `"redirect": "<its sign-in page>"`
when it redirects. The interface's files always load; before rendering anything it asks
`GET /actors/me`, and any call refused with a `redirect` sends the browser there, once, with
`next=<path, query and hash>` appended so the sign-in page brings the person back to the deep
link they opened. `/ws` and a terminal's PTY are refused at their upgrade; their sockets ask
`GET /actors/me` when they close abnormally and follow the redirection the same way.

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
