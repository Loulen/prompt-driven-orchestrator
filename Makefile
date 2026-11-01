SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.PHONY: help dev build fast scoped gate test e2e release-bump bench-gates run-costs bench-transcript-search check lint fmt clean types types-check support-table demo-daemon demo-daemon-stop readme-media readme-media-publish readme-media-export readme-media-import install update service-install service-status service-restart service-logs

PORT := 6172
VITE_PORT := 5174
SANDBOX := /tmp/pdo-dev-sandbox
# The one file carrying the generated harness support table (#855).
SUPPORT_TABLE := docs/reference/harnesses.md
# The README media tool (#856, ADR-0074). `node:sqlite` is still flagged
# experimental: its warning is noise here.
README_MEDIA := node --disable-warning=ExperimentalWarning scripts/readme-media/readme-media.mjs
# The media config (scripts/readme-media/lib/config.mjs): the core's own
# (scripts/readme-media/readme-media.config.mjs) unless CONFIG names another
# repo's — an absolute path, or relative to this checkout.
README_MEDIA_CONFIG = $(if $(CONFIG),--config "$(CONFIG)")
# The TypeScript generated from the daemon's DTOs by ts-rs (#915). Never edit it
# by hand: `make types` rewrites it, `make check` fails when it drifts.
GENERATED_TS := frontend/src/shared/generated
TS_CHECK_DIR := $(CURDIR)/target/ts-rs-check
# Each `#[ts(export)]` type owns one `export_bindings_*` test that writes its file.
# PDO_SKIP_FRONTEND_BUILD: the crate's build.rs typechecks the frontend, which
# imports these very files — generating them must not depend on them. build.rs
# then embeds whatever frontend its OUT_DIR already holds (an empty folder on a
# fresh target/), which is enough for a test build.
EXPORT_TS := PDO_SKIP_FRONTEND_BUILD=1 cargo test --quiet --workspace --lib export_bindings_ -- --test-threads=1

# sccache (ADR draft « les contrôles structurels tournent dans un crate tidy »):
# every PDO worktree starts with an empty target/, and sccache, which lives
# outside of it, hands the compiled dependencies from one worktree to the next.
# Used when installed, unless the caller set RUSTC_WRAPPER itself; without it,
# the build works the same, only slower, and says how to get it.
ifeq ($(origin RUSTC_WRAPPER),undefined)
ifneq ($(shell command -v sccache 2>/dev/null),)
export RUSTC_WRAPPER := sccache
else
$(info warning: sccache is not installed, so every fresh worktree compiles the dependencies again. Install it with `cargo install sccache --locked` (see CONTRIBUTING.md).)
endif
endif

# ---- installed global daemon (build-from-source; runs as a systemd --user service) ----
REPO_URL      := git@github.com:Loulen/prompt-driven-orchestrator.git
PDO_PROD_DIR  ?= $(HOME)/.pdo/app
PDO_PROD_PORT ?= 6160
PDO_BIN       ?= $(HOME)/.local/bin/pdo

