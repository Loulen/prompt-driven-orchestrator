# Scenario — `banner-run-gate-star-popover`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below in a real browser (Chrome DevTools MCP preferred; Playwright MCP works as
> a fallback) and emits the verdict format at the end. Validates the two fixes for
> issue **#225**:
>
> 1. **Run-gate (Part 1).** The lint-diagnostics banner
>    (`data-testid="lint-banner"`) must render in **edit** mode but be **absent**
>    on a **run** tab (`tab.runId != null`). Before the fix it leaked into the run
>    view (`EditCanvas.tsx:567` was gated only on `diagnostics.length > 0`).
> 2. **Star-popover clickability (Part 2).** With the banner visible in edit mode,
>    the green star's popover item **"Remove from library"** must be the topmost
>    element at its own coordinates and must actually fire its handler when
>    clicked. Before the fix the `z-10` star container
>    (`EditCanvas.tsx:555`) trapped the popover's `z-50` inside its stacking
>    context, and the full-width `z-10` lint-banner overlay painted on top and
>    swallowed the click. The fix raises the container to `z-20`.
>
> Why a browser (not just a unit test): the click-swallow in Part 2 is a layout /
> hit-testing property — jsdom cannot reproduce it, and the existing
> `PipelineStar.test.tsx` handler test passes today *despite* the real-browser
> swallow. Part 1 is unit-checkable, but this scenario also confirms it live,
> the way a human would eyeball both states.

## Setup

The user's main dev daemon (port `6172`) and Vite (`5174`) serve the **MAIN**
checkout, **not** this branch's worktree. Build and run the **worktree's own**
daemon, which serves the worktree's embedded frontend, on a free port. Never
reuse `6172`/`5172` and never restart inside a live run.

Let `WT` be this branch's worktree root (the dir with `frontend/` and `crates/`)
and pick a free `PORT` (e.g. `6272`).

```bash
WT=<path-to-this-branch-worktree>     # the dir with frontend/ and crates/
PORT=6272                             # free port; NOT 6172 (user's daemon), NOT 5172

# Prereqs: pipelines dir + frontend build inputs (both absent in a fresh worktree)
mkdir -p "$WT/.pdo/pipelines"
( cd "$WT/frontend" && npm ci )       # so build.rs → npm run build can produce dist

# Build the worktree daemon: build.rs runs `npm run build` (→ frontend/dist) and
# bakes the worktree's dist into the binary (rust_embed; debug = read-from-disk).
( cd "$WT" && cargo build -p pdo-daemon )

# Seed a pipeline that (a) produces exactly one lint diagnostic — an unknown
# top-level key (same technique as lint-banner-single-render) — and (b) has a
# Start node so it can be launched as a run.
cat > "$WT/.pdo/pipelines/zbanner-star-scenario.yaml" <<'YAML'
name: zbanner-star-scenario
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
# PDO_TMUX_CMD_OVERRIDE replaces the real `claude …` tail with a harmless sleep,
# so the run we create in Phase A spawns NO real Claude agent (no token burn) —
# the run tab still opens with the snapshot's diagnostics, which is all we test.
( cd "$WT" && PDO_PORT=$PORT PDO_TMUX_CMD_OVERRIDE="exec sleep 600" ./target/debug/pdo daemon ) &
```

Sanity checks before driving the UI:

```bash
# Worktree daemon lists the pipeline:
curl -fs "http://127.0.0.1:$PORT/pipelines" | grep -q zbanner-star-scenario
# Daemon serves the real embedded frontend, NOT the dev placeholder:
curl -fs "http://127.0.0.1:$PORT/" | grep -vq 'run the Vite frontend separately'
```

If `curl /` returns the placeholder ("…run the Vite frontend separately"),
`frontend/dist` did not build — re-run `cargo build` and confirm `npm ci`
succeeded. The diagnostic this pipeline produces is the string
`unknown field 'auto_merge_resolver' (ignored)`, surfaced into `tab.diagnostics`.

