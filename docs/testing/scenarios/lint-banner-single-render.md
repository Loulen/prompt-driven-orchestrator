# Scenario — `lint-banner-single-render`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> and emits the verdict format below. Asserts that the pipeline-wide lint
> diagnostics banner (`data-testid="lint-banner"`) renders **exactly once** when
> no node is selected — the regression in bug #63, where the banner rendered
> twice (the floating canvas overlay in `EditCanvas.tsx` **and** a duplicate in
> `PipelineInspector.tsx`). After the fix the canvas overlay is the single home;
> the inspector renders no banner. This scenario validates the rendered DOM in a
> real browser, the way a human would eyeball it.

## Why a browser test (not just unit)

The duplication is an emergent property of two sibling components being mounted
at once (`EditCanvas` centre panel + `PipelineInspector` right panel). The
isolated component unit tests cannot see it. This scenario counts the live
`lint-banner` nodes in the actual composed app, with a real diagnostic present.

## Setup

The user's main dev daemon (port `6172`) and the Vite dev server (port `5174`)
serve the **MAIN** checkout, **not** this fix branch's worktree. To validate this
branch you must build and run the **worktree's own** daemon, which serves the
worktree's embedded frontend. Do **not** reuse port `6172` or `5172`, and never
restart inside a live run — spin up a dedicated daemon on a free port.

Let `WT` be the implementation worktree root (the dir containing this branch's
`frontend/` and `crates/`) and pick a free `PORT` (e.g. `6272`).

```bash
WT=<path-to-this-branch-worktree>     # the dir with frontend/ and crates/
PORT=6272                             # free port; NOT 6172 (user's daemon), NOT 5172

# Prereqs: pipelines dir + frontend build inputs (both are absent in a fresh worktree)
mkdir -p "$WT/.pdo/pipelines"
( cd "$WT/frontend" && npm ci )       # so build.rs → npm run build can produce dist

# Build the worktree daemon: build.rs runs `npm run build` (→ frontend/dist) and
# bakes the worktree's dist path into the binary (rust_embed, debug = read-from-disk).
( cd "$WT" && cargo build -p pdo-daemon )

# Seed the trigger pipeline (one unknown top-level field → exactly one diagnostic).
cat > "$WT/.pdo/pipelines/lint-banner-scenario.yaml" <<'YAML'
name: lint-banner-scenario
version: "1.0"
auto_merge_resolver: true   # unknown top-level key → exactly one info-only diagnostic
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 0, y: 100 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 300, y: 100 }
edges: []
YAML

# Launch the worktree daemon FROM the worktree CWD (it reads .pdo/ from CWD).
( cd "$WT" && PDO_PORT=$PORT ./target/debug/pdo daemon ) &
```

Sanity checks before driving the UI:

```bash
# Pipeline is listed by the worktree daemon:
curl -fs "http://127.0.0.1:$PORT/pipelines" | grep -q lint-banner-scenario
# The daemon serves the real embedded frontend, NOT the dev placeholder:
curl -fs "http://127.0.0.1:$PORT/" | grep -vq 'run the Vite frontend separately'
```

If `curl /` returns the placeholder ("PDO daemon running… run the Vite
frontend separately"), `frontend/dist` did not build — re-run `cargo build` and
confirm `npm ci` succeeded. Browser: Chrome DevTools MCP preferred; Playwright
MCP works as a fallback.

The diagnostic this pipeline produces is the string
`unknown field 'auto_merge_resolver' (ignored)` (backend parser,
`crates/pdo-daemon/src/pipeline.rs`), surfaced into `tab.diagnostics`.

## Steps the agent executes

1. Navigate the browser MCP to `http://127.0.0.1:$PORT`. Confirm the app shell
   loads (status bar shows **`Daemon: connected`**), and it is NOT the dev
   placeholder page. Run a console-message check (`list_console_messages` /
   `browser_console_messages`) and note any errors.
2. Click the **`lint-banner-scenario`** row in the **Pipelines** sidebar (a
   `<button>` whose text is the pipeline name). The canvas renders two nodes
   (`Start`, `End`). Opening a pipeline sets `selection.kind === "none"`, so **do
   not click any node** — the no-selection state is the one under test.
3. Assert the **Pipeline Inspector** is the active right panel (it renders only
   when `selection.kind === "none"`): its header text **`Pipeline Inspector`** is
   visible. This proves both render sites are co-mounted (canvas + inspector) —
   the exact condition that produced the #63 duplication.
4. **Core assertion.** Evaluate in the page:
   ```js
   document.querySelectorAll('[data-testid="lint-banner"]').length
   ```
   Assert the result is **exactly `1`**. (Before the fix this is `2` ⇒ FAIL.)
   Take a screenshot.
5. Evidence probe — confirm the surviving banner is the **canvas overlay** and
   carries the real diagnostic text:
   ```js
   Array.from(document.querySelectorAll('[data-testid="lint-banner"]')).map(el => ({
     parentClass: el.parentElement?.className?.toString().slice(0, 80),
     text: el.textContent.trim(),
   }))
   ```
   Assert the single entry's `parentClass` contains the overlay positioning
   (`absolute`, `top-10`) and its `text` contains
   `unknown field 'auto_merge_resolver' (ignored)`.

## Control path — selection toggles the inspector, never the count

6. Click the **`Start`** node on the canvas. The inspector panel switches away
   from the Pipeline Inspector (a node inspector opens). Re-evaluate the count:
   ```js
   document.querySelectorAll('[data-testid="lint-banner"]').length
   ```
   Assert it is still **exactly `1`** (only the canvas overlay; the inspector
   copy is gone in both states after the fix). Take a screenshot.
7. Click an **empty area of the canvas pane** to deselect
   (`onPaneClick` → `selection.kind === "none"`). The Pipeline Inspector
   re-appears. Re-evaluate the count and assert it is **still exactly `1`**.
   This is the precise transition that triggered the original bug; it must now
   stay at one.

## Cleanup

- Delete `$WT/.pdo/pipelines/lint-banner-scenario.yaml`.
- Kill the worktree daemon launched in Setup (and any Vite server, if you used
  the dev-server fallback).

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: app shell loaded on the worktree daemon (not the dev placeholder); status bar 'Daemon: connected'; console clean",
    "step 3: Pipeline Inspector header visible (canvas + inspector co-mounted, selection=none)",
    "step 4: querySelectorAll('[data-testid=lint-banner]').length === 1 with no node selected",
    "step 5: the single banner is the canvas overlay (parent has absolute/top-10) and reads \"unknown field 'auto_merge_resolver' (ignored)\"",
    "step 6: after selecting the Start node, banner count still === 1",
    "step 7: after clicking the empty pane (back to selection=none), banner count still === 1"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations, e.g. console warnings, layout glitches>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. The decisive
check is **step 4** (and its re-check at step 7): exactly one `lint-banner` when
no node is selected.

## Fallback — Vite dev server instead of the embedded build

If the embedded `cargo build` path is impractical, serve this worktree's
frontend via its own Vite dev server pointed at a worktree-built daemon (still on
a free port, never 6172/5172):

```bash
# daemon (worktree-built, for branch fidelity) on $PORT first, then:
cd "$WT/frontend"
PDO_PORT=$PORT npx vite --port 5274 --strictPort --cacheDir /tmp/vite-cache-lint-banner
# browse http://127.0.0.1:5274  (API calls proxied to the daemon on $PORT)
```

Prefer the embedded build (it tests exactly what ships); use the Vite fallback
only for fast iteration. The assertions in steps 1-7 are identical either way.
