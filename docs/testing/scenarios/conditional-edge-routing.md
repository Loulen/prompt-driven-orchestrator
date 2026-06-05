# Scenario — `conditional-edge-routing`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies that conditional
> routing lives on the **edge** (not a Switch node): a producer's artifact fans
> out to **all** matching guarded edges (multi-match), an `else` edge catches the
> unmatched case, and condition pills are always visible at each conditional
> edge's midpoint on the canvas. Covers ADR-0011 (conditional edges replace
> Switch) and issue #144.

## Setup

- Maestro daemon running on the user's repo (`maestro daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser.
- `claude` available on `PATH`.
- A pipeline `conditional-edge-routing.yaml` exists in `.maestro/pipelines/`. If
  it isn't already there, the agent creates it before driving the UI:

  ```yaml
  name: conditional-edge-routing
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 220 }
    - id: classifier
      name: classifier
      type: doc-only
      inputs:
        - name: task
          side: left
      outputs:
        - name: triage
          side: right
          frontmatter:
            severity:
              type: enum
              allowed: [low, medium, high]
            security:
              type: bool
      view: { x: 260, y: 220 }
    - id: hotfix
      name: hotfix
      type: code-mutating
      inputs:
        - name: triage
          side: left
      outputs:
        - name: patch
          side: right
      view: { x: 560, y: 60 }
    - id: security-review
      name: security-review
      type: doc-only
      inputs:
        - name: triage
          side: left
      outputs:
        - name: review
          side: right
      view: { x: 560, y: 220 }
    - id: backlog
      name: backlog
      type: doc-only
      inputs:
        - name: triage
          side: left
      outputs:
        - name: note
          side: right
      view: { x: 560, y: 380 }
    - id: merge1
      name: merge
      type: merge
      inputs:
        - name: branches
          side: left
          repeated: true
      outputs:
        - name: merged
          side: right
      view: { x: 880, y: 220 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 1140, y: 220 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: classifier, port: task }
    # Multi-match fan-out: a high-severity security issue fires BOTH guarded
    # edges leaving the `triage` port (hotfix AND security-review).
    - source: { node: classifier, port: triage }
      target: { node: hotfix, port: triage }
      when:
        severity: { eq: high }
    - source: { node: classifier, port: triage }
      target: { node: security-review, port: triage }
      when:
        security: { eq: true }
    # else: fires only when NO sibling guarded edge on `triage` matched.
    - source: { node: classifier, port: triage }
      target: { node: backlog, port: triage }
      else: true
    - source: { node: hotfix, port: patch }
      target: { node: merge1, port: branches }
    - source: { node: security-review, port: review }
      target: { node: merge1, port: branches }
    - source: { node: backlog, port: note }
      target: { node: merge1, port: branches }
    - source: { node: merge1, port: merged }
      target: { node: end, port: result }
  ```

## Steps

1. **Open Edit mode** — click the pencil icon in the top bar.
2. **Load the pipeline** — select `conditional-edge-routing` from the pipelines
   list. Verify the canvas renders:
   - The three guarded edges leaving `classifier:triage` each show an
     **always-visible condition pill** at their midpoint:
     - `classifier → hotfix`: pill reads `severity = high` (or `when:` shape).
     - `classifier → security-review`: pill reads `security = true`.
     - `classifier → backlog`: pill reads `else`.
   - Unconditional edges (e.g. `start → classifier`, `*→ merge`) show **no**
     pill.
3. **Inspect a conditional edge** — confirm the pill stays visible without
   hovering or selecting the edge (it is part of the derived edge, not a hover
   affordance).
4. **Switch to Run mode** — click the pencil icon to exit edit mode.
5. **Create a new run** — click "New Run", select `conditional-edge-routing`,
   enter a prompt, and start the run.
6. **Observe the classifier** — `classifier` spawns and runs. When it completes,
   its `triage` artifact carries a `severity` and `security` frontmatter.
7. **Observe multi-match routing** — with `severity: high` and `security: true`
   on the artifact:
   - **Both** `hotfix` and `security-review` spawn (multi-match fan-out — no
     first-match short-circuit).
   - `backlog` does **not** spawn (an `else` edge fires only when no sibling on
     the same source port matched).
8. **Observe the `else` fallback** — re-run (or edit the prompt) so the artifact
   is `severity: low`, `security: false`:
   - Neither `hotfix` nor `security-review` spawns.
   - `backlog` spawns (the `else` edge catches the unmatched case).
9. **Observe convergence** — the fired branches converge on `merge1`, which fires
   once all its received branches complete, then the run reaches `end` and
   completes.

## Verdict format

```
scenario: conditional-edge-routing
result: PASS | FAIL
notes: <free-form observations>
```

### Pass criteria

- Each conditional edge shows an always-visible condition pill at its midpoint
  (`when:` clause for guarded edges, `else` for the fallback). Unconditional
  edges show none.
- At runtime, an artifact matching multiple guarded edges fans out to **all**
  matching targets (multi-match, no first-match ordering).
- An `else` edge fires **iff** no sibling guarded edge on the same source port
  matched.
- The fan-out converges via the `Merge` node and the run completes via the
  `merged → end` edge.
- No console errors or rendering glitches.