help:
	@echo "Targets:"
	@echo "  make dev     Run dev daemon (port $(PORT)) + Vite (port $(VITE_PORT)) for chrome-MCP testing"
	@echo "  make build   cargo build (build.rs runs pnpm run build, frontend embedded into daemon)"
	@echo "  make fast    The fast gate (< 1 min, even on a cold target/): cargo fmt --check + the structural checks of crates/tidy"
	@echo "  make scoped [BASE=<ref>] [DRY_RUN=1]  The scoped gate: make fast, then only the checks the diff since the merge-base touches (plan printed first)"
	@echo "  make gate    The full gate, once per ticket: every CI step but the E2E, without fail-fast, + vitest + script tests; lists every failure"
	@echo "  make test    cargo nextest (every failure, no fail-fast) + doctests + vitest + the scripts' node tests"
	@echo "  make e2e [E2E_PORT=5273]  The frontend's Playwright suite on a real daemon (installs Chromium if missing)"
	@echo "  make release-bump [LEVEL=…] [API_LEVEL=…] [DRY_RUN=1]  At integration -> main: bump the versions, release ## Unreleased, number the draft ADRs"
	@echo "  make bench-gates REF=\"<ref> [<ref> …]\"  Measure the dev loop at each ref in a throwaway worktree (cold/warm build, gates); one markdown table"
	@echo "  make run-costs RUN=<run-id> [URL=…]  Cost and duration per ticket of a Run and its child Runs (read only)"
	@echo "  make bench-transcript-search [RELEASE=1]  Bench 15 concurrent searches over a Run's transcripts"
	@echo "  make check   cargo check + generated TS types current + tsc --noEmit + the harness support table (docs/reference/harnesses.md) is current"
	@echo "  make types   Regenerate the TS types of the daemon's DTOs into $(GENERATED_TS)"
	@echo "  make lint    cargo clippy + eslint"
	@echo "  make fmt     cargo fmt"
	@echo "  make clean   cargo clean (embedded frontend included) + rm a stray frontend/dist"
	@echo "  make demo-daemon PORT=<p> [TEST_EXT=1]  Start a demo daemon for an agentic test: this branch's pdo, fresh HOME under /tmp/pdo-demo-<p>"
	@echo "  make demo-daemon-stop PORT=<p>          Stop it: its pid and its tmux socket pdo-<p>, nothing else"
	@echo "  make support-table  Regenerate the harness support table in docs/reference/harnesses.md from the code"
	@echo "  make readme-media [SCENE=stats]   Record the README media (2 variants per scene) into .readme-media/ — see CONTRIBUTING"
	@echo "  make readme-media-publish [SCENE=…]  Copy the selected variants into docs/assets/readme/"
	@echo "    … CONFIG=<file>             Same, for another repo's media config (its binary, scenes, selection, destination)"
	@echo "  make readme-media-export   Copy the README target pipelines into your library as readme-* (to redraw them)"
	@echo "  make readme-media-import   Bring your readme-* pipelines back into scripts/readme-media/fixture/targets/"
	@echo ""
	@echo "Installed global daemon ($(PDO_PROD_DIR), port $(PDO_PROD_PORT)):"
	@echo "  make install          Clone if needed + build release + install $(PDO_BIN)"
	@echo "  make service-install  Generate+enable systemd --user unit + linger (starts at boot)"
	@echo "  make update           Pull latest main + rebuild + swap binary + restart service"
	@echo "  make service-status   systemctl --user status pdo"
	@echo "  make service-restart  systemctl --user restart pdo"
	@echo "  make service-logs     journalctl --user -u pdo -f"

# No `--` before `--port`: npm swallowed it, pnpm forwards it verbatim to the
# script, and `vite -- --port 5174` silently falls back to vite's own 5173.
dev:
	@mkdir -p $(SANDBOX)
	@cargo build
	@trap 'kill 0' EXIT INT TERM; \
	  (cd $(SANDBOX) && PDO_PORT=$(PORT) $(CURDIR)/target/debug/pdo daemon) & \
	  (cd frontend && PDO_PORT=$(PORT) pnpm run dev --port $(VITE_PORT)) & \
	  wait

# build.rs runs `pnpm run build` itself, into target/ (#21): a `pnpm run build`
# here would only write a frontend/dist nothing embeds.
build:
	cargo build

# Les `PDO_*` de l'environnement appelant, retirés des tests Rust : lancée depuis une
# session de node PDO, la suite héritait de `PDO_UPDATE_CHECK=0`, `PDO_RUN_ID`,
# `PDO_DAEMON_URL`… et ses daemons de test les lisaient (check de mise à jour coupé,
# release_channel rouge). Un test qui veut une variable la pose lui-même. Les
# réglages du build (`PDO_FRONTEND_DIST`, `PDO_SKIP_FRONTEND_BUILD`) restent.
NO_PDO_ENV := env $(foreach v,$(filter-out PDO_FRONTEND_DIST PDO_SKIP_FRONTEND_BUILD,$(filter PDO_%,$(.VARIABLES))),-u $(v))

