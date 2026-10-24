# Architecture Decision Record: Lightning Nodes Service

- **Status:** Accepted
- **Date:** 2026-10-21
- **Context:** Bipa Back-end Coding Challenge
- **Related documents:**
  - `../../README.md` — setup, environment variables, API examples, and run commands.
  - `tech-spec/01-database.md` — schema, migrations, persistence, and database operations.
  - `tech-spec/02-api.md` — HTTP routing, middleware, rate limiting, and error mapping.
  - `tech-spec/03-logging.md` — the two logging mechanisms, the `RunLogger` port, and backends.
  - `Back-end Coding Challenge - Superhuman Docs.md` — challenge requirements.

This document records architectural decisions only. Operational instructions belong in the README and database reference.

---

## 1. Decision

Build a single Rust service that periodically imports Lightning node data from
mempool.space, stores a validated snapshot in PostgreSQL, and serves it through
`GET /nodes`.

The service uses:

- A pure `domain` core with no I/O.
- An `application` layer with two use cases that depend on two ports.
- Framework-isolated boundary modules:
  - `http` for inbound HTTP;
  - `gateways` for external APIs;
  - `repositories` for persistence;
  - `logging` for import-run audit records.
- `main.rs` and `composition.rs` as the composition root.
- Native async traits with generics, without `async-trait`.
- Explicit SQL with `sqlx`, without an ORM.
- A factory per boundary (gateway, repository, run logger) for production and
  test implementations.
- Process-scoped singletons shared through `Arc`.
- A single binary with three commands (`serve` by default, `migrate`,
  `import`) plus PostgreSQL, orchestrated by Docker Compose.

The design is intentionally small: **two use cases, three ports, and one
factory per boundary**. Clean Architecture requires dependencies to point
inward and effects to stay at the edges, not a class per concept.

---

## 2. Constraints and Non-goals

The challenge requires:

- Importing the mempool.space connectivity ranking.
- Persisting nodes in a database.
- Serving `GET /nodes` with exact `public_key` and `alias`, Bitcoin-denominated
  `capacity`, and formatted `first_seen`.
- Keeping the external API call outside the request path.
- Not crashing under failure.
- A README with setup instructions and English code/comments/commits.

Priorities:

| Priority | Meaning |
| --- | --- |
| Reviewability | The flow should be understandable quickly. |
| Testability | The core must be testable without infrastructure. |
| Failure safety | A failed import must not erase the previous snapshot. |
| Realism | The project should resemble a small production service. |
| Minimalism | Frameworks stay at the edges; no speculative infrastructure. |

Non-goals:

- Authentication, authorization, or API keys.
- Pagination, filtering, caching, or ETags.
- Metrics, dashboards, or distributed tracing.
- OpenAPI generation.
- Multiple replicas, distributed locks, or a separate worker.
- Kubernetes, queues, Redis, or a multi-crate workspace.
- A dependency-injection container.

---

## 3. Principles

1. **Dependencies point inward.** `domain` knows no frameworks; `application`
   knows only `domain` and its ports.
2. **Effects live at the edges.** HTTP, SQL, network, time, and logging are
   boundary concerns.
3. **Ports exist only at real boundaries.** The boundaries are node ingestion,
   node persistence, and import-run auditing.
4. **Pure functions decide, imperative code executes.** Import policy is
   testable without I/O.
5. **Make illegal states unrepresentable.** Validated newtypes replace raw
   strings and integers where invariants matter.
6. **Errors are values.** `Result` and enums replace panics and sentinels.
7. **Composition is explicit.** The composition root constructs and injects
   concrete implementations.
8. **Factories create implementations, they do not implement them.** A
   factory returns a port implementation and never implements the port itself.
9. **Singleton means process scope, not global access.** Instances are created
   once and shared explicitly.
10. **Names reflect responsibility.** Repositories persist and query; gateways
    integrate with external services; run loggers record import runs.

---

## 4. System Shape

```text
main.rs / composition.rs
        │ wires concrete implementations
        ├── http         (inbound HTTP)
        ├── gateways     (external APIs)
        ├── repositories (persistence)
        └── logging      (import-run audit)
                     │ implement ports
                     ▼
application (use cases + ports)
        │ depends on
        ▼
domain (pure types, invariants, formatting)
```

