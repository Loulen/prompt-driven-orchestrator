# Contributing to PDO

## License

PDO is [MIT licensed](LICENSE). By opening a pull request you agree that your contribution
is released under the same license. Only contribute code you have the right to
contribute: work done under an employment or client contract may belong to that employer
or client. Agent-generated code is fine, and the project itself is built that way; you
remain the author of record.

The maintainer does not intend to change the license of this code. If that ever changed,
it would be announced in the CHANGELOG with a major version bump, and previous versions
would remain MIT.

## Workflow

The git flow lives in `.agents/skills/git-flow/SKILL.md`. In short: branch from `main`
(`feature/<issue>-<slug>` or `fix/<issue>-<slug>`), open a PR against `main`, and let CI
run. A maintainer reviews and merges. Nobody merges their own PR.

Before pushing:

```bash
make check        # cargo check, frontend typecheck, harness support table
make test         # daemon and frontend tests
```

Commit messages follow Conventional Commits with a French or English body, as the rest of
the history does. Reference the issue in the title.

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

Every command is documented in [docs/reference/cli.md](docs/reference/cli.md).

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

### Harness support table

The table in [docs/reference/harnesses.md](docs/reference/harnesses.md) is generated from
`crates/pdo-daemon/src/harness_probes.rs`. Never edit the block by hand: run
`make support-table` after adding a harness, adding a capability, or moving a "last validated
version". `make check` fails if the committed block has drifted from the code.

## Architecture

| Resource | Content |
| --- | --- |
| [CONTEXT.md](CONTEXT.md) | Domain glossary and module map |
| [`docs/adr/`](docs/adr/) | Architecture decisions |
| [docs/features.md](docs/features.md) | What each feature does, and how PDO works |
| [docs/reference/](docs/reference/) | CLI, reverse proxy, terminal, harness support |

## Reporting a security issue

Do not open a public issue. Email the maintainer at the address on the GitHub profile.
