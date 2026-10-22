# Rust Lightning Nodes Service

A small Rust service that imports Lightning Network node data from
[mempool.space](https://mempool.space), stores it in PostgreSQL, and exposes it
through a JSON API.

```text
mempool.space → background importer → PostgreSQL → GET /nodes
```

The request path never calls the external API.

## Quick Start

Requirements: Docker Engine and Docker Compose v2.

```bash
cp .env.example .env
docker compose up --build -d
docker compose ps
curl http://localhost:3000/health
curl http://localhost:3000/nodes
```

The app runs migrations automatically at startup; there is no separate
migration command. The first import starts immediately after the database
connection is ready. If `/nodes` returns `[]`, wait a few seconds and retry.

Stop the stack:

```bash
docker compose down
```

Stop and delete the database volume:

```bash
docker compose down -v
```

## Make Shortcuts

If `make` is available:

```bash
make up
make ps
make db-migrate
make bash
make db-shell
make down
```

`make bash` and `make db-shell` require the stack to be running. Run
`make help` to list all targets.

## Environment Variables

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | Required for local runs; supplied by Compose | PostgreSQL connection string. |
| `PORT` | `3000` | HTTP port used by the Rust process. |
| `IMPORT_INTERVAL_SECS` | `300` | Import interval in seconds; minimum `30`. |
| `MEMPOOL_API_URL` | Challenge endpoint | External ranking API. |
| `RUST_LOG` | `info` | `tracing` filter. |
| `APP_PORT` | `3000` | Compose-only host port for the app. |
| `DB_PORT` | `5432` | Compose-only host port for PostgreSQL. |

Notes:

- Compose supplies `DATABASE_URL` internally as
  `postgres://lightning:lightning@db:5432/lightning`.
- `APP_PORT` and `DB_PORT` only change host port mappings.
- Inside Compose, `PORT` is fixed at `3000`; use `APP_PORT` to change the host
  port.
- The Rust process does not load `.env` automatically. Export variables before
  running it outside Docker.
- The credentials in `.env.example` are development defaults.

## Local Development

Requires Rust 1.99 or newer. Start only PostgreSQL:

```bash
docker compose up -d db
```

Run the app:

```bash
export DATABASE_URL=postgres://lightning:lightning@localhost:5432/lightning
export IMPORT_INTERVAL_SECS=60
cargo run
```

## Migrations

Migrations run automatically when the application starts. To apply pending
migrations manually:

```bash
make db-migrate
```

Equivalent commands:

```bash
docker compose run --rm app migrate
```

```bash
export DATABASE_URL=postgres://lightning:lightning@localhost:5432/lightning
cargo run -- migrate
```

The command is idempotent and forward-only; it does not run down migrations.
After adding a migration, rebuild the image with `make build` or `make up`.

## API

| Method | Path | Description |
| --- | --- | --- |
| `GET` | `/nodes` | Returns the stored snapshot ordered by ranking; `500` with a generic error if PostgreSQL fails. |
| `GET` | `/health` | Returns `200` when PostgreSQL responds, `503` otherwise. |

Example `GET /nodes` item:

```json
{
  "public_key": "03864ef025fde8fb587d989186ce6a4a186895ee44a926bfc370e2c366597a3f8f",
  "alias": "ACINQ",
  "capacity": "360.10516297",
  "first_seen": "2018-04-05T15:13:42Z"
}
```

`capacity` is a string with eight decimal places, converted from satoshis using
`1 BTC = 100,000,000 sats`. `first_seen` is RFC 3339 UTC with a `Z` suffix.

## Tests

```bash
cargo test --lib
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The current suite covers domain formatting, configuration, import policy, and
mempool parsing. PostgreSQL and HTTP adapter integration tests are a next step.

## Documentation

- [`prd/Architecture.md`](prd/Architecture.md) — architecture and design decisions.
- [`prd/tech-spec/01-database.md`](prd/tech-spec/01-database.md) — schema, migrations, persistence, and database operations.
- [`prd/Back-end Coding Challenge - Superhuman Docs.md`](<prd/Back-end Coding Challenge - Superhuman Docs.md>) — challenge requirements.

## Challenge README Template

### Build tools & versions used

Rust 1.99.1, axum 0.8.9, sqlx 0.9.0, reqwest 0.13.5, PostgreSQL 17, Docker
Compose v2.

### Steps to run the app

```bash
cp .env.example .env
docker compose up --build -d
curl http://localhost:3000/health
curl http://localhost:3000/nodes
```

### What was the reason for your focus? What problems were you trying to solve?

The focus was correctness of the data contract and resilience of the import
pipeline: exact Bitcoin precision, consistent timestamps, and keeping the last
valid snapshot when the external API or database is temporarily unavailable.

The domain and application layers are framework-free, so formatting and import
decisions are testable without a database or network.

### How long did you spend on this project?

_To be filled in by the author._

### Did you make any trade-offs for this project? What would you have done differently with more time?

- Full snapshot replacement keeps the implementation simple; with more time I
  would add batch metadata and upsert-plus-prune.
- A single instance avoids distributed locking; multiple replicas would need an
  advisory lock or a dedicated worker.
- Runtime SQL keeps Docker builds simple; I would consider `sqlx::query!` with
  offline metadata for compile-time validation.
- Pagination, caching, authentication, and CORS are out of scope.

See [`prd/Architecture.md`](prd/Architecture.md) for the full reasoning.

### What do you think is the weakest part of your project?

The snapshot replacement strategy trusts the external endpoint to return a
complete ranking. The non-empty check reduces the risk but does not prove
completeness. Adapter integration tests would be the next investment.

### Is there any other information you'd like us to know?

The architecture is documented in [`prd/Architecture.md`](prd/Architecture.md),
and the database details are in
[`prd/tech-spec/01-database.md`](prd/tech-spec/01-database.md).