There are two paths, plus a write-only audit side-channel:

- **Import:** scheduler → `NodesGateway` → import use case → `NodesRepository`
  → PostgreSQL.
- **Read:** HTTP → list use case → `NodesRepository` → PostgreSQL.
- **Audit:** every import run → `RunLogger` → one line in `LOG_DIR`
  (`tech-spec/03-logging.md`).

The read path never calls the external API. The import path never handles HTTP
requests. The audit sink is written from the import side only.

---

## 5. Layers and Dependency Rule

| Layer | Responsibility | May depend on |
| --- | --- | --- |
| `domain` | Value objects, invariants, formatting, domain errors | `std`, `chrono`, `thiserror` |
| `application` | Ports, use cases, application errors | `domain`, `gateways::nodes_gateway`, `repositories::nodes_repository` |
| `gateways` | External API interface, mempool implementation, test double | `domain`, `reqwest` (implementation only) |
| `repositories` | Persistence interface, PostgreSQL implementation, test double | `domain`, `sqlx` (implementation only) |
| `routes` | Inbound HTTP routes, handlers, DTOs, and middleware | `domain`, `application`, `repositories::nodes_repository`, `axum`, `serde` |
| `logging` | Import-run audit port (`RunLogger`), file backend, reusable file sink, and the logger factory | `application` (outcome types), `config`, `chrono`, `std`, `tracing` (warn only) |
| `infrastructure` | Process-scoped PostgreSQL manager: pool, connection retry, migrations | `sqlx` |
| `composition` / `main` | Wiring, configuration, lifecycle, factory selection | all layers |

Forbidden dependencies:

- `domain` must not import `axum`, `sqlx`, `reqwest`, or `serde`.
- `application` must not import `axum`, `sqlx`, `reqwest`, or `serde`.
- `application` must not import any concrete gateway or repository module.
- `application` must not import `logging`; the audit sink is recorded by the
  scheduler and the CLI, not by the use cases.
- `domain` must not perform I/O, read environment variables, or log.
- `application` must not know concrete implementation types.
- Boundary modules must not call each other directly.

Only `main.rs` and `composition.rs` know every concrete type. `gateways` and
`repositories` are packaged by feature: the interface file is the abstract port,
while the technology-specific files are implementations. The dependency rule
still holds because `application` imports only the port modules.

---

## 6. Domain and Ports

The domain contains validated types and pure transformations:

| Type | Invariant |
| --- | --- |
| `PublicKey` | Non-empty string. |
| `Sats` | Unsigned 64-bit value; no floating point. |
| `FirstSeen` | Valid UTC timestamp derived from Unix seconds. |
| `Node` | Composed only of validated values. |

Formatting rules:

- `capacity` uses integer arithmetic and exactly eight decimal places.
- `first_seen` uses RFC 3339 UTC with a `Z` suffix.
- `public_key` and `alias` are never transformed.

Three ports exist, each at a real boundary:

| Port | Responsibility | Methods |
| --- | --- | --- |
| `NodesGateway` | External node source | `fetch` |
| `NodesRepository` | Persistence and querying | `save_snapshot`, `list`, `ping` |
| `RunLogger` | Import-run audit records — deliberately not a leveled logger | `record`, `record_or_warn` |

Ports use native async traits with `impl Future + Send` and generic dispatch.
This avoids `async-trait`, boxing, and dynamic dispatch. The trade-off is that
the traits are not dyn-compatible and their signatures are slightly more
verbose.

Terminology:

- **Repository** — persistence and querying (`NodesRepository`,
  `PostgresNodesRepository`).
- **Gateway** — external service integration (`NodesGateway`,
  `MempoolNodesGateway`).
- **Run logger** — audit sink for import runs (`RunLogger`,
  `FileRunLogger`); not a leveled logger — diagnostics stay with `tracing`.
- **Sink** — reusable file mechanics behind a logging backend (`FileSink`).
- **Boundary implementation** — a concrete implementation of a port.
- **Factory** — creates and returns a port implementation. It never implements
  the port itself.
- **Singleton** — a process-scoped instance shared through `Arc`.

Factories are not application ports.

### Repository Pattern and Naming

