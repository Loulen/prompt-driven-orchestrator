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

Homebrew (macOS and Linux):

```bash
brew install Loulen/tap/pdo
```

Or the install script (Linux and macOS, x86_64 and ARM64):

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Loulen/prompt-driven-orchestrator/releases/latest/download/pdo-daemon-installer.sh | sh
```

Both install a checksum-verified `pdo` binary and put it on your `PATH`. Update with `brew upgrade pdo` or by re-running the script. Pin a version by replacing `latest` with a tag, e.g. `.../releases/download/v1.31.2/pdo-daemon-installer.sh`.

### Runtime requirements

The binary embeds the whole web UI. The daemon shells out to two tools you install separately:

- **tmux.** Every node and run shell is a tmux session.
- **git.** Each node runs in its own git worktree.

```bash
brew install tmux git   # or your system package manager
```

Each node drives its agent through a harness of your choice. PDO ships descriptors for `claude` (the default), `opencode` and `copilot`, and the set is pluggable ([ADR-0045](docs/adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md)). You install the harness and its own dependencies. For Claude Code that means the `claude` CLI, Node.js >= 22 for MCP servers, and ripgrep. What each harness can and cannot do is in [Support](#support); what you have to set up yourself is in [Prerequisites](#prerequisites).

Start the daemon:

```bash
pdo daemon                 # http://localhost:5172
```

To run it at boot and survive logout, install it as a service (this works as-is after a Homebrew install):

```bash
pdo service install        # add --port to change 5172, --dry-run to preview
pdo service status
pdo service uninstall
```

### Behind a reverse proxy

The daemon binds `0.0.0.0:<port>` with **no authentication and no TLS** — it is meant to sit behind something that provides both. As a DNS-rebinding / cross-site-WebSocket-hijacking guard, both WebSockets (`/ws`, the dashboard event stream, and `/sessions/<id>/pty`, the terminal) reject any browser `Origin` other than `localhost` / `127.0.0.1:<port>`. Behind a reverse proxy or an ALB on a public domain the browser sends the *public* origin (`https://pdo.example.tld`), so with the default allowlist the terminal and dashboard go dark (HTTP 403).

Name the public origin(s) with `PDO_ALLOWED_WS_ORIGINS`, a comma-separated list of **exact** origins (`scheme://host[:port]`, as the browser sends them) that **add to** the localhost defaults — loopback keeps working:

```bash
PDO_ALLOWED_WS_ORIGINS="https://pdo.example.tld,https://pdo.internal:8443" pdo daemon
```