# Le fast gate : le format, puis les contrôles structurels (taille des fichiers,
# couverture des routes, règle hexagonale, liens de la doc, version du module
# partagé du front). `tidy` ne dépend que de std : il compile en quelques
# secondes même sur un target/ froid, avant toute suite longue.
fast:
	cargo fmt --all --check
	$(NO_PDO_ENV) cargo test --quiet -p tidy --no-fail-fast

# The scoped gate: `make fast`, then the plan computed from the diff since the
# merge-base with the branch this one started from (BASE= to change it), printed
# before it runs (DRY_RUN=1: the plan only, without make fast): frontend -> typecheck, lint, vitest; a crate -> nextest and
# clippy on it and its dependents; a type exported to the front -> types-check;
# doc only -> nothing more. scripts/gates/lib/plan.mjs, tested by `node --test`.
scoped: $(if $(filter 1,$(DRY_RUN)),,fast)
	@BASE=$(BASE) DRY_RUN=$(DRY_RUN) $(NO_PDO_ENV) node scripts/gates/scoped.mjs

# The full gate: every step .github/workflows/ci.yml declares, read from it, the
# E2E job aside (`make e2e`, before `integration -> main`), nextest without
# fail-fast, plus vitest and the script tests. Every step runs whatever failed
# before it; the failures are listed at the end. Once per ticket, after the FP.
gate:
	@$(NO_PDO_ENV) node scripts/gates/gate.mjs

# nextest : un process par test (isolation retrouvée depuis que les 50 binaires de
# tests sont fusionnés en un seul, cf. crates/pdo/tests/it.rs) et un pool
# global au lieu d'un pool par binaire. nextest NE LANCE PAS les doctests, d'où la
# ligne `--doc` séparée : la retirer ferait disparaître leur couverture en silence.
# `--no-fail-fast` : un passage liste tous les échecs, de tous les crates, au lieu
# de s'arrêter au premier et de coûter une relance par échec.
test:
	$(NO_PDO_ENV) cargo nextest run --workspace --no-fail-fast
	$(NO_PDO_ENV) cargo test --workspace --doc
	cd frontend && pnpm test
	node --disable-warning=ExperimentalWarning --test 'scripts/*/test/*.test.mjs'

# The frontend's E2E (Playwright, a real daemon built by `cargo run`), once per
# story before `integration/*` -> `main`. `playwright install` downloads Chromium
# only when its cache lacks it. E2E_PORT: the e2e daemon's port (5273 by default).
e2e:
	cd frontend && pnpm exec playwright install chromium
	cd frontend && $(NO_PDO_ENV) $(if $(E2E_PORT),PDO_E2E_PORT=$(E2E_PORT)) pnpm run e2e

# At the merge of `integration/*` into `main`, never on a ticket (ADR-0095):
# proposes the semver level from the commits since the last tag, bumps the
# workspace, the extension API, the front's shared module and Cargo.lock,
# renames `## Unreleased` and numbers the draft ADRs. LEVEL= / API_LEVEL= force
# a level, DRY_RUN=1 prints the plan only.
release-bump:
	LEVEL=$(LEVEL) API_LEVEL=$(API_LEVEL) DRY_RUN=$(DRY_RUN) node scripts/release/release-bump.mjs

# The dev loop measured before/after: for each
# ref of REF, a throwaway worktree (removed even on failure), then the cold build,
# the build with a warm sccache, the time to the first structural failure, the
# scoped gate on a front-only diff and the full suite; a gate the ref lacks is
# « n/a ». Needs ~25 GB free per cold build (MIN_FREE_GB=). OUT= names the table.
bench-gates:
	@REF='$(REF)' OUT='$(OUT)' MIN_FREE_GB='$(MIN_FREE_GB)' node scripts/bench/bench-gates.mjs

