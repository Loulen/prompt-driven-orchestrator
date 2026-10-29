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
| `pdo complete [--auto]` | Complete the current node; `--auto` is reserved for runtime hooks. In a Session hébergée (`PDO_SESSION_ID`, no `PDO_RUN_ID`): refused, exit 3, nothing changes. |
| `pdo fail --reason <text>` | Fail the current node with a recorded reason. Refused (exit 3) in a Session hébergée. |
| `pdo skip --reason <text>` | End the current run as skipped when there is legitimately no work. Refused (exit 3) in a Session hébergée. |
| `pdo wait-user [--message <text>]` | Declare that the current node waits on its user: the node and run turn awaiting-user with the message on the banner (capped at 100 characters). Accepted on any node with a live session; refused (exit 3) on a script node or without a session; a repeat is a no-op. Lifted by the user's Enter in the PDO terminal or by the completion release. In a Session hébergée (`PDO_SESSION_ID`, no `PDO_RUN_ID`), the session turns awaiting-user instead, lifted by an Enter in its terminal; refused (exit 3) when it is not running. |
| `pdo migrate [--dir <path>] [--dry-run]` | Migrate legacy pipeline YAML files. |
| `pdo reap [--count] [--dry-run] [--ttl-hours <hours>] [--terminal-ttl-hours <hours>] [--budget-secs <seconds>]` | Report or archive old terminal runs according to the retention policy. |
| `pdo docs support-table [--check\|--write] [--file <path>]` | Check or regenerate the harness support table in [harnesses.md](harnesses.md) (the default `--file`). |
| `pdo run create <pipeline> [options]` | Create a run through the daemon, with optional input, repository, harness, sandbox, and provisioning settings. `--input-file <path>` reads the prompt from a file; `--image <path>` / `--file <path>` (repeatable) attach files the entry node sees under `## Input Images` / `## Input Files` (one budget per run, `max_attachments_mb` in Settings). `--ext <id>.<key>=<value>` (repeatable) sends `extensions.<id>.<key>` = `"<value>"`, data PDO never interprets: the extension `<id>` checks it before the Run is created (a refusal prints its reason, exit 1) and receives it before the first node; an id no installed extension has is refused. |
| `pdo run wait [--all] [--timeout <seconds>]` | From a node session, block until a child run of this node becomes terminal (`--all`: every child). Exit 0 with one JSON line per settled child (`run_id`, `name`, `status`, `reason`, `children_active`), or `{"noop":true}` when no child is active; exit 2 when `--timeout` elapses (nothing on stdout); exit 1 when the daemon is unreachable or outside a node session. |
| `pdo run list [--json]` | List the runs the daemon knows, one tab-separated line per run (`run_id`, status, pipeline, name); `--json` prints the daemon's `GET /runs` answer as is. Exit 1 with the daemon's reason when it refuses the call. |
| `pdo page mount <name> <directory> [--guide <file>]` | Serve a directory under `/pages/<name>/` and print its full address. `--guide` attaches a reading guide (see [below](#reading-guide-of-a-mounted-page)): PDO then shows the page in a tab with the guide beside it, and the command prints that view's address first. |
| `pdo page list` | List active page mounts. |
| `pdo page unmount <name>` | Remove a page mount. |
| `pdo review list [--state <state>] [--run <id>]` | List review comments for a run. |
| `pdo review reply <id> --text <markdown> [--resolved] [--run <id>]` | Reply to a review comment and optionally propose or record its resolution. |
| `pdo ext` | List the extensions of the running daemon that have a command line, one `<id>\t<summary>` per line. Exit 1 when the daemon is unreachable. |
| `pdo ext <id> <args…>` | Run the command line of extension `<id>` through the daemon. The arguments (flags included), the standard input (when it is a pipe or a file: `… | pdo ext <id>` or `pdo ext <id> < file`; a terminal, `/dev/null` or the socket an agent's shell tool leaves open relay nothing, so the command never waits for its input) and the node session the command runs in reach the extension, which sees the agent `agent:<run>/<node>` from a node session and the local operator otherwise. Its stdout and stderr come back byte for byte. Exit code: the extension's, as is; 1 when the daemon is unreachable or the extension fails; 127 when the daemon has no extension `<id>` or one without a command line. |

Every command that talks to the daemon reaches it at `PDO_DAEMON_URL` (default
`http://localhost:5172`) and, when `PDO_TOKEN` holds a token, sends it as
`Authorization: Bearer <token>`: an extension that identifies its callers (sign-in, personal
tokens) reads it. A daemon whose extension requires one refuses a call without it with `401`
`{"error": "unauthenticated", …}`, which the command prints before it exits non-zero.

Exposing the daemon beyond `localhost` is covered in [reverse-proxy.md](reverse-proxy.md).

## Reading guide of a mounted page

A guide is a YAML (or JSON) list of entries; `text` is optional:

```yaml
- title: Start with option A
  text: The chosen layout. B and C stay for comparison.
  link: "#option-a"
- title: Where it plugs in
  link: "#/runs/20261019-125310-885cb5c/nodes/designer"
- title: The code of the prototype
  link: /runs/20261019-125310-885cb5c/review?file=frontend/proto/
```

A link leads into PDO or into the page:

| Link | Opens |
| --- | --- |
| `#/runs/<run>` | The Run |
| `#/runs/<run>/nodes/<node>` | The node, selected on the Run's canvas |
| `#/runs/<run>/nodes/<node>/artifacts/<port>` | The node's I/O, where its artifact is |
| `/runs/<run>/review?file=<path>` | The Run's Review, on that file (or the first file under that folder) |
| `#/ext/<extension>/…` | An extension's page |
| `#<anchor>` | That anchor of the page |
| `<relative path>` | That file of the page, beneath the mounted directory |

The daemon checks the guide before mounting: at most 50 entries, a title on each, every Run and
node named exists, every path is a file of the page. A guide it refuses mounts nothing, and the
command prints why, for example ``guide entry 2: Run `r1` not found``. The guide is stored with the mount and
goes with it: `pdo page unmount`, or the archive of the Run whose agent mounted the page.

The printed addresses start at the instance's **Public URL** (Settings › General, or
`PDO_PUBLIC_URL`; the stored setting wins), else at `PDO_DAEMON_URL`. A node session receives the
Public URL as `PDO_PUBLIC_URL`, and its preamble names it.
