# Scenario — `run-stats-panel`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Asserts that a Run's **Info
> panel** displays a **Stats** block with three live-correct metrics — **Duration**
> (client-derived, ticking while the run is alive), **Node sessions started**
> (cumulative `NodeStarted` count, manager excluded), and **Lines changed / LOC**
> (`git diff --numstat` of the run branch, `.pdo/`-excluded, `—` once cleaned) —
> and that **cost is absent** (out of scope, #100). The agent cross-checks every
> rendered value against the daemon endpoint AND against git / event-log ground
> truth, the way a careful human reviewer would.

## Setup

- Daemon running and built **from the source tree under test** (`pdo daemon`, or
  `cargo run -p pdo-daemon` with `PDO_SKIP_FRONTEND_BUILD` unset so the embedded
  frontend is current).
- Resolve the base URL once:
  `PORT=${PDO_PORT:-$(ss -ltnp 2>/dev/null | grep -oE '127.0.0.1:(5172|6172|6160)' | head -1 | cut -d: -f2)}; PORT=${PORT:-5172}; BASE=http://127.0.0.1:$PORT`
  Confirm `curl -sf $BASE/sessions | jq .` returns 200 with `{live,cap,version}`.
- Frontend reachable in a browser. **Chrome DevTools MCP preferred**; Playwright MCP
  is the fallback. (If `:5174`/your vite serves a *different* tree, prefer the
  daemon-embedded UI at `$BASE` so you're testing the tree under test.)
- `curl`, `jq`, `git`, and `sqlite3` on PATH.
- `REPO=$(git rev-parse --show-toplevel)` — the target repo where `pdo/run-*`
  branches and the event-log DB live.
- Locate the event-log DB: `DB=$(find "$REPO/.pdo" -maxdepth 2 -name '*.db' | head -1)`.
  Confirm it has an `events` table: `sqlite3 "$DB" '.tables' | grep -q events`.

### Pick the runs to inspect

`curl -sf $BASE/runs | jq -r '.[] | "\(.run_id)\t\(.status)"'`

- **LIVE** = a run whose status is `running` / `awaiting_user` / `paused`. Used for
  the *ticking duration* assertion. If none exists, you MAY launch one (see the
  `run-minimal` scenario) — but that costs API budget; otherwise **skip** the
  ticking assertions and note it in `anomalies`.
- **TERMINAL+branch** = a `completed` / `failed` / `halted` run whose branch still
  exists: `git -C "$REPO" rev-parse --verify "pdo/run-<id>" >/dev/null 2>&1`. Used
  for the LOC and *frozen duration* assertions. Prefer one that touched code.
- **ARCHIVED/cleaned** = a run whose `pdo/run-<id>` branch does **not** resolve
  (cleaned up). Used for the LOC-`—` assertion. If none exists, skip and note it.

## Steps the agent executes

### Step 1 — `GET /runs/:id` carries the new stat fields (refs #100)

For the TERMINAL+branch run id `$R`:

1. `curl -sf "$BASE/runs/$R" | jq '{status, started_at, completed_at, sessions_spawned, loc}'`
2. Assert HTTP 200 and a JSON object.
3. Assert `sessions_spawned` is a **number ≥ 1**.
4. Assert `started_at` is a non-null ISO-8601 `…Z` string and (terminal run)
   `completed_at` is non-null.
5. Assert `loc` is either an object `{insertions, deletions, files_changed}` (all
   numbers) **or** null/absent. Record which.
6. Assert there is **no `cost` field** anywhere in the payload (`jq 'has("cost")'`
   is `false`; `jq` for any key matching `/cost|token|price/i` finds nothing). Cost
   is out of scope and must not have leaked in.

### Step 2 — `sessions_spawned` equals the raw `NodeStarted` count (ground truth)

1. `EXPECT_SESS=$(sqlite3 "$DB" "SELECT count(*) FROM events WHERE run_id='$R' AND kind='node_started';")`
2. Assert `sessions_spawned == $EXPECT_SESS` — a **raw** event count, not deduplicated.
3. Compute distinct iterations:
   `DISTINCT=$(sqlite3 "$DB" "SELECT count(*) FROM (SELECT DISTINCT node_id, iter FROM events WHERE run_id='$R' AND kind='node_started');")`
   Assert `sessions_spawned >= $DISTINCT` (raw ≥ distinct). If `>` , that's the
   restart/recovery case the stat is designed to count — record it as positive
   evidence, not an anomaly.
4. Manager sanity: confirm a `pdo-mgr-$R` style session exists (or existed) yet the
   manager contributes **0** to the count — i.e. there are **no** `node_started`
   rows with a manager node id (`sqlite3 "$DB" "SELECT count(*) FROM events WHERE run_id='$R' AND kind='node_started' AND node_id LIKE '%manager%';"` is 0).

### Step 3 — `loc` equals an independent numstat, with `.pdo/` excluded (ground truth)

For the TERMINAL+branch run (branch resolves):

1. Recompute LOC the way the feature should:
   ```
   git -C "$REPO" diff --numstat HEAD...pdo/run-$R -- . ':(exclude).pdo/'
   ```
   Sum column 1 → `INS`, column 2 → `DEL` (treat `-` binary rows as 0/0 but count
   the file), row count → `FILES`.
2. Assert the API `loc` equals `{insertions:INS, deletions:DEL, files_changed:FILES}`.
3. **`.pdo/` exclusion proof:** recompute *without* the exclusion
   (`git -C "$REPO" diff --numstat HEAD...pdo/run-$R`). If the two totals differ
   (the run wrote tracked `.pdo/` content), assert the API matched the **excluded**
   value, not the unexcluded one. If they're equal, note that `.pdo/` happened to be
   untracked here (still consistent).
4. Three-dot proof (no drift): `git -C "$REPO" diff --numstat HEAD..pdo/run-$R`
   (two-dot) may differ from three-dot if `main` advanced since the fork. The API
   must match the **three-dot** value. If two-dot == three-dot here, note that main
   has not advanced (assertion still holds).

For the ARCHIVED/cleaned run (branch absent): assert the API `loc` is **null/absent**.

### Step 4 — UI Stats block renders and matches the endpoint (refs #100)

1. Open the UI at `$BASE` in the browser. Select the TERMINAL+branch run `$R` from
   the Runs list (left panel) so a run-scoped tab opens.
2. In the run **Info** panel, locate the **Stats** block (it sits just above the
   diff section). Assert all three rows are present: a **Duration**, a **Node
   sessions started** (or equivalent non-"Sessions"-bare label, see Step 6), and a
   **Lines changed / LOC** row.
3. **Node sessions started:** assert the displayed integer equals `sessions_spawned`
   from Step 1 — character-for-character (allowing a thousands separator, e.g.
   `1,234`).
4. **Lines changed:** assert the UI shows `+INS` and `−DEL` (matching Step 3). If
   the API `loc` was null (cleaned run), assert the UI shows **`—`** (an em/en dash),
   **not** `0` and not `+0 −0`.
5. **Duration:** assert the displayed duration ≈ `completed_at − started_at` for the
   terminal run (within the rounding of the chosen format, e.g. whole seconds).
6. Take a screenshot of the Stats block as evidence.

### Step 5 — Duration ticks live, then freezes (refs #100)

If a LIVE run `$L` is available:

1. Open run `$L`'s Info panel. Read the Duration value `D1`.
2. Wait ~4 seconds (do not reload). Read the Duration value `D2`.
3. Assert `D2 > D1` — the duration **ticks** on a live run without a reload.
4. Assert `D1` ≈ `now − started_at` from `curl -sf "$BASE/runs/$L" | jq -r .started_at`
   (within a few seconds).
5. Switch to (or reload) the TERMINAL run `$R`. Read its Duration twice ~4s apart and
   assert it is **identical** both times — a terminal run's duration is **frozen**.

If no LIVE run exists, skip 1–4 and record in `anomalies`; still run 5.

### Step 6 — No naming collision with the live-sessions gauge (refs #100)

1. Assert the Stats block's session label is **not** the bare word "Sessions" — it
   reads "Node sessions started" (or the FR "Sessions de nœud lancées"), to avoid
   confusion with the footer gauge.
2. Assert the footer (bottom status bar) still shows the live **"X/Y"** sessions
   gauge and that it reflects `GET /sessions` `{live,cap}` — a **different, live**
   number from the cumulative stat. The two must be visibly distinct concepts.

## Negative checks

- `grep -rniE '\bcost\b|tokens?|price|\$[0-9]' frontend/src/components/PipelineInfoPanel.tsx`
  returns no cost/token/price rendering in the stats block (cost is out of scope).
- `curl -sf "$BASE/runs/$R" | jq 'paths | map(tostring) | join(".")' | grep -iE 'cost|token'`
  returns nothing (no cost field on the payload).
- `frontend/package.json` version is unchanged (`"0.0.0"`); this feature does not bump it.
- `frontend/vite.config.ts` proxy whitelist is unchanged (the stats ride on the
  already-proxied `/runs/:id`; no new route was added).

## Cleanup

- If you launched a run for Step 5, stop/clean it the way `run-minimal` prescribes;
  otherwise the scenario is read-only. Close browser pages you opened. **Never** stop
  or clean a run/daemon you did not create.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: /runs/:id returns sessions_spawned=<N> and loc=<obj|null>; no cost field",
    "step 2: sessions_spawned == node_started count <N> (raw), >= distinct (node,iter) <M>; manager contributes 0",
    "step 3: loc matches three-dot numstat +<INS>/-<DEL> over <FILES> files, .pdo/ excluded",
    "step 3: archived run loc is null (branch gone)",
    "step 4: Info-panel Stats block renders Duration / Node sessions started / Lines changed, matching the endpoint; cleaned run shows — not 0",
    "step 5: live run duration ticks (D2>D1) and ~= now-started; terminal run duration frozen across reads",
    "step 6: stat label is not bare 'Sessions'; footer X/Y live gauge still works and is distinct",
    "negative: no cost/token rendering in panel or payload; package.json 0.0.0; proxy whitelist unchanged"
  ],
  "anomalies": [
    "<optional — e.g. no live run available so ticking assertions skipped; daemon not built from this tree; .pdo/ untracked so exclusion was a no-op here>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
