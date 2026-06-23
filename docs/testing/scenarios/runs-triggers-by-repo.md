# Scenario — `runs-triggers-by-repo`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below by driving the **real UI in a browser** (Chrome DevTools MCP preferred;
> Playwright MCP fallback) plus `curl`/`jq` against the daemon, exactly as a
> human would, and emits the verdict JSON at the bottom. Asserts #258: the Runs
> and Triggers lists **group by project (target repo)** — conditionally (only
> when ≥ 2 distinct repos are present), with null `target_repo` resolved to the
> daemon's `repo_root` (no "Unassigned" bucket), and the single-repo common case
> left byte-identical to today.

The behavior under test is a **display/grouping** concern. The cheap, deterministic
vehicle is **Triggers** (creating a Trigger persists a row, it does not spawn a Run —
create them **disabled** so they never fire during the test). Runs are validated at the
API level always, and in the UI when the daemon already holds runs across ≥ 2 repos.

## Setup

- Daemon running and built **from the source tree under test** (`pdo daemon`, or
  `cargo run -p pdo-daemon`). Do **not** restart or stop a daemon you did not start.
- Discover the daemon URL: honor `$PDO_PORT`; otherwise probe known ports
  (dev `6172`, prod `6160`, legacy `5172`) — pick the one whose `GET /sessions`
  returns 200:
  ```bash
  for p in "${PDO_PORT:-}" 6172 6160 5172; do [ -n "$p" ] && \
    curl -sf "http://127.0.0.1:$p/sessions" >/dev/null 2>&1 && { PORT=$p; break; }; done
  echo "daemon on $PORT"
  ```
  (or `ss -ltnp | grep -i pdo` to find the listening port). Export `BASE=http://127.0.0.1:$PORT`.
- Frontend reachable in a browser at `$BASE` (or the vite dev server proxying to it).
  Chrome DevTools MCP preferred; Playwright MCP fallback.
- `curl`, `jq`, `git`, `mktemp` on PATH.
- Record the daemon's own repo root (the null-resolution target). It is the repo the
  daemon was launched from. Capture it for later equality checks:
  - Read it from any run that has **no** `target_repo`: see Step 1; or from the
    daemon's launch cwd if known. Store as `REPO_ROOT`.
- Pick an existing pipeline id to attach triggers to:
  `PIPE=$(curl -sf "$BASE/pipelines" | jq -r '.[0].id')` (any valid id is fine; the
  triggers stay disabled and never run it).
- Create three throwaway git repos for seeding, plus a basename-collision pair:
  ```bash
  mk(){ d=$(mktemp -d "/tmp/pdo258-$1.XXXX"); git -C "$d" init -q; \
        git -C "$d" -c user.email=a@b.c -c user.name=t commit -q --allow-empty -m init; echo "$d"; }
  REPO_A=$(mk alpha)        # e.g. /tmp/pdo258-alpha.AB12
  REPO_B=$(mk beta)         # e.g. /tmp/pdo258-beta.CD34
  COL1=$(mktemp -d /tmp/pdo258-x.XXXX)/svc; COL2=$(mktemp -d /tmp/pdo258-y.XXXX)/svc
  for c in "$COL1" "$COL2"; do mkdir -p "$c"; git -C "$c" init -q; \
        git -C "$c" -c user.email=a@b.c -c user.name=t commit -q --allow-empty -m init; done
  # COL1 and COL2 share basename "svc" but differ by parent → collision case.
  ```

## Steps the agent executes

### Step 0 — Baseline: confirm the single-repo flat case is untouched (regression)
1. Before seeding anything, open the UI, select the **Triggers** tab, and the **Runs**
   tab. Take a screenshot of each.
2. Via DOM, assert how many `data-testid="trigger-repo-group"` and
   `data-testid="run-repo-group"` elements exist. Record both counts as the baseline.
3. If the daemon currently holds triggers/runs spanning only **one** distinct repo,
   assert **zero** `*-repo-group` headers are present (the list is flat — today's
   behavior). If it already spans ≥ 2, record that the grouped state pre-exists and
   note it (the Step-3/Step-4 assertions still apply).

