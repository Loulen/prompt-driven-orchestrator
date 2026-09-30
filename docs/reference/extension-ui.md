# Extension UI reference

How an extension adds to PDO's interface (ADR-0081, amendment #29): a catalogue of **named
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
| Decoration (`decoration`) | `UiContribution::decoration(…)` | the exported component, next to an item (no location accepts it yet) |
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
(`entity`, `filter`, `setFilter`, `resource`) plus `context`. A 0.1 list filter or detail tab of
another resource than the Runs maps onto nothing: 0.1 never rendered them.

## Adding a location

A location is added to the core, never by an extension:

1. A variant of `UiLocation` (its id in `serde(rename)`), its line in `UiLocation::ALL`,
   `UiLocation::id` and `UiLocation::accepts`, then `make types`.
2. Its section on this page (a test fails while a location of `UiLocation::ALL` has none).
3. Its rendering in the front: a component that builds the location's context with
   `useLocationContext("<id>", extra)` and renders `useContributions("<id>", context)`.
4. A contribution of the internal test extension (`crates/pdo/tests/test_extension/`), and a
   minor bump of `pdo-extension-api`.
