# 01 — Database

- **Status:** Accepted
- **Scope:** PostgreSQL schema, persistence behavior, migrations, and operational queries.
- **Related documents:**
  - `../../../README.md` — setup, environment variables, and run commands.
  - `../Architecture.md` — layer boundaries and persistence decisions.
  - `../../../migrations/0001_create_nodes.sql` — schema source of truth.

This document is the detailed reference for the database. The README only contains the minimum needed to start the service.

---

## 1. Overview

The service uses a single PostgreSQL 17 database and one table, `nodes`.

The import model is a **full snapshot**:

- The importer reads the current connectivity ranking.
- It validates and normalizes the records.
- On success, it replaces the entire `nodes` table inside a transaction.
- On failure, the previous snapshot remains available.

There is no incremental merge, audit table, or node history in the current scope.

---

## 2. Connection and Local Access

The application reads the connection string from `DATABASE_URL`.

| Environment | Connection string |
| --- | --- |
| Local app, database in Compose | `postgres://lightning:lightning@localhost:5432/lightning` |
| App container, database container | `postgres://lightning:lightning@db:5432/lightning` |

The Compose stack uses the following development credentials:

| Setting | Value |
| --- | --- |
| Host (from the host machine) | `localhost` |
| Host (inside Compose) | `db` |
| Port | `5432` |
| Database | `lightning` |
| User | `lightning` |
| Password | `lightning` |

These credentials are development defaults only. Use protected environment variables or a secret manager in production. The host port can be changed with `DB_PORT`; the container port remains `5432`.

Connect with `psql` through the database container:

```bash
docker compose exec db psql -U lightning -d lightning
```

Shortcut: `make db-shell`.

The Rust process does not load `.env` automatically. Export `DATABASE_URL` before running the app outside Docker, or source the file manually.

---

## 3. Schema

```sql
CREATE TABLE nodes (
    public_key     TEXT PRIMARY KEY,
    alias          TEXT NOT NULL,
    capacity_sats  BIGINT NOT NULL CHECK (capacity_sats >= 0),
    first_seen     TIMESTAMPTZ NOT NULL,
    rank_position  INTEGER NOT NULL,
    imported_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX nodes_rank_position_idx
    ON nodes (rank_position);
```

The full schema is created by the migrations in
[`migrations/`](../../../migrations/).

---

## 4. Column Semantics

| Column | Type | Meaning |
| --- | --- | --- |
| `public_key` | `TEXT` | Lightning node public key, exactly as imported. Primary key. |
| `alias` | `TEXT` | Node alias, exactly as imported. Empty string if the source omits it. |
| `capacity_sats` | `BIGINT` | Total channel capacity in satoshis. Non-negative. |
| `first_seen` | `TIMESTAMPTZ` | First observation time, converted from Unix seconds. |
| `rank_position` | `INTEGER` | Position in the source connectivity ranking. |
| `imported_at` | `TIMESTAMPTZ` | Time of the snapshot insert. Operational metadata. |
| `created_at` | `TIMESTAMPTZ` | Row creation time. Defaults to `now()`. |
| `updated_at` | `TIMESTAMPTZ` | Last update time. Defaults to `now()`; maintained by the trigger. |

The `nodes_updated_at_trigger` updates `updated_at` before each `UPDATE`. Snapshot imports use inserts, so the trigger mainly supports future update paths.

Why these types:

- `BIGINT` for sats keeps exact integer arithmetic. Bitcoin supply is far below the signed 64-bit limit.
- `TIMESTAMPTZ` stores a point in time correctly and avoids timezone ambiguity.
- `TEXT` preserves the public key and alias without padding or truncation.
- `CHECK (capacity_sats >= 0)` is defense in depth; the importer also rejects negative values.

---

## 5. Index

```sql
CREATE INDEX nodes_rank_position_idx ON nodes (rank_position);
```

`GET /nodes` orders by `rank_position`, so the index supports the read path. The table is small, so this is a low-cost optimization rather than a scalability requirement.

---

## 6. Migrations

Migrations live in [`migrations/`](../../../migrations/) and are embedded into the binary with `sqlx::migrate!`.

The current migration set is:

- `0001_create_nodes.sql` — creates the `nodes` table and index.
- `0002_add_node_timestamps.sql` — adds `created_at`, `updated_at`, and the update trigger.

At startup, the application:

1. Connects to PostgreSQL with bounded retries.
2. Runs all pending migrations.
3. Starts the importer and the HTTP server only after migrations succeed.

Migrations also run automatically on application startup. To apply pending
migrations manually:

```bash
make db-migrate
```

Equivalent commands:

```bash
docker compose run --rm app migrate
```

```bash
cargo run -- migrate
```

