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
| `pdo complete [--auto]` | Complete the current node; `--auto` is reserved for runtime hooks. In a hosted session (`PDO_SESSION_ID`, no `PDO_RUN_ID`): refused, exit 3, nothing changes. |
| `pdo fail --reason <text>` | Fail the current node with a recorded reason. Refused (exit 3) in a hosted session. |
| `pdo skip --reason <text>` | End the current run as skipped when there is legitimately no work. Refused (exit 3) in a hosted session. |
| `pdo wait-user [--message <text>]` | Declare that the current node waits on its user: the node and run turn awaiting-user with the message on the banner (capped at 100 characters). Accepted on any node with a live session; refused (exit 3) on a script node or without a session; a repeat is a no-op. Lifted by the user's Enter in the PDO terminal or by the completion release. In a hosted session (`PDO_SESSION_ID`, no `PDO_RUN_ID`), the session turns awaiting-user instead, lifted by an Enter in its terminal; refused (exit 3) when it is not running. |
| `pdo migrate [--dir <path>] [--dry-run]` | Migrate legacy pipeline YAML files. |
| `pdo reap [--count] [--dry-run] [--ttl-hours <hours>] [--terminal-ttl-hours <hours>] [--budget-secs <seconds>]` | Report or archive old terminal runs according to the retention policy. |
| `pdo docs support-table [--check\|--write] [--file <path>]` | Check or regenerate the harness support table in [harnesses.md](harnesses.md) (the default `--file`). |
| `pdo run create <pipeline> [options]` | Create a run through the daemon, with optional input, repository, harness, sandbox, and provisioning settings. `--input-file <path>` reads the prompt from a file; `--image <path>` / `--file <path>` (repeatable) attach files the entry node sees under `## Input Images` / `## Input Files` (one budget per run, `max_attachments_mb` in Settings). `--ext <id>.<key>=<value>` (repeatable) sends `extensions.<id>.<key>` = `"<value>"`, data PDO never interprets: the extension `<id>` checks it before the Run is created (a refusal prints its reason, exit 1) and receives it before the first node; an id no installed extension has is refused. |
| `pdo run wait [--all] [--timeout <seconds>]` | From a node session, block until a child run of this node becomes terminal (`--all`: every child). Exit 0 with one JSON line per settled child (`run_id`, `name`, `status`, `reason`, `children_active`), or `{"noop":true}` when no child is active; exit 2 when `--timeout` elapses (nothing on stdout); exit 1 when the daemon is unreachable or outside a node session. Without `--timeout` it waits as long as it takes: run it as a background task of your harness when it offers one, otherwise with a `--timeout` no longer than the harness's tool timeout. |
| `pdo run status <run-id> [--json]` | Print the state of one run in a single line: `<id> <status> · nodes <done>/<total> done[, <n> running][, <n> awaiting_user][, <n> failed] · children <n> active · $<cost>`. Nodes are the pipeline's agent and script nodes; `running` includes a node waiting for a session slot, `awaiting_user` a node whose session died on an incident and waits for someone to resume it; the cost reads `$—` when unknown and `(lower bound)` when a model had no price. `--json` prints one compact object with the same fields (`run_id`, `status`, `nodes_done`, `nodes_total`, `nodes_running`, `nodes_awaiting_user`, `nodes_failed`, `children_active`, `cost_usd`, `cost_partial`). A read of the run's detail and children, not a wait. Exit 0 when the run is found, 4 when the daemon knows no such run, 1 when the daemon is unreachable or refuses the read. |
| `pdo run list [--json]` | List the runs the daemon knows, one tab-separated line per run (`run_id`, status, pipeline, name); `--json` prints the daemon's `GET /runs` answer as is. Exit 1 with the daemon's reason when it refuses the call. |
| `pdo run show <run-id> [--json]` | The Run in a line (id, status, pipeline, name), then one line per node: id, status, iteration. `--json` prints the daemon's detail of the Run. |
| `pdo run cancel <run-id> [--json]` | Give up on a live Run: no node is launched any more, every node holding a session is stopped (never failed), and the Run ends `halted` with the reason `cancelled`. It can then be retried or reopened. A Run that is not live is refused, exit 1. |
| `pdo run pause\|resume <run-id> [--json]` | Pause a running Run (nothing new is launched; a node already working still finishes, and what it would launch waits for the resume), or resume a paused, halted or failed one, as the Runs list does. |
| `pdo run retry <run-id> [--json]` | Retry all, as the Runs list does: a finished Run is archived and a new Run starts with the same pipeline, input and repositories; prints `<run-id> retried as <new-id>`. A live Run is refused, exit 1. |
| `pdo run reopen <run-id> [--json]` | Reopen a finished (or incident-parked) Run: what was done stays done, the rest runs again (an interrupted or stopped node included). An archived Run is refused, as in the run view. |
| `pdo run archive <run-id> [--json]` | Archive a Run and its finished child Runs (the Runs list's Cleanup): their worktrees go, their history stays. Refused while a child Run is live. |
| `pdo run forget <run-id> [--json]` | Forget an archived Run. |
| `pdo run rename <run-id> <name> [--json]` | Rename a Run; an empty name clears it. |
| `pdo run repos <run-id> [--add <repo>]… [--read-only] [--remove <repo>]… [--json]` | Add or remove secondary repositories of a running Run. Paths are made absolute like `pdo project`'s. |
| `pdo run inject <run-id> <path> --content <text>\|--file <path> [--json]` | Write an artifact into the Run's blackboard, a path under its artifacts. |
| `pdo run force-route <run-id> --from <node-or-region> --to <node> [--json]` | Leave a node or a loop region through `--to`, past its conditions (`--to end` completes the Run). |
| `pdo run region <run-id> <region-id> --end\|--bump <n>\|--max <n>\|--max-cost <usd\|none> [--json]` | End a loop region, run it `n` more laps, set its lap cap, or set its cost cap in dollars (`none` removes it; under the spend, the loop stops at its next re-entry). |
| `pdo run manager start\|stop <run-id> [--json]` | Start or stop the Run's Pipeline Manager. |
| `pdo run events <run-id> [--json]` | The Run's event log, one tab-separated line per event: time, kind, node, iteration, and who made it (the Actor's display name, else its id; empty for the Run's own transitions). |
| `pdo run costs <run-id>… [--json]` | The cost of Runs, one line each: id and USD (`?` unknown, `≥` a lower bound). |
| `pdo run artifact <run-id> <path> [--zip] [-o <file>]` | One artifact of the Run, or with `--zip` a port folder zipped; written to `-o`, else to stdout. |
| `pdo run file <run-id> <path> [--ref <ref>] [-o <file>]` | A file of the Run's repository at one of its refs (default: its worktree while it exists, else its tip). |
| `pdo run refs\|source-drift <run-id> [--json]` | The Run's refs (`id`, sha, label), or how far its source branch moved since it forked (JSON). |
| `pdo run search <run-id> <text> [--json]` | Search the transcripts of the Run's nodes: node, iteration, entry, speaker or tool, excerpt with the match in `[…]`. |
| `pdo run stale-health [--json]` | The liveness sweep's heartbeat: last tick, interval, nodes blocked on a usage limit. |
| `pdo run pipeline show <run-id> [--json]` | The Run's own pipeline: its YAML, or with `--json` the YAML, the parsed pipeline, its prompts and its diagnostics. |
| `pdo run pipeline export <run-id> [-o <file>] [--skills <zip>]` | The Run's pipeline as a portable document, and its skills sidecar when it carries skills. |
| `pdo run pipeline save <run-id> --yaml <file>\|--file <json> [--json]` | Edit the Run's pipeline mid-run, within what its progress allows: a YAML (its prompts stay), or the `{yaml, prompts}` object `show --json` prints. |
| `pdo run pipeline overwrite-default <run-id> [--preview] [--json]` | Copy the Run's pipeline over the shared one it was launched from; `--preview` says what it would touch (changed since the launch, the Triggers that fire it). |
| `pdo node retry <run-id> <node-id> [--preview] [--json]` | Retry a node, as the canvas does: what it fed downstream is invalidated and it runs again, reopening a finished Run. `--preview` only prints what would be invalidated. |
| `pdo node skip <run-id> <node-id> [--iter <n>] [--output <port>=<content>]… [--json]` | Skip a node: it counts as done with empty outputs (or those given) and the Run goes on. |
| `pdo node restart\|kill\|recover\|done <run-id> <node-id> [--iter <n>] [--json]` | Restart an iteration from scratch, kill it (it fails), recover an interrupted one, or mark it done. Without `--iter`, the node's latest iteration. |
| `pdo node start\|stop <run-id> <node-id> [--json]` | The canvas's Start and Stop; a stopped node is `stopped`, never failed. |
| `pdo node release <run-id> <node-id> [--iter <n>] [--json]` | Release the completion an interactive node holds for its user. |
| `pdo node extend <run-id> <node-id> <n> [--json]` | Run a cycle node `n` more iterations. |
| `pdo node attach <run-id> <node-id> <file>… [--iter <n>] [--json]` | Import files into a node that holds a live session (one budget per import, `max_attachments_mb`). |
| `pdo node prompt\|io <run-id> <node-id> [--iter <n>]` | The prompt an iteration received, or its inputs and outputs (JSON). |
| `pdo node diff <run-id> <node-id>` | The diff the node made on its branch. |
| `pdo node transcript <run-id> <node-id> [--iter <n>] [--after <i>] [--limit <n>] [--tool <index>] [--json]` | A page of the node's transcript, one entry a line (`#index`, speaker or tool, text); `--tool` shows one tool call in full (JSON). |
| `pdo pipeline list [--json]` | The pipelines, one tab-separated line each: id, name, latest version, node count, validation (`all`, `none`, `projects: …`), and why one is invalid. |
| `pdo pipeline show <id> [--json]` | A pipeline's YAML as stored; its diagnostics go to stderr. |
| `pdo pipeline create <name> [--json]` | Create an empty pipeline. A name already taken is refused, exit 1. |
| `pdo pipeline save <id> <file\|-> [--prompt <node>=<file>]… [--json]` | Save a pipeline's YAML and prompts, as the editor's Save does without an open draft: the daemon freezes a new version and prints it. |
| `pdo pipeline rename\|duplicate\|delete <id> …` | `rename <id> <name>`, `duplicate <id>`, `delete <id>`, as the pipeline editor does. |
| `pdo pipeline validate <id> [--none\|--project <id>…\|--first <id>…] [--json]` | The Projects whose launch forms offer the pipeline: all (no flag), none, or the ones given. |
| `pdo pipeline versions <id> [--json]` | The versions a Save froze, newest first: number, title, when, author, Runs launched on it. |
| `pdo pipeline version <id> <n> [--json]` | One version's YAML. |
| `pdo pipeline restore <id> <n> [--json]` | Bring version `n` back: its YAML and prompts are saved as a new version, under the pipeline's current name (Restore then Save in the editor). |
| `pdo pipeline export <id> [-o <file>] [--skills <zip>]` | The pipeline as a portable document, and its skills sidecar when it references skills. |
| `pdo pipeline import <file\|-> [--skills <zip>] [--json]` | Import a portable document (and its skills sidecar): `export` then `import` gives back the same pipeline. |
| `pdo pipeline parse-node <file\|->` | Read one node's YAML as the canvas would add it. |
| `pdo pipeline library list\|save\|delete\|instantiate\|import …` | The node library: `list`, `save <file>` (an entry, JSON or YAML), `delete <name>`, `instantiate <name>` (the node spec and its prompt), `import <workflow.js>` (a Claude Code workflow as a draft pipeline). |
| `pdo page mount <name> <directory> [--guide <file>]` | Serve a directory under `/pages/<name>/` and print its full address. `--guide` attaches a reading guide (see [below](#reading-guide-of-a-mounted-page)): PDO then shows the page in a tab with the guide beside it, and the command prints that view's address first. |
| `pdo page list` | List active page mounts. |
| `pdo page unmount <name>` | Remove a page mount. |
| `pdo review list [--state <state>] [--run <id>]` | List review comments for a run (JSON). |
| `pdo review reply <id> --text <markdown> [--resolved] [--run <id>]` | Reply to a review comment and optionally propose or record its resolution. |
| `pdo review diff [--stat\|--git] [--from <ref>] [--to <ref>] [--run <id>] [--json]` | The Run's diff as the Review page shows it, work not committed yet included: its patch, or with `--stat` one tab-separated line per file (status, `+additions`, `-deletions`, path). `--from`/`--to` compare two Run refs (fork → tip by default): `fork`, `tip`, `worktree` (destination only), `node:<id>:<iter>:before`, `node:<id>:<iter>:after`, `live:<id>` (a running isolated node), or a revision of the Run's repository; an unknown one is named, exit 4. `--json` prints the structured diff (files, hunks, lines). The patch is for reading; `--git` prints git's own patch of the work committed so far, for `git apply`. |
| `pdo review comment <path> <line> --text <markdown> [--side new\|old] [--from <ref>] [--to <ref>] [--run <id>] [--json]` | Write a comment on a line of the Run's diff and send it, as the Review page does: the Pipeline Manager receives it (started if it is not running) and the page lists it. Prints `<comment-id> sent · <path>:<line> · run <run-id>`. The file is named as the page names it (its new path, also for `--side old`). A line the page does not let one comment (between `--from` and `--to`: a line of a hunk, or of the file's content its context expanders reveal) is refused before anything is sent, exit 1. |
| `pdo review resend <id>... [--run <id>] [--json]` | Send comments the manager never answered again, as written, as the Review page's « Send again »: the Pipeline Manager is started if it is not running. A comment already answered or resolved is refused, exit 1. Prints `<ids> sent again · run <run-id>`. |
| `pdo review resolve\|reopen <id> [--run <id>] [--json]` | Resolve a comment, or reopen a resolved one, as the Review page's buttons. |
| `pdo ext` | List the extensions of the running daemon that have a command line, one `<id>\t<summary>` per line. Exit 1 when the daemon is unreachable. |
| `pdo ext <id> <args…>` | Run the command line of extension `<id>` through the daemon. The arguments (flags included), the standard input (when it is a pipe or a file: `… | pdo ext <id>` or `pdo ext <id> < file`; a terminal, `/dev/null` or the socket an agent's shell tool leaves open relay nothing, so the command never waits for its input) and the node session the command runs in reach the extension, which sees the agent `agent:<run>/<node>` from a node session and the local operator otherwise. Its stdout and stderr come back byte for byte. Exit code: the extension's, as is; 1 when the daemon is unreachable or the extension fails; 127 when the daemon has no extension `<id>` or one without a command line. |
| `pdo project list [--deleted] [--json]` | List the Projects you may read, one tab-separated line each: id, name, member count, primary repository. `--deleted`: the deleted Projects (id, last name), which the Runs launched in them keep. |
| `pdo project show <id> [--json]` | Show one Project: name, members, primary repository, harness or agentic profile, auto-fail preference, skills. |
| `pdo project create <name> [--member <path>]… [--json]` | Create a Project with its first member repositories and print `<id>\t<name> created`. A repository path is made absolute from the working directory, without `.`, `..` or a trailing `/` (symlinks are not followed: the daemon compares paths verbatim). |
| `pdo project update <id> [--name <name>] [--harness <h>\|--clear-harness] [--profile <id>\|--agent-harness <h> [--model <m>] [--effort <e>]\|--clear-agent] [--auto-fail on\|off\|inherit] [--primary-repo <path>\|--first-primary] [--json]` | Rename a Project or change the settings it carries, as Settings › Projects does: only the flags given change. No flag: refused, exit 1. |
| `pdo project member add\|remove <id> <path> [--json]` | Attach a repository to the Project (a no-op when it already is one), or detach it. |
| `pdo project skill list\|add\|remove\|set <id> …` | The skills the Project selects for every Run launched in it: `list <id>` (`id\tname` lines), `add <id> <skill> [--name <label>]`, `remove <id> <skill>` (exit 1 when it is not selected), `set <id> [<skill>…]` replaces the selection (a skill already selected keeps its label), none clears it. The daemon refuses a skill that is not in the bank and not already selected (exit 1). Each prints the resulting selection; `--json` prints it as JSON. |
| `pdo project provisioning <id> [--file <path>\|-] [--json]` | Show the Project's provisioning rules, one line per kind (`copy`, `hardlink`, `symlink`), or replace them with the JSON of `--file` (`-` for the standard input), shaped as `--json` prints them. |
| `pdo project delete <id> [--json]` | Delete a Project; its Runs keep it under its last name. `--json` prints `{"id", "deleted": true}`. |
| `pdo trigger list [--json]` | List the Triggers, one tab-separated line each: id, state (`enabled`, `disabled`, `suspended`), cron, pipeline, name, next fire. While every Trigger is paused (`pause --all`), standard error says so. |
| `pdo trigger show <id> [--json]` | Show one Trigger: pipeline, cron, repository, Project, guard, overlap, next and last fire, input. |
| `pdo trigger create <name> --pipeline <id> --cron <expr> [--repo <path>] [--project <id>] [--source-branch <b>] [--input <text>\|--input-file <path>] [--var <key>=<value>]… [--guard <cmd>] [--overlap skip\|allow] [--max-concurrent <n>] [--sandbox <mode>] [--harness <h>] [--profile <id>\|--agent-harness <h> [--model <m>] [--effort <e>]] [--secondary-repo <path>[=<branch>]]… [--read-only-repo <path>[=<branch>]]… [--skill <id>[=<label>]]… [--no-auto-name] [--json]` | Create a Trigger, a Run template its 5-field cron fires. Repository paths are made absolute like `pdo project`'s. The daemon validates it as the Triggers surface does: an invalid cron, a missing repository, a skill the bank does not hold or a prompt-required pipeline without input is refused with its reason, exit 1. |
| `pdo trigger update <id> [the flags of create] [--auto-name on\|off] [--clear-guard] [--clear-max-concurrent] [--clear-sandbox] [--clear-harness] [--clear-project] [--clear-skills] [--clear-agent] [--clear-secondary-repos] [--json]` | Change a Trigger: only the flags given change; `--skill` and the secondary repositories replace their list. The daemon refuses a `--skill` the bank does not hold (exit 1), unless the Trigger already carries it. No flag: refused, exit 1. |
| `pdo trigger fire <id> [--json]` | Fire a Trigger now (the surface's Run now) and print `<id> fired: run <run-id>`, or why it did not fire (a guard or overlap skip, exit 0; an error outcome, such as a refused Run or a guard that could not run, exit 1). A disabled or suspended Trigger is refused, exit 1. On a Trigger a missing pipeline or repository put to sleep, a fire once they are back also resumes its schedule. |
| `pdo trigger pause\|resume <id> [--json]` | Disable a Trigger (it fires neither on its cron nor by `fire`) or enable it again; `disable` and `enable` are aliases. |
| `pdo trigger pause\|resume --all [--json]` | Flip the global switch: every Trigger paused, or resumed. |
| `pdo trigger fires <id> [--json]` | The fires of a Trigger, newest first: when, how (`manual`, `cron`), outcome, Run, reason. |
| `pdo trigger test-guard --guard <cmd> [--repo <path>] [--json]` | Run a guard command once, as a fire would, without firing anything: `pass`, `skip` with its exit code, or `error`, then its stdout and stderr. |
| `pdo trigger health [--json]` | The heartbeat: last tick, interval, whether every Trigger is paused. |
| `pdo trigger delete <id> [--json]` | Delete a Trigger. `--json` prints `{"id", "deleted": true}`. |
| `pdo session status [--json]` | The status bar's gauges in one line: version, live node sessions and the cap, last orphan sweep. |
| `pdo session show\|events <id> [--json]` | A hosted session: its extension, kind, state, folder and configuration; or its events, oldest first (`at`, kind, who, message). |
| `pdo session terminate\|relaunch <id> [--json]` | Terminate a hosted session, or relaunch an ended one (resuming its conversation when its harness can). |
| `pdo session configure <id> [--harness <h>] [--model <m>] [--effort <e>] [--prompt <text>] [--json]` | Change its configuration, applied at its next relaunch: only the flags given change, an empty value clears the field. |
| `pdo settings get [<key>…] [--json]` | The instance settings, one tab-separated line each: key, effective value, where it comes from (`stored`, `env`, `default`). The instance's own tiers (`skills`, `agent_choice`) follow as stored, source `stored`; the facts shown beside the settings (harness catalogues, price table…) are left out. With keys, those settings only, each with its whole value; an unknown key is refused, exit 1. |
| `pdo settings set <key>=<value>… [--file <path>\|-] [--json]` | Change instance settings as Settings saves them. A value is read as JSON when it is JSON (`session_cap=5`, `review_agent_can_resolve=true`, `skills='[{"id":"tdd"}]'`), as a string otherwise (`public_url=https://pdo.example.net`); `<key>=` clears a text setting; `--file` gives the change as a JSON object. The daemon validates the whole change as for the UI (ranges, a staging profile, harness or agentic profile that must exist, skills of the bank) and refuses it with its reason, writing nothing, exit 1. Prints each setting changed (`skills` included). |
| `pdo settings refresh-harness <name> [--json]` | Probe a harness's catalogue (models, efforts) again now. |
| `pdo settings agent-profile list\|show\|create\|update\|referents\|delete …` | The agentic profiles of Settings › Agents: `list` (id, name, harness, model, effort), `show <id>`, `create <name> --harness <h> [--model <m>] [--effort <e>]`, `update <id> [--name] [--harness] [--model] [--effort]` (an empty value clears model or effort), `referents <id>` (who names it), `delete <id>` (even a profile something names: it falls back to the next tier; never `Default`). |
| `pdo settings sandbox-profile list\|show\|set\|referents\|delete …` | The staging profiles of Settings › Sandbox: `list`, `show <name>` (the paths it stages, its environment, its image), `set <name> [--disable <path>]… [--enable <path>]… [--extra <path>]… [--remove-extra <path>]… [--env <k>=<v>]… [--unset-env <k>]… [--dockerfile <path>\|--registry <ref>\|--default-image]` (only the flags given change; a profile not stored yet starts from the default), `referents <name>`, `delete <name>`. |
| `pdo settings provisioning show\|set <file\|->\|preview --repo <path> [--git-ref <ref>] [--project <id>] [--file <path>]` | The instance's provisioning rules, shown or replaced (JSON shaped as `show --json`); `preview` lists what a worktree of the repository would receive, one line per path (mode, path, level, pattern), then the conflicts. |
| `pdo settings appearance [--file <path>\|-] [--json]` | Your Appearance (Theme, Dark mode, Colour-blind friendly status colours, Compact nodes, hidden elements), one choice a line, `default` for one left at its default, or replace it with a document shaped as `--json` prints it under `appearance`. |
| `pdo settings telemetry [--test] [--json]` | The OpenTelemetry export: exporting or not, `/metrics`, the last export; `--test` exports one measure now (exit 1 when no collector is set). |
| `pdo settings sync-prices [--json]` | Refresh the price table of the harnesses' models from its source. |
| `pdo stats runs\|sessions\|triggers\|cost\|performance [--period 7d\|30d\|90d\|all] [--from <day> --to <day>] [--bucket day\|week\|month] [--completed-only\|--every-run] [--uncombined] [--json]` | A tab of Stats, with the figures it shows: Runs (runs, failed, failure rate, busiest bucket, then one line per bucket), Sessions (by harness, by Pipeline), Triggers (fires that created a Run, Triggers fired of the enabled ones, fires by Pipeline), Cost (total and median, by harness, Pipeline, Project, model), Performance (median peak context, median duration, steered share, by harness, by Pipeline and for the Infrastructure row; `--duration total\|active\|waiting` picks the duration as the tab's toggle does, `active` by default; `--refresh` reads the transcripts again; `No successful executions in this period.` when there is none, as the tab says). The period is the surface's (`30d` by default, as Stats opens: today and the 29 days before, in UTC days), and so is each tab's cohort (Performance counts completed Runs only); `--json` prints the daemon's answer. |
| `pdo stats absorptions\|combine\|uncombine …` | The absorptions: `absorptions` lists them; `combine --dimension pipeline\|node\|model\|effort\|couple [--scope <key>[=<name>]] --into <key>[=<name>] --member <key>[=<name>]…` counts the members under the absorbent; `uncombine --dimension <d> [--scope <key>] --member <key>` counts a member on its own again (exit 4 when it is not combined). |
| `pdo skill list\|show\|add\|update\|remove\|referents …` | The skill bank: `list` (id, name, folder, `locked`), `show <id>` (its facts, files and `SKILL.md`), `add <SKILL.md\|-> [--name <label>] [--folder <id>]`, `update <id> [--name] [--folder <id>\|--root] [--content <SKILL.md>]`, `remove <id>`, `referents <id>`. A skill is named by its id or its name, here and in `pdo skill file`. A skill an extension (or the core) provides is locked: the daemon refuses to change or remove it, exit 1; so is a name the bank holds or an invalid `SKILL.md`. |
| `pdo skill file add\|show\|write\|remove <id> …` | A skill's reference files: `add <id> <file>… [--as <path>]`, `show <id> <path>`, `write <id> <path> <file\|->` (writing `SKILL.md` validates it as the editor does), `remove <id> <path>`. |
| `pdo skill folder list\|add\|update\|remove\|rescan\|sync …` | The folders of the bank: `add <name> [--parent <id>]`, `update <id> [--name] [--parent <id>\|--root]`, `remove <id>`; for a folder imported from a Source, `rescan <id>` compares it with the Source, `sync <id> [--new] [--only <path>]…` brings the changed skills (and the new ones with `--new`) up to date. |
| `pdo skill scan\|import\|cancel-scan\|sources …` | A Source's skills: `scan <source> [--scan-id <id>]` lists its candidates (status, path, name); `import <source> [--only <path>]… [--on-collision skip\|replace] [--folder <id>\|--folder-name <name> [--parent <id>]]` imports its new valid skills (exit 1 when there is none); `cancel-scan <id>` stops a scan; `sources` lists the Sources imported lately. |
| `pdo skill export --skill <skill>… --folder <id>… -o <zip>` / `pdo skill upload <zip>` | Export skills (each by its id or its name) and folders as a zip, or upload such a zip: its new skills go to a folder named after the file, the ones the bank holds are kept. |
| `pdo update status\|check\|changelog\|apply\|log <attempt> [--json]` | The version section of Settings › General: the installed and latest versions and whether an update may run now; a check of the release source now; « What's new »; the update itself (detached, then the daemon restarts); an attempt's journal. |
| `pdo audit [--from <rfc3339>] [--to <rfc3339>] [--target-kind <kind>] [--target-id <id>] [--limit <n>] [--json]` | The journal of the mutations made outside a Run, newest first, one tab-separated line each: time, action, target kind, target id, actor hint, and what it changed to when that is short (`{"paused":true}`). |
| `pdo whoami [--json]` | The Actor the daemon names you: `local_operator`, `agent:<run>/<node>` from a node session, or `<namespace>:<id>` when an extension establishes you from `PDO_TOKEN`, followed by its display name. |

The gestures of `pdo run` (all but `create`, `wait` and `list`), `pdo node`, `pdo pipeline`,
`pdo project`, `pdo trigger`, `pdo session`, `pdo settings`, `pdo stats`, `pdo skill`, `pdo update`,
`pdo review`, `pdo audit` and `pdo whoami` are thin clients of the daemon's routes: they send what the UI sends, print what it answers in text or, with `--json`, as
JSON, and leave every rule to the daemon (who may, which state allows what). Their exit codes: 0
success; 1 when the daemon is unreachable or refuses the call (its reason on stderr); 2 for a
command line they cannot read; 4 when the daemon knows no such Run, node, pipeline, Project, Trigger,
session, review comment, profile, skill, folder or update attempt. Every route of the daemon has a
command or a written exclusion: see `crates/tidy/src/cli_parity/exclusions.rs`.

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
| `#/runs/<run>/nodes/<node>` | The node, selected on the Run's canvas, its detail shown |
| `#/runs/<run>/nodes/<node>/iter/<n>` | The same, at its iteration `n` |
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
