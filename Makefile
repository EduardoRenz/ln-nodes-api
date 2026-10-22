COMPOSE ?= docker compose
APP ?= app
DB ?= db
DB_USER ?= lightning
DB_NAME ?= lightning

.DEFAULT_GOAL := help

.PHONY: help up down ps logs bash db-shell db-migrate db-reset build test

help:
	@echo "Usage:"
	@echo "  make up          Start the app and database"
	@echo "  make down        Stop the stack"
	@echo "  make ps          Show container status"
	@echo "  make logs        Follow app logs"
	@echo "  make bash        Open a shell in the app container"
	@echo "  make db-shell    Open psql in the database container"
	@echo "  make db-migrate  Apply pending database migrations"
	@echo "  make db-reset    Stop the stack and delete the database volume"
	@echo "  make build       Build the app image"
	@echo "  make test        Run all Rust tests"

up:
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
	cargo test --all-targets