# The cost and duration per ticket of a story: a Run and its child Runs, read on
# the daemon of URL= (else PDO_DAEMON_URL, PDO_TOKEN as the bearer). Read only.
run-costs:
	@RUN='$(RUN)' URL='$(URL)' OUT='$(OUT)' node scripts/bench/run-costs.mjs

# The bench of the search over a Run's transcripts (CORE-82): 15 searches at once
# on a realistic Run, against the list of the Runs and the opening of a node.
# Ignored by `make test`; the numbers are printed. RELEASE=1 measures an
# optimized build.
bench-transcript-search:
	$(NO_PDO_ENV) cargo nextest run -p pdo-daemon --test it $(if $(RELEASE),--release) \
	  --run-ignored only --no-capture bench_fifteen_concurrent_searches

check:
	cargo check --workspace
	$(MAKE) --no-print-directory types-check
	cd frontend && pnpm run typecheck
	# The harness support table in docs/reference/harnesses.md is generated from the
	# capability declaration in crates/pdo-harness/src/domain/probes/ (#617). A
	# hand-edited table would be wrong at the next capability; this fails and names
	# the drift instead. Fix it with `make support-table`, never by editing the block.
	cargo run --quiet -p pdo-daemon -- docs support-table --check --file $(CURDIR)/$(SUPPORT_TABLE)

# Regenerate the TS types of every DTO the daemon sends (ts-rs). Run it after
# changing a serialized Rust struct or enum, and commit the result.
types:
	rm -rf $(GENERATED_TS)
	mkdir -p $(GENERATED_TS)
	export TS_RS_EXPORT_DIR=$(CURDIR)/$(GENERATED_TS) && $(EXPORT_TS) > /dev/null

# Fails, and shows the diff, when a DTO changed without `make types`: the front
# would otherwise compile against a contract the daemon no longer speaks.
types-check:
	rm -rf $(TS_CHECK_DIR)
	export TS_RS_EXPORT_DIR=$(TS_CHECK_DIR) && $(EXPORT_TS) > /dev/null
	@diff -r $(TS_CHECK_DIR) $(GENERATED_TS) || { \
	  echo "FAIL: $(GENERATED_TS) is out of date with the Rust DTOs. Run 'make types' and commit the result." >&2; \
	  exit 1; }

# Rewrite the generated block of docs/reference/harnesses.md from the code. Run it
# after adding a harness, adding a capability, or moving a "last validated version".
support-table:
	cargo run --quiet -p pdo-daemon -- docs support-table --write --file $(CURDIR)/$(SUPPORT_TABLE)

# The demo daemon of the agentic tests (docs/agents/run-scenario.md): this
# branch's `pdo daemon` (or, TEST_EXT=1, the test extension's demo daemon) on
# PORT, under a fresh HOME in /tmp/pdo-demo-<PORT>, with no PDO_* nor TMUX*
# inherited. PORT is required: the 6172 of `make dev` is not a default here.
# DEMO_ENV='NAME=value …' sets variables on the daemon explicitly. The stop
# reaches the pid of the pidfile and the tmux socket pdo-<PORT>, never the
# user's daemon or tmux server; KEEP=1 keeps /tmp/pdo-demo-<PORT> (logs).
DEMO_DAEMON := node --disable-warning=ExperimentalWarning scripts/demo-daemon/demo-daemon.mjs
comma := ,
DEMO_PORT_HINT = $(if $(filter demo-daemon-stop,$@),the port of a running demo daemon,a free port$(comma) 6400 to 6999)
DEMO_PORT = $(if $(filter command line environment,$(origin PORT)),$(PORT),$(error PORT=<port> is required, e.g. make $@ PORT=6491 ($(DEMO_PORT_HINT))))

demo-daemon:
	@$(DEMO_DAEMON) start $(DEMO_PORT) $(if $(filter 1,$(TEST_EXT)),--test-ext) $(if $(DEMO_ENV),--env '$(DEMO_ENV)')

demo-daemon-stop:
	@$(DEMO_DAEMON) stop $(DEMO_PORT) $(if $(filter 1,$(KEEP)),--keep)

