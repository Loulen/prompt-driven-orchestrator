# Design prompt — Maestro unified-mode canvas, inline terminals, library-driven runs (modification)

## Reference

The existing design bundle lives at `docs/design/` of the Maestro repo. Read it before producing anything new — it is the source of truth for tokens, panel geometry, node visuals, and motion.

- **Index of existing screens (with mapping to spec):** `docs/design/README.md`.
- **Composite of all 12 prior screens:** `docs/design/project/Maestro.html` (open in a browser to see them side-by-side as artboards).
- **Per-component sources** to evolve:
  - `docs/design/project/chrome.jsx` — top bar, status bar, panel headers (currently hosts the Edit/Run toggle pencil).
  - `docs/design/project/runs-list.jsx` — left panel: `RunsListPanel` (run mode) and `PipelinesListPanel` (edit mode). These will fuse.
  - `docs/design/project/dag.jsx` — canvas, node, edges, run overlay.
  - `docs/design/project/inspector.jsx` — `NodeDetail` (run-mode right panel, currently shows the polled tmux text preview), `NodeInspectorEdit` (edit-mode form), `PipelineInspector`.
  - `docs/design/project/screens.jsx` — full screen compositions.
- **Design tokens:** `docs/design/project/styles.css` — keep the existing palette (cool dark surfaces `--bg-0..5`, status colors `--st-running` blue / `--st-await` amber / `--st-done` emerald / `--st-blocked` orange / `--st-failed` red / `--st-pending` gray, accent `--acc` emerald, edit-tint `--edit-tint` violet which becomes obsolete — see "What changes"). Geist sans + Geist Mono. Triangle handles for ports (direction encodes flow: input inward, output outward).
- **Spec:** `CONTEXT.md` was just updated with all decisions in this iteration. Authoritative ADRs:
  - `docs/adr/0005-inline-xterm-over-os-spawn.md` — inline xterm.js terminal replaces the polled `capture-pane` preview; OS-native fallback survives only as a "detach" icon.
  - `docs/adr/0006-merge-as-first-class-node.md` — `Merge` is now an authored DAG node, not auto-spawned.
  - `docs/adr/0007-edit-during-run.md` — the canvas is always interactive; the Edit/Run toggle disappears.

## What stays the same

- **App frame and chrome geometry**: top bar, status bar, three-panel shell, panel widths (left ~280 px, right ~360 px). Daemon status indicator and active-runs counter on status bar.
- **Cool-dark palette and tokens** as defined in `styles.css`. No new accent.
- **Canvas idiom**: xyflow-rendered DAG, status-driven node border-left color, triangle handles, animated dashed edges from `running`/`done` upstream nodes, MiniMap and zoom controls bottom-right.
- **Start and End nodes** stay mandatory authored YAML nodes (cf. `start-node.jsx`, `end-node.jsx`). They render in every state. They are not deletable.
- **Run overlay card** (floating top-right of canvas) keeps showing pipeline name, status, run-id (truncated, copyable), version, started, elapsed, iter, vars summary. It still has primary actions (`Open Manager`, `Cancel` while running, `Cleanup` on terminal states).
- **Markdown viewer modal** (`md-modal.jsx`) — verdict and repeated-port-with-navigator variants — unchanged. Still opens from output port rows.
- **"+ New Run" modal** opens from a top-of-left-panel `+ New Run` primary button. Its general layout (~480 px wide, pipeline picker + variables form + Launch) survives; only the source feeding the picker changes (see below).
- **Status icons and dot semantics** across all surfaces (left panel run rows, node header dot, banners, Run overlay).
- **Empty state** for "no run selected" stays as today; only the secondary copy is touched (see below).

## What changes

### 1. The Edit/Run toggle disappears — the canvas is always interactive

