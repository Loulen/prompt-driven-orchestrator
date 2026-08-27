SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.PHONY: help dev build test check lint fmt clean support-table install update service-install service-status service-restart service-logs

PORT := 6172
VITE_PORT := 5174
SANDBOX := /tmp/pdo-dev-sandbox

# ---- installed global daemon (build-from-source; runs as a systemd --user service) ----
REPO_URL      := git@github.com:Loulen/prompt-driven-orchestrator.git
PDO_PROD_DIR  ?= $(HOME)/.pdo/app
PDO_PROD_PORT ?= 6160
PDO_BIN       ?= $(HOME)/.local/bin/pdo

help:
	@echo "Targets:"
	@echo "  make dev     Run dev daemon (port $(PORT)) + Vite (port $(VITE_PORT)) for chrome-MCP testing"
	@echo "  make build   cargo build + pnpm run build (frontend embedded into daemon)"
	@echo "  make test    cargo nextest + doctests + vitest"
	@echo "  make check   cargo check + tsc --noEmit + the README support table is current"
	@echo "  make lint    cargo clippy + eslint"
	@echo "  make fmt     cargo fmt"
	@echo "  make clean   cargo clean + rm frontend/dist"
	@echo "  make support-table  Regenerate the README harness support table from the code"
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

build:
	cd frontend && pnpm run build
	cargo build

# nextest : un process par test (isolation retrouvée depuis que les 50 binaires de
# tests sont fusionnés en un seul, cf. crates/pdo-daemon/tests/it.rs) et un pool
# global au lieu d'un pool par binaire. nextest NE LANCE PAS les doctests, d'où la
# ligne `--doc` séparée : la retirer ferait disparaître leur couverture en silence.
test:
	cargo nextest run --workspace
	cargo test --workspace --doc
	cd frontend && pnpm test

check:
	cargo check --workspace
	cd frontend && pnpm run typecheck
	# The README support table is generated from the capability declaration in
	# crates/pdo-daemon/src/harness_probes.rs (#617). A hand-edited table would be
	# wrong at the next capability; this fails and names the drift instead. Fix it
	# with `make support-table`, never by editing the README block.
	cargo run --quiet -p pdo-daemon -- docs support-table --check --file $(CURDIR)/README.md

# Rewrite the README's generated block from the code. Run it after adding a
# harness, adding a capability, or moving a "last validated version".
support-table:
	cargo run --quiet -p pdo-daemon -- docs support-table --write --file $(CURDIR)/README.md

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd frontend && pnpm run lint

fmt:
	cargo fmt --all

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
