# Prompt Driven Orchestrator (PDO)

**PDO is a graphical, agentic orchestrator built for software development.**
You design pipelines on a visual canvas and run them on a deterministic runtime that keeps agents on the rails.

- **Deterministic routing** decides what runs next by mechanical rules, not an LLM's guess, so pipelines don't drift.
- **Typed, enforced outputs** make every step produce a structured artifact you can read and trust, so you always understand what happened.
- **Interactive Claude Code sessions** back every node, with a terminal right in the web UI: step in any time to watch, correct, or take over the development loop.
- **Automated triggers** start pipelines on a schedule or from a script's signal, hands-free.

![PDO web UI: a feature pipeline mid-run, with a bounded implement/test loop on the canvas, a live Claude Code session in the web terminal, and a typed output (verdict, iteration, screenshots) on the right.](docs/pdo-ui.png)

*A feature pipeline mid-run: a bounded implement/test loop, a live session in the web UI, and typed outputs the runtime routes on.*

## Install

**Homebrew** (macOS + Linux) — upgrade later with `brew upgrade pdo`:

```bash
brew install Loulen/tap/pdo
```

**Or the install script** (Linux/macOS, x86_64/ARM64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-installer.sh | sh
```

Both fetch the prebuilt binary for your platform, verify its checksum, and install `pdo` to `~/.local/bin`.

To install a specific version, use that release's installer:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/download/v1.31.2/pdo-installer.sh | sh
```

### Runtime requirements

The installer fetches only the `pdo` binary. The daemon shells out to a couple of tools on the host at runtime — install them yourself (they are **not** bundled, and their absence surfaces only when a node tries to run):

- **tmux** — every node and run shell is a tmux session on the host. Required, always, whatever harness you use.
- **git** — each node's work is isolated in a git worktree. Required.

Each node runs its agent through a **harness**, which is your choice, not a fixed dependency. PDO ships descriptors for `claude` (default) and `opencode`, and the set is pluggable ([ADR-0045](docs/adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md)). PDO neither bundles nor requires any specific harness: installing the harness and **its** own dependencies is up to you — e.g. for Claude Code, the `claude` CLI, plus Node.js >= 22 for MCP servers and ripgrep for search.

Then start the daemon:

```bash
pdo daemon
```

Open `http://localhost:5172` in your browser.

For **unattended Triggers** (the daemon starts at boot and survives logout, instead of dying when you close your session), install it as a persistent service:

```bash
pdo service install          # systemd --user unit (Linux) / launchd LaunchAgent (macOS)
pdo service install --dry-run # preview the unit + commands, change nothing
pdo service status            # inspect the installed service
```

### Behind a reverse proxy

The daemon binds `0.0.0.0:<port>` with **no authentication and no TLS** — it is meant to sit behind something that provides both. As a DNS-rebinding / cross-site-WebSocket-hijacking guard, both WebSockets (`/ws`, the dashboard event stream, and `/sessions/<id>/pty`, the terminal) reject any browser `Origin` other than `localhost` / `127.0.0.1:<port>`. Behind a reverse proxy or an ALB on a public domain the browser sends the *public* origin (`https://pdo.example.tld`), so with the default allowlist the terminal and dashboard go dark (HTTP 403).

Name the public origin(s) with `PDO_ALLOWED_WS_ORIGINS`, a comma-separated list of **exact** origins (`scheme://host[:port]`, as the browser sends them) that **add to** the localhost defaults — loopback keeps working:

```bash
PDO_ALLOWED_WS_ORIGINS="https://pdo.example.tld,https://pdo.internal:8443" pdo daemon
```

Set it in the service unit (`pdo service install` runs `pdo daemon` for you; add the variable to that unit's environment). Nothing changes on the client: the UI derives its WebSocket URL from `window.location`, so `wss://` behind TLS already works. This only restores the origin guard for your domain — it adds **neither auth nor TLS**; the proxy must carry authentication. The "Mono-user, local" posture is unchanged.

## Design principles

**Deterministic orchestration.** The graph decides what runs next by mechanical rules on typed outputs, never an LLM router. Agents do the work inside each node, but they never decide the path between nodes, so a pipeline runs the same way every time and does not drift. ([ADR-0002](docs/adr/0002-mechanical-conditionals-only.md), [ADR-0011](docs/adr/0011-conditional-edges-and-loop-regions.md))

**Tailored, typed outputs you can read and trust.** This is the heart of PDO. Each node emits a document whose shape you designed: a schema-checked frontmatter (verdict, score, decisions) plus a body that can be markdown, a mermaid diagram, an image set, or whatever fits the decision. The runtime validates it on completion (with one bounded chance for the agent to self-correct), routes the graph from it, feeds it to the next node as compact context, and keeps it after the run as a durable, auditable record. Your solution's reasoning becomes structured knowledge instead of a lost chat log. ([ADR-0020](docs/adr/0020-archive-preserves-outputs.md))

**Expert in the loop.** The evolution of "human in the loop". Rather than babysitting a run, the expert is handed only the information a decision actually needs, in the format built to convey it. That is the reason the outputs are typed and tailored: less watching, more pertinent input. And when you do step in, every node is a real Claude Code session with a terminal in the web UI, so you can watch, converse, correct, or take over. Mark a node `interactive` and it waits for your input by design. ([ADR-0005](docs/adr/0005-inline-xterm-over-os-spawn.md))

**Deliberate, then autonomous.** The default keeps the expert in the loop; full autonomy is something a pipeline earns, never a favor the runtime grants. PDO never pushes, opens PRs, or merges on its own: only a node you placed does. A pipeline behaves identically whether you launch it by hand or a trigger fires it. ([ADR-0012](docs/adr/0012-triggers-and-trust-earned-autonomy.md))

**It's your Claude Code.** PDO does not manage skills or config. Sessions load your `~/.claude` skills, your MCP servers, and your setup verbatim.

See [CONTEXT.md](CONTEXT.md) for the full domain model.

## Prerequisites (development)

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) >= 22

## Local development

### Frontend (Vite HMR)

```bash
cd frontend
npm install
npm run dev
```

The Vite dev server starts on `http://localhost:5173` and proxies `/ws`, `/sessions` and the REST routes to the daemon at `127.0.0.1:5172`. The two WebSocket proxies set `rewriteWsOrigin: true` so the daemon's Origin check (see *Behind a reverse proxy*) accepts the dev server — without it the dashboard and terminal would 403 in `make dev`.

### Daemon

```bash
cargo run -p pdo-daemon -- daemon
# or with a custom port:
cargo run -p pdo-daemon -- daemon --port 9999
```

The daemon binds `0.0.0.0:5172` by default and serves the real embedded UI (the frontend `dist/` is built by `build.rs` and bundled via `rust-embed`), so `http://localhost:5172` works on its own. For frontend work, use the Vite dev server anyway — it gives you HMR and proxies API + WebSocket calls back to the daemon.

### Production build

```bash
cd frontend && npm run build && cd ..
cargo build --release -p pdo-daemon
```

The release binary embeds the frontend `dist/` via `rust-embed` and serves it at `/`.

### CLI

```bash
cargo run -p pdo-daemon -- --help
```

## Build & test commands

| Purpose             | Command                                              |
| ------------------- | ---------------------------------------------------- |
| Type-check Rust     | `cargo check --workspace --all-targets`              |
| Test Rust           | `cargo test --workspace`                             |
| Lint Rust           | `cargo clippy --workspace --all-targets -- -D warnings` |
| Format Rust         | `cargo fmt --all --check`                            |
| Type-check frontend | `cd frontend && npm run typecheck`                   |
| Test frontend       | `cd frontend && npm run test`                        |
| Lint frontend       | `cd frontend && npm run lint`                        |
| Build frontend      | `cd frontend && npm run build`                       |

## Architecture

See [CONTEXT.md](CONTEXT.md) for the domain glossary and `docs/adr/` for architectural decisions.
