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

Three gates, from the fastest to the most complete:

```bash
make fast         # while you work: cargo fmt --check, then the structural checks of crates/tidy (< 1 min)
make scoped       # after a change: make fast, then only the checks your diff touches
make gate         # once, before pushing: every CI step but the E2E, plus vitest and the script tests
```

`make scoped` diffs the working tree (untracked files included) against the merge-base with the
branch you started from, and prints its plan, each step with its reason, before running it. A
frontend change runs typecheck, lint and vitest; a crate runs nextest and clippy on that crate and
every crate depending on it; a type exported to the front (ts-rs) runs `types-check`; a changed
script runs its `node --test` suite; a file a crate embeds (the CHANGELOG) runs that crate's
tests; doc alone runs nothing more. `BASE=<ref>` diffs against another branch, `DRY_RUN=1` prints
the plan only.

`make gate` reads its steps from `.github/workflows/ci.yml`, so it follows the CI, and leaves out
the E2E job: `make e2e` runs once per story, before it merges into `main`. Every step runs even
when one before it failed, and the failures are listed at the end. `make test` and `make check`
remain for a narrower pass.

Commit messages follow Conventional Commits with a French or English body, as the rest of
the history does. Reference the issue in the title.

### Versions, CHANGELOG and ADRs

A contribution bumps **no version**: not the workspace, not `pdo-extension-api`, not the front's
shared module, not `Cargo.lock`. Write what it changes under `## Unreleased`, the first entry of
[CHANGELOG.md](CHANGELOG.md), in English and without ticket ids; "What's new" hides that entry. A
new ADR is born `docs/adr/draft-<slug>.md`, without a number, and is cited `ADR draft-<slug>`.
`.gitattributes` merges `CHANGELOG.md` with git's `union` driver: two branches adding a bullet at
the same place keep both lines instead of conflicting. Read the merged entry once in a while: a
line both sides edited comes out twice.

The version is set when a story merges into `main`, by one command
([docs/agents/git-flow.md](docs/agents/git-flow.md) § *integration/\* -> main*):

```bash
make e2e                     # the frontend's Playwright suite (installs Chromium if missing)
make release-bump DRY_RUN=1  # the proposed semver level and the plan, nothing written
make release-bump            # LEVEL=patch|minor|major forces the level
```

`make release-bump` proposes the level from the conventional commits since the last tag (`feat` a
minor, `fix` a patch, `!` a major), bumps the four versions and `Cargo.lock`, turns `## Unreleased`
into the version's entry and numbers the draft ADRs, rewriting their citations.

## Development

### Prerequisites