The repository pattern isolates persistence and querying from the use cases.
`NodesRepository` is the abstract boundary; the application imports only this
interface and never imports `sqlx` or a concrete repository type.

```text
src/repositories/
├── mod.rs
├── repository_error.rs          # RepositoryError (shared persistence vocabulary)
├── nodes_repository.rs          # NodesRepository port
├── postgres_nodes_repository.rs # PostgresNodesRepository
├── sqlite_nodes_repository.rs   # SqliteNodesRepository
├── nodes_repository_factory.rs  # NodesRepositoryFactory + backend factories + selection
├── any_nodes_repository.rs      # AnyNodesRepository (runtime-selected wrapper)
└── mock_nodes_repository.rs     # MockNodesRepository (IN_MEMORY backend and tests)

src/infrastructure/
├── mod.rs
├── postgres.rs     # Postgres manager: pool, connection retry, migrations
└── sqlite.rs       # Sqlite manager: pool, pragmas, migrations
```

Method surface:

- `save_snapshot` — replace the stored snapshot; used by the import use case.
- `list` — retrieve the current snapshot; used by the list use case.
- `ping` — check persistence availability; used by the health endpoint.

The interface is use-case-specific, not a generic CRUD repository.

Naming conventions:

- The interface is named after the aggregate in the plural plus `Repository`:
  `NodesRepository`.
- Concrete implementations use a technology or role prefix:
  `PostgresNodesRepository`, `MockNodesRepository`.
- The file name mirrors the type in `snake_case`:
  `nodes_repository.rs`, `postgres_nodes_repository.rs`,
  `mock_nodes_repository.rs`.
- One file per repository, so several PostgreSQL repositories can coexist
  without collision: `postgres_nodes_repository.rs`,
  `postgres_channels_repository.rs`, and so on.
- External API integrations are `Gateway`, not `Repository`.
- Avoid `DAO`, `Store`, `Provider`, and generic `Service` names.
- Files and modules use `snake_case`; Rust types use `UpperCamelCase`.

`PostgresNodesRepository` is the production implementation and owns the SQL,
transactions, and row mapping for the `nodes` table. `MockNodesRepository` is an
in-memory test double compiled only in test builds; it is never selected by the
production factory.

### Repository Factory

`NodesRepositoryFactory` creates and returns a `NodesRepository` implementation.
It never implements the repository behavior itself, and it is never a global
accessor:

```rust
pub trait NodesRepositoryFactory: Send + Sync {
    type Repository: NodesRepository + 'static;

    fn create(
        &self,
        config: &Config,
    ) -> impl Future<Output = Result<Self::Repository, RepositoryFactoryError>> + Send;
}
```

- `PostgresNodesRepositoryFactory` is the production implementation. It
  connects to PostgreSQL, applies migrations, and returns the repository.
- `MockNodesRepositoryFactory` is a commented placeholder for a future fully
  mocked environment. It would require compiling the mock outside test builds
  and selecting the factory at runtime.
- A factory is created and used once, in the composition root. There is no
  `get_instance()` and no global registry.

Shared PostgreSQL infrastructure — the connection manager, pool, connection
retry, and migrations — lives in `src/infrastructure/postgres.rs` as the
process-scoped `Postgres` manager. It is created only by the factory that
selects PostgreSQL, and it is reused by any number of PostgreSQL repositories
instead of being duplicated per repository.

The `repositories` folder is organized by feature rather than by layer. The
interface file is framework-free; the concrete files depend on the interface and
on their infrastructure. This is a deliberate packaging trade-off for
discoverability. The dependency rule still holds: `application` depends on
`repositories::nodes_repository`, not on the concrete repository modules.

### Gateway Pattern and Naming

The gateway pattern isolates external API access from the use cases.
`NodesGateway` is the abstract boundary; the application imports only this
interface and never imports `reqwest` or a concrete gateway type.

```text
src/gateways/
├── mod.rs
├── nodes_gateway.rs                # NodesGateway + GatewayError
├── mempool_nodes_gateway.rs        # MempoolNodesGateway
├── nodes_gateway_factory.rs        # NodesGatewayFactory + MempoolNodesGatewayFactory
└── mock_nodes_gateway.rs           # MockNodesGateway (test builds only)
```

Method surface:

- `fetch` — retrieve the current node ranking from the external source.

Naming conventions:

- The interface is named after the aggregate in the plural plus `Gateway`:
  `NodesGateway`.
- Concrete implementations use a source or role prefix:
  `MempoolNodesGateway`, `MockNodesGateway`.
- The file name mirrors the type in `snake_case`.
- One file per gateway implementation, so several external sources can coexist
  without collision.
- Persistence is a `Repository`, never a `Gateway`.
- Avoid `Client`, `Provider`, and generic `Service` names for ports.
- Files and modules use `snake_case`; Rust types use `UpperCamelCase`.

`MempoolNodesGateway` implements the mempool.space connectivity-ranking
endpoint. `MockNodesGateway` is an in-memory test double compiled only in test
builds; it is never selected by the production factory.

### Gateway Factory

`NodesGatewayFactory` creates and returns a `NodesGateway` implementation. It
never implements the gateway behavior itself, and it is never a global accessor:

```rust
pub trait NodesGatewayFactory: Send + Sync {
    type Gateway: NodesGateway + 'static;

    fn create(&self, config: &Config) -> Result<Self::Gateway, GatewayFactoryError>;
}
```

- `MempoolNodesGatewayFactory` is the production implementation. It builds the
  HTTP client for `MempoolNodesGateway`.
- `MockNodesGatewayFactory` is a commented placeholder for a future fully mocked
  environment. It would require compiling the mock outside test builds and
  selecting the factory at runtime.
- Gateway creation is synchronous because it only builds the HTTP client.
- A factory is created and used once, in the composition root. There is no
  `get_instance()` and no global registry.

The `gateways` folder is organized by feature rather than by layer. The interface
file is framework-free; the concrete file depends on the interface and on
`reqwest`. The dependency rule holds because `application` imports only
`gateways::nodes_gateway`.

---

## 7. Use Cases and Boundary Implementations

### Import use case

1. Fetch validated nodes through `NodesGateway`.
2. Apply a pure import plan: empty input is skipped; non-empty input replaces
   the snapshot.
3. Persist through `NodesRepository`.
4. Return an outcome or a typed error.

### List use case

Reads the snapshot through `NodesRepository` and returns domain nodes. It is
intentionally thin: it keeps the HTTP layer behind the application boundary and
provides a stable error type.

### MempoolNodesGateway

- Performs the HTTP request with a timeout and User-Agent.
- Deserializes the response into transport DTOs.
- Validates records into domain nodes.
- Skips invalid individual records, logs them, and keeps valid records.
- Deduplicates public keys, keeping the first occurrence.
- Preserves the source ranking position.
- Maps transport failures to `GatewayError`.

The parser is pure and tested with JSON fixtures.

### PostgreSQL repository

- Owns the schema and all SQL.
- Replaces the snapshot transactionally.
- Reads nodes ordered by ranking.
- Maps rows back into validated domain values.
- Reports database health with `SELECT 1`.

Schema, column types, migrations, transaction behavior, pool settings, and
operational queries are documented in `tech-spec/01-database.md`.

### HTTP layer

- Exposes `/nodes` and `/health`.
- Calls the application use cases; contains no domain logic.
- Maps repository failures to a generic error response and logs the cause.
- Uses a small `axum` middleware for request logging.
- Leaves CORS, authentication, pagination, caching, and OpenAPI out of scope.

Endpoint examples and status codes are in `../api.md`.

---

## 8. Composition Root

The composition root selects the concrete implementations and injects them:

```text
main
 ├─ MempoolNodesGatewayFactory.create(&config)        → gateway
 ├─ ConfiguredNodesRepositoryFactory.create(&config)  → repository
 ├─ ConfiguredRunLoggerFactory.create(&config)        → logger
 └─ build_app(gateway, repository, &config)           → App
```

- `build_app` receives already constructed dependencies, so it holds no
  infrastructure knowledge.
- The repository and gateway factories use associated types, preserving static
  dispatch and avoiding `Box<dyn Trait>` and `async-trait`.
- `ConfiguredNodesRepositoryFactory` maps `PERSISTENCE` to the matching
  backend factory and returns `AnyNodesRepository`, the enum wrapper that
  implements the port by delegation.
