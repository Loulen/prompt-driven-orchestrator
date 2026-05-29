SHELL := /usr/bin/env bash
.DEFAULT_GOAL := help
.PHONY: help dev build test check lint fmt clean

PORT := 6172
VITE_PORT := 5174
SANDBOX := /tmp/maestro-dev-sandbox

help:
	@echo "Targets:"
	@echo "  make dev     Run dev daemon (port $(PORT)) + Vite (port $(VITE_PORT)) for chrome-MCP testing"
	@echo "  make build   cargo build + npm run build (frontend embedded into daemon)"
	@echo "  make test    cargo test + vitest"
	@echo "  make check   cargo check + tsc --noEmit"
	@echo "  make lint    cargo clippy + eslint"
	@echo "  make fmt     cargo fmt"
	@echo "  make clean   cargo clean + rm frontend/dist"

dev:
	@mkdir -p $(SANDBOX)
	@cargo build
	@trap 'kill 0' EXIT INT TERM; \
	  (cd $(SANDBOX) && MAESTRO_PORT=$(PORT) $(CURDIR)/target/debug/maestro daemon) & \
	  (cd frontend && MAESTRO_PORT=$(PORT) npm run dev -- --port $(VITE_PORT)) & \
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
