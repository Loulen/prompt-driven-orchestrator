# Scenario — `daemon-version-display`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Asserts that the daemon
> exposes its compiled version (`CARGO_PKG_VERSION`) on `GET /sessions` and
> that the UI footer displays that live version instead of the historical
> hardcoded `v0.1.0` (#139).

## Setup

- Daemon running and built **from the source tree under test** (`pdo daemon`,
  or `cargo run -p pdo-daemon` with `PDO_SKIP_FRONTEND_BUILD` unset so
  the embedded frontend is current). Daemon URL defaults to
  `http://127.0.0.1:5172`; honor `PDO_PORT` if set.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `curl` and `jq` on PATH.
- Record the expected version up front:
  `EXPECTED=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)` from the repo
  root of the tree the daemon was built from.

## Steps the agent executes

### Step 1 — `GET /sessions` carries the daemon version (refs #139)

1. `curl -sf http://127.0.0.1:5172/sessions | jq .`
2. Assert HTTP status is **200** and the body is a JSON object.
3. Assert the object still has numeric `live` and `cap` fields (regression:
   the status-bar counter payload must be intact).
4. Assert it has a string `version` field matching `^\d+\.\d+\.\d+`.
5. Assert `version` equals `$EXPECTED` (the Cargo.toml workspace version of
   the tree the daemon was built from). If the running daemon was *not* built
   from this tree, report that as an anomaly and keep only assertions 1–4.

### Step 2 — Footer displays the endpoint's version (refs #139)

1. Open the UI in the browser. Confirm the footer (bottom status bar) shows
   the daemon connection dot and the session counter.
2. Assert the footer contains the exact text `v<version>` where `<version>`
   is the value returned in Step 1 — character-for-character equality, not
   just a semver-shaped string.
3. Assert the footer does **not** contain `v0.1.0`, *unless* Step 1 itself
   returned `0.1.0` (the assertion that matters is endpoint↔footer equality).
4. Take a screenshot of the footer as evidence.

### Step 3 — Version survives reload and stays consistent

1. Reload the page.
2. Assert the footer shows the same `v<version>` after reload.
3. Re-run the Step 1 `curl` and assert the response is unchanged.

### Step 4 — Unknown version renders nothing, not a lie

1. With the daemon **stopped** (only do this if you started the daemon
   yourself for this scenario — never stop a user's live daemon; otherwise
   skip and note it), load the UI from a separately served frontend (e.g.
   vite dev server) so the page itself still loads.
2. Assert the footer shows **no version string at all** (no `v0.1.0`, no
   placeholder) while the daemon is unreachable, and that the connection
   label reads `Daemon: disconnected`.
3. Restart the daemon; after the WebSocket reconnects, assert the version
   reappears in the footer **without a manual page reload** (the sessions
   payload is re-fetched on WS events).

## Negative checks

- `grep -rn "v0\.1\.0" frontend/src/` returns no matches (the hardcoded
  literal is gone from source).
- `frontend/package.json` still says `"version": "0.0.0"` (it must NOT have
  been bumped — Cargo.toml is the single source of truth).
- `frontend/vite.config.ts` proxy whitelist is unchanged (no `/version`
  entry was needed; the field rides on the already-proxied `/sessions`).

## Cleanup

- If you started a daemon for this scenario, stop it. Close browser pages you
  opened. Nothing else: the scenario is read-only.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 1: /sessions returns 200 with live, cap and version=<X.Y.Z>",
    "step 1: version equals Cargo.toml workspace version",
    "step 2: footer displays v<X.Y.Z>, identical to the endpoint value",
    "step 3: value stable across reload",
    "step 4: no version rendered while daemon down; reappears on reconnect",
    "negative: no hardcoded v0.1.0 in frontend/src; package.json still 0.0.0"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations, e.g. daemon not built from this tree>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