- `ConfiguredRunLoggerFactory` maps `LOG_BACKEND` to the audit backend and
  returns `AnyRunLogger`. The instance is created once per process and used by
  either the one-shot `import` command or the scheduler task
  (`tech-spec/03-logging.md`).
- Migrations are a database lifecycle operation, not a repository behavior, so
  the `migrate` command calls `composition::run_migrations` instead of the
  factory.

`build_app` constructs the graph once:

- One gateway implementation.
- One repository implementation.
- One database manager holding the pool, when the selected backend uses one.
- One router.
- One import interval.

The HTTP router and scheduler share the repository through `Arc`. This is the
process-scoped singleton: one instance per process, no global `static`, no
service locator.

Rules:

- No `static DB: OnceLock<PgPool>` or similar boundary singletons.
- No `lazy_static` or `once_cell`; `OnceLock` is only for rare process-wide
  infrastructure, not boundary implementations.
- Tests create isolated instances; they never share production singletons.
- Runtime repository selection is provided by the `PERSISTENCE` variable
  (`IN_MEMORY`, `POSTGRES`, `SQLITE`). The composition root maps it to a
  factory through `ConfiguredNodesRepositoryFactory`, and `AnyNodesRepository`
  is the enum wrapper implementing the port; the in-memory variant reuses the
  test mock, which is therefore compiled outside test builds. Adding another
  backend means a new factory, a new enum variant, and a new match arm.

---

## 9. Import Consistency

The importer uses a **full snapshot**, not incremental merging:

- The first import runs at startup; later imports follow the configured
  interval.
- The scheduler awaits each cycle, so cycles never overlap.
- A minimum interval is validated to prevent a zero-duration timer.
- Missed ticks do not cause burst catch-up.
- An empty or fully invalid response does not replace the current snapshot.
- A successful import replaces the snapshot transactionally.
- The request path never calls the external API.
- Every run — immediate, periodic, skipped, or failed — appends one line to
  the daily audit file through `RunLogger`; a logging failure degrades to a
  warning and never interrupts the loop (`tech-spec/03-logging.md`).

Failure semantics are fail-closed:

- Transport, HTTP, or JSON failures keep the previous snapshot.
- Invalid individual records are skipped; valid records continue.
- Persistence failures roll back the transaction.
- The next interval retries automatically.

The trade-off is that a valid but unexpectedly small response could shrink the
dataset. The non-empty validation limits the risk but does not prove
completeness. Transaction details and the schema are in
`tech-spec/01-database.md`.

Multiple replicas would need a PostgreSQL advisory lock or a dedicated worker;
that is out of scope for a single instance.

---

## 10. Error Handling, Resilience, and Concurrency

- No `unwrap()` or `expect()` in request handling, import, or startup parsing.
- Startup PostgreSQL connection uses bounded retries with exponential backoff.
- Migrations run before the HTTP server starts.
- Outbound HTTP timeout: 30 seconds.
- Database acquire and statement timeouts: 5 seconds.
- Import failures are logged and never terminate the process.
- Database failures in `/nodes` return a generic error response.
- `SIGINT` and `SIGTERM` trigger graceful shutdown.

Error layering:

```text
framework error → port error → application error → HTTP error
```

Concrete causes are logged at the boundary and carried inside the error
values (`RepositoryError::Unavailable` holds the database cause as a string;
application errors embed their sources in the display), so console lines,
audit records, and exit output stay self-contained; internal details are
never returned to clients.

Error vocabulary belongs to the boundary, not to one entity:

- Persistence ports share `RepositoryError`, defined once in
  `repositories/repository_error.rs`. Its variants describe the persistence
  boundary itself (unreachable database, unmappable rows), which is why the
  type is not duplicated per port.
- A port with richer semantics (`NotFound`, `Conflict`, …) defines its own
  error enum instead of widening the shared one. Variants are added to
  `RepositoryError` only when every persistence port can exhibit them.
- The gateway keeps a separate `GatewayError`, because transport failures have
  different causes than storage failures.
- Ports re-export their error type, so callers import vocabulary from the port
  they already depend on.

Concurrency:

- One Tokio multi-threaded runtime.
- One scheduler task.
- Immutable shared dependencies in `Arc`.
- No `Arc<Mutex<_>>`.
- Shutdown coordinated with `tokio::sync::watch`.
- The scheduler is cancelled and awaited; the pool is dropped afterwards.

---

## 11. Functional Programming and SOLID

| Practice | Implementation |
| --- | --- |
| Functional core, imperative shell | Pure domain and import policy; effects in boundary modules. |
| Algebraic data types | `Result`, `Option`, import plans/outcomes, error enums. |
| Validated value objects | `PublicKey`, `Sats`, `FirstSeen`. |
| Immutability | Immutable nodes; slices in functions; local `mut` only. |
| Composition over inheritance | Small functions, iterators, no class hierarchy. |
| Explicit dependencies | Ports and configuration passed as arguments. |
| Errors as values | Typed enums and `?`; no exceptions or sentinels. |
| Total functions after validation | Formatting cannot fail once values exist. |
| Effects at the boundary | HTTP, SQL, network, time, and logging in boundary modules. |
| No global mutable state | No global singletons, lazy statics, or shared mutable caches. |

| SOLID principle | Implementation |
| --- | --- |
| Single Responsibility | Domain formats, application decides, boundary modules do I/O. |
| Open/Closed | New gateway or repository implementations do not change use cases. |
| Liskov Substitution | Fakes and real implementations honor the same ports. |
| Interface Segregation | Two small ports instead of one broad service. |
| Dependency Inversion | Application defines traits; composition root injects. |

---

## 12. Dependency and Framework Policy

Frameworks stay at the infrastructure boundary:

- `axum` — HTTP layer only.
- `sqlx` — PostgreSQL repository, factory, and infrastructure only.
- `reqwest` — mempool gateway only.
- `serde` — boundary DTOs only.
- `tokio`, `chrono`, `tracing`, `thiserror` — runtime, time, logs, and errors.

None appears in `domain` or `application`. Replacing an implementation does not change
the core.

Deliberately excluded:

- `async-trait` — native async traits and generics are sufficient.
- `tower-http` — a small middleware covers request logging.
- `dotenvy` — environment variables are explicit.
- `anyhow` — typed errors are preferred; `Box<dyn Error>` is enough in `main`.
- `mockall` — hand-written test doubles remain clearer than generated mocks.
- `testcontainers` — PostgreSQL tests can use `sqlx::test` and Compose.
- `lazy_static` / `once_cell` — `OnceLock` is enough for rare global values.
- A DI container or service locator.
- Any ORM — SQL is explicit and small.

`wiremock` is a dev-dependency, used only by gateway HTTP integration tests.

Key versions are in `../../README.md`; the full dependency list is in `Cargo.toml`.

---

## 13. Testing

Current coverage spans unit tests, gateway/repository integration tests,
route tests, and the logging module; live tests are ignored by default:

- Unit tests cover domain formatting, configuration, import policy, mempool
  parsing, and the logging module (line format, date partitioning, sink
  mechanics, factory wiring).
- Integration tests exercise `MempoolNodesGateway` through the real `reqwest`
  path against a local `wiremock` server, and `SqliteNodesRepository` against
  temporary databases.
- Route tests drive the real `Router` in memory and cover status codes, the
  `404` fallback, and rate limiting.
- A live smoke test calls the real mempool.space API and an end-to-end test
  boots a real server; both are marked `#[ignore]`, so the default suite stays
  offline and deterministic.

Target test matrix:

| Level | What is tested |
| --- | --- |
| Domain | Bitcoin and timestamp formatting, value validation |
| Application | Import policy and use cases with `MockNodesGateway` and `MockNodesRepository` |
| MempoolNodesGateway (unit) | Parsing, duplicates, missing/invalid fields |
| MempoolNodesGateway (integration) | HTTP fetch, invalid-record skipping, non-success status, invalid payload, connection failure |
| MempoolNodesGateway (live) | One real fetch from mempool.space, ignored by default |
| PostgreSQL repository | Snapshot replace, ordering, rollback, health |
| HTTP layer | Status codes, JSON, error mapping, health |
| Logging | Line format, date partitioning, `FileSink` mechanics, factory wiring |

`MockNodesGateway` and `MockNodesRepository` are hand-written and compiled only
in test builds. The gateway integration tests use `wiremock`; there is no
testcontainer and no external network dependency in the default suite. Test
commands are in `../../README.md`.