### Step 1 — API: `/runs` carries a resolved `effective_repo` (no Unassigned)
1. `curl -sf "$BASE/runs" | jq '.[0]'`. Assert 200 and a JSON array.
2. Assert **every** row has a non-empty string `effective_repo`
   (`curl -sf "$BASE/runs" | jq -e 'all(.[]; .effective_repo | type=="string" and length>0)'`).
3. Find a run with **no** raw target repo and confirm it still resolved:
   pick any row, and assert that rows which do not target an explicit repo carry
   `effective_repo == REPO_ROOT` (the daemon's own repo). If `REPO_ROOT` wasn't known
   from Setup, derive it here as the most common `effective_repo` among rows and record
   the assumption as an anomaly note.
4. Assert there is **no** literal "Unassigned"/"unknown"/null bucket: no row has
   `effective_repo` equal to `null`, `""`, `"Unassigned"`, or `"(none)"`.

### Step 2 — Seed triggers across repos (disabled, so nothing fires)
Create five disabled triggers via `POST $BASE/triggers` (Content-Type application/json),
each `{"name": "...", "pipeline_id": "'$PIPE'", "cron": "0 0 1 1 *", "enabled": false,
"target_repo": <as below>}`:
- `t-a1`, `t-a2` → `target_repo = REPO_A`  (two triggers, same repo)
- `t-b1`         → `target_repo = REPO_B`
- `t-null`       → **omit** `target_repo` entirely (null → must resolve to `REPO_ROOT`)
- (collision pair, used in Step 5) `t-c1` → `COL1`, `t-c2` → `COL2`
Assert each POST returns 201. Record the created trigger ids for cleanup.

### Step 3 — API: `/triggers` resolves null but never mutates raw `target_repo`
1. `curl -sf "$BASE/triggers" | jq '.'`.
2. For the `t-null` trigger: assert its raw **`target_repo` is null/absent** AND its
   **`effective_repo == REPO_ROOT`** (null was resolved for grouping only, the raw field
   was not rewritten — this is the core no-regression guarantee).
3. For `t-a1`: assert `target_repo == REPO_A` **and** `effective_repo == REPO_A`.
4. Assert the response rows still expose all the flat Trigger fields (`name`, `cron`,
   `enabled`, `pipeline_name`) at top level (the `effective_repo` wrapper must not
   nest the trigger).

### Step 4 — UI: Triggers list is now grouped by repo
1. In the browser, open the **Triggers** tab. Take a screenshot.
2. Assert `data-testid="trigger-repo-group"` headers are now present and number **≥ 3**
   (at minimum: `REPO_A`, `REPO_B`, `REPO_ROOT` — plus the two collision repos from
   Step 2). The baseline from Step 0 must have increased.
3. Read every `data-testid="trigger-repo-label"`. Assert:
   - the `REPO_A` group's label equals the basename of `REPO_A` (e.g. `alpha.AB12`);
   - the `t-null` trigger appears under the group whose `title` (hover/full-path
     attribute) equals `REPO_ROOT` — i.e. it grouped with the daemon's own repo, **not**
     a separate "Unassigned" group;
   - the `t-a1` and `t-a2` rows sit under the **same** single `REPO_A` group (two
     triggers, one header), in `created_at DESC` order (most-recent first).
4. Hover the `REPO_A` header (or read its `title` attribute) and assert the **full path**
   `REPO_A` is shown on hover while the visible label is the basename.
5. Confirm the per-row **repo badge** behavior is unchanged: the `t-null` row shows **no**
   repo badge (its raw `target_repo` is null), while `t-a1` shows a badge with the
   `REPO_A` basename. (Regression guard for G1/G7.)

### Step 5 — UI: basename collision renders two disambiguated groups
1. Still on the Triggers tab, locate the two collision groups for `COL1` and `COL2`
   (both basename `svc`).
2. Assert they render as **two distinct** `trigger-repo-group` headers (not merged).
3. Assert their `trigger-repo-label` texts are **disambiguated** (not both bare `svc`) —
   they should show the minimal distinguishing trailing-path suffix (e.g. the differing
   parent segment + `/svc`). Each header's `title` still carries the full path.

### Step 6 — Runs UI grouping (conditional)
1. If `GET /runs` (Step 1) shows **≥ 2 distinct** `effective_repo` values, open the
   **Runs** tab, screenshot, and assert ≥ 2 `data-testid="run-repo-group"` headers
   render, each with a `run-repo-label` (basename) and a `title` (full path), and that
   runs sharing a repo sit under one header in most-recent-first order. Assert run rows
   carry **no** per-row repo badge (header is the only repo surface — G2).
2. If `/runs` shows only **one** distinct repo, assert the Runs tab renders **zero**
   `run-repo-group` headers (flat — confirms the conditional rule and the no-regression
   guarantee), and record that runs-UI grouping was exercised only at the API level
   (Step 1) for this environment. Optionally, if you can safely seed runs (a
   self-sufficient/`doc-only` pipeline exists and you will clean them up), create one run
   targeting `REPO_A` and one targeting `REPO_B`, re-open the Runs tab, and assert the
   two headers appear — then clean those runs up in Cleanup.

### Step 7 — Per-list independence
1. Compare the Runs tab and Triggers tab grouped states. Assert the "≥ 2 repos"
   threshold is evaluated **per list**: e.g. with the seeded triggers spanning many repos
   but runs spanning one, the **Triggers** tab is grouped while the **Runs** tab is flat
   (or vice versa). The two tabs must not share a single threshold.

## Negative checks
- `grep -rn "Unassigned" frontend/src/` returns no matches related to repo grouping
  (the design has no such bucket).
- The daemon never mutates raw `target_repo`: re-run Step 3.2 — `t-null.target_repo`
  stays null across repeated `GET /triggers`.
- Single-repo regression: if you can momentarily view a single-repo list (Step 0 or
  Step 6.2), confirm it is pixel-equivalent to before — no header row inserted, no new
  badge on trigger rows whose `target_repo` is null.
- `frontend/vite.config.ts` proxy whitelist is unchanged — `/runs` and `/triggers`
  were already proxied; no new top-level route was added.

## Cleanup
- Delete every trigger created in Step 2: `DELETE $BASE/triggers/<id>` for each recorded
  id (assert 204). Confirm `GET /triggers` no longer lists them.
- If you seeded runs in Step 6, clean them up (cleanup/forget via the UI trash action or
  the run cleanup endpoint) and confirm they leave the list.
- Remove the temp repos: `rm -rf` the `/tmp/pdo258-*` dirs you created.
- Close any browser pages you opened. Do not stop a daemon you did not start.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 0: baseline group-header counts recorded (runs=<n>, triggers=<n>)",
    "step 1: /runs rows all carry non-empty effective_repo; null-target runs resolve to REPO_ROOT; no Unassigned",
    "step 3: t-null has target_repo=null but effective_repo=REPO_ROOT; t-a1 target_repo==effective_repo==REPO_A; flat fields intact",
    "step 4: Triggers tab shows >=3 repo-group headers; t-a1+t-a2 under one REPO_A group; t-null under REPO_ROOT group; label=basename, full path on hover; t-null shows no badge",
    "step 5: COL1/COL2 render as two distinct groups with disambiguated labels (not both 'svc')",
    "step 6: runs grouped in UI when >=2 repos, else flat with zero headers (conditional rule)",
    "step 7: threshold evaluated per-list (triggers grouped while runs flat, or vice versa)",
    "negative: raw target_repo never mutated; no Unassigned bucket; vite proxy unchanged"
  ],
  "screenshots": [
    "<path/ref to Triggers-tab grouped screenshot>",
    "<path/ref to Runs-tab screenshot>"
  ],
  "anomalies": [
    "<optional — e.g. daemon not built from this tree; REPO_ROOT inferred rather than known; runs-UI validated at API level only>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. If the feature is not
yet implemented, the API fields (`effective_repo`) and the `*-repo-group` test ids will
be absent — report `FAIL` with the missing pieces listed in `evidence`.