> Vite fallback (fast iteration): worktree-built daemon on `$PORT`, then
> `cd "$WT/frontend" && PDO_PORT=$PORT npx vite --port 5274 --strictPort --cacheDir /tmp/vite-cache-banner-star`,
> browse `http://127.0.0.1:5274`. Assertions are identical. Prefer the embedded
> build (it tests exactly what ships).

## Steps the agent executes

### Phase A — Part 1: banner shows in edit, hidden on a run tab

1. Navigate the browser MCP to `http://127.0.0.1:$PORT`. Confirm the app shell
   loads (status bar shows **`Daemon: connected`**) and it is NOT the dev
   placeholder. Run a console check (`list_console_messages` /
   `browser_console_messages`); note any errors.
2. In the left panel, click the **Library** tab, then click the
   **`zbanner-star-scenario`** row to open it in **edit mode** (this opens an
   edit tab — `scope` is `repo`/`user`, `runId` is unset). Do not click any node.
3. **Edit-mode banner present.** Evaluate in the page:
   ```js
   document.querySelectorAll('[data-testid="lint-banner"]').length
   ```
   Assert the result is **exactly `1`**, and confirm its text contains
   `unknown field 'auto_merge_resolver' (ignored)`. **Take a screenshot.**
4. Create a run of the same pipeline **without launching a real agent** (the
   daemon was started with `PDO_TMUX_CMD_OVERRIDE`). Easiest deterministic path —
   POST directly, then open the run tab in the UI:
   ```bash
   curl -fsS -X POST "http://127.0.0.1:$PORT/runs" \
     -H 'content-type: application/json' \
     -d '{"pipeline_id":"zbanner-star-scenario","prompt":"scenario noop"}'
   # → note the returned run id
   ```
   (If the New-Run modal is preferred: click **New Run**, pick
   `zbanner-star-scenario`, enter any prompt, launch.) Then in the left panel
   open the **Runs** tab and click the new run's row to open its **run tab**.
5. **Run tab confirmed.** Evaluate that the active tab is a run tab, e.g. check
   the URL/tab state or:
   ```js
   // a run tab id is prefixed "__run__"; confirm one is active
   !!document.querySelector('[data-testid^="tab-"][data-active="true"]') // adjust to actual tab markup if needed
   ```
   If tab markup is unclear, rely on the visual: the Runs list highlights the
   open run and the canvas shows the run snapshot.
6. **Core assertion (Part 1).** With the run tab active, evaluate:
   ```js
   document.querySelectorAll('[data-testid="lint-banner"]').length
   ```
   Assert the result is **exactly `0`** — the banner is suppressed on the run
   tab even though the snapshot carries the same diagnostic. **Take a
   screenshot.** (Before the fix this is `1` ⇒ FAIL.)

### Phase B — Part 2: "Remove from library" is clickable in edit mode

7. Switch back to the **edit tab** for `zbanner-star-scenario` (click its tab, or
   re-open it from the Library list). Confirm the lint banner is visible again
   (`[data-testid="lint-banner"]`.length === 1) — this is the coexistence state
   where the bug lived.
8. Save the pipeline to the library so the star turns **synced** and its popover
   offers "Remove from library": click the star button
   (`[data-testid="pipeline-star"]`). It starts as `outline`
   (`data-sync-state="outline"`); one click saves to library. Assert it is now
   `data-sync-state="synced"` (re-query the attribute). The lint banner must
   still be present (the unknown-key diagnostic persists in the verbatim library
   copy and on the working tab).
9. Click the star again to **open the popover**. Assert
   `[data-testid="pipeline-star-popover"]` exists and contains a button reading
   **"Remove from library"**. **Take a screenshot** (you should see the popover
   overlapping the lint banner band).