- *Before*: `chrome.jsx > TopBar` exposes a pencil icon that toggles a global `edit-mode` class on the app frame, swapping the violet `--edit-tint` ribbon, the left panel from `RunsListPanel` to `PipelinesListPanel`, the breadcrumb's `Run | Edit` label, and the right panel form variant. The Run overlay also has a per-run "Edit this run" button that locally tints the canvas violet.
- *After*: no global mode. No pencil in the top bar. No violet edit-tint ribbon. No "Edit this run" toggle on the Run overlay. The canvas is **always editable** — drag to add an edge, drag from a library item, click a node to inspect/edit, delete an unselected/non-running node. The breadcrumb shows what is selected (a Run, a Pipeline template) without using the words "Run mode" / "Edit mode".
- *Why*: the dichotomy was forcing a mental switch that did not match the user's actual flow. The canvas now mirrors how the daemon already works (every Run has its own snapshot, the scheduler re-evaluates after every mutation). What the user is doing is determined by what they have selected in the left panel, not by a toggle.

### 2. Left panel fuses Runs and Library

- *Before*: two distinct panels (`RunsListPanel` for Run mode, `PipelinesListPanel` for Edit mode), swapped by the toggle.
- *After*: one left panel hosting both, selectable via tabs (or equivalent grouping). The user always sees:
  - **Runs** — the same list as today: filterable by `All / Active / Done / Failed / Archived`, with status dot, pipeline name, title, when, elapsed.
  - **Library** — two sub-sections:
    - **Pipeline templates**: starable, named, with node count and last-modified timestamp. Selecting a template loads it on the canvas in template-edit context (no run overlay). The starred subset of templates is what populates the dropdown of the "+ New Run" modal.
    - **Reusable nodes**: starable individual node specs (Implementer, Reviewer, Plan, etc.) the user has saved. These are draggable from the panel onto the canvas.
- Selecting a Run loads it on the canvas with the Run overlay visible. Selecting a template loads it without the overlay. Selecting nothing falls back to the existing empty state.
- *Why*: the bibliothèque becomes the entry point for both designing pipelines and dragging reusable nodes; runs and templates share the same canvas, only the contextual chrome (overlay, banners) differs.

### 3. Inline terminal replaces the polled tmux preview

- *Before*: `NodeDetail` shows a static read-only `term-preview` block populated by the daemon polling `tmux capture-pane` every 1–2 s, with a `Open terminal` button that spawns an OS-native terminal attached to the tmux session.
- *After* (cf. ADR-0005): the same slot in `NodeDetail` renders a **real interactive xterm.js terminal**, backed by a WebSocket PTY bridge attached to the node's tmux session. The user types into it, scrolls back, copy/pastes. Two icons sit on the terminal toolbar:
  - **Expand** — the terminal occupies the full vertical height of the right panel; the surrounding sections (Inputs / Outputs / Initial prompt) collapse into a compact stack the user can scroll past.
  - **Detach** — opt-in fallback that spawns an OS-native terminal window attached to the same tmux session (legacy behavior, kept as escape hatch).
- The `tmux: maestro/run-…/<node> · 80×24` toolbar header survives, but its meaning shifts (live attach instead of poll). No more "polled" or "preview" wording.
- *Why*: keep the user inside the workshop. Removing the OS-fenêtre context-switch is the practical condition that makes mid-run intervention low-friction (cf. *Deliberate over autonomous*).

### 4. New toolbar `i` info button → pipeline info panel with manager terminal

