# CLI reference

Everything the `pdo` binary does: install, update, run as a service, and the commands a node
session calls. Every command also answers `--help`.

## Install

Homebrew works on macOS and Linux.

```bash
brew install Loulen/tap/pdo
```

The install script supports Linux and macOS on x86_64 and ARM64.

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh | sh
```

Both methods install a checksum-verified `pdo` binary on your `PATH`.

## Update

| Task | Command |
| --- | --- |
| Update from the app | Click the version in the status bar (or Settings › General › Version & update) → **Update**. PDO runs your install method's command in a detached process, reinstalls the service unit, restarts, and the page reloads on the new version |
| Update with Homebrew | `brew update && brew upgrade Loulen/tap/pdo` (`brew update` first: a stale tap hides the latest formula) |
| Update with the script | Run the install script again |
| Install a specific release | Replace `latest` with a tag such as `v1.31.2` |

## Runtime requirements

| Requirement | Purpose |
| --- | --- |
| `tmux` | Runs node and shell sessions |
| `git` | Creates an isolated worktree for each node |
| An agent harness | Runs the agent inside each node |

```bash
brew install tmux git
```

PDO includes descriptors for `claude`, `opencode`, `copilot` and `pi`. What each one supports, and
what PDO assumes you configured, is in [harnesses.md](harnesses.md).

## Start PDO

Run `pdo daemon`, then open [http://localhost:5172](http://localhost:5172).

## Keep PDO running

Run `pdo service install` to start PDO at boot and keep it running after logout. Check it with
`pdo service status` or remove it with `pdo service uninstall`.

## CLI commands

| Command | Description |
| --- | --- |
| `pdo daemon [--port <port>] [--bind <ip>]` | Start the daemon. `PDO_PORT` and `PDO_BIND` provide defaults, while command-line flags take precedence. |
| `pdo service install [--port <port>] [--bind <ip>] [--dry-run]` | Install the systemd user service or launchd agent; `--dry-run` prints the definition without changing the host. Keep `PDO_ALLOWED_WS_ORIGINS` in a `pdo.service.d/override.conf` drop-in, which survives unit rewrites including Update. |
| `pdo service status` | Show the installed service status. |
| `pdo service uninstall` | Stop, disable, and remove the installed service. |
| `pdo complete [--auto]` | Complete the current node; `--auto` is reserved for runtime hooks. |
| `pdo fail --reason <text>` | Fail the current node with a recorded reason. |
| `pdo skip --reason <text>` | End the current run as skipped when there is legitimately no work. |
| `pdo wait-user [--message <text>]` | Declare that the current node waits on its user: the node and run turn awaiting-user with the message on the banner (capped at 100 characters). Accepted on any node with a live session; refused (exit 3) on a script node or without a session; a repeat is a no-op. Lifted by the user's Enter in the PDO terminal or by the completion release. |
| `pdo migrate [--dir <path>] [--dry-run]` | Migrate legacy pipeline YAML files. |
| `pdo reap [--count] [--dry-run] [--ttl-hours <hours>] [--terminal-ttl-hours <hours>] [--budget-secs <seconds>]` | Report or archive old terminal runs according to the retention policy. |
| `pdo docs support-table [--check\|--write] [--file <path>]` | Check or regenerate the harness support table in [harnesses.md](harnesses.md) (the default `--file`). |
| `pdo run create <pipeline> [options]` | Create a run through the daemon, with optional input, repository, harness, sandbox, and provisioning settings. `--input-file <path>` reads the prompt from a file; `--image <path>` / `--file <path>` (repeatable) attach files the entry node sees under `## Input Images` / `## Input Files` (one budget per run, `max_attachments_mb` in Settings). |
| `pdo run wait [--all] [--timeout <seconds>]` | From a node session, block until a child run of this node becomes terminal (`--all`: every child). Exit 0 with one JSON line per settled child (`run_id`, `name`, `status`, `reason`, `children_active`), or `{"noop":true}` when no child is active; exit 2 when `--timeout` elapses (nothing on stdout); exit 1 when the daemon is unreachable or outside a node session. |
| `pdo page mount <name> <directory>` | Serve a directory under `/pages/<name>/`. |
| `pdo page list` | List active page mounts. |
| `pdo page unmount <name>` | Remove a page mount. |
| `pdo review list [--state <state>] [--run <id>]` | List review comments for a run. |
| `pdo review reply <id> --text <markdown> [--resolved] [--run <id>]` | Reply to a review comment and optionally propose or record its resolution. |

Exposing the daemon beyond `localhost` is covered in [reverse-proxy.md](reverse-proxy.md).