10. **Core assertion (Part 2) — hit-test.** Confirm the "Remove from library"
    button is the *topmost* element at its own center (i.e. nothing, especially
    the lint banner, is painted over it). Evaluate in the page:
    ```js
    (() => {
      const btn = [...document.querySelectorAll('[data-testid="pipeline-star-popover"] button')]
        .find(b => b.textContent.trim() === 'Remove from library');
      if (!btn) return { ok: false, reason: 'button not found' };
      const r = btn.getBoundingClientRect();
      const top = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return {
        ok: top === btn || btn.contains(top),
        hitTag: top && top.tagName,
        hitTestId: top && top.closest('[data-testid]')?.getAttribute('data-testid'),
        hitText: top && top.textContent?.trim().slice(0, 40),
      };
    })()
    ```
    Assert `ok === true`. Before the fix, `hitTestId` would be `lint-banner`
    (or its overlay wrapper) ⇒ FAIL. Record the returned object as evidence.
11. **Click it for real** and confirm the handler fires end-to-end. Start
    watching network, then click the "Remove from library" button via the
    browser MCP click (real synthetic mouse event at the element, NOT a
    JS `.click()`):
    - Assert a `DELETE` request to `/library/pipelines/...` is observed
      (`list_network_requests` / `browser_network_requests`) and returns 2xx.
    - Assert the popover closes and the star reverts to
      `data-sync-state="outline"` (`[data-testid="pipeline-star"]`).
    - Assert the Library list no longer lists `zbanner-star-scenario` as a
      starred/library entry (re-open the Library tab; the library-only row /
      star badge for it is gone).
    **Take a screenshot.**

## Negative control — no false occlusion elsewhere

12. Re-open the pipeline in edit mode, click the star to save to library again
    (`synced`), open the popover. Assert the popover and its button render at the
    expected position and the star button itself (`[data-testid="pipeline-star"]`)
    is still independently clickable (the `z-20` bump must not break the toolbar
    or other top-right affordances). A quick check: `elementFromPoint` over the
    star button center returns the star button. (This guards against the bump
    over-occluding a neighbor.)

## Cleanup

- Delete the seeded pipeline: `rm -f "$WT/.pdo/pipelines/zbanner-star-scenario.yaml"`.
- Remove any library copy left behind (if step 11 didn't, DELETE it):
  `curl -fsS "http://127.0.0.1:$PORT/library/pipelines" | …` then
  `curl -X DELETE "http://127.0.0.1:$PORT/library/pipelines/<id>"`.
- Stop/archive the run created in Phase A (run command "stop"/archive via UI or
  `POST /runs/<id>/commands`), then kill the worktree daemon launched in Setup.
  Killing the daemon reaps its dedicated tmux socket (`pdo-<PORT>`) and the
  `exec sleep 600` stub sessions with it — they are isolated to this scenario's
  daemon. (If any linger: `tmux -L pdo-$PORT kill-server`.)
- Remove the Vite cache dir if you used the fallback.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: app shell on the worktree daemon (not the dev placeholder); 'Daemon: connected'; console clean",
    "step 3: edit tab → exactly 1 lint-banner, text contains \"unknown field 'auto_merge_resolver' (ignored)\"",
    "step 6: run tab → 0 lint-banner (Part 1 run-gate holds) [screenshot]",
    "step 10: elementFromPoint over 'Remove from library' returns the button itself (ok=true); not the lint-banner (Part 2) — paste the returned object",
    "step 11: clicking fired DELETE /library/pipelines/<id> (2xx); star reverted to outline; library list no longer shows the entry [screenshot]",
    "step 12: negative control — star button still clickable; z-20 bump occludes nothing intended"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations, e.g. console warnings, layout glitches, run-creation quirks under the tmux stub>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. The decisive
checks are **step 6** (Part 1: zero banners on a run tab) and **step 10/11**
(Part 2: "Remove from library" is the topmost element at its coordinates and
its click fires the DELETE). If you cannot create the run tab in Phase A (e.g.
the daemon rejects the noop run), record it as an anomaly and still run Phase B —
but Part 1 then rests on the unit test, and the scenario is `FAIL` for Part 1's
in-browser leg unless the run tab is reachable.