The local command requires `DATABASE_URL` to be exported. Migrations are
embedded at compile time, so rebuild the image after adding a migration:
`make build` or `make up-build`.

To add a migration:

1. Create the next numbered file, for example `0002_add_some_column.sql`.
2. Use forward-only SQL that is safe to run once.
3. Rebuild the application so the embedded migration set is updated.

Do not edit migrations that have already been applied. `sqlx` validates migration checksums.

The project uses forward-only migrations; there are no down migrations.

---

## 7. Write Path

`PostgresNodesRepository::save_snapshot` writes a complete snapshot in one transaction:

```text
BEGIN
  DELETE FROM nodes
  INSERT INTO nodes (...) VALUES (...)
  ...
COMMIT
```

If any insert fails, the transaction rolls back and the previous snapshot is preserved.

### Why `DELETE` instead of `TRUNCATE`

- `DELETE` in a transaction does not block concurrent readers for the duration of the import.
- PostgreSQL MVCC lets readers continue seeing the previous snapshot until the new transaction commits.
- `TRUNCATE` takes a stronger lock and would block reads.

The importer never calls `save_snapshot` for an empty or fully invalid response. This prevents a bad response from erasing valid data.

### Full snapshot trade-off

The external endpoint is a ranking snapshot, so replacing the table mirrors the source and avoids stale nodes.

The trade-off is that a valid but unexpectedly small response would shrink the dataset. The non-empty validation limits the risk, but does not prove that the external response is complete.

---

## 8. Read Path

The list query is:

```sql
SELECT public_key, alias, capacity_sats, first_seen, rank_position
FROM nodes
ORDER BY rank_position ASC;
```

Rows are mapped back into validated domain values. A row that violates domain invariants is treated as `RepositoryError::InvalidData`; it is not silently exposed.

The health check runs:

```sql
SELECT 1;
```

It does not read the `nodes` table.

---

## 9. Pool, Timeouts, and Retry

| Setting | Value |
| --- | --- |
| Maximum connections | 10 |
| Acquire timeout | 5 seconds |
| Statement timeout | 5 seconds |
| Startup connection attempts | Up to 10 |
| Retry backoff | 500 ms, doubling, capped at 5 seconds |

The pool sets `statement_timeout` on each new connection. A database that is unavailable during startup is retried before the process exits.

---

## 10. Data Mapping

The database stores raw values; formatting belongs to the domain/API layer.

### Capacity

`capacity_sats` is converted to Bitcoin as a string with exactly eight decimal places:

```text
1 BTC = 100,000,000 sats

36,010,516,297 sats -> "360.10516297"
550,000 sats        -> "0.00550000"
```

The conversion uses integer division and remainder, not floating point.

### First seen

`first_seen` is stored as `TIMESTAMPTZ` and returned as RFC 3339 UTC with a `Z` suffix:

```text
1522941222 -> "2018-04-05T15:13:42Z"
```

The database stores the instant; the API decides the text representation.

---

## 11. Operational Queries

Count imported nodes:

```bash
docker compose exec db psql -U lightning -d lightning \
  -c "SELECT count(*) FROM nodes;"
```

Inspect the top of the ranking:

```bash
docker compose exec db psql -U lightning -d lightning \
  -c "SELECT public_key, alias, capacity_sats, first_seen, rank_position \
      FROM nodes ORDER BY rank_position LIMIT 10;"
```

Inspect the table and index definitions:

```bash
docker compose exec db psql -U lightning -d lightning \
  -c "\d+ nodes"
```

Reset the database and remove the volume:

```bash
make db-reset
```

Equivalent:

```bash
docker compose down -v
```

---

## 12. Limitations and Trade-offs

- **Full snapshot replacement** is simple and mirrors the source, but it trusts the source to return a complete ranking.
- **Single writer:** there is no advisory lock. Multiple replicas could import concurrently; that is out of scope for the current single-instance deployment.
- **No audit history:** previous snapshots are not retained.
- **No partitioning or archival:** the current dataset is small.
- **No down migrations:** the project is forward-only.
- **No compile-time SQL validation:** queries use runtime `sqlx` APIs to keep Docker builds simple.

---

## 13. References

- `../../../README.md` — setup and run instructions.
- `../Architecture.md` — layer boundaries and persistence decisions.
- `../../../migrations/0001_create_nodes.sql` — schema source of truth.
- `src/repositories/nodes_repository.rs` — `NodesRepository` interface.
- `src/repositories/postgres_nodes_repository.rs` — `PostgresNodesRepository` implementation.
- `src/repositories/nodes_repository_factory.rs` — factory that creates the PostgreSQL repository.
- `src/repositories/mock_nodes_repository.rs` — in-memory test double.
- `src/infrastructure/postgres.rs` — process-scoped PostgreSQL manager: pool, connection retry, and migrations.
