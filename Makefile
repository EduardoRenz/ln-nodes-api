COMPOSE ?= docker compose
APP ?= app
DB ?= db
DB_USER ?= lightning
DB_NAME ?= lightning

.DEFAULT_GOAL := help

.PHONY: help up up-build down ps logs bash db-shell db-migrate db-reset import build test test-unit test-integration test-live

# Load `.env` for variables that are not already set.
#
# The Rust process never reads `.env` on its own; that is deliberate and
# documented. Tests that follow the ambient configuration therefore need the
# Makefile to supply it. Variables already present in the environment are left
# alone, so `PERSISTENCE=sqlite make test-live` still wins over `.env`.
#
# `printenv` does the presence check rather than `${!key}`, because Make
# recipes run under `/bin/sh`, which is dash on Debian and Ubuntu and has no
# indirect expansion. `export "$$line"` keeps the value quoted as a whole, so
# values containing spaces or `#` survive.
define load_dotenv
if [ -f .env ]; then \
	while IFS= read -r line || [ -n "$$line" ]; do \
		case "$$line" in ''|\#*) continue ;; esac; \
		key="$${line%%=*}"; \
		printenv "$$key" >/dev/null 2>&1 && continue; \
		export "$$line"; \
	done < .env; \
fi
endef

help:
	@echo "Usage:"
	@echo "  make up          Start the app and database with the existing image"
	@echo "  make up-build    Rebuild the image, then start the stack"
	@echo "  make down        Stop the stack"
	@echo "  make ps          Show container status"
	@echo "  make logs        Follow app logs"
	@echo "  make bash        Open a shell in the app container"
	@echo "  make db-shell    Open psql in the database container"
	@echo "  make db-migrate  Apply pending database migrations"
	@echo "  make db-reset    Stop the stack and delete the database volume"
	@echo "  make import      Fetch the ranking once, persist it, and exit"
	@echo "  make build       Build the app image"
	@echo "  make test        Run all tests (unit and integration)"
	@echo "  make test-unit   Run unit tests only"
	@echo "  make test-integration  Run gateway integration tests only"
	@echo "  make test-live   Call the real mempool.space API and run the end-to-end HTTP test"

up:
	$(COMPOSE) up -d

up-build:
	$(COMPOSE) up --build -d

down:
	$(COMPOSE) down

ps:
	$(COMPOSE) ps

logs:
	$(COMPOSE) logs -f $(APP)

bash:
	$(COMPOSE) exec $(APP) bash

db-shell:
	$(COMPOSE) exec $(DB) psql -U $(DB_USER) -d $(DB_NAME)

db-migrate:
	$(COMPOSE) run --rm $(APP) migrate

db-reset:
	@echo "This deletes the PostgreSQL volume."
	$(COMPOSE) down -v

import:
	$(COMPOSE) run --rm $(APP) import

build:
	$(COMPOSE) build

test:
	@echo "### Unit"
	@cargo test --lib --bins -- --skip integration_tests --skip live_tests
	@echo "### Integration"
	@cargo test --lib integration_tests
	@echo "### Live (skipped, run: make test-live)"

test-unit:
	@echo "### Unit"
	@cargo test --lib --bins -- --skip integration_tests --skip live_tests

test-integration:
	@echo "### Integration"
	@cargo test --lib integration_tests

test-live:
	@echo "### Live (gateway against the real mempool.space API)"
	@cargo test --lib live_tests -- --ignored
	@echo "### E2E (real HTTP server, ambient configuration)"
	@$(load_dotenv); cargo test --test e2e_live -- --ignored
