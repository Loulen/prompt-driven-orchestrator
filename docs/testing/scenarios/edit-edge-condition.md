# Scenario — `edit-edge-condition`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies the **edge detail
> panel** (issue #147, design screen 02): clicking an edge opens a panel that
> shows the route (`source.port → target`) and authors the edge's `when:` clause
> as field / operator / value. When the chosen field is **boolean**, the value
> is a true/false toggle (not a text box) and the written value is canonical
> `true` / `false`. The enclosing region's `iter` counter is selectable as a
> condition field. At runtime the same panel — and only here, never on the
> canvas — shows the edge's trigger status (fired, last value, evaluated-at,
> iter). Covers ADR-0011 (conditional edges replace Switch) and replaces the old
> Switch detail panel.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser.
- `claude` available on `PATH`.
- A pipeline `edit-edge-condition.yaml` exists in `.maestro/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI:

  ```yaml
  name: edit-edge-condition
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 200 }
    - id: reviewer
      name: reviewer
      type: doc-only
      inputs:
        - name: task
          side: left
      outputs:
        - name: verdict
          side: right
          frontmatter:
            verdict:
              type: enum
              allowed: [PASS, FAIL, NEEDS_WORK]
            is_blocking:
              type: bool
      view: { x: 260, y: 200 }
    - id: implementer
      name: implementer
      type: code-mutating
      inputs:
        - name: review
          side: left
      outputs:
        - name: diff
          side: right
      view: { x: 560, y: 80 }
    - id: archiver
      name: archiver
      type: doc-only
      inputs:
        - name: review
          side: left
      outputs:
        - name: note
          side: right
      view: { x: 560, y: 320 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 860, y: 200 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: reviewer, port: task }
    - source: { node: reviewer, port: verdict }
      target: { node: implementer, port: review }
    - source: { node: reviewer, port: verdict }
      target: { node: archiver, port: review }
    - source: { node: implementer, port: diff }
      target: { node: end, port: result }
    - source: { node: archiver, port: note }
      target: { node: end, port: result }
  ```

  Note: the two `reviewer.verdict → …` edges start **unconditional** on purpose —
  the agent authors their `when:` clauses through the panel in the steps below.

## Steps

1. **Open Edit mode** — click the pencil icon in the top bar.
2. **Load the pipeline** — select `edit-edge-condition` from the pipelines list.
   Verify the canvas renders all five nodes and the five edges, with **no**
   condition pills (every edge is unconditional at this point).
3. **Select the `reviewer.verdict → implementer` edge** — click the edge on the
   canvas. The right panel opens the **edge detail panel** (not a node inspector):
   - The header shows the route `reviewer.verdict → implementer`.
   - A `when:` editor is present with a field dropdown, an operator dropdown, and
     a value control.
4. **Author an enum condition** — in the field dropdown pick `verdict`, operator
   `=` (`eq`), value `FAIL` (the enum exposes its allowed values). Save (Cmd/Ctrl
   +S). Confirm:
   - The canvas edge `reviewer → implementer` now shows a condition pill reading
     `verdict = FAIL` (or the `when:` shape).
   - `.maestro/pipelines/edit-edge-condition.yaml` on disk now has
     `when: { verdict: { eq: FAIL } }` on that edge.
5. **Select the `reviewer.verdict → archiver` edge** and author a **boolean**
   condition:
   - In the field dropdown pick `is_blocking` (a `bool` field).
   - Confirm the value control is a **true/false toggle**, not a free-text input.
   - Toggle the value to `true`. Save.
   - Confirm `.maestro/pipelines/edit-edge-condition.yaml` now has
     `when: { is_blocking: { eq: true } }` on that edge — the value is canonical
     `true` (a YAML boolean), not the string `"true"`, `1`, or `True`.
6. **Confirm `iter` is selectable** — with an edge selected, open the field
   dropdown and verify `iter` appears as a selectable field (labelled as the
   enclosing region's counter). This lets an exhaust-exit such as `iter ≥ max` be
   authored.
7. **Switch to Run mode** and **create a new run** of `edit-edge-condition` with a
   prompt that drives the reviewer to a blocking `FAIL` verdict.
8. **Observe runtime trigger status in the panel** — once `reviewer` completes,
   select the `reviewer.verdict → implementer` edge again. The edge detail panel's
   **Runtime** section shows the edge's trigger status: whether it **fired**, its
   **last evaluated value** (e.g. `verdict = FAIL`), the **evaluated-at** time,
   and the **iter**.
9. **Confirm trigger status is panel-only** — verify the canvas itself never
   renders fired / not-fired state on the edge; that information appears **only**
   in the edge detail panel (design screen 02).

## Verdict format

```
scenario: edit-edge-condition
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- Clicking an edge opens an **edge detail panel** showing the route
  (`source.port → target`) and a field / operator / value editor for `when:`.
- A `bool` field renders a **true/false toggle** (not a text box); the value
  written to YAML is canonical `true` / `false`.
- An enum field exposes its allowed values; the authored clause persists to the
  pipeline YAML on disk in the `when: { field: { op: value } }` shape.
- `iter` is selectable as a condition field.
- At runtime the panel shows the edge's trigger status (fired, last value,
  evaluated-at, iter), and this status is **not** rendered on the canvas.
- The old Switch detail panel no longer appears anywhere (Switch has been
  removed).
- No console errors or rendering glitches.