Set it in the service unit (`pdo service install` runs `pdo daemon` for you; add the variable to that unit's environment). Nothing changes on the client: the UI derives its WebSocket URL from `window.location`, so `wss://` behind TLS already works. This only restores the origin guard for your domain — it adds **neither auth nor TLS**; the proxy must carry authentication. The "Mono-user, local" posture is unchanged.

## Support

<!-- support-table:begin -->
<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: run `make support-table`. `make check` fails if this block has drifted. -->

PDO ships these harnesses compiled in. **Launching, attaching, resuming and completing a node work on every one of them.** Everything *beyond* launching is a **capability**, written harness by harness — and a harness that lacks one says so rather than quietly doing nothing.

| Capability | What PDO does with it | `claude` 2.1.246 | `opencode` 1.18.18 | `copilot` 1.0.80 |
| --- | --- | --- | --- | --- |
| **Cost** | Turn a Run into a dollar figure. Absent ⇒ the Run's cost reads "—" and names the harness, never `$0` | ✅ derived — per-message token usage × the price table | ❌ | ✅ reported — the harness's own billing unit × a published constant |
| **Transcript** | Find the session's transcript on disk — what cost and end-of-turn read | ✅ the JSONL transcript, keyed by working directory | ❌ | ✅ the event journal, keyed by the session identity PDO imposed |
| **End of turn** | Complete a node by itself when its turn ends. Absent ⇒ the agent runs `pdo complete`, or you do | ✅ an injected `Stop` hook, plus the transcript tail as the sweep's fallback | ❌ | ✅ the journal's explicit `assistant.turn_end` event |
| **Usage-limit menu** | Notice a session parked on the harness's usage-limit menu (informational, no recovery) | ✅ the interactive "wait for limit to reset" menu, matched in a pane capture | ❌ | ❌ |
| **Sandbox staging floor** | Hold a sandboxed session's staged home — credentials, settings, pre-granted trust | ✅ a staged `.claude` home — credentials, org managed settings, pre-granted trust | ❌ | ❌ |

The version beside each harness is the **last validated** one — the build PDO's knowledge of that harness was measured against. It is a documented bound, not a guard: PDO launches on whatever version you have installed and says nothing about the difference. It is written down because the same harness can sit on one machine twice, months apart, with different event schemas and different model lists — and an inventory taken against the wrong install is worse than no inventory.

Why a capability is absent:

| Harness | Capability | Why |
| --- | --- | --- |
| `opencode` | Cost | It writes its own per-message cost into a SQLite in four buckets that do not map onto `claude`'s. A cost is code, never a declared mini-language (ADR-0045), and nobody has written that code yet. |
| `opencode` | Transcript | It migrated its sessions into a SQLite and left months of dead JSON on disk. A store is not a contract, so PDO declares no resolution rather than read zeros off stale files. |
| `opencode` | End of turn | It exposes no end-of-turn signal PDO can read: its argv template carries no `{settings}` hole for a `Stop` hook, and it has no transcript for a sweep to tail (see above). |
| `opencode` | Usage-limit menu | The menu wording is `claude`'s. Matching another harness's pane against it would invent a state, and the probe triggers no recovery anyway (ADR-0012). |
| `opencode` | Sandbox staging floor | Configuring a harness is a documented prerequisite, not PDO code. A sandboxed Run on it holds by your image and the profile's `$HOME` exceptions, and PDO says so once, visibly. |
| `copilot` | Usage-limit menu | The menu wording is `claude`'s, its own documentation admits the textual anchor drifts each release, and the probe triggers no recovery (ADR-0012). Declaring it absent degrades nothing actionable. |
| `copilot` | Sandbox staging floor | Configuring a harness is a documented prerequisite, not PDO code (ADR-0031). A sandboxed Run on it holds by your image and the profile's `$HOME` exceptions, and PDO says so once, visibly. |

A harness **you** declare in `~/.pdo/harnesses/descriptors.yaml` carries no code, so it is absent on all five — it still launches, attaches, resumes, and completes when its agent runs `pdo complete`. That is a legitimate way to run a harness, not a broken one.

<!-- support-table:end -->

## Prerequisites

PDO neither embeds nor installs a harness, and it does not configure one for you. Three things are yours to set up. None of them is code PDO could write on your behalf, and none of them is a bug when it is missing — but each one turns into a node that goes nowhere, so they are worth naming.

**Authentication.** Log each harness in yourself, the way its own documentation says. PDO launches the binary and inherits whatever session you already have — it never handles your credentials. **Outside a sandboxed Run, PDO does not stage any harness's home**: sessions load your `~/.claude`, your MCP servers, your `copilot` config, verbatim. Inside a sandbox it stages a home only for a harness that declares a staging floor (see [Support](#support)); for the others, a sandboxed Run holds by your image and the profile's `$HOME` exceptions, and PDO says so once rather than pretending.

**An approved working directory.** A harness may gate its first turn behind a one-time "do you trust this folder?" dialog, and **the autonomy flags do not cover it**. Measured on `copilot`: `--allow-all` and `--no-ask-user` remove every tool, path and URL permission prompt, and the trust dialog still blocks interactive mode — the node sits there alive and mute, which is the least readable failure there is. The fix is one approval, not a flag: **trust cascades to subdirectories**, so trusting your target repository's root once covers every node sub-worktree PDO creates beneath it, for every Run, forever. Do it once, by hand, before the first Run on a new repo.

**An installed version.** The [Support](#support) table names the **last validated version** of each harness. PDO does not read your installed version, does not compare it, and will not refuse to launch on a different one — it is a bound you can read, not a guard that runs. Worth checking which build you actually have before trusting a row: half of a first `copilot` inventory was taken against a second, months-old install of the same harness sitting on the same machine.

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
