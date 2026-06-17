# Scenario — `manual-edge-routing`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Verifies orthogonal edge
> routing + manual waypoints + shareable persistence (issue #154, PRD #143,
> design screen 14 "Edge shaping interaction"):
>
> - Edges render as **right-angle (orthogonal) connectors**, not bezier curves.
> - An auto edge **re-routes** when either endpoint node is moved.
> - Hovering an edge reveals **perpendicular-only segment handles**; the first
>   manual drag **pins** the route to persisted waypoints (`mode: manual`).
> - A per-edge **"Re-route automatically"** action in the edge detail panel
>   clears the waypoints back to `mode: auto`.
> - `mode` + `waypoints` persist **inside the pipeline file** so the routing
>   travels when a workflow is shared.
> - A layout-only change (move a node, nudge a waypoint) is **excluded from the
>   semantic pipeline-diff**: the star indicator does NOT flip to "diverged".

## Setup

- PDO daemon running on the user's repo (`pdo daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser.
- A pipeline `manual-edge-routing.yaml` exists in `.pdo/pipelines/`. If it
  isn't already there, the agent creates it before driving the UI. The two work
  nodes are stacked vertically and offset so the straight line between them
  would cross the canvas at an angle — making the right-angle routing obvious:

  ```yaml
  name: manual-edge-routing
  version: "1.0"
  nodes:
    - id: start
      name: Start
      type: start
      inputs: []
      outputs:
        - name: user_prompt
      view: { x: 0, y: 160 }
    - id: reviewer
      name: reviewer
      type: doc-only
      inputs:
        - name: task
          side: left
      outputs:
        - name: verdict
          side: right
      view: { x: 240, y: 40 }
    - id: implementer
      name: implementer
      type: code-mutating
      inputs:
        - name: verdict
          side: left
      outputs:
        - name: code
          side: right
      view: { x: 560, y: 300 }
    - id: end
      name: End
      type: end
      inputs:
        - name: result
          side: left
      outputs: []
      view: { x: 860, y: 160 }
  edges:
    - source: { node: start, port: user_prompt }
      target: { node: reviewer, port: task }
    - source: { node: reviewer, port: verdict }
      target: { node: implementer, port: verdict }
    - source: { node: implementer, port: code }
      target: { node: end, port: result }
  ```

## Steps

1. **Open Edit mode** — click the pencil icon in the top bar.
2. **Load the pipeline** — select `manual-edge-routing` from the pipelines list.
   Verify the canvas renders and note the **star indicator** state (top-right of
   the canvas). It should read `synced` (or `outline` if never starred) — NOT
   `diverged`.
3. **Verify orthogonal routing** — every edge is drawn with **right-angle
   bends** (horizontal/vertical segments only), not a smooth bezier curve. Take a
   screenshot. In particular `reviewer → implementer` (the offset pair) shows a
   clear step shape, not a diagonal.
4. **Move a node, observe auto re-route** — drag `implementer` to a new position.
   The `reviewer → implementer` edge **re-computes** its right-angle path to the
   new location (the bends move; the edge stays orthogonal). Confirm the star
   indicator does **NOT** flip to `diverged` from the move alone.
5. **Hover an edge, reveal handles** — hover the `reviewer → implementer` edge.
   **Perpendicular-only segment handles** appear on its segments (a handle on a
   horizontal segment drags vertically; a handle on a vertical segment drags
   horizontally). Take a screenshot.
6. **Drag a segment to pin the route** — drag one segment handle. The edge route
   bends to follow the drag and is now **pinned**: re-fetch the pipeline file
   (`cat .pdo/pipelines/manual-edge-routing.yaml`, or
   `curl http://127.0.0.1:5172/pipelines/<id>` after the autosave) and confirm
   the corresponding edge now carries `mode: manual` and a non-empty `waypoints`
   list.
7. **Re-anchor an endpoint** — drag the edge's endpoint to a different point on
   the `implementer` node body. The arrow now lands at the new spot; the edge
   stays orthogonal.
8. **Open the edge detail panel** — click the `reviewer → implementer` edge. The
   panel shows a **Routing** section reading **"Manually pinned"** with the
   waypoint count, and a **"Re-route automatically"** button.
9. **Re-route automatically** — click **"Re-route automatically"**. The edge
   reverts to the computed right-angle path; the Routing section now reads
   **"Automatic"**. Re-fetch the pipeline file and confirm the edge no longer
   carries `waypoints` (and `mode` is `auto` or absent).
10. **Save and reload** — re-pin the route (repeat step 6), then reload the
    pipeline (refresh the browser, or re-open the pipeline tab). The pinned
    waypoints are **preserved**: the edge renders along the same pinned path, and
    the detail panel still reads "Manually pinned".
11. **Confirm layout is not "dirty"** — with only layout changes applied (node
    moves + waypoint pins), confirm the star indicator reads `synced` against its
    library twin (if the pipeline is starred). Two pipelines differing only in
    node positions and edge `mode`/`waypoints` compare **equal** semantically.

## Verdict format

```json
{
  "verdict": "PASS",
  "evidence": [
    "step 3: edge reviewer→implementer rendered with right-angle bends (screenshot)",
    "step 6: pipeline YAML edge now carries mode: manual + waypoints: [...]",
    "step 10: after reload, pinned waypoints preserved, panel reads 'Manually pinned'"
  ],
  "anomalies": []
}
```

### Pass criteria

- Edges render as right-angle (orthogonal) connectors, not bezier curves.
- An auto edge re-routes its path when an endpoint node moves.
- Hovering an edge reveals perpendicular-only segment handles; the first drag
  pins the route to `mode: manual` waypoints.
- The per-edge "Re-route automatically" action resets the edge to `mode: auto`
  and clears its waypoints.
- `mode` + `waypoints` round-trip through the pipeline file (save and reload
  preserves the pinned route).
- A layout-only change (node move, waypoint pin) does NOT mark the pipeline as
  diverged / dirty against its library twin (excluded from the semantic diff).
- No console errors or rendering glitches.