| Tool | Version |
| --- | --- |
| [Rust](https://rustup.rs/) | Stable |
| [Node.js](https://nodejs.org/) | 22 or newer |
| [pnpm](https://pnpm.io/) | Use the version declared by the project |
| [cargo-nextest](https://nexte.st/) | Any recent version (`make test`) |
| [sccache](https://github.com/mozilla/sccache) | Optional, recommended |

#### sccache

Every PDO worktree starts with an empty `target/`, so each one compiles the dependencies again.
sccache keeps the compiled crates in a cache outside `target/` and hands them to the next
worktree. Install it once:

```bash
cargo install sccache --locked
```

The Makefile then sets `RUSTC_WRAPPER=sccache` for every `make` target (unless you set
`RUSTC_WRAPPER` yourself). Without sccache, the build works the same and `make` prints a warning
saying how to install it. `sccache --show-stats` shows the cache hits.

### Where the code lives

PDO is a modular monolith ([ADR-0079](docs/adr/0079-pdo-est-un-monolithe-modulaire-un-crate-par-bounded-context-hexagonal-a-l-interieur.md)).
Each bounded context is a crate under `crates/` (`pdo-run`, `pdo-pipeline`, `pdo-review`,
`pdo-session`, `pdo-harness`, `pdo-sandbox`, `pdo-skill`, `pdo-trigger`, `pdo-stats`,
`pdo-workspace`, `pdo-telemetry`, `pdo-settings`, `pdo-platform`), hexagonal inside (`domain/`, `application/`,
`adapters/`). Next to them sit `pdo-kernel`, the shared kernel, and `pdo-extension-api`, the public
contract of the extensions. `crates/pdo` is the composition root: it wires the contexts, carries
the `pdo` binary and embeds the SPA. Its Cargo package is named `pdo-daemon`, because the installer
is named after it. The frontend follows the same contexts
(`frontend/src/<context>/{api,model,store,components}/`), plus `shared/` and `app/`. Where a new
file goes, and the rules the test suite checks (a pure `domain/`, 400 lines per production file,
a test for every route): [docs/agents/module-layout.md](docs/agents/module-layout.md).

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
cd frontend && pnpm install --frozen-lockfile && cd ..
cargo build --release -p pdo-daemon
```

The binary embeds the frontend. `crates/pdo/build.rs` runs `pnpm run build` (`tsc -b && vite
build`) and Vite writes into cargo's `OUT_DIR` (`target/<profile>/build/pdo-daemon-*/out/frontend-dist`),
never into `frontend/dist/`. A release build compiles the files in; a debug build reads them from
that folder at runtime. Two variables change what gets embedded:

| Variable | Effect |
| --- | --- |
| `PDO_FRONTEND_DIST=<absolute path>` | Copies an already built frontend (a `dist/` holding `index.html`) instead of building one. No Node or pnpm needed. |
| `PDO_SKIP_FRONTEND_BUILD=1` (any non-empty value) | Builds nothing and keeps whatever `OUT_DIR` already holds: an empty folder on a fresh `target/`, so the debug binary shows its dev placeholder (a release build refuses to embed an empty one). For Rust-only loops such as `make types`. |

Without either and without `frontend/node_modules`, the build stops and says which of the two to
set up.

### CLI

```bash
cargo run -p pdo-daemon -- --help
```

Every command is documented in [docs/reference/cli.md](docs/reference/cli.md).

### Build and test commands

| Purpose | Command |
| --- | --- |
| Fast gate: formatting and structural checks | `make fast` |
| Scoped gate: the checks the diff touches | `make scoped` |
| Full gate: the CI's steps but the E2E, plus vitest and the script tests | `make gate` |
| Check Rust | `cargo check --workspace --all-targets` |
| Regenerate the TS types of the DTOs | `make types` |
| Test Rust | `cargo test --workspace` |
| Lint Rust | `cargo clippy --workspace --all-targets -- -D warnings` |
| Check Rust formatting | `cargo fmt --all --check` |
| Type-check frontend | `cd frontend && pnpm run typecheck` |
| Test frontend | `cd frontend && pnpm run test` |
| Lint frontend | `cd frontend && pnpm run lint` |
| Build frontend | `cd frontend && pnpm run build` |
| Test the scripts (README media, release) | `node --test 'scripts/*/test/*.test.mjs'` |
| Run the frontend's E2E | `make e2e` |
| Measure the dev loop at refs (builds, gates), cost per ticket of a Run | `make bench-gates REF=…`, `make run-costs RUN=…` |

### TypeScript types of the daemon's DTOs

Every Rust struct or enum the daemon sends over HTTP or `/ws` derives `ts_rs::TS` with
`#[ts(export)]`, and its TypeScript lives in `frontend/src/shared/generated/`, one file per type.
Never edit those files: run `make types` after changing a serialized Rust type, and commit the
result. `make check` fails and shows the diff when they have drifted from the Rust side.

A payload the daemon still builds with `json!` has no Rust type yet: its hand-written TypeScript
sits in the `model/` folder of its context (`frontend/src/<context>/model/`) until it gets a DTO.

### Harness support table

The table in [docs/reference/harnesses.md](docs/reference/harnesses.md) is generated from
`crates/pdo-harness/src/domain/probes/`. Never edit the block by hand: run
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
review, interactive & orchestrator nodes…) runs real `claude` sessions on `claude-opus-5-5`, stopped as soon as the
scene is recorded. The hero runs the complete demo pipeline twice, one run per variant, each to its end
(a few minutes each, more if the reviewer sends a lap back); a run that does not end on a `pass`
verdict fails the scene. Typed outputs and Diff review each play one run to the end (off camera, a few minutes),
shared by their two variants; Diff review then wakes the run's manager once per variant for its
answer. Interactive & orchestrator nodes plays one run per variant: its `implementer` asks a question,
gets the answer typed in its terminal, then starts two child runs of the whole pipeline (a minute or
two each). The reviewer drives Playwright's Chromium from your cache (`~/.cache/ms-playwright`). Each regeneration also adds its GIFs to
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

**The media config.** What the tool films is declared by a config
(`scripts/readme-media/readme-media.config.mjs` for the core): the PDO binary the demo instance
starts, the scenes folder, the fixture (repo and pipelines), the selection file, the review folder,
the destination of the published media, the demo instance's bootstrap (`seed`: the core's is the
mocked history) and where Playwright resolves from. The core is its first consumer; its scenes,
media and `make` commands are the ones above. **Another repo** (a repo of extensions) keeps its own
config, scenes and selection, and calls the tool from a checkout of the core at the tag it pins:

```bash
make -C <core checkout> readme-media CONFIG=/abs/path/to/media.config.mjs [SCENE=…]
make -C <core checkout> readme-media-publish CONFIG=/abs/path/to/media.config.mjs
```

Its variants land in its review folder and its selected media in its destination, never in the
core's; with `CONFIG`, the core is not built (the binary is the consumer's). A missing field or an
absent binary fails with the field and the path. The fields, the bootstrap API (`instance.exec`,
`instance.api`) and a worked example:
[scripts/readme-media/README.md § From another repo](scripts/readme-media/README.md#from-another-repo).

## Architecture

| Resource | Content |
| --- | --- |
| [CONTEXT.md](CONTEXT.md) | Domain glossary and module map |
| [`docs/adr/`](docs/adr/) | Architecture decisions |
| [docs/features.md](docs/features.md) | What each feature does, and how PDO works |
| [docs/reference/](docs/reference/) | CLI, reverse proxy, terminal, harness support |

## Reporting a security issue

Do not open a public issue. Email the maintainer at the address on the GitHub profile.
