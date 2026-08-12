#!/usr/bin/env bash
# Layer 4 smoke test (testing pyramid per ADR 0004).
#
# Boots the daemon on an ephemeral port, hits the wired-up endpoints, asserts
# they respond JSON / HTML, and kills the daemon. Exits 0 on success.
#
# Usage: bash tests/smoke.sh
# Requires: target/debug/pdo built (the script will run `cargo build` if missing).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

BIN="$REPO_ROOT/target/debug/pdo"
if [[ ! -x "$BIN" ]]; then
  echo "[smoke] Building pdo daemon (debug)..."
  PDO_SKIP_FRONTEND_BUILD="${PDO_SKIP_FRONTEND_BUILD:-}" cargo build -p pdo-daemon
fi

# Pick a free port. Prefer python3 (available in CI + most dev setups);
# fall back to a high random-ish port if nothing else works.
pick_port() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()'
  elif command -v node >/dev/null 2>&1; then
    node -e 'const s=require("net").createServer().listen(0,()=>{console.log(s.address().port);s.close()})'
  else
    # Crude fallback — risks clash but unblocks smoke runs without python/node.
    echo $(( 49152 + RANDOM % 16000 ))
  fi
}

PORT=$(pick_port)
TMPDIR=$(mktemp -d)
LOG="$TMPDIR/daemon.log"
echo "[smoke] Using port $PORT, working dir $TMPDIR"

# Run daemon from the tempdir so its .pdo lives there, not in the repo.
# PDO_PRICE_SYNC=off (#427): the smoke test polls /runs for up to 30 s, so a boot
# price fetch waiting on a DNS timeout would pass anyway while masking exactly that.
#
# exec (#514): without it, the subshell runs two commands (cd && pdo), so bash
# forks pdo as a child and blocks in wait — $! then names the *subshell*, not pdo.
# cleanup would kill the subshell; the real daemon, re-adopted by init, kept its
# port and tmux socket — one orphan daemon leaked per CI run. The leak was invisible
# because the subshell stays alive while pdo runs, so kill -0 and the HTTP poll both
# passed. exec replaces the subshell with pdo so $! is the daemon's real PID.
( cd "$TMPDIR" && PDO_PRICE_SYNC=off exec "$BIN" daemon --port "$PORT" >"$LOG" 2>&1 ) &
DAEMON_PID=$!

cleanup() {
  if kill -0 "$DAEMON_PID" 2>/dev/null; then
    kill "$DAEMON_PID" 2>/dev/null || true            # SIGTERM (daemon has no handler → immediate exit)
    for _ in $(seq 1 20); do                           # bounded wait ~2 s
      kill -0 "$DAEMON_PID" 2>/dev/null || break
      sleep 0.1
    done
    kill -9 "$DAEMON_PID" 2>/dev/null || true          # SIGKILL backstop (future graceful-shutdown)
    wait "$DAEMON_PID" 2>/dev/null || true
  fi
  rm -rf "$TMPDIR"
}
trap cleanup EXIT INT TERM

URL="http://127.0.0.1:$PORT"

# Poll /runs until the daemon answers (max ~30s).
echo "[smoke] Waiting for daemon to accept requests at $URL ..."
for i in $(seq 1 60); do
  if curl -fsS -o /dev/null "$URL/runs" 2>/dev/null; then
    echo "[smoke] Daemon ready after ${i} polls."
    break
  fi
  if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "[smoke] Daemon exited before becoming ready. Log:"
    cat "$LOG" || true
    exit 1
  fi
  sleep 0.5
done

if ! curl -fsS -o /dev/null "$URL/runs"; then
  echo "[smoke] Timed out waiting for daemon. Log:"
  cat "$LOG" || true
  exit 1
fi

fail() {
  echo "[smoke] FAIL: $*" >&2
  echo "[smoke] Daemon log:" >&2
  cat "$LOG" >&2 || true
  exit 1
}

# Non-regression guard (#514): DAEMON_PID must name the pdo daemon, not the
# wrapping subshell. Checked after the readiness gate so exec has certainly
# replaced the subshell. `comm` (basename), not `args`: the subshell's argv
# contains pdo's path and would be a false positive. Linux comm=pdo, macOS
# comm=/…/pdo; both match *pdo*. Had this been in place, the original bug
# (comm=bash) would have failed loudly instead of leaking silently.
DAEMON_COMM=$(ps -p "$DAEMON_PID" -o comm= 2>/dev/null || true)
case "$DAEMON_COMM" in
  *pdo*) : ;;
  *) fail "DAEMON_PID=$DAEMON_PID does not name the pdo daemon (comm='$DAEMON_COMM'); cleanup would kill the wrong process and leak the daemon" ;;
esac

# 1. GET /runs returns JSON
CT=$(curl -fsS -o /dev/null -w '%{content_type}' "$URL/runs")
[[ "$CT" == application/json* ]] || fail "/runs content-type not JSON: '$CT'"

# 2. GET /pipelines returns JSON
CT=$(curl -fsS -o /dev/null -w '%{content_type}' "$URL/pipelines")
[[ "$CT" == application/json* ]] || fail "/pipelines content-type not JSON: '$CT'"

# 3. GET / returns the SPA shell with the app title. (The "PDO" wordmark is
# rendered by React at runtime; the static index only carries the <title>.)
INDEX=$(curl -fsS "$URL/")
echo "$INDEX" | grep -q "Prompt Driven Orchestrator" || fail "index does not contain the app title"

# 4. Asset JS referenced in index.html responds 200
ASSET_PATH=$(echo "$INDEX" | grep -oE '/assets/[^"]+\.js' | head -n1)
[[ -n "$ASSET_PATH" ]] || fail "no /assets/*.js script tag in index.html"
ASSET_STATUS=$(curl -fsS -o /dev/null -w '%{http_code}' "$URL$ASSET_PATH")
[[ "$ASSET_STATUS" == "200" ]] || fail "asset $ASSET_PATH returned status $ASSET_STATUS"

echo "[smoke] OK — /runs JSON, /pipelines JSON, / HTML, $ASSET_PATH 200"
exit 0