---

## 14. Trade-offs

| Decision | Trade-off | Alternative |
| --- | --- | --- |
| Full snapshot replacement | Simple and mirrors the ranking; a small valid response can shrink the dataset. | Upsert plus prune by batch ID. |
| PostgreSQL | Extra service; closer to production. | SQLite is simpler locally but less realistic. |
| Native async traits + generics | Verbose signatures; no trait objects. | `async-trait` + `Arc<dyn Trait>`. |
| Boundary factories | Add traits and generic parameters; centralize construction and keep the composition root explicit. | Direct construction in `main`. |
| Process-scoped singleton | Explicit ownership; no global access point. | `static OnceLock`, easier global access but harder tests. |
| Single crate | Weaker compile-time layer enforcement. | A workspace. |
| In-process scheduler | Simple deployment; one replica only. | A dedicated worker. |
| File-only audit log | Append-only daily files; no rotation, retention, or structured export. | A hosted log backend behind the same `RunLogger` port. |
| Runtime SQL | No compile-time SQL validation. | `sqlx::query!` with offline metadata. |
| No pagination/cache/auth/CORS | Minimal scope; less flexible API. | Add at the boundary layer when needed. |

---

## 15. Guardrails

- Maximum of three ports (gateway, repository, audit logger) and two use
  cases.
- No trait for clock or configuration; `RunLogger` is the one deliberate
  logging port — an audit sink behind a swappable backend, not a diagnostics
  facade.
- No generic CRUD repository.
- No workspace, DI container, domain events, CQRS, or message bus.
- One factory trait per boundary; no service locator and no global accessor.
- No global mutable boundary singleton.
- No `Arc<Mutex<_>>`; use immutable sharing or channels.
- No mockall or testcontainers; `wiremock` only as a dev-dependency for
  gateway HTTP tests.
- No framework types in `domain` or `application`.
- No abstraction without a real boundary and a test seam.
- No premature pagination, caching, or authentication.

---

## 16. Consequences and Revisit Triggers

### Positive

- The core is testable without infrastructure.
- Frameworks are replaceable without touching business rules.
- Failures preserve the previous snapshot.
- The request path never depends on the external API.
- Formatting is exact and unit-tested.
- Real and test gateways and repositories are constructed through their factories.
- Instances are shared explicitly, without global mutable state.

### Negative

- Native async trait signatures are verbose.
- The factories add generic parameters.
- Mock gateways and repositories are selected by test factories, not a runtime toggle.
- Runtime SQL trades compile-time validation for simpler builds.
- Full snapshot replacement is coarse.
- PostgreSQL remains the production default; SQLite and in-memory cover
  lightweight setups.
- A single instance is assumed.
- The API is intentionally minimal.

### Revisit triggers

- **Multiple replicas:** add an advisory lock or a worker.
- **More repository backends:** extend the enum wrapper and its dispatch
  factory.
- **More log backends:** add an enum variant and a factory arm
  (`tech-spec/03-logging.md` §7).
- **Large dataset:** add pagination and indexes for query patterns.
- **Compile-time SQL safety:** adopt `sqlx::query!` with offline metadata.
- **Browser clients:** add explicit CORS.
- **Production secrets:** replace Compose defaults with a secret manager.
- **Operational visibility:** add metrics and distributed tracing.

---

## 17. References

- `../../README.md` — setup, environment variables, API examples, and commands.
- `tech-spec/01-database.md` — database reference.
- `tech-spec/02-api.md` — HTTP routing, middleware, rate limiting, and error mapping.
- `tech-spec/03-logging.md` — logging mechanisms, the `RunLogger` port, and backends.
- `Back-end Coding Challenge - Superhuman Docs.md` — challenge requirements.
- `src/domain/` — pure domain types and formatting.
- `src/application/` — import and list use cases.
- `src/routes/` — inbound HTTP layer.
- `src/gateways/` — external API gateway interface and implementations.
- `src/repositories/` — repository interface and implementations.
- `src/infrastructure/` — process-scoped PostgreSQL manager.
- `src/logging/` — audit port, file backend, file sink, and factories.
- `src/composition.rs` — composition root wiring.
