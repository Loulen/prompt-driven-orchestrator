# Maestro

Visual orchestrator for deterministic Claude Code pipelines.

## Install

```bash
curl -fsSL https://github.com/Loulen/Maestro/releases/latest/download/install.sh | bash
```

This downloads the latest release binary for your platform (Linux/macOS, x86_64/ARM64), verifies the SHA256 checksum, and installs to `~/.local/bin/maestro`.

To install a specific version:

```bash
MAESTRO_VERSION=v0.1.0 curl -fsSL https://github.com/Loulen/Maestro/releases/latest/download/install.sh | bash
```

Then start the daemon:

```bash
maestro daemon
```

Open `http://localhost:5172` in your browser.

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

The Vite dev server starts on `http://localhost:5173` and proxies `/ws` to the daemon at `127.0.0.1:5172`.

### Daemon

```bash
cargo run -p maestro-daemon -- daemon
# or with a custom port:
cargo run -p maestro-daemon -- daemon --port 9999
```

The daemon binds to `127.0.0.1:5172` by default. In dev mode it shows a placeholder page — use the Vite dev server for frontend work.

### Production build

```bash
cd frontend && npm run build && cd ..
cargo build --release -p maestro-daemon
```

The release binary embeds the frontend `dist/` via `rust-embed` and serves it at `/`.

### CLI

```bash
cargo run -p maestro-daemon -- --help
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
