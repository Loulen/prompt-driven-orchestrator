# Scenario — `render-mermaid-artifact`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent drives a real browser
> (Chrome DevTools MCP preferred; Playwright MCP fallback) and emits the verdict
> format below. Asserts #240: a ` ```mermaid ` fenced block in an artifact's
> markdown body renders as an inline **SVG diagram** in `MarkdownArtifactModal`,
> styled to the dark palette; invalid mermaid **degrades gracefully** to the raw
> `<pre><code>` source (never a blank pane, never a thrown error); and the security
> posture (ADR-0013: `securityLevel: 'strict'`, no script execution) holds.
>
> jsdom cannot execute mermaid (no `getBBox`), so this Layer-5 + the Playwright
> spec `frontend/e2e/render-mermaid-artifact.spec.ts` are the only layers that
> exercise the *real* render path. Treat this scenario as the human-equivalent
> acceptance gate.

## Setup

- A PDO daemon running on the user's repo. Discover the port — commonly
  `http://127.0.0.1:5172` (debug dev), `6160` (installed prod), or `6172`; find it
  with `ss -ltnp | grep -i pdo` and use that base URL for both the browser and the
  `POST /runs` call.
- Frontend reachable in a browser at that base URL (the daemon serves the embedded
  `frontend/dist`). **The build under test must already include the #240 changes** —
  if validating a local branch, rebuild the frontend and re-embed before driving the
  UI (the daemon serves the *embedded* bundle, not the vite dev server, unless you
  point the browser at a vite dev port).
- `WORKSPACE_ROOT` = the repo the daemon runs against (where `.pdo/` lives).

### Seed a pipeline + run + artifacts

The scenario does not need a live Claude session — it seeds the artifact files on
disk directly (the same seam the e2e suite uses). Steps:

1. Write `.pdo/pipelines/render-mermaid-scenario.yaml`:

   ```yaml
   name: render-mermaid-scenario
   version: "1.0"
   nodes:
     - id: diagrammer
       name: diagrammer
       type: doc-only
       prompt_file: render-mermaid-scenario.prompts/diagrammer.md
       outputs:
         - name: good       # valid flowchart → must render an <svg>
         - name: complex    # valid sequence diagram w/ labels → render + dark theme
         - name: bad         # invalid syntax → must fall back to <pre><code>
         - name: xss          # hostile payload → must NOT execute script
       view: { x: 200, y: 200 }
   edges: []
   ```

   And `.pdo/pipelines/render-mermaid-scenario.prompts/diagrammer.md` with any
   one-line body (e.g. `Draw diagrams.`).

2. `POST {baseURL}/runs` with JSON body `{"pipeline": "render-mermaid-scenario",
   "input": "mermaid layer5"}`. Expect `201`; capture `run_id` from the response.

3. Seed the four artifacts. **Path convention is
   `.pdo/runs/<run_id>/worktree/.pdo/artifacts/<node>/iter-<n>/<port>/output.md`** —
   the per-port subdirectory + `output.md`, NOT a flat `<port>.md` (the older e2e
   specs use the stale flat form; do not copy them):

   - `diagrammer/iter-1/good/output.md`:
     ````markdown
     ## Flow

     ```mermaid
     graph TD;
       A[Start] --> B{Decision};
       B -->|yes| C[Ship];
       B -->|no| D[Iterate];
     ```
     ````
   - `diagrammer/iter-1/complex/output.md`:
     ````markdown
     ## Sequence

     ```mermaid
     sequenceDiagram
       participant U as User
       participant D as Daemon
       U->>D: POST /runs
       D-->>U: 201 run_id
     ```
     ````
   - `diagrammer/iter-1/bad/output.md`:
     ````markdown
     ## Broken

     ```mermaid
     this is not ::: valid mermaid @@@ ->> nonsense
     ```
     ````
   - `diagrammer/iter-1/xss/output.md` — a payload that, pre-fix or under a weak
     security level, would execute script; under ADR-0013's `strict` it must render
     inertly or fall back, **never** pop a dialog:
     ````markdown
     ## Hostile

     ```mermaid
     graph LR
       A["<img src=x onerror='window.__mermaidXss=1'>"] --> B[B]
     ```
     ````

## Steps the agent executes — render path

1. Open the UI; confirm the **`Daemon: connected`** label is visible in the status
   bar.
2. Click the run row for `run_id` (the list shows the first 8 chars —
   `run_id.slice(0,8)`).
3. Wait for the canvas (`.react-flow`) to render; click the **`diagrammer`** node.
4. Wait for the **`Outputs`** section; the four ports (`good`, `complex`, `bad`,
   `xss`) appear as clickable `button.port-row` cards (a card is only clickable once
   its file exists on disk — it does, from Setup).
