# Scenario — `duplicate-library-pipeline`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict JSON at the bottom. Asserts the #224 feature —
> duplicating a library pipeline template produces an **unlinked clone** (fresh
> id, name suffixed `(copy)`/`(copy 2)`, **no** `promoted_from`/`meta.json`),
> driven exactly as a human would from the left-panel **Library** tab. Guards
> three regressions: (a) the rename trap — the copy's YAML is rewritten
> **verbatim except the `name:` line**, so unknown top-level keys
> (`auto_merge_resolver: true`) and comments survive; (b) computed-unique
> naming across repeated duplicates; (c) the duplicate affordance appears only
> on library-only rows, never on starred working-pipeline rows.

## Setup

- A daemon is running and reachable. Resolve its base URL once:
  `DAEMON=http://127.0.0.1:${PDO_PORT:-6172}` (dev default 6172; installed/prod is
  6160). Confirm: `curl -sf "$DAEMON/pipelines" | jq 'length'` returns a number.
- The frontend that **contains the #224 change** is reachable in a browser. In
  dev that is the Vite server (`http://127.0.0.1:5174`, which proxies the API to
  the daemon); in prod it is the daemon itself (`http://127.0.0.1:6160`). Set
  `UI=<that URL>`. Chrome DevTools MCP preferred; Playwright MCP works as a
  fallback.
- Library store path for disk assertions: user scope = `~/.pdo/library/pipelines/`.
- **Seed a library-only fixture** (a valid pipeline whose name matches no working
  `/pipelines` entry, so it renders in the library-only block). Derive it from
  the on-disk `review-loop.yaml` (which already contains the non-standard
  top-level key `auto_merge_resolver: true` and comments — ideal for the
  byte-fidelity assertion), renaming only its top-level `name:` line:

  ```bash
  SRC=.pdo/pipelines/review-loop.yaml          # known-valid, has auto_merge_resolver + comments
  FIX_YAML="$(awk 'NR==1 && $1=="name:" {print "name: dup-scenario-fixture"; next} {print}' "$SRC")"
  curl -sf -X POST "$DAEMON/library/pipelines" \
    -H 'content-type: application/json' \
    -d "$(jq -n --arg n dup-scenario-fixture --arg y "$FIX_YAML" \
            '{name:$n, yaml:$y, scope:"user"}')" | jq .
  ```
  Expect `201` with `{"id":"dup-scenario-fixture", ...}`. (If `review-loop.yaml`
  is absent, seed any valid pipeline YAML containing a top-level
  `# comment` line and a column-0 `auto_merge_resolver: true`, named
  `dup-scenario-fixture`.) If seeding fails, the precondition is unmet ⇒ verdict
  `FAIL`; do not hand-fix the store.

## Steps the agent executes

1. Navigate to `UI`. Open the **Library** tab in the left panel (the
   `role="tablist"` with `Runs` / `Triggers` / `Library`; click the `Library`
   tab). Wait for the list to populate. **Assert** a library-only row with text
   `dup-scenario-fixture` is present — it carries `data-testid="library-only-entry"`
   and a `data-testid="left-panel-star"` star. Take a screenshot.

2. Hover that row. **Assert** a duplicate affordance
   `data-testid="library-duplicate-button"` (title `Duplicate pipeline`) becomes
   visible alongside the existing `Remove from library` trash icon. Take a
   screenshot of the hovered row.

3. Click `library-duplicate-button` on the `dup-scenario-fixture` row.
   - **Assert** the network request `POST /library/pipelines/dup-scenario-fixture/duplicate`
     returns **201** with a JSON body `{ id, scope: "user", entry }` where
     `id != "dup-scenario-fixture"` (inspect via the DevTools network panel /
     `list_network_requests`).
   - **Assert** the view did **not** navigate away / auto-open an editor (the
     Library list is still showing — refresh is in-place).
   - **Assert** a new library-only row titled exactly `dup-scenario-fixture (copy)`
     now appears in the list. Take a screenshot.