- *Before*: the canvas toolbar (`EditToolbar` in `dag.jsx`) lists the placeable node types. There is no surface to inspect the pipeline-level info while looking at the canvas — pipeline-level data is in `PipelineInspector` but only when nothing is selected in edit mode.
- *After*: the canvas toolbar gains an `i` icon. Clicking it opens a **pipeline info panel** (replacing the right panel content while open, or as a dedicated overlay — designer's call). Content:
  - **Always visible (compact, top of panel)**: pipeline name, status (running / idle / blocked / failed), the variable list with current values.
  - **Star (favorite)** action — toggles whether this pipeline lives in the library as a starred template (i.e. whether it shows up in the "+ New Run" modal's dropdown). Also the entry point for naming/saving an unsaved pipeline.
  - **When a Run is in progress (dominant content)**: an **inline xterm.js terminal attached to the Pipeline Manager session** (`maestro-mgr-<run-id>`). It is the principal element of the panel; metadata sits compact above it. Same expand / detach behavior as the node terminal.
  - **When no Run is in progress**: the metadata block alone, no terminal.
- *Why*: the Manager session was previously only reachable via "Open Manager" on the run overlay (which spawned an OS terminal). Surfacing it inline keeps the workshop intact and gives a permanent place for pipeline-level meta. The star button consolidates the "save / favorite this pipeline" gesture into one canonical surface.

### 5. New first-class nodes: ForEach and Merge

- *Before*: ForEach didn't exist visually. Merge was a hidden runtime concept (auto-spawned merge resolver) gated by a pipeline-level `auto_merge_resolver` toggle in `PipelineInspector`.
- *After*:
  - **`ForEach` node** — a new node kind, placeable from the toolbar. Visual signature distinct from regular `code-mutating` / `doc-only` nodes (similar to how `Loop` and `Switch` are visually distinct in `Screen14` / `Screen13`). Two input ports (`in`, `break`), two output ports (`body`, `done`). The `body` port fans out N parallel iterations from the `items: [...]` frontmatter of the upstream artifact; `done` fires after all iterations complete (intrinsic barrier). The node card hints at this fan-out semantic.
  - **`Merge` node** (cf. ADR-0006) — a new node kind, always `code-mutating`. One input port `branches` (marked `repeated`, accumulates N parallel edges into one fan-in), one output port `merged`. Visually communicates the fan-in / barrier semantic. Authored explicitly by the user — never auto-spawned. The pipeline-level `auto_merge_resolver` toggle in `PipelineInspector` is removed.
  - **Lint info-only**: when an editor session has a `code-mutating` fan-out without a downstream `Merge`, a small info-only diagnostic surfaces on the canvas (non-blocking, dismissible).
- *Why*: the Merge auto-spawn was implicit orchestration (incompatible with *Sharp tool*, ADR-0001); ForEach makes parallel fan-out a first-class user gesture. Both nodes need to be visible in the toolbar and recognizable in the canvas.

### 6. Output schema declared inline in the node inspector

- *Before*: `NodeInspectorEdit > Outputs` shows a static frontmatter card with key/value pairs as illustration only — no real schema editor, no per-field type. Inputs and outputs both look like static lists.
- *After*: the **Outputs** section of `NodeInspectorEdit` becomes an editable schema declaration. For each output port, the user adds rows of `field name × type` where `type ∈ { enum, int, string, bool, list }`. When `type = enum`, an `allowed: [...]` collection is editable inline. The runtime validates the frontmatter the agent writes against this schema at completion time. **Inputs are not typed** — the Inputs section keeps its current shape (port name, repeated on/off, side picker). This asymmetry is intentional: only the producer declares the shape.
- *Why*: schema-on-output gives the runtime a contract to validate (cf. retry-with-tmux-message behavior in change 7), without bloating inputs (fan-in shape is determined by upstream producers).

### 7. Frontmatter validation banner with tmux fallback retry

- *Before*: failure banner in `NodeDetail` shows a `failure_reason` and an optional 409 sub-banner about output validation.
- *After*: when a NodeRun completes but its frontmatter does not match the output schema (change 6), the runtime sends a message inside the node's tmux session ("ton frontmatter ne respecte pas le schéma : <détail>, corrige et retry") and keeps the node in `running` status. **One retry max.** The node detail panel surfaces this state inline:
  - A subtle in-progress banner: "Awaiting frontmatter retry — agent prompted to fix `<field>: expected <type>, got <value>`". Transient, amber-flavored (uses `--st-await`), not the loud red failure banner.
  - On second failure, the existing red `fail-banner` appears with `failure_reason: output validation failed` and the same 409-style sub-banner listing the offending fields. The Mark-complete / Open-terminal action row is preserved.
- *Why*: gives the agent a chance to self-correct in-line before failing loud; surfaces the soft-fail state distinctly from a hard fail.

### 8. Editing during a Run — visible in Run context, mutation invariants honored

- *Before*: editing a running pipeline required the local "Edit this run" toggle on the Run overlay, which tinted the canvas violet, swapped the right panel to `NodeInspectorEdit`, and footed a "Editing run-scoped copy · template unchanged" line.
- *After* (cf. ADR-0007): no toggle. While a Run is selected, the canvas is editable in place. Mutations follow these rules and the UI must respect them:
  - **Adding a node or edge to a running pipeline is allowed at any time.** The canvas reflects the addition immediately; the scheduler picks it up at the next tick. Newly-added nodes appear in `pending` status, edges from running upstreams animate as soon as they activate.
  - **Deleting a node in `running` / `done` / `awaiting_user` / `failed` / `blocked` status is forbidden.** The delete affordance is suppressed (or surfaces a tooltip explaining why) on those nodes. Only `pending` nodes are deletable. Their incoming/outgoing edges remain editable (disconnect is allowed).
  - **Editing `max_iter` on a live `Loop` node is allowed and applies on the fly** (replaces the legacy `extend_cycle` Manager command).
  - The Run overlay no longer footnotes "Editing run-scoped copy" — the auto-sync to the template happens silently. A small persistent hint near the pipeline name in the info panel (change 4) communicates "linked to library template `<name>`" when applicable, with a chevron to navigate to the template.
- *Why*: editing during a Run is the dominant case the user wants. Making it the default — with structurally enforced invariants instead of a mode toggle — keeps the gesture honest.

## New or changed states

- **Pipeline-info panel — running variant**: dominant inline xterm.js terminal attached to the Manager session, compact metadata above it, star button visible.
- **Pipeline-info panel — idle variant**: metadata only, no terminal slot rendered.
- **Node terminal — expanded**: terminal occupies the full vertical height of the right panel.
- **Node — frontmatter retry pending**: amber transient banner inside `NodeDetail`, node still `running` in canvas.
- **Node — frontmatter retry exhausted (failed)**: red failure banner with validation sub-banner.
- **Canvas — non-deletable node selected during a Run**: the delete/disconnect controls reflect the invariant (delete suppressed, disconnect available).
- **Canvas — newly-added node mid-run**: appears in `pending`, edges to/from it render immediately.
- **Left panel — Library / Pipeline templates section** with star toggles and `+ New` action.
- **Left panel — Library / Reusable nodes section** with draggable items.

## Hierarchy impact

- The single most-important change to hierarchy is the **dominance of the inline terminal** (node-level in `NodeDetail` and pipeline-level in the new info panel). Where the current `NodeDetail` treats the terminal preview as one of many sections (sandwiched between status header and Inputs), the new design must let it grow to be the focal point on demand (expand) while not crushing the metadata at default size.
- The **Run overlay loses the "Edit this run" button**, which slightly simplifies its action stack; remaining actions (`Open Manager`, `Cancel`, `Cleanup`) keep the same priority order.
- The **left panel** carries more weight now (two stacked content classes — runs and library — instead of one), but should still feel like one panel, not a navigation sidebar.
- The **top bar simplifies** with the pencil gone; brand + breadcrumb + settings/theme remain.

## Constraints

- Carry-overs from the existing bundle:
  - Cool-dark palette, status color semantics, Geist typography, geometry of frame, triangle port handles, xyflow-rendered DAG, shadcn primitives for forms/dialogs (cf. ADR-0003 stack).
  - Desktop-only (mono-user local app — no mobile target).
  - `--edit-tint` violet is **deprecated** by this iteration (no global edit mode, no per-run edit toggle). Do not use it.
- Inline terminal must look like a real terminal (the user types in it). Block-aligned mono font, blinking cursor, scrollback. Not a dimmed "preview".
- Star action is the canonical favorite/save-to-library gesture and must look distinct from the Run / Pipeline name (current `ih-star` styling in `inspector.jsx` is a starting point — keep its synced/diverged states for nodes; for pipeline templates the simpler outline/filled binary is enough).
- No emoji in surface copy. Use the existing iconography vocabulary from `icons.jsx`.

## Screens to produce

Three happy paths plus a component spotlight family. Each screen earns its place by exposing a design element no other screen fully shows.

### Happy path A — Monitor and intervene during a Run (~4 screens)

1. **Run — pipeline running, node terminal at default height.**
   *Why this screen:* shows the new inline xterm.js terminal embedded in `NodeDetail` at default size, surrounded by Inputs / Outputs / Initial prompt sections. Replaces existing Screen 1.
   *What it shows:* fused left panel with Runs tab active, the running DAG, a selected `running` node, the right panel with status header, live terminal toolbar (expand + detach icons), terminal content typeable, Inputs/Outputs sections below.

2. **Run — pipeline-info panel open with Manager terminal dominant.**
   *Why this screen:* shows the new toolbar `i` info panel in its running-pipeline variant. The Pipeline Manager terminal occupies most of the panel height; metadata + star action sit compact at the top.
   *What it shows:* canvas behind, `i` button in toolbar highlighted, info panel revealed with pipeline name + status + variables compact, star toggled on, full-height inline xterm.js attached to the Manager session.

3. **Run — edit during run: node added mid-run, mutation invariant in action.**
   *Why this screen:* shows the canvas-is-always-interactive principle in a Run context. A new `pending` node has been dropped from the Library section onto a graph that includes a `running` node. The `running` node's delete affordance is visibly suppressed; the new node's edges are wireable.
   *What it shows:* fused left panel with Library section visible (templates + reusable nodes), drag-source highlight, canvas with mixed `running` / `done` / new `pending` node, contextual cue on the running node explaining why it cannot be deleted, no Edit/Run toggle anywhere.

4. **Run — failed node with frontmatter retry exhausted.**
   *Why this screen:* shows the new validation banner cascade and ties into the output schema declaration. Replaces Screen 12.
   *What it shows:* node selected in `failed` status, red `fail-banner` with `failure_reason: output validation failed`, sub-banner listing offending fields with their expected types from the schema, action row with Mark-complete + Open-terminal, terminal still visible below.

### Happy path B — Design a pipeline from the library (~4 screens)

5. **Library — pipeline template selected, canvas in design context.**
   *Why this screen:* shows the fused left panel with the Library section as the focus. A pipeline template is selected (no Run overlay on canvas). All nodes are `pending`.
   *What it shows:* left panel with Runs tab quiet and Library section active (Pipeline templates list with stars + Reusable nodes list), canvas with the template's graph, no Run overlay, right panel showing `NodeInspectorEdit` for a selected node OR an empty hint to select something.

6. **Node inspector — output schema editor with enum.**
   *Why this screen:* shows the new typed Outputs section. One output has multiple fields with mixed types; one of them is `enum` with an editable `allowed: [...]` list. Inputs section remains untyped for contrast.
   *What it shows:* `NodeInspectorEdit` for a selected node, Identity / Type / Behavior / Prompt sections collapsed or compact, Outputs section expanded with per-field rows (name, type select, plus an `allowed` chip-list visible on the enum row), Inputs section showing only port name / repeated / side picker.

7. **Canvas — ForEach + Merge convergence with downstream artefact flow.**
   *Why this screen:* shows the two new first-class nodes in their natural pattern: a ForEach upstream fans into N parallel `body` iterations, all converging into a single Merge downstream. Surfaces the fan-out and fan-in visual semantics.
   *What it shows:* canvas with a ForEach node (2 in / 2 out, `body` and `done` outputs labeled), N parallel `code-mutating` nodes downstream of `body`, a Merge node (1 in `branches: repeated`, 1 out `merged`) collecting them, the `done` port of ForEach also wired further downstream past the Merge. Right panel showing the Merge selected with its repeated input port spec.

8. **Canvas — fan-out without Merge: lint info-only diagnostic.**
   *Why this screen:* shows the dismissible lint diagnostic when a `code-mutating` fan-out has no downstream Merge. Establishes that lint is non-blocking and stylistically distinct from runtime errors.
   *What it shows:* canvas with two parallel `code-mutating` nodes whose outputs feed disparate downstreams (no convergence), a small floating info-only diagnostic anchored near the fan-out point, copy explaining the suggestion to add a Merge node, dismiss/ack control.

### Happy path C — Launch a new Run (~1 screen)

9. **+ New Run modal — pipeline picker fed from starred templates.**
   *Why this screen:* the modal shape is largely unchanged but its data source is now the Library's starred templates section, and the empty/few-templates state must communicate "star a template to make it launchable". Replaces Screen 7.
   *What it shows:* modal open over a quiet canvas, pipeline picker dropdown showing starred templates only with star icon next to each name, variables form for the picked template, Launch CTA, inline link to "Manage library" that opens the Library section in the left panel.

### Component spotlight (~3 screens)

10. **Spotlight — inline xterm.js terminal expanded in node detail.**
    *Why this screen:* shows the maximum-size variant of the node terminal: full vertical height of the right panel, surrounding sections collapsed into a compact stack. Demonstrates expand state distinctly from default.
    *What it shows:* right panel with terminal at full height, expand icon now in "collapse" state, status pill anchored at the top so the user always knows which node, surrounding sections appear as a thin collapsed strip the user can scroll/expand from.

11. **Spotlight — pipeline info panel with star popover and template-link breadcrumb.**
    *Why this screen:* shows the star action's interaction surface (popover with `Update library entry` / `Reset from library` / `Remove from library` for diverged state) at the pipeline level, plus the small persistent "linked to template `<name>`" hint that anchors a Run to its library template.
    *What it shows:* info panel open, star clicked, popover surfaced with three actions, metadata above intact, no Manager terminal (this can be the idle variant for clarity).

12. **Spotlight — output schema row, enum-with-allowed editor.**
    *Why this screen:* zoomed-in detail of one Outputs row showing a port with three fields, one of them being `enum` with an inline editable `allowed: [PASS, FAIL, NEEDS_WORK]` chip list. Establishes the editing affordance distinct from the static frontmatter card today.
    *What it shows:* the row in focus, type select open on `enum`, chip list of allowed values with add/remove affordances, validation hint below.

## Out of scope

- Light theme. Stay dark.
- Mobile or responsive breakpoints.
- The Edit/Run toggle, the violet `--edit-tint` ribbon, the pencil icon in the top bar, the per-run "Edit this run" button on the Run overlay, the "Editing run-scoped copy · template unchanged" footnote — all removed by this iteration. Do not redraw them.
- The pipeline-level `auto_merge_resolver` toggle in `PipelineInspector` — removed by ADR-0006. Do not redraw it.
- The polled tmux `term-preview` block (static lines fed by `capture-pane`). Replaced by inline xterm.js everywhere.
- Editing screens 5 (edge with `when:`), 9 / 9b (markdown viewer), 13 (Switch inspector), 14 (Loop inspector), 15 (Library dropdown), 16 (Loop+Switch run), 17 (star save-state strip). They remain as-is unless a downstream change forces a touch-up.
- New iconography. Reuse the existing `icons.jsx` set; introduce a new icon only if no existing one fits ForEach, Merge, info, expand, or detach.
