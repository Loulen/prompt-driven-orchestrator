# Extension UI reference

How an extension adds to PDO's interface (ADR-0081, amendments #29 and #31): a catalogue of **named
locations**, a few **kinds of contribution**, **commands** and **`when` clauses**. The locations,
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
    let new_task = UiCommand::new("pm.new-task", "New task", "newTask")
        .with_icon("/ext/pm/new-task.svg");
    Some(
        UiContributions::bundle("/ext/pm/ui.js", 19)
            .contribute(
                UiContribution::page(UiLocation::TopbarEnd, "home", "Project", "PmPage")
                    .with_icon("/ext/pm/icon.svg"),
            )
            .contribute(
                UiContribution::action(UiLocation::TopbarEnd, "new-task", &new_task)
                    .when(When::has("run")),
            )
            .command(new_task),
    )
}
```

- The bundle (`bundle_url`) is one ES module. It imports `react`, `react/jsx-runtime` and
  `react-dom` as bare specifiers: the front shares its own React through an import map. A bundle
  built for another React major (`react_major`) is never loaded, and a banner says so.
- A component-backed contribution names the component the bundle exports (`PmPage`); an action
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

### `settings`

- **Accepts**: section.
- **Renders**: the Extensions page of the Settings, one section per contribution, titled with its
  label. A section saves the way its extension decides, never with the instance form.
- **Context**: `LocationContext`.
- **0.1**: the `SettingsPanel` slot.

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
  (the id picked, or `null`) + `repository` (the target repository, as typed) + `trigger` (the id
  of the Trigger being edited, or `null`). `form` is present; `onSave` receives the Run launched
  (`{ kind: "run", id }`) or the Trigger saved (`{ kind: "trigger", id, name, attributes }`). The
  Run or the Trigger exists whatever the handlers answer: a failure is an error toast, and the form
  closes.

### `node/config`

- **Accepts**: section.
- **Renders**: the inspector of a node of the pipeline editor, after Outputs.
- **Context**: `NodeConfigContext` = `LocationContext` + `entity` (`{ kind: "node", id, name?,
  attributes: {} }`) + `pipeline` (`{ id, name }`) + `readOnly` (the canvas shows a Run). No
  `form`: the inspector saves as it edits, and so does a section, the way its extension decides.

### `pipeline/config`

- **Accepts**: section.
- **Renders**: the inspector of a pipeline (nothing selected on the canvas), after Stats.
- **Context**: `PipelineConfigContext` = `LocationContext` + `entity` (`{ kind: "pipeline", id,
  name, attributes }`). No `form`.

### `run/info`

- **Accepts**: section.
- **Renders**: the Info tab of a Run's panel, after its stats, before the description.
- **Context**: `RunInfoContext` = `LocationContext` + `entity` (`{ kind: "run", id, name?,
  attributes }`). No `form`.

### Decorations

A decoration location renders the marks the extensions put on one item, side by side, each behind
its own error boundary (a failing one becomes a small « ! » whose tooltip names the extension).
**Without a contribution nothing is rendered at all**: the item keeps its layout. Several wrap
rather than push the item's own text out. The component receives `entity` and `context`:
`DecorationContext` = `LocationContext` + `entity` (`EntityContext`, `null` on the status bar).
Its `when` clause sees the entity: `When::has("entity.attributes.pm/task")` marks only the items
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
- **Renders**: a row of the Triggers list, at the end of its second line (after the schedule).
- **Context**: `DecorationContext`, `entity` = `{ kind: "trigger", id, name, attributes }`.

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

- **Accepts**: decoration.
- **Renders**: the header of a file of the Review page, before its comment counts.
- **Context**: `ReviewFileContext` = `DecorationContext` + `file` (`{ run, path, status }`),
  `entity` = `{ kind: "file", id: <path>, name: <basename>, attributes: {} }`. The Review page
  selects no Run: `context.run` is `null`, `file.run` names the Run.

### `statusbar`

- **Accepts**: decoration.
- **Renders**: the status bar, right-hand group, before the service health.
- **Context**: `DecorationContext`, `entity` = `null`.

### List filters

A list-filter location renders a row under the list's own filters, one control per contribution.
The component receives `resource`, `filter` (prefixed key → value) and `setFilter(next)`, and
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
  any Projet is left out while the filter is active.
- **0.1**: the `ListFilter { resource: Project }` slot.

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

A page shows in the full-window shell Settings and Stats use: its header (icon, title, breadcrumb,
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
  plain text (`#/ext/pm/topbar/end/task/PM-12`); text in code is left alone. From the Review page,
  the link loads the app on the page.
- A link toward an extension that is not installed, whose interface is not loaded, or that has no
  page at that location shows an error toast and opens nothing.

## Commands

```ts
interface CommandContext extends LocationContext { extension: ExtensionRef; contribution: string }
interface CommandHost {
  notify(message: string, kind?: "info" | "error"): void; // a toast
  open(link: string): void;                               // follow a deep link
}
type CommandFunction = (context: CommandContext, host: CommandHost) => unknown;
```

`UiCommand::new(id, title, export)` declares a command the bundle exports under `export`. The
core calls it with the context of the location of the action that was clicked (`contribution` is
that action's id). It may return a promise. A throw or a rejection reaches the operator in an
error toast. An action whose command the bundle does not export is not rendered.

## `when` clauses

A declarative condition on the location's context, evaluated by the core. A key is a dotted path:
`run`, `run.status`, `project.name`, `run.attributes.<extension>/<key>`, `entity.kind`, `path`. An
absent key reads as nothing.

| Rust | Wire | Holds when |
| --- | --- | --- |
| `When::has(key)` | `{"has": "run"}` | the key holds something: not absent, `null`, `false` nor `""` |
| `When::eq(key, value)` | `{"eq": ["run.status", "running"]}` | the key's value, as text, equals `value` |
| `When::not(c)` | `{"not": …}` | `c` is false |
| `When::all([…])` | `{"all": […]}` | every clause holds (true when empty) |
| `When::any([…])` | `{"any": […]}` | one clause holds (false when empty) |

An operator the front does not know is false: the element stays hidden.

## Version 0.1

An extension written for 0.1 keeps working: the daemon accepts an extension requiring any API
version from `OLDEST_COMPATIBLE_API_VERSION` (0.1.0) to its own, and maps each `UiSlot` onto its
location (`UiSlot::contribution`). The components receive the props they received in 0.1
(`entity`, `filter`, `setFilter`, `resource`) plus `context`. A 0.1 list filter of the Triggers,
the pipelines or the Projets, and a detail tab of a node, a Trigger, a skill or a Projet, map onto
the locations #31 added (0.1 declared them, never rendered them); another resource maps onto
nothing.

## Adding a location

A location is added to the core, never by an extension:

1. A variant of `UiLocation` (its id in `serde(rename)`), its line in `UiLocation::ALL`,
   `UiLocation::id` and `UiLocation::accepts`, then `make types`.
2. Its section on this page (a test fails while a location of `UiLocation::ALL` has none).
3. Its rendering in the front: a component that builds the location's context with
   `useLocationContext("<id>", extra)` and renders `useContributions("<id>", context)`.
4. A contribution of the internal test extension (`crates/pdo/tests/test_extension/`), and a
   minor bump of `pdo-extension-api`.
