---
id: HP-01
covers: [pipeline, pipeline-registry, shared-draft, pipeline-version, node, edge, conditional-edge, loop-region, pipeline-validation]
---

# HP-01 — Author & save a pipeline

## Goal

A user opens a pipeline from the **Pipelines** tab, edits it on the single **unified canvas** (add a
node, draw edges, author a conditional edge, materialize a bounded loop region, move several nodes
at once) and **saves a version**. The edits live in the pipeline's **shared draft** until the Save,
then land in the YAML on disk and in the pipeline's **versions** — the core "shape a pipeline before
running it" loop, on the core alone (no extension).

## Drive-by

Features validated while crossing the editing screens:

- **Duplicate** (#224): a pipeline duplicates into a copy named `<name> (copy)`, with the id
  `<id>-copy`; the copy opens on the canvas at once and is born a **test pipeline** (CONTEXT.md
  « Pipeline validé »: the flask with 0 in the tab bar).
- **Undo/redo** of a structural edit (#226 / ADR-0014): one rename is one step, Undo only undoes
  this tab's own changes, and the toolbar's Undo tooltip names the change (« Undo move 2 nodes »).
- **Group move** (#232): select several nodes, drag once, every selected node moves.
- **Node inspector**: the node id with « Copy id », the prompt editor, the declared **Outputs** with
  their schema editor, and the **Inputs** derived from incoming edges, pooled as `name ← a, b`
  (« Not wired » when there is none) (#149 / #153).
- **Edge detail panel** (#147): its **Outputs** section (one checkbox per declared output of the
  source), its **Conditions** section (Else switch, « Add condition », one row = Output, Field,
  Operator, Value; a boolean field writes canonical `true`/`false`), its **Routing** section
  (« Manually pinned · N waypoints », « Re-route automatically »).
- **Lint banner** renders **exactly once**, on the canvas, never again in the inspector (#63).
- **Shared draft** (ADR-0088): an unsaved edit survives a page reload; the Pipelines row says
  `draft: N changes` and the canvas banner « Draft · N changes since vM ».
- **Pipeline versions** (ADR-0088): the Save window, then Pipeline info › **Versions** — « View vN »
  opens that version read-only, « Restore vN into the draft » puts it back in the draft.
- **Validate** (CONTEXT.md « Pipeline validé »): next to Save, its modal turns the test pipeline into
  a validated one, and the tab bar's indicator follows.

## Preconditions

- The app (the core alone, no extension) is running locally and reachable in a browser; the status
  bar shows « Daemon: connected ». See `docs/agents/run-scenario.md` for how to start a demo daemon
  and drive it; no hard-coded ports, ids or paths in the journey.
- The **Pipelines** tab lists at least one pipeline with **two agent nodes or more**. A fresh
  instance has none: seed one (a YAML under the demo `HOME`'s `~/.pdo/pipelines/`, or « New
  pipeline » in the Pipelines tab and two nodes added by hand). Step 5 needs a **boolean** field on
  the source's output: declare it `type: bool` in the seed's output schema (`boolean` is read as the
  same type).

## Journey

1. Open the app → open the **Pipelines** tab on the left. Each row reads on two lines: the name and
   its validation indicator, then `vN`, `draft: N changes` when there are some, and the edit date.
2. **Duplicate** the seeded pipeline (the row's hover action « Duplicate pipeline », or ⋯ ›
   « Duplicate ») → a row `<name> (copy)` appears, and the copy **opens on the canvas** in its own
   tab `<id>-copy.yaml`. Continue the journey on that copy.
3. **Add a node**: the toolbar's `+` (« Add ») › « Node » → a new agent card appears: a header with
   its type icon and name, a meta grid with its agent, its worktree (a fork icon and `isolated`, the cell's
   tooltip « Worktree: isolated ») and one line per output (`out`). The card carries no id text. Select it → the **node inspector** shows its id, its prompt
   editor, its Outputs and its Inputs (« Not wired »).
4. **Draw an edge**: press **anywhere on the source card's border** (there is no output dot — the
   whole rim is the drag source) and drag. The preview grows **on the wiring grid**, one orthogonal
   segment per crossed cell, a cursor badge reading `snap ×1` (or `snap ×N · Mpx`); hold **Shift**
   mid-gesture → the badge reads `free` and the trace leaves the grid; release Shift → the grid
   re-originates on the current point. Drop on another card's **body** → an edge is created,
   carrying the source's **first declared output**, landing **perpendicular** to the side it was
   aimed at. The edge lands on the target's first declared input: a card made with `+` › Node
   declares `in`, so the edge panel's header reads `<source>.out → <target>.in` and the target's
   **Inputs** section, in the node inspector, reads `in ← <source>`. Only a target that declares
   no input gets one named after the output the edge carries. The toolbar Undo's tooltip names the
   new edge (« add edge »).
   Then **reshape it**: select the edge, drag a middle segment → it snaps to the grid (Shift frees
   it) and the neighbouring waypoints do not move; drag it until it lines up with a neighbour → the
   waypoint between them is **merged away** on release. **Right-click on a segment handle does
   nothing**; right-click on the edge offers « Delete edge ».
5. **Author a conditional edge**: click an edge → the **edge detail panel** opens; in **Conditions**,
   « Add condition », pick the output, a field, an operator and a value → the condition pill appears
   at the edge's midpoint, always visible. Tick another output in the **Outputs** section → the edge
   carries both.
6. **Materialize a loop region**: draw a back-edge that closes a cycle over two nodes or more → a
   translucent bounded region appears, its corner reading `max 5` with a loop icon. Click the corner
   → the **region inspector** opens; change `max_iter` in its **Bound** section → the corner follows.
7. **Group-move**: select several nodes (Shift+drag a box on empty canvas, or Ctrl/Cmd+click each)
   and drag them together → all move; unselected nodes stay put.
8. **Undo** the move (Ctrl/Cmd+Z or the toolbar Undo, whose tooltip names the move) → the nodes go
   back; **redo** (Ctrl/Cmd+Y, Ctrl/Cmd+Shift+Z or the toolbar Redo) → they move again. Rename a node
   (type a name, Enter) and undo → the whole name reverts in one step.
9. **Reload the page without saving**, reopen the copy from the Pipelines tab → every edit from
   steps 3–8 is there, the tab is still dirty, the row reads `draft: N changes` and the canvas banner
   « Draft · N changes since vM ». Undo and Redo are disabled now: the undo history lives in the
   tab, in memory, and the reload opened a new tab (CONTEXT.md « Undo/redo »).
10. **Save**: the tab bar's « Save… » (or Ctrl/Cmd+S) opens the window « Save `<name>` »: « Save as »
    offers « New version vN+1 » (the default), « Keep vN apart » and « Replace vN », with a Title
    field. Confirm with « Save vN+1 » → a toast « Saved … », the tab's unsaved dot clears, « Saved
    just now » shows, the Save button disables and the draft banner is gone.
11. Open **Pipeline info** (the toolbar's ⓘ button; with nothing selected the right pane is
    closed) › **Versions** → the new version is
    listed. « View vN » on the previous one → a read-only tab `<id>-copy.yaml @ vN` opens without the
    edits; close it.
12. **Validate**: the tab bar's « Validate » (next to Save) opens its modal; tick « All projects
    (current and future) » and « Save validation » → the tab bar's indicator turns from the flask
    (0) to the globe « All ».

## Checks

### Surface

- After steps 3–7: the canvas reflects each edit (new card, edge with its arrow on the target's body,
  condition pill, loop region with its corner).
- During step 4: the wiring grid shows only while a wire is being drawn (dots by default —
  Settings › Appearance › Interface › « Wiring grid »: Hidden / Dots / Lines), and the snap
  indicator is a filled ring on the grid, a hollow dashed one while Shift frees the trace.
- After step 8: one undo reverts exactly one change (a typed rename is one step, not one per
  keystroke); redo restores it.
- After step 9: nothing was lost by the reload, and nothing was written yet: the YAML on disk is
  still the duplicate's.
- After step 10: unsaved dot gone, « Saved just now » visible, Save disabled, no draft banner.
- After step 12: the copy reads « All » in the tab bar and on its row of the Pipelines tab.
- The lint banner, when the pipeline has a warning, is drawn once, on the canvas.
- The browser console shows no uncaught error along the journey.

### Internals

- The duplicated pipeline is a YAML of its own under the demo `HOME`'s `~/.pdo/pipelines/`
  (`<id>-copy.yaml`, its prompts under `<id>-copy.prompts/<node>.md`), its `name:` suffixed
  `(copy)`; the original's file is untouched.
- After step 10, that YAML (and its `*.prompts/<node>.md` sidecar) reflects the edits:
  - the conditional edge's boolean value is canonical (`true`/`false`, not `"true"`);
  - the drawn edge writes its `source_anchor` / `target_anchor` (`{side, offset}`), and `mode:
    manual` with grid-aligned `waypoints` when the trace has bends — the wiring travels with the
    file;
  - the loop region carries the `max_iter` set in step 6.
- `GET /pipelines` gives the copy a version number one higher than before the Save and, after
  step 12, the validation `{"kind":"all"}`.

## Cleanup (best-effort)

- Delete the copy (the row's ⋯ › Delete, or its `*.yaml` + `*.prompts/` dir) and any pipeline the
  agent seeded.

## Notes

- **Emergent inputs are read-only.** Inputs are derived from incoming edges (named after the source
  document); the source of truth is the **edge**, not the node — never edit an input directly.
- **A tab accepts no edit until its draft has synced** (a fraction of a second after it opens): an
  edit typed in that window is dropped by design, not lost by the draft.
- **The draft is not the file.** A reload proves the draft (stored by the daemon); only the Save
  writes the YAML and a version. Asserting the file before step 10 is a false finding.
- **A file written outside PDO merges into the draft** rather than wiping it: the notice reads
  « `<id>.yaml` changed outside · kept as vN », the local edits and the dirty state stay, and no
  conflict window opens. The undo history is the tab's, in memory: a page reload clears it (the
  draft stays), and so does a draft the daemon starts afresh.
- **Undo history survives a Save**: the Save only moves the draft's baseline.
- The edit toolbar has **no Switch, Loop, ForEach nor Merge button** — routing lives on edges,
  loops are regions drawn as cycles, a convergence is any node fed by several edges.
- A loop over a **single** node is a badge on its card, not a region box: step 6 needs two nodes.
- Nothing in the journey names the product or shows its logo: the brand shown in the window is
  out of scope here.
