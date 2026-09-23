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
| Test the README media tooling | `node --test 'scripts/readme-media/test/*.test.mjs'` |

### Harness support table

The table in [docs/reference/harnesses.md](docs/reference/harnesses.md) is generated from
`crates/pdo-daemon/src/harness_probes.rs`. Never edit the block by hand: run
`make support-table` after adding a harness, adding a capability, or moving a "last validated
version". `make check` fails if the committed block has drifted from the code.

### README media

The GIFs and posters of the README (`docs/assets/readme/`) are produced by a script: nothing is
captured by hand. `make readme-media` drives the real UI of a **demo instance**, a PDO daemon sealed
off from yours (its own working directory, `HOME` and port; see
[ADR-0074](docs/adr/0074-les-medias-du-readme-sont-joues-par-de-vrais-agents-sur-une-instance-de-demo-isolee.md)).
It records two variants per scene into `.readme-media/` (not versioned), with a `manifest.json`.
`make readme-media-publish` then copies the variant `scripts/readme-media/selection.txt` names for each
scene into `docs/assets/readme/`.

```bash
make readme-media SCENE=stats      # one scene; without SCENE, all of them
make readme-media-publish          # the selected variants → docs/assets/readme/
make readme-media-export           # the target pipelines → your library, as readme-*
make readme-media-import           # your readme-* pipelines → the fixture
```

The pipelines the scenes show are **target pipelines** drawn by the maintainer in the PDO editor
(`scripts/readme-media/fixture/targets/`): the built and the complete `implement-review`, and
`prod-check`, the pipeline the trigger fires. The demo instance gets them as is, byte for byte but
their `name:`; the tool never generates a layout. To redraw one, `make readme-media-export` copies
them into your library (`~/.pdo/pipelines`) as `readme-pipelines`, `readme-implement-review` and
`prod-check`, with their prompts, and never overwrites one you modified since. Edit them in your
editor, then `make readme-media-import` brings them back into the fixture under their demo name.
An export followed by an import without a retouch changes no file.

Prerequisites:

| Tool | Why |
| --- | --- |
| Node.js 22.13 or newer | the script, and `node:sqlite` for the mocked history |
| `ffmpeg` (with `ffprobe`) | cutting, framing and encoding the GIFs and posters |
| Playwright's Chromium: `cd frontend && pnpm install && pnpm exec playwright install chromium` | filming the UI |
| `tmux`, `git` | the demo daemon, like any PDO |
| An authenticated harness (`claude`) | only for the scenes played live: its auth files are copied into the demo `HOME` for the recording, then wiped |

Cost of a regeneration: the Stats scene uses a mocked history, and the Visual pipelines and Routing
scenes are canvas gestures; each costs nothing but about a minute. Each live scene (hero, diff
review, orchestration…) runs real `claude` sessions on `claude-opus-5-5`, stopped as soon as the
scene is recorded. The hero runs the complete demo pipeline twice, one run per variant, each to its end
(a few minutes each, more if the reviewer sends a lap back); a run that does not end on a `pass`
verdict fails the scene. Typed outputs and Diff review each play one run to the end (off camera, a few minutes),
shared by their two variants; Diff review then wakes the run's manager once per variant for its
answer. Recursive orchestration plays one orchestrating run per variant, and each starts two child
runs of the whole pipeline (a minute or two each). The reviewer drives Playwright's Chromium from your cache (`~/.cache/ms-playwright`). Each regeneration also adds its GIFs to
the git history, since they are committed without a weight budget.

When to regenerate: only when a scene visibly changes (the UI it films, the copy of its README row),
and only that scene (`SCENE=…`). Open both variants from `.readme-media/`, pick one in `selection.txt`,
publish, then commit `docs/assets/readme/` and the selection. Switching to the other variant later
is a one-line edit and `make readme-media-publish`; nothing is recorded again.

Recording never touches your instance: not your daemon, port, `~/.pdo`, event log or tmux sessions.
Only `make readme-media-export` writes to your library, and only its three `readme-*` pipelines.
Whatever the ending (success, failure, Ctrl+C), it stops the demo agents and daemon, waits for every
agent process to exit, wipes the copied auth files and removes the demo root: nothing is left in
`/tmp`, and no `claude` outlives the command. How it works, and how to add a scene (one file under `scripts/readme-media/scenes/`):
[scripts/readme-media/README.md](scripts/readme-media/README.md).

## Architecture

| Resource | Content |
| --- | --- |
| [CONTEXT.md](CONTEXT.md) | Domain glossary and module map |
| [`docs/adr/`](docs/adr/) | Architecture decisions |
| [docs/features.md](docs/features.md) | What each feature does, and how PDO works |
| [docs/reference/](docs/reference/) | CLI, reverse proxy, terminal, harness support |

## Reporting a security issue

Do not open a public issue. Email the maintainer at the address on the GitHub profile.
