# Architecture Decision Record: Lightning Nodes Service

- **Status:** Accepted
- **Date:** 2026-10-21
- **Context:** Bipa Back-end Coding Challenge
- **Related documents:**
  - `README.md` — setup, environment variables, API examples, and run commands.
  - `prd/tech-spec/01-database.md` — schema, migrations, persistence, and database operations.
  - `prd/Back-end Coding Challenge - Superhuman Docs.md` — challenge requirements.

This document records architectural decisions only. Operational instructions belong in the README and database reference.

---

## 1. Decision

Build a single Rust service that periodically imports Lightning node data from
mempool.space, stores a validated snapshot in PostgreSQL, and serves it through
`GET /nodes`.

The service uses:

- A pure `domain` core with no I/O.
- An `application` layer with two ports and two use cases.
- Framework-isolated `adapters` for HTTP, PostgreSQL, and the external API.
- `main.rs` and `composition.rs` as the composition root.
- Native async traits with generics, without `async-trait`.
- Explicit SQL with `sqlx`, without an ORM.
- A repository factory that creates production and test implementations.
- Process-scoped singletons shared through `Arc`.
- A single binary plus PostgreSQL, orchestrated by Docker Compose.

The design is intentionally small: **two ports, two use cases, three production
adapters, and one factory**. Clean Architecture requires dependencies to point
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

- Authentication, authorization, rate limiting, or API keys.
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
   adapter concerns.
3. **Ports exist only at real boundaries.** The boundaries are node ingestion
   and node persistence.
4. **Pure functions decide, imperative code executes.** Import policy is
   testable without I/O.
5. **Make illegal states unrepresentable.** Validated newtypes replace raw
   strings and integers where invariants matter.
6. **Errors are values.** `Result` and enums replace panics and sentinels.
7. **Composition is explicit.** The composition root constructs and injects
   concrete adapters.
8. **Factories create implementations, they do not implement them.** A
   factory returns a port implementation and never implements the port itself.
9. **Singleton means process scope, not global access.** Instances are created
   once and shared explicitly.
10. **Names reflect responsibility.** Repositories persist and query; gateways
    integrate with external services.

---

## 4. System Shape

```text
main.rs / composition.rs
        │ wires concrete adapters
        ▼
adapters (HTTP, PostgreSQL, mempool.space)
        │ implement ports
        ▼
application (use cases + ports)
        │ depends on
        ▼
domain (pure types, invariants, formatting)
```

There are two paths:

- **Import:** scheduler → gateway → import use case → repository → PostgreSQL.
- **Read:** HTTP adapter → list use case → repository → PostgreSQL.

The read path never calls the external API. The import path never handles HTTP
requests.

---

## 5. Layers and Dependency Rule

| Layer | Responsibility | May depend on |
| --- | --- | --- |
| `domain` | Value objects, invariants, formatting, domain errors | `std`, `chrono`, `thiserror` |
| `application` | Ports, use cases, application errors | `domain`, `repositories::nodes_repository` |
| `repositories` | Persistence interface, PostgreSQL implementation, test double | `domain`, `sqlx` (implementation only) |
| `infrastructure` | Process-scoped PostgreSQL manager: pool, connection retry, migrations | `sqlx` |
| `adapters` | HTTP and mempool integration | `domain`, `application`, frameworks |
| `composition` / `main` | Wiring, configuration, lifecycle, factory selection | all layers |

Forbidden dependencies:

- `domain` must not import `axum`, `sqlx`, `reqwest`, or `serde`.
- `application` must not import `axum`, `sqlx`, `reqwest`, or `serde`.
- `application` must not import any concrete repository module.
- `domain` must not perform I/O, read environment variables, or log.
- `application` must not know concrete adapter types.
- Adapters must not call each other directly.

Only `main.rs` and `composition.rs` know every concrete type. The `repositories`
folder is packaged by feature: `nodes_repository.rs` is the abstract port, while
`postgres_nodes_repository.rs` and `mock_nodes_repository.rs` are
implementations. The dependency rule still holds because `application` imports
only `repositories::nodes_repository`.

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

The application depends on two ports:

| Port | Responsibility | Methods |
| --- | --- | --- |
| `NodeGateway` | External node source | `fetch` |
| `NodesRepository` | Persistence and querying | `save_snapshot`, `list`, `ping` |

Ports use native async traits with `impl Future + Send` and generic dispatch.
This avoids `async-trait`, boxing, and dynamic dispatch. The trade-off is that
the traits are not dyn-compatible and their signatures are slightly more
verbose.

Terminology:

- **Repository** — persistence and querying (`NodesRepository`,
  `PostgresNodesRepository`).
- **Gateway** — external service integration (`NodeGateway`,
  `MempoolNodeGateway`).
- **Adapter** — a concrete implementation of a port.
- **Factory** — creates and returns a port implementation. It never implements
  the port itself.