# README media (ADR-0074): a sealed demo instance, real UI, real agents for the
# live scenes. Costs an agent session per live scene — regenerate only when a
# scene visibly changed. `SCENE=a,b` limits the run; `VARIANT=a` one variant.
# `CONFIG=<file>`: another repo's media config; its binary is its own, so the
# core is not built.
readme-media:
	$(if $(CONFIG),,cargo build)
	SCENE=$(SCENE) VARIANT=$(VARIANT) KEEP_DEMO=$(KEEP_DEMO) $(README_MEDIA) record $(README_MEDIA_CONFIG)

readme-media-publish:
	SCENE=$(SCENE) $(README_MEDIA) publish $(README_MEDIA_CONFIG)

# The target pipelines of the README scenes (ADR-0074 §6), between the fixture
# and your library (~/.pdo/pipelines): export as readme-*, redraw them in the
# editor, import. The export never overwrites a readme-* you modified.
# Core-only: `CONFIG` is passed on so another repo's config is refused.
readme-media-export:
	$(README_MEDIA) export $(README_MEDIA_CONFIG)

readme-media-import:
	$(README_MEDIA) import $(README_MEDIA_CONFIG)

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd frontend && pnpm run lint

fmt:
	cargo fmt --all

# The embedded frontend lives under target/ (cargo clean removes it); frontend/dist
# only exists after a manual `pnpm run build` in frontend/.
clean:
	cargo clean
	rm -rf frontend/dist

# ---- installed global daemon ----
# `install` / `update` build the PRODUCTION daemon in $(PDO_PROD_DIR): pnpm must
# be on the PATH of the shell running make (nvm, corepack or a package manager),
# same as node already had to be. `pnpm install --frozen-lockfile` is the `npm ci`
# equivalent: it refuses to touch pnpm-lock.yaml. On the first update after the
# npm -> pnpm switch, the pull brings pnpm-lock.yaml in before the install runs,
# and pnpm relinks the clone's existing node_modules onto its store.

install:
	@test -d $(PDO_PROD_DIR)/.git || git clone $(REPO_URL) $(PDO_PROD_DIR)
	cd $(PDO_PROD_DIR)/frontend && pnpm install --frozen-lockfile
	cd $(PDO_PROD_DIR) && cargo build --release
	install -m755 $(PDO_PROD_DIR)/target/release/pdo $(PDO_BIN)
	@$(PDO_BIN) --version

update:
	cd $(PDO_PROD_DIR) && git fetch origin && git checkout main && git pull --ff-only
	cd $(PDO_PROD_DIR)/frontend && pnpm install --frozen-lockfile
	cd $(PDO_PROD_DIR) && cargo build --release
	install -m755 $(PDO_PROD_DIR)/target/release/pdo $(PDO_BIN)
	systemctl --user restart pdo
	@echo "updated -> $$($(PDO_BIN) --version)"

service-install:
	@mkdir -p $(HOME)/.config/systemd/user
	@printf '[Unit]\nDescription=PDO (Prompt-Driven Orchestrator) daemon\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nWorkingDirectory=$(PDO_PROD_DIR)\nEnvironment=PDO_PORT=$(PDO_PROD_PORT)\nEnvironment=PATH=$(HOME)/.local/bin:%s:/usr/local/bin:/usr/bin:/bin\nExecStart=$(PDO_BIN) daemon\nRestart=on-failure\nRestartSec=3\nKillMode=process\n\n[Install]\nWantedBy=default.target\n' "$$(dirname $$(command -v node))" > $(HOME)/.config/systemd/user/pdo.service
	systemctl --user daemon-reload
	loginctl enable-linger $(USER)
	systemctl --user enable --now pdo
	@systemctl --user --no-pager status pdo | head -6

service-status:
	systemctl --user --no-pager status pdo

service-restart:
	systemctl --user restart pdo

service-logs:
	journalctl --user -u pdo -f