4. Hover the **original** `dup-scenario-fixture` row again and click its
   `library-duplicate-button` a second time. **Assert** a new row
   `dup-scenario-fixture (copy 2)` appears (computed-unique suffix; the first
   copy did not become `(copy)(copy)`). Take a screenshot.

5. Out-of-band confirm the API view (the parser-normalized names):
   ```bash
   curl -sf "$DAEMON/library/pipelines" | jq '[.[] | select(.name|startswith("dup-scenario-fixture")) | {id,name,scope}] | sort_by(.name)'
   ```
   **Assert** exactly three entries with names `dup-scenario-fixture`,
   `dup-scenario-fixture (copy)`, `dup-scenario-fixture (copy 2)`, all
   `scope: "user"`, with three **distinct** ids. Record the two copy ids as
   `COPY1_ID`, `COPY2_ID`.

6. Disk assertions on the first copy (the clean-fork + byte-fidelity guards):
   ```bash
   LIB=~/.pdo/library/pipelines
   test -f "$LIB/$COPY1_ID.yaml"                     # the copy exists
   grep -qx 'auto_merge_resolver: true' "$LIB/$COPY1_ID.yaml"   # unknown top-level key SURVIVED
   grep -q '^#' "$LIB/$COPY1_ID.yaml" || true        # comments survived (review-loop has them)
   test ! -e "$LIB/$COPY1_ID.meta.json"              # NO promotion metadata -> clean fork
   ```
   **Assert** the `.yaml` exists, `auto_merge_resolver: true` is still present at
   column 0 (proves no serde round-trip / verbatim-except-name), and **no**
   `.meta.json` sidecar exists. **Assert** the copy's parsed `name` (from step 5)
   is `dup-scenario-fixture (copy)` while every other line equals the fixture
   (spot-check: `diff <(grep -v '^name:' "$LIB/dup-scenario-fixture.yaml") <(grep -v '^name:' "$LIB/$COPY1_ID.yaml")` shows no differences).

## Negative checks

7. **404 on missing source.**
   `curl -s -o /dev/null -w '%{http_code}' -X POST "$DAEMON/library/pipelines/does-not-exist/duplicate"`
   **Assert** `404`.

8. **No duplicate button on starred working rows.** If a working pipeline that is
   also starred is visible in block 1 (a row rendered from `/pipelines` showing a
   star badge), hover it and **assert** it shows the `Delete pipeline` trash icon
   but **no** `library-duplicate-button`. (If no such row exists in the current
   environment, record this in `anomalies` as "not exercised — no starred working
   pipeline present" and do not fail.)

## Cleanup

- Delete the three library entries created above:
  ```bash
  for ID in dup-scenario-fixture "$COPY1_ID" "$COPY2_ID"; do
    curl -s -X DELETE "$DAEMON/library/pipelines/$ID" -o /dev/null -w "$ID:%{http_code}\n"
  done
  ```
  Each should return `204`. Confirm `~/.pdo/library/pipelines/` no longer contains
  the fixture or the copies (`.yaml`, `.prompts/`, `.meta.json` all gone).
- No runs are started by this scenario, so there is nothing to archive.

## Verdict format

Emit exactly this, one observation per `evidence` entry, each prefixed with its
step number:

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: 'dup-scenario-fixture' library-only-entry rendered",
    "step 2: library-duplicate-button revealed on hover (title 'Duplicate pipeline')",
    "step 3: POST .../duplicate -> 201, new id != source, row 'dup-scenario-fixture (copy)' appeared, no navigation",
    "step 4: second duplicate -> 'dup-scenario-fixture (copy 2)' appeared",
    "step 5: GET /library/pipelines shows 3 distinct user-scope entries with the expected names",
    "step 6: copy .yaml present, 'auto_merge_resolver: true' survived, NO .meta.json (clean fork), bodies identical except name:",
    "step 7: POST duplicate on missing id -> 404",
    "step 8: starred block-1 row has Delete but no duplicate button"
  ],
  "anomalies": [ "<optional — surprising-but-non-fatal observations>" ]
}
```

A single failed assertion ⇒ verdict: "FAIL". Don't half-pass.
