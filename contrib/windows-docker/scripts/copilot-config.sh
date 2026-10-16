#!/bin/bash
# Trusts ~/work (cascades to every clone below it) and ~/.pdo, where PDO creates the run
# worktrees; pins autoUpdate=false. Copilot writes JSONC (leading // comments): strip them
# before jq. Run again after `copilot login`.
set -euo pipefail
mkdir -p ~/.copilot

jsonc_update() { # file jq-filter [jq args...]
  local f=$1 filter=$2; shift 2
  local tmp; tmp=$(mktemp)
  if [ -s "$f" ]; then sed -E '/^[[:space:]]*\/\//d' "$f"; else echo '{}'; fi \
    | jq "$@" "$filter" > "$tmp"
  cat "$tmp" > "$f" && rm -f "$tmp"
  chmod 600 "$f"
}

jsonc_update ~/.copilot/config.json \
  '.trustedFolders = (((.trustedFolders // []) + [$a, $b]) | unique)' \
  --arg a ~/work --arg b ~/.pdo
jsonc_update ~/.copilot/settings.json '.autoUpdate = false'