5. **Valid flowchart (`good`):** click the `good` port card. In the modal
   (`.artifact-markdown`):
   - Assert an element `[data-testid="mermaid-diagram"]` is visible **and contains an
     `<svg>`** with non-zero width and height (a real rendered diagram, not the raw
     fence). Read its bounding box to confirm `width > 0 && height > 0`.
   - Assert the modal does **not** show the literal text `graph TD` as code (i.e. the
     fence was consumed, not printed verbatim).
   - Assert no console error was thrown by mermaid (check the devtools console; a
     benign >500 kB chunk warning at load is acceptable, a thrown render error is not).
   - Close the modal (Escape).
6. **Sequence diagram + dark theme (`complex`):** open the `complex` port card.
   - Assert `[data-testid="mermaid-diagram"] svg` is visible with non-zero box.
   - Assert the diagram is themed dark, not the mermaid light default: sample a node
     rectangle's computed `fill` (or the SVG root/background) and confirm it is a dark
     palette colour (e.g. matches `#1a1e25`/`#14171d` family), **not** white
     `#ffffff`/`#ECECFF` (mermaid's default light fills). Screenshot for the evidence
     trail.
   - Close the modal.
7. **Invalid syntax → graceful degrade (`bad`):** open the `bad` port card.
   - Assert the modal is **not blank** and shows the raw source as a fallback: either
     `[data-testid="mermaid-error"]` is visible, **or** the raw fence text
     (`this is not ::: valid mermaid`) is shown inside a `.artifact-markdown pre code`.
   - Assert **no** `[data-testid="mermaid-diagram"] svg` rendered for this port.
   - Assert no uncaught exception reached the console (the failure was caught and
     degraded, per ADR-0013).
   - Close the modal.

## Steps — security (ADR-0013 strict)

8. **Hostile payload (`xss`):** before opening, in the devtools console set a probe
   baseline and ensure `window.__mermaidXss` is `undefined`.
9. Open the `xss` port card. Wait for the modal to settle (render or fallback).
10. Assert **no JS executed from the payload**:
    - `window.__mermaidXss` is still `undefined` (the `onerror` never fired).
    - No `alert`/`dialog` was triggered (no pending dialog handler hit).
    - Either a diagram rendered with the label as **inert text** (the `<img onerror>`
      neutralised by mermaid's strict DOMPurify) or it fell back to `<pre><code>` —
      both are acceptable; script execution is not.
11. Close the modal.

## Negative control — plain markdown still works

12. (Optional, if a non-mermaid markdown port is handy, or reuse an existing run's
    `.md` output.) Open any artifact whose body has **no** mermaid fence. Assert it
    renders as normal markdown (headings, lists, code blocks in other languages shown
    as plain `<pre><code>`), and that a regular ` ```bash ` or ` ```ts ` fence is
    **not** swallowed by the mermaid path — it must still appear as a code block.
    This proves the `pre`/`code` override only intercepts `language-mermaid`.

## Cleanup

- Delete `.pdo/pipelines/render-mermaid-scenario.yaml` and the
  `render-mermaid-scenario.prompts/` dir.
- Optionally archive/clean the seeded run (`POST {baseURL}/runs/<run_id>/archive`
  or the project's cleanup path) so it doesn't linger in the Runs list. The daemon
  may have spawned a `pdo-<run_id>-diagrammer-iter-1` tmux session under the stubbed
  command — kill it with `tmux kill-session -t pdo-<run_id>-diagrammer-iter-1` if
  present (kill-session by full name only, never `kill <pid>` — per the reaper rule).

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "setup: pipeline seeded, run <id> created (201), 4 artifacts written at <port>/output.md paths",
    "step 1: status bar shows 'Daemon: connected'",
    "step 5: 'good' port renders [data-testid=mermaid-diagram] > svg with non-zero bbox; no raw 'graph TD' text; no thrown console error",
    "step 6: 'complex' renders an svg themed dark (node fill ~#1a1e25, not #ffffff/#ECECFF)",
    "step 7: 'bad' degrades to raw <pre><code> (or [data-testid=mermaid-error]); no svg; no uncaught exception",
    "step 10: 'xss' — window.__mermaidXss undefined, no dialog fired; label inert or fell back",
    "step 12: a non-mermaid ```ts fence still renders as a normal code block (override is mermaid-only)"
  ],
  "anomalies": [
    "<optional — e.g. diagram overflowed the 560px modal without horizontal scroll; light-theme fill leaked; chunk-size warning>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. The security
assertion (step 10) and the graceful-degrade assertion (step 7) are non-negotiable:
either failing is an automatic `FAIL` regardless of the happy path.
