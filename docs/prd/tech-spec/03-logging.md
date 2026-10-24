# 03 — Logging

- **Status:** Accepted
- **Scope:** The two logging mechanisms in the service (console diagnostics and the import-run audit sink), the `RunLogger` abstraction, existing backends, configuration, and extension points.
- **Related documents:**
  - `../Architecture.md` — layer boundaries and the dependency rule.
  - `01-database.md` — what an import run persists.
  - `02-api.md` — how request diagnostics appear on the console.
  - `../../../README.md` — environment variables and run commands.

This document explains **what logging exists in the service, why there are two mechanisms, and what each one is for**. It is the reference for adding a backend or for understanding where a given line ends up.

---

## 1. Overview

The service logs through two independent mechanisms with different jobs:

| | Console diagnostics | Import-run audit |
| --- | --- | --- |
| Mechanism | [`tracing`](https://docs.rs/tracing) | `RunLogger` trait (this document) |
| Purpose | Debug the process: requests, errors, lifecycle | Answer "what happened to the imports on a given day?" |
| Levels | `RUST_LOG` filter (`error`, `info`, `debug`, ...) | None — every run is recorded |
| Destination | stdout/stderr (subscriber-swappable) | One file per UTC day (`LOG_DIR`) |
| Configured by | `RUST_LOG` | `LOG_BACKEND`, `LOG_DIR` |
| Selection | `telemetry::init` (single global subscriber) | `ConfiguredRunLoggerFactory` (per-process instance) |

They never overlap: diagnostic messages stay with `tracing`, and the audit record stays with `RunLogger`. `tracing` is already a pluggable abstraction — a future "export everything to Datadog" story would add a **subscriber layer** there, without touching any call site. This document covers the second mechanism.

```text
scheduler / import command
        │  outcome: Result<ImportOutcome, ImportError>
        ▼
  RunLogger::record_or_warn ──► LoggerError? → tracing::warn (never interrupts)
        │
        ▼
  AnyRunLogger ── selected by LOG_BACKEND ──► FileRunLogger
                                                    │
                                                    ├─ format_run  (domain line)
                                                    └─ FileSink    (date-partitioned file)
                                                              │
                                                              ▼
                                              log/2026-10-24.log
```

---

## 2. The `RunLogger` port

Source: [`src/logging/run_logger.rs`](../../../src/logging/run_logger.rs).

```rust
pub trait RunLogger: Send + Sync {
    fn record(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> impl Future<Output = Result<(), LoggerError>> + Send;

    fn record_or_warn(
        &self,
        outcome: &Result<ImportOutcome, ImportError>,
    ) -> impl Future<Output = ()> + Send { /* ... */ }
}
```

Design decisions:

- **Typed payload, not a message string.** The caller hands over the run's outcome; the backend decides how to represent it. The file backend renders a canonical line, a future Datadog backend would map fields to metrics/tags. Callers never build log strings.
- **`impl Future + Send` (RPITIT), not `async fn`.** Same pattern as `NodesGateway`/`NodesRepository`: the `+ Send` bound is required because the scheduler runs inside `tokio::spawn`, and `async fn` in a trait does not guarantee a `Send` future.
- **No levels.** This is an audit sink: every run is recorded regardless of `RUST_LOG`. Suppressing records under `error` would lose audit precisely when the process is misbehaving.
- **`record_or_warn` is a provided method.** The policy "a logging failure degrades to a warning and never interrupts an import" lives in exactly one place, shared by the scheduler and the one-shot `import` command. Both call sites are a single line.
- **`LoggerError` is a sum of backend errors** (`Sink` today; future variants for HTTP-backed backends). The `#[error(transparent)]` keeps the source's text — which already carries the path — visible on the console.

---

## 3. Existing backends

### 3.1 File (`LOG_BACKEND=file`, the default)

Source: [`src/logging/file_run_logger.rs`](../../../src/logging/file_run_logger.rs).

`FileRunLogger` composes two responsibilities:

- **`FileSink`** (`src/logging/file_sink.rs`) — the reusable file mechanics: directory creation, date partitioning, append-only writes.
- **`format_run`** — the domain line format, kept private to this backend because a non-file backend maps fields instead of rendering strings.

Construction wires what the backend needs from configuration:

```rust
FileSink::new("import").in_dir(config.log_dir).pattern("{date}.log")
```

The line format (one line per run, append-only):

```text
<RFC 3339 UTC> imported persistence=<backend> count=<total> new=<delta>
<RFC 3339 UTC> skipped persistence=<backend> mempool returned no valid nodes
<RFC 3339 UTC> error persistence=<backend> <ImportError message with cause>
```

Examples:

```text
2026-10-24T03:54:43Z imported persistence=sqlite count=100 new=100
2026-10-24T03:59:12Z imported persistence=postgres count=100 new=13
2026-10-24T04:02:00Z error persistence=sqlite failed to fetch nodes: mempool request failed
2026-10-24T19:22:55Z error persistence=postgres repository operation failed: error communicating with database: failed to lookup address information: Temporary failure in name resolution
```

**Error lines carry the full cause chain.** `ImportError`, `ListError`, and
`RepositoryFactoryError` embed their `source` in the display, and
`RepositoryError::Unavailable` carries the concrete database cause as a
string — so the file alone answers *why* a run failed, without the console
(§6.1).

Field meanings:

| Field | Meaning |
| --- | --- |
| `persistence` | Backend the run wrote to: `postgres`, `sqlite`, `in_memory` (canonical label from `RepositoryType::as_str`, independent of how `PERSISTENCE` was spelled) |
| `count` | Total rows in the replaced snapshot |
| `new` | Rows whose public key was absent from the previous snapshot (the delta; `count_new` in `application/import_nodes.rs`) |

File: `LOG_DIR/YYYY-MM-DD.log` (UTC date of the run). Runs that failed before reaching the repository are recorded too — the file answers "what happened", not only "what succeeded".

### 3.2 Runtime selection

Sources: [`src/logging/any_run_logger.rs`](../../../src/logging/any_run_logger.rs), [`src/logging/run_logger_factory.rs`](../../../src/logging/run_logger_factory.rs).

Mirrors the repository layer exactly:

- `AnyRunLogger` — enum delegating to the selected variant (pattern: `AnyNodesRepository`). Adding a backend means adding a variant and a match arm; no call site changes.
- `RunLoggerFactory` trait + `ConfiguredRunLoggerFactory` — reads `config.log_backend` and builds the backend with the configuration it needs.

Construction is **infallible today**: the file backend has no build-time failure mode. When an HTTP-backed backend arrives (which may validate credentials), the return type widens to `Result` — mechanical, one `?` at the call site.

The instance is created **once per process** in `main` after `telemetry::init`, then either used by the one-shot `import` command or moved into the scheduler task.

---

## 4. `FileSink` — reusable file mechanics

Source: [`src/logging/file_sink.rs`](../../../src/logging/file_sink.rs).

`FileSink` is a plain value with no global state, so **any module may build one by choice** for its own diagnostics — no factory involved (the factory exists for *runtime selection* of the audit backend; a compile-time choice does not need it):

```rust
let sink = FileSink::new("nodes_gateway");
sink.append("fetch failed: connection refused")?;
// → log/2026-10-24_nodes_gateway.log
```

Capabilities:

| Capability | API | Notes |
| --- | --- | --- |
| Origin name | `FileSink::new("nodes_gateway")` | Default pattern `{date}_{origin}.log` |
| Custom directory | `.in_dir(path)` | Defaults to `config::DEFAULT_LOG_DIR` (`log`) |
| Custom filename pattern | `.pattern("{origin}.{date}.txt")` | Placeholders `{date}` (`YYYY-MM-DD`, UTC) and `{origin}`; a pattern without `{date}` appends to one ever-growing file |
| Append a line | `.append(line)` / `.append_at(timestamp, line)` | Creates the directory on demand; newline appended |
| Resolve the path | `.path_for(timestamp)` | Pure — useful for tests and diagnostics |

Implementation notes:

- **Stateless:** the file is opened per append and closed on drop, so creating many instances never leaks file descriptors. `O_APPEND` keeps concurrent writers from interleaving lines.
- **Errors carry the path:** `SinkError { path, source }` renders `failed to write log at <path>`, so a caller's warning is self-contained.
- **No levels, no async:** a single small append is microseconds of blocking I/O, same as the audit path. If a module ever needs more (levels, buffering), that is a reason to consider `tracing` layers first.

**Current usage:** only `FileRunLogger` consumes it in-tree. No module wires its own sink yet — the capability exists so a module can opt in without growing the logging core.

---

## 5. Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `LOG_BACKEND` | `file` | Backend selected at startup. Unknown values fail fast with `ConfigError::Invalid { name: "LOG_BACKEND", ... }` — the seam future backends plug into. |
| `LOG_DIR` | `log` | Directory for the `file` backend. Compose overrides it to `/app/log` (bind-mounted `log/`, `HOST_UID`/`HOST_GID` so files stay owned by the host user). |
| `RUST_LOG` | `info` | Console diagnostics only; has no effect on the audit file. Tuning suggestions in §6.1. |

Like every other setting, the Rust process does not load `.env` on its own — export the variables (or `set -a; source .env; set +a`) when running outside Compose.

---

## 6. Failure policy

- **A logging failure never interrupts an import.** `record_or_warn` degrades to `tracing::warn!`; the scheduler keeps its loop and the `import` command keeps its exit code (0 for imported/skipped, non-zero only for fetch/database failures).
- **A failed run is still recorded.** The `error persistence=...` line exists so the daily file is a complete audit, not a success log.
- **A run attempt that cannot reach the database is recorded too.** The `import` command wraps a repository *construction* failure (`repository_failure` in `main.rs`) as a run failure, so a CLI import against a down database produces an audit line instead of disappearing into stderr.
- **`serve` startup failures are console-only by design.** If the process cannot build the repository it exits before any run exists; the scheduler only records once it is running. Diagnose boot crashes with `docker compose logs app` (retry ladder in §6.1).
- **The write itself is best-effort append.** No rotation, no retention, no crash-recovery queue — out of scope (see README trade-offs).

### 6.1 Operating under failure

What each channel shows when the database is down (verified against the live
Compose stack):

| Where you look | Content |
| --- | --- |
| `log/YYYY-MM-DD.log` | `<ts> error persistence=postgres repository operation failed: <concrete sqlx cause>` — one line per attempted run, self-contained |
| Console (`docker compose logs app`) | Boundary line with the raw cause (`repositories::postgres_nodes_repository: database operation failed error=…`), then the contextual line (`scheduler: import failed…`, `routes::errors: request failed…`), then the access line (`http request … status=500`) |
| Boot / CLI import | `WARN … connection failed; retrying attempt=N` (backoff), `ERROR … connection attempts exhausted attempt=10`, exit non-zero |
| Client | `GET /nodes` → `500 {"error":"internal server error"}` (never leaks internals); `GET /health` → `503 {"status":"unavailable"}` |

Recovery is automatic: the next scheduler tick records a normal `imported`
line; the pool needs no restart.

**Tuning `RUST_LOG`.** The default `info` is verbose during an outage:
`sqlx` logs pool ping failures and the access log prints every request.
For a quieter incident view:

```bash
RUST_LOG=rust_lightning_service=info,sqlx=warn
```

Set `debug` on `rust_lightning_service::scheduler` while investigating import
cadence, or `warn` globally when only failures matter. None of this affects
the audit file, which is written regardless of `RUST_LOG`.

---

## 7. Adding a backend (Datadog, Sentry, ...)

The whole point of the seam: no existing file grows logic.

```text
1. src/logging/datadog_run_logger.rs   — impl RunLogger (map outcome → fields)
2. src/logging/any_run_logger.rs       — variant + match arm
3. src/logging/run_logger_factory.rs   — arm reading LogBackend::Datadog
4. src/config.rs                       — LogBackend::Datadog, parse arm, as_str
5. Cargo.toml                          — dependency behind a cargo feature
                                         (default build stays dependency-free)
```

If the backend fails to *build* (missing API key, bad URL), widen `RunLoggerFactory::create` to `Result` at the same time.

If the goal instead is "export **all console diagnostics** to Datadog/Sentry", that is a `tracing` **subscriber layer**, not a `RunLogger` backend — see §1; the two mechanisms are complementary.

---

## 8. Testing

| Layer | Where | What |
| --- | --- | --- |
| Line format + partitioning | `file_run_logger.rs` (unit) | exact lines, per-date files, skipped/error rows, current-UTC path |
| Error cause chains | `import_nodes.rs`, `list_nodes.rs`, `nodes_repository_factory.rs`, `main.rs` (unit) | displays embed the concrete cause (database, sqlx, factory) so audit lines explain themselves |
| File mechanics | `file_sink.rs` (unit) | default/custom patterns, directory creation, unreachable-path error carries the path |
| Delegation | `any_run_logger.rs` (unit) | variant forwards success and error; `record_or_warn` returns even when the sink fails |
| Wiring | `run_logger_factory.rs` (unit) | default selection, `LOG_BACKEND` selection, records land in `LOG_DIR` |
| Config parsing | `config.rs` (unit) | default, case-insensitive, unknown → `Invalid` |

Run everything with `cargo test --all-targets` (or `make test`, which includes the binary tests).