- **Singleton** — a process-scoped instance shared through `Arc`.

The factory is not an application port.

### Repository Pattern and Naming

The repository pattern isolates persistence and querying from the use cases.
`NodesRepository` is the abstract boundary; the application imports only this
interface and never imports `sqlx` or a concrete repository type.

```text
src/repositories/
├── mod.rs
├── nodes_repository.rs          # NodesRepository + RepositoryError
├── postgres_nodes_repository.rs # PostgresNodesRepository
├── nodes_repository_factory.rs  # NodesRepositoryFactory + PostgresNodesRepositoryFactory
└── mock_nodes_repository.rs     # MockNodesRepository (test builds only)

src/infrastructure/
├── mod.rs
└── postgres.rs     # Postgres manager: pool, connection retry, migrations
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

---

## 7. Use Cases and Adapters

### Import use case

1. Fetch validated nodes through `NodeGateway`.
2. Apply a pure import plan: empty input is skipped; non-empty input replaces
   the snapshot.
3. Persist through `NodesRepository`.
4. Return an outcome or a typed error.

### List use case

Reads the snapshot through `NodesRepository` and returns domain nodes. It is
intentionally thin: it keeps the HTTP adapter behind the application boundary
and provides a stable error type.

### Mempool gateway

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
operational queries are documented in `prd/tech-spec/01-database.md`.

### HTTP adapter

- Exposes `/nodes` and `/health`.
- Calls the application use cases; contains no domain logic.
- Maps repository failures to a generic error response and logs the cause.
- Uses a small `axum` middleware for request logging.
- Leaves CORS, authentication, pagination, caching, and OpenAPI out of scope.

Endpoint examples and status codes are in `README.md`.

---

## 8. Composition Root

The composition root selects the concrete implementations and injects them:

```text
main
 ├─ MempoolNodeGateway::new(&config)          → gateway
 ├─ PostgresNodesRepositoryFactory.create()   → repository
 └─ build_app(gateway, repository, interval)   → App
