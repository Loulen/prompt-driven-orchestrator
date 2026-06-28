SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.PHONY: help dev build test check lint fmt clean install update service-install service-status service-restart service-logs

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
	@echo "  make build   cargo build + npm run build (frontend embedded into daemon)"
	@echo "  make test    cargo test + vitest"
	@echo "  make check   cargo check + tsc --noEmit"
	@echo "  make lint    cargo clippy + eslint"
	@echo "  make fmt     cargo fmt"
	@echo "  make clean   cargo clean + rm frontend/dist"
	@echo ""
	@echo "Installed global daemon ($(PDO_PROD_DIR), port $(PDO_PROD_PORT)):"
	@echo "  make install          Clone if needed + build release + install $(PDO_BIN)"
	@echo "  make service-install  Generate+enable systemd --user unit + linger (starts at boot)"
	@echo "  make update           Pull latest main + rebuild + swap binary + restart service"
	@echo "  make service-status   systemctl --user status pdo"
	@echo "  make service-restart  systemctl --user restart pdo"
	@echo "  make service-logs     journalctl --user -u pdo -f"

dev:
	@mkdir -p $(SANDBOX)
	@cargo build
	@trap 'kill 0' EXIT INT TERM; \
	  (cd $(SANDBOX) && PDO_PORT=$(PORT) $(CURDIR)/target/debug/pdo daemon) & \
	  (cd frontend && PDO_PORT=$(PORT) npm run dev -- --port $(VITE_PORT)) & \
	  wait

build:
	cd frontend && npm run build
	cargo build

test:
	cargo test --workspace
	cd frontend && npm test

check:
	cargo check --workspace
	cd frontend && npm run typecheck

lint:
	cargo clippy --workspace --all-targets -- -D warnings
	cd frontend && npm run lint

fmt:
	cargo fmt --all

clean:
	cargo clean
	rm -rf frontend/dist

# ---- installed global daemon ----

install:
	@test -d $(PDO_PROD_DIR)/.git || git clone $(REPO_URL) $(PDO_PROD_DIR)
	cd $(PDO_PROD_DIR)/frontend && npm ci
	cd $(PDO_PROD_DIR) && cargo build --release
	install -m755 $(PDO_PROD_DIR)/target/release/pdo $(PDO_BIN)
	@$(PDO_BIN) --version

update:
	cd $(PDO_PROD_DIR) && git fetch origin && git checkout main && git pull --ff-only
	cd $(PDO_PROD_DIR)/frontend && npm ci
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
