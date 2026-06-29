---
id: HP-01
covers: [pipeline, node, edge, conditional-edge, loop-region, library]
---

# HP-01 — Author & save a pipeline

## Goal

A user opens a pipeline from the **Library**, edits it on the single **unified canvas** (add a node,
draw edges, author a conditional edge, materialize a bounded loop region, rearrange nodes), and
**saves**. The edits persist to disk faithfully and survive the daemon's pipeline-watcher broadcast —
the core "shape a pipeline before running it" loop.

## Drive-by

Features validated while crossing the editing screens (grafted from retired per-issue scenarios):

- **Undo/redo** of a structural edit, with typed-text coalescing (one rename = one step) and native
  field-undo shielding (#226 / ADR-0014).
- **Group move**: box-/additive-select several nodes, drag once, every selected node persists (#232).
- **Node inspector** surfaces the node id, the prompt editor, declared **output port schemas**, and
  the **derived inputs** list with pooling spelled out (#149 / #153).
- **Edge detail panel** authors a `when` clause; a boolean field renders a true/false toggle and
  writes canonical `true`/`false` (#147).
- **Lint banner** renders **exactly once** (the canvas overlay, never duplicated in the inspector) (#63).
- **Library duplicate**: a library-only template duplicates into an **unlinked clone** that preserves
  unknown YAML keys and comments verbatim, name suffixed `(copy)` (#224).

## Preconditions

- The app is running locally and reachable in a browser; status bar shows the daemon **connected**.
- The pipeline library has at least one pipeline (or the agent seeds a small one). No hard-coded
  ports, ids, or paths in the journey — see `docs/agents/run-scenario.md` for how to drive PDO.

## Journey

1. Open the app → the **Library** lists the available pipelines.
2. **Duplicate** a library-only pipeline → an unlinked copy appears, name suffixed `(copy)`, opened on
   the canvas. (Duplicate is offered only on library-only rows, never on starred working rows.)
3. **Add a node** from the edit toolbar → a new slim card appears (type icon + name + code/doc marker,
   no id text, no `interactive` badge).
4. **Draw an edge**: drag from a node's green **output dot** and drop on another node's **card body**
   → an emergent input edge is created, named after the source document (there is no input dot to aim at).
5. **Author a conditional edge**: click an edge → the **edge detail panel** opens; add a `when`
   condition (pick a field, operator, value) → the always-visible condition pill appears at the edge
   midpoint.
6. **Materialize a loop region**: draw a back-edge that closes a cycle → a translucent bounded region
   with a `↻ X/Y` header appears; its `max_iter` is editable from the header / region inspector.
7. **Group-move**: box-select several nodes and drag them together → all move; unselected nodes stay put.
8. **Undo** the last structural edit (Ctrl/Cmd+Z or the toolbar Undo) → the edit reverts; **redo**
   reapplies it.
9. **Save** (Save button or Ctrl/Cmd+S) → the dirty `•` indicator clears, "Saved just now" shows, the
   Save button disables.
10. **Reload** the page and reopen the pipeline → every edit from steps 3–8 is present.

## Checks

### UI

- After step 3–7: the canvas reflects each edit (new node, edge with arrow on the body, condition pill,
  loop region box with `↻` header).
- After step 8: undo reverts exactly one structural step (a typed rename undoes as one step, not per
  keystroke); redo restores it.
- After step 9: dirty `•` gone, `saved-ago` text visible, Save disabled.
- After step 10: reopened pipeline shows the persisted edits (no silent droppage).

### Backing store

- The saved `*.yaml` (and `*.prompts/<node>.md` sidecar) on disk reflects the edits; a duplicated
  pipeline's YAML is byte-faithful **except the `name:` line** (unknown keys and comments survive).
- The conditional edge's boolean value is written canonical (`true`/`false`, not `"true"`).
- A change that is **layout-only** (node position, edge waypoints) does **not** flip the pipeline to
  "diverged" against its library twin.

## Cleanup (best-effort)

- Delete any pipeline the agent seeded/duplicated (`*.yaml` + its `*.prompts/` dir).

## Notes

- **Emergent inputs are read-only.** Inputs are derived from incoming edges (named after the source
  document); the source of truth is the **edge**, not the node — never edit an input directly.
- **Undo history survives a Save** but is **cleared by a clean external hot-reload** (the watcher
  re-parsing the file from disk), not by a dirty auto-save.
- The pipeline-watcher broadcast must **not** wipe unsaved in-memory edits (wait out the debounce
  window and confirm the textarea still holds the unsaved value, tab still dirty, disk unchanged).
- The edit toolbar has **no Switch and no Loop button** — routing lives on edges, loops are regions
  drawn as cycles.
