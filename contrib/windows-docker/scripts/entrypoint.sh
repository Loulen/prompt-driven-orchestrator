#!/bin/bash
set -euo pipefail

[ -z "${GIT_USER_NAME:-}" ]  || git config --global user.name  "$GIT_USER_NAME"
[ -z "${GIT_USER_EMAIL:-}" ] || git config --global user.email "$GIT_USER_EMAIL"
# PDO expects target repos to ignore its runtime dir: one global exclude covers every clone.
# .pdo/pipelines/ stays visible, since a repo may version its pipelines there.
mkdir -p ~/work ~/.config/git
for pattern in '/.pdo/*' '!/.pdo/pipelines/'; do
  grep -qxF "$pattern" ~/.config/git/ignore 2>/dev/null || printf '%s\n' "$pattern" >> ~/.config/git/ignore
done
# A broken Copilot config must not keep the daemon from starting (restart loop).
/opt/pdo-docker/copilot-config.sh || echo "entrypoint: copilot-config failed (see above), starting PDO anyway." >&2

# Inside the container only: the host side is published on 127.0.0.1 (compose.yaml).
exec pdo daemon --bind 0.0.0.0 --port 5172
