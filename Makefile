COMPOSE ?= docker compose
APP ?= app
DB ?= db
DB_USER ?= lightning
DB_NAME ?= lightning

.DEFAULT_GOAL := help

.PHONY: help up up-build down ps logs bash db-shell db-migrate db-reset build test test-unit test-integration test-live

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
	@echo "  make build       Build the app image"
	@echo "  make test        Run all tests (unit and integration)"
	@echo "  make test-unit   Run unit tests only"
	@echo "  make test-integration  Run gateway integration tests only"
	@echo "  make test-live   Call the real mempool.space API"

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

build:
	$(COMPOSE) build

test:
	@echo "### Unit"
	@cargo test --lib -- --skip integration_tests --skip live_tests
	@echo "### Integration"
	@cargo test --lib integration_tests
	@echo "### Live (skipped, run: make test-live)"

test-unit:
	@echo "### Unit"
	@cargo test --lib -- --skip integration_tests --skip live_tests

test-integration:
	@echo "### Integration"
	@cargo test --lib integration_tests

test-live:
	@echo "### Live"
	@cargo test --lib live_tests -- --ignored