```

- `build_app` receives already constructed dependencies, so it holds no
  infrastructure knowledge.
- The repository factory uses associated types, preserving static dispatch and
  avoiding `Box<dyn Trait>` and `async-trait`.
- Migrations are a database lifecycle operation, not a repository behavior, so
  the `migrate` command calls `composition::run_migrations` instead of the
  factory.

`build_app` constructs the graph once:

- One gateway adapter.
- One repository adapter.
- One PostgreSQL manager holding the pool.
- One router.
- One import interval.

The HTTP router and scheduler share the repository through `Arc`. This is the
process-scoped singleton: one instance per process, no global `static`, no
service locator.

Rules:

- No `static DB: OnceLock<PgPool>` or similar adapter singletons.
- No `lazy_static` or `once_cell`; `OnceLock` is only for rare process-wide
  infrastructure, not adapters.
- Tests create isolated instances; they never share production singletons.
- No runtime mock mode selected by an environment variable yet. When it is
  added, the mock repository must be compiled outside test builds and the
  composition root must select the factory at runtime, for example through an
  enum wrapper that implements `NodesRepository`.

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

Failure semantics are fail-closed:

- Transport, HTTP, or JSON failures keep the previous snapshot.
- Invalid individual records are skipped; valid records continue.
- Persistence failures roll back the transaction.
- The next interval retries automatically.

The trade-off is that a valid but unexpectedly small response could shrink the
dataset. The non-empty validation limits the risk but does not prove
completeness. Transaction details and the schema are in
`prd/tech-spec/01-database.md`.

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

Concrete causes are logged at the adapter boundary; internal details are never
returned to clients.

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
| Functional core, imperative shell | Pure domain and import policy; effects in adapters. |
| Algebraic data types | `Result`, `Option`, import plans/outcomes, error enums. |
| Validated value objects | `PublicKey`, `Sats`, `FirstSeen`. |
| Immutability | Immutable nodes; slices in functions; local `mut` only. |
| Composition over inheritance | Small functions, iterators, no class hierarchy. |
| Explicit dependencies | Ports and configuration passed as arguments. |
| Errors as values | Typed enums and `?`; no exceptions or sentinels. |
| Total functions after validation | Formatting cannot fail once values exist. |
| Effects at the boundary | HTTP, SQL, network, time, and logging in adapters. |
| No global mutable state | No global singletons, lazy statics, or shared mutable caches. |

| SOLID principle | Implementation |
| --- | --- |
| Single Responsibility | Domain formats, application decides, adapters do I/O. |
| Open/Closed | New gateway or repository adapters do not change use cases. |
| Liskov Substitution | Fakes and real adapters honor the same ports. |
| Interface Segregation | Two small ports instead of one broad service. |
| Dependency Inversion | Application defines traits; composition root injects. |

---

## 12. Dependency and Framework Policy

Frameworks stay at the infrastructure boundary:

- `axum` — HTTP adapter only.
- `sqlx` — PostgreSQL repository, factory, and infrastructure only.
- `reqwest` — mempool gateway only.
- `serde` — boundary DTOs only.
- `tokio`, `chrono`, `tracing`, `thiserror` — runtime, time, logs, and errors.

None appears in `domain` or `application`. Replacing an adapter does not change
the core.

Deliberately excluded:

- `async-trait` — native async traits and generics are sufficient.
- `tower-http` — a small middleware covers request logging.
- `dotenvy` — environment variables are explicit.
- `anyhow` — typed errors are preferred; `Box<dyn Error>` is enough in `main`.
- `mockall` / `wiremock` — hand-written fakes and pure parsing are clearer.
- `testcontainers` — PostgreSQL tests can use `sqlx::test` and Compose.
- `lazy_static` / `once_cell` — `OnceLock` is enough for rare global values.
- A DI container or service locator.
- Any ORM — SQL is explicit and small.

Key versions are in `README.md`; the full dependency list is in `Cargo.toml`.

---

## 13. Testing

Current coverage is unit-level: domain formatting, configuration, import
policy, and mempool parsing.

Target test matrix:

| Level | What is tested |
| --- | --- |
| Domain | Bitcoin and timestamp formatting, value validation |
| Application | Import policy and use cases with `MockNodesRepository` |
| Mempool gateway | Parsing, duplicates, missing/invalid fields |
| PostgreSQL repository | Snapshot replace, ordering, rollback, health |
| HTTP adapter | Status codes, JSON, error mapping, health |

`MockNodesRepository` is hand-written and compiled only in test builds. There
is no mocking framework, HTTP mock server, or testcontainer in the core tests.
Test commands are in `README.md`.

---

## 14. Trade-offs

| Decision | Trade-off | Alternative |
| --- | --- | --- |
| Full snapshot replacement | Simple and mirrors the ranking; a small valid response can shrink the dataset. | Upsert plus prune by batch ID. |
| PostgreSQL | Extra service; closer to production. | SQLite is simpler locally but less realistic. |
| Native async traits + generics | Verbose signatures; no trait objects. | `async-trait` + `Arc<dyn Trait>`. |
| Repository factory | Adds a trait and generic parameters; centralizes construction and keeps the composition root explicit. | Direct construction in `main`. |
| Process-scoped singleton | Explicit ownership; no global access point. | `static OnceLock`, easier global access but harder tests. |
| Single crate | Weaker compile-time layer enforcement. | A workspace. |
| In-process scheduler | Simple deployment; one replica only. | A dedicated worker. |
| Runtime SQL | No compile-time SQL validation. | `sqlx::query!` with offline metadata. |
| No pagination/cache/auth/CORS | Minimal scope; less flexible API. | Add at the adapter boundary when needed. |

---

## 15. Guardrails

- Maximum of two ports and two use cases.
- No trait for clock, logger, or configuration.
- No generic CRUD repository.
- No workspace, DI container, domain events, CQRS, or message bus.
- One repository factory trait; no service locator and no global accessor.
- No global mutable adapter singleton.
- No `Arc<Mutex<_>>`; use immutable sharing or channels.
- No mockall, wiremock, or testcontainers.
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
- Real and test repositories are constructed through one factory.
- Instances are shared explicitly, without global mutable state.

### Negative

- Native async trait signatures are verbose.
- The factory adds generic parameters.
- Mock repositories are selected by the test factory, not a runtime toggle.
- Runtime SQL trades compile-time validation for simpler builds.
- Full snapshot replacement is coarse.
- PostgreSQL and Docker add setup compared with SQLite.
- A single instance is assumed.
- The API is intentionally minimal.

### Revisit triggers

- **Multiple replicas:** add an advisory lock or a worker.
- **Runtime repository selection:** add an enum wrapper implementing `NodesRepository`.
- **Large dataset:** add pagination and indexes for query patterns.
- **Compile-time SQL safety:** adopt `sqlx::query!` with offline metadata.
- **Browser clients:** add explicit CORS.
- **Production secrets:** replace Compose defaults with a secret manager.
- **Operational visibility:** add metrics and distributed tracing.

---

## 17. References

- `README.md` — setup, environment variables, API examples, and commands.
- `prd/tech-spec/01-database.md` — database reference.
- `prd/Back-end Coding Challenge - Superhuman Docs.md` — challenge requirements.
- `src/domain/` — pure domain types and formatting.
- `src/application/` — ports and use cases.
- `src/adapters/` — HTTP and mempool adapters.
- `src/repositories/` — repository interface and implementations.
- `src/infrastructure/` — process-scoped PostgreSQL manager.
- `src/composition.rs` — composition root wiring.
