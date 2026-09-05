# Prompt Driven Orchestrator (PDO)

PDO is a visual orchestrator for software-development agents.

![PDO web UI showing a feature pipeline, a live agent session, and typed output.](docs/pdo-ui.png)

*A bounded implementation loop with a live terminal and typed output.*

| Capability | Result |
| --- | --- |
| Visual pipelines | Build agent workflows on a canvas |
| Deterministic routing | Route nodes with mechanical rules |
| Typed outputs | Validate, preserve, and pass structured artifacts |
| Interactive sessions | Watch, guide, or take over from the web terminal |
| Triggers | Start runs on a schedule or from a script |

## Quick start

### 1. Install PDO

Homebrew works on macOS and Linux.

```bash
brew install Loulen/tap/pdo
```

The install script supports Linux and macOS on x86_64 and ARM64.

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh | sh
```

Both methods install a checksum-verified `pdo` binary on your `PATH`.

| Task | Command |
| --- | --- |
| Update with Homebrew | `brew upgrade pdo` |
| Update with the script | Run the install script again |
| Install a specific release | Replace `latest` with a tag such as `v1.31.2` |

### 2. Install runtime requirements

| Requirement | Purpose |
| --- | --- |
| `tmux` | Runs node and shell sessions |
| `git` | Creates an isolated worktree for each node |
| An agent harness | Runs the agent inside each node |

```bash
brew install tmux git
```

PDO includes descriptors for `claude`, `opencode`, and `copilot`.

### 3. Start PDO

```bash
pdo daemon
```

Open [http://localhost:5172](http://localhost:5172).

### 4. Run PDO as a service

```bash
pdo service install
pdo service status
pdo service uninstall
```

`pdo service install --port <port>` changes the default port, and `--dry-run` previews the service definition.

## Reverse proxy

The daemon listens on `0.0.0.0:<port>` without authentication or TLS.

Put authentication and TLS in front of PDO before exposing it beyond a trusted network.

Set every public browser origin as an exact `scheme://host[:port]` value.

```bash
PDO_ALLOWED_WS_ORIGINS="https://pdo.example.tld,https://pdo.internal:8443" pdo daemon
```

| Setting | Behavior |
| --- | --- |
| Default WebSocket origins | `localhost` and `127.0.0.1:<port>` |
| Extra WebSocket origins | Comma-separated `PDO_ALLOWED_WS_ORIGINS` values |
| WebSocket routes | `/ws` and `/sessions/<id>/pty` |
| TLS proxy | The UI uses `wss://` automatically |

Add `PDO_ALLOWED_WS_ORIGINS` to the service environment when the daemon runs as a service.

## Harness support

<!-- support-table:begin -->
<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: run `make support-table`. `make check` fails if this block has drifted. -->

PDO can launch, attach, resume, and complete nodes with every built-in harness.

| Capability | What PDO does with it | `claude` 2.1.246 | `opencode` 1.18.18 | `copilot` 1.0.80 |
| --- | --- | --- | --- | --- |
| **Cost** | Show the Run cost | ✅ derived: per-message token usage × the price table | ❌ | ✅ reported: the harness's own billing unit × a published constant |
| **Transcript** | Find the session transcript | ✅ the JSONL transcript, keyed by working directory | ❌ | ✅ the event journal, keyed by the session identity PDO imposed |
| **End of turn** | Complete a node when its turn ends | ✅ an injected `Stop` hook, plus the transcript tail as the sweep's fallback | ❌ | ✅ the journal's explicit `assistant.turn_end` event |
| **Usage-limit menu** | Detect the harness usage-limit menu | ✅ the interactive "wait for limit to reset" menu, matched in a pane capture | ❌ | ❌ |
| **Sandbox staging set** | Stage the harness home in a sandbox and disarm its blocking dialogs | ✅ the `.claude` home: credentials and org managed settings copied, trust and permissions bypass fixed up, transcripts harvested back | ❌ | ❌ |
| **Context usage** | Show peak context-window usage | ✅ derived: per-turn token usage from the transcript, deduplicated and maxed | ❌ | ✅ derived: the journal's cumulative usage counters, converted to a per-turn contribution and maxed |

Each header shows the last validated harness version; PDO does not enforce it. The sandbox image is not provided by PDO: it is the profile's image, and the harness binary must already be in it (ADR-0063).

Custom descriptors in `~/.pdo/harnesses/descriptors.yaml` can launch, attach, resume, and complete nodes through `pdo complete`.

<!-- support-table:end -->

## Harness prerequisites

| Requirement | Setup |
| --- | --- |
| Authentication | Log in with each harness before using it through PDO |
| An approved working directory | Trust the target repository root once; trust cascades to subdirectories |
| An installed version | Compare your installed harness with the last validated version in the support table |

Outside sandboxed runs, PDO does not stage any harness's home.

Inside a sandbox, the image and profile define the available harness configuration.

## How PDO works

| Principle | Behavior | Reference |
| --- | --- | --- |
| Deterministic orchestration | Typed outputs and graph rules choose the next node | [ADR-0002](docs/adr/0002-mechanical-conditionals-only.md), [ADR-0011](docs/adr/0011-conditional-edges-and-loop-regions.md) |
| Typed artifacts | Each node emits validated frontmatter and a content body | [ADR-0020](docs/adr/0020-archive-preserves-outputs.md) |
| Expert control | Interactive nodes wait for input and expose a live terminal | [ADR-0005](docs/adr/0005-inline-xterm-over-os-spawn.md) |
| Deliberate autonomy | Only nodes placed in the pipeline can push, open PRs, or merge | [ADR-0012](docs/adr/0012-triggers-and-trust-earned-autonomy.md) |
| Local agent setup | Sessions use your harness configuration and skills | [ADR-0045](docs/adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md) |

See [CONTEXT.md](CONTEXT.md) for the domain model.

## Development

### Prerequisites

| Tool | Version |
| --- | --- |
| [Rust](https://rustup.rs/) | Stable |
| [Node.js](https://nodejs.org/) | 22 or newer |
| [pnpm](https://pnpm.io/) | Use the version declared by the project |

### Frontend

```bash
cd frontend
pnpm install
pnpm run dev
```

The Vite server runs at [http://localhost:5173](http://localhost:5173) with hot reload.

It proxies API and WebSocket traffic to `127.0.0.1:5172`.

### Daemon

```bash
cargo run -p pdo-daemon -- daemon
cargo run -p pdo-daemon -- daemon --port 9999
```

The daemon serves the embedded frontend at [http://localhost:5172](http://localhost:5172).

### Production build

```bash
cd frontend && pnpm run build && cd ..
cargo build --release -p pdo-daemon
```

The release binary embeds `frontend/dist/`.

### CLI

```bash
cargo run -p pdo-daemon -- --help
```

### Build and test commands

| Purpose | Command |
| --- | --- |
| Check Rust | `cargo check --workspace --all-targets` |
| Test Rust | `cargo test --workspace` |
| Lint Rust | `cargo clippy --workspace --all-targets -- -D warnings` |
| Check Rust formatting | `cargo fmt --all --check` |
| Type-check frontend | `cd frontend && pnpm run typecheck` |
| Test frontend | `cd frontend && pnpm run test` |
| Lint frontend | `cd frontend && pnpm run lint` |
| Build frontend | `cd frontend && pnpm run build` |

## Architecture

| Resource | Content |
| --- | --- |
| [CONTEXT.md](CONTEXT.md) | Domain glossary and module map |
| [`docs/adr/`](docs/adr/) | Architecture decisions |
