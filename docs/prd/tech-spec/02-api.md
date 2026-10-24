# 02 — API Mechanics

- **Status:** Accepted
- **Scope:** Routing composition, middleware pipeline, rate limiting, error mapping, and the HTTP server wiring.
- **Related documents:**
  - `../../../docs/api.md` — the consumer-facing contract: endpoints, request/response shapes, and examples.
  - `../Architecture.md` — layer boundaries and the dependency rule.
  - `01-database.md` — schema, read path, and data mapping.

This document explains **how** the HTTP layer behaves and why it is built that way. [`docs/api.md`](../../../docs/api.md) documents **what** the API exposes; it stays free of implementation rationale so it remains a stable contract for callers.

---

## 1. Overview

The service exposes a small, read-only JSON API over Axum 0.8.

| Route | Handler | Rate limited |
| --- | --- | --- |
| `GET /nodes` | List the stored ranking snapshot | Yes, per client IP |
| `GET /health` | Report service and database availability | No, deliberately |
| Anything else | `404` fallback | No |

The inbound layer lives in [`src/routes/`](../../../src/routes/) and is split by resource:

```text
src/routes/
├── mod.rs          router assembly: merge, fallback, logging layer
├── nodes.rs        GET /nodes, NodeResponse, rate-limit application
├── health.rs       GET /health, HealthResponse
├── errors.rs       ApiError, ErrorResponse, 404 fallback
└── middleware.rs   log_request, RateLimiter, rate_limit
```

Splitting is by **resource**, not by individual route. `GET /nodes` and a future `GET /nodes/{public_key}` belong in the same file because they share the same DTO, use case, and error set. The cut follows the domain concept, which already exists as `nodes` and `health`.

---

## 2. Routing Model

`src/routes/mod.rs` is the only place that knows the complete router:

```rust
nodes::router::<R>(limiter)
    .merge(health::router::<R>())
    .fallback(errors::not_found)
    .layer(axum::middleware::from_fn(middleware::log_request))
    .with_state(repository)
```

Three Axum rules constrain this ordering, and all three fail at runtime rather than at compile time (verified against the Axum 0.8.9 source in the local Cargo registry):

| Rule | Consequence if violated |
| --- | --- |
| Only one router may carry a `fallback` | `.merge()` **panics** (Axum `routing/mod.rs:289`) |
| `.layer()` applies only to routes registered before it | Later routes silently lose logging (Axum `docs/routing/layer.md`) |
| `.route_layer()` applies only to routes registered before it | Later routes silently lose the rate limit (Axum `docs/routing/route_layer.md`) |

The first rule is why `not_found` lives in `mod.rs` and never inside a sub-router. The second is why the logging layer is declared **after** every `.merge()`.

Each sub-router returns `Router<Arc<R>>` — state not yet applied. The final `.with_state(repository)` converts the merged router to `Router<()>`. Because all sub-routers share the same state type, `.merge()` succeeds without adapters.

---

## 3. Middleware Pipeline

Two middlewares run, at different scopes.

### `log_request` — global

Applied with `.layer()` in `mod.rs`, so it covers every route including the `404` fallback. It records method, path, response status, and elapsed milliseconds, then always delegates. It never short-circuits a request.

### `rate_limit` — scoped to `/nodes`

Applied with `.route_layer()` **inside** `nodes::router`, not at the root:

```rust
Router::new()
    .route("/nodes", get(get_nodes::<R>))
    .route_layer(axum::middleware::from_fn_with_state(limiter, rate_limit))
```

`route_layer` was chosen over `layer` deliberately. `Router::layer` wraps every request, including ones that match no route, so an over-limit client would receive `429` instead of `404` for an unknown path. `route_layer` runs only when the request actually matches a route, preserving correct `404` semantics.

The rate limiter is passed via `from_fn_with_state`, which gives the middleware its **own** state independent of the router's. That matters here: the router's state is the generic `Arc<R>`, and this trick avoids having to bundle a limiter into an application state struct. The `Arc<R>` design is unchanged.

---

## 4. Rate Limiting

### Algorithm

A **fixed window per client IP**. Each client IP owns a counter and a reset instant:

```text
count:   how many requests this client made in the current window
reset_at: when the window expires and the counter resets
```

On each request: lock the map, create or reuse the client's window, reset it if expired, increment, and compare against the maximum. Over the limit, the request is rejected with `429` and a `Retry-After` header carrying the seconds remaining in the window.

### Configuration

| Variable | Default | Bounds |
| --- | --- | --- |
| `RATE_LIMIT_MAX_REQUESTS` | `60` | Must be ≥ 1 |
| `RATE_LIMIT_WINDOW_SECS` | `60` | Must be ≥ 1 |

Both are read in `Config::from_lookup`. A value of `0` is rejected with a dedicated `ConfigError` rather than silently disabling the limit.

### Which routes are limited

Only `GET /nodes`. `GET /health` is **exempt by construction**: the limiter is applied inside `nodes::router`, so the health router never passes through it.

This is intentional. `/health` exists for monitoring and uptime probes. If an abusive client could exhaust a global limit, monitoring would go blind exactly when it is most needed — the failure would be invisible to the very system meant to detect it.

### Keying by client IP

The client address comes from `ConnectInfo<SocketAddr>`, which the server populates via `into_make_service_with_connect_info` (see §6).

There is a subtlety in how it is extracted. `Option<T>` as an extractor requires `T: OptionalFromRequestParts`, but `ConnectInfo` implements only `FromRequestParts`. So `Option<ConnectInfo<SocketAddr>>` does not compile.

`ConnectInfo` is stored internally as an extension, and `Extension<T>` **does** implement `OptionalFromRequestParts` with `Rejection = Infallible`. The middleware therefore reads:

```rust
connect_info: Option<Extension<ConnectInfo<SocketAddr>>>
```

This never fails. When the extension is absent the limiter falls back to a **shared bucket** (`SHARED_BUCKET`, the unspecified IPv4 address) instead of rejecting with `500`. The limit still applies; it just becomes global rather than per-client.

### Memory bounds

The window map would grow without limit as distinct client IPs arrive. Once it reaches `PRUNE_THRESHOLD` (10 000 entries), expired windows are pruned before inserting. Pruning is deferred until the map is large enough to justify the cost, so the common case stays a cheap map lookup.

### Known trade-offs

- **Window boundary burst.** A fixed window permits up to twice the configured maximum across a boundary — `max` at the end of one window plus `max` at the start of the next, seconds apart. Acceptable for this service. If stricter smoothing becomes necessary, a GCRA or token-bucket implementation (for example the `governor` crate) would replace this one.
- **Single mutex.** All clients share one `Mutex<HashMap<..>>`. Under heavy concurrent load the lock could contend. Not a concern at the current read-only ranking workload; sharding or a concurrent map is the escape hatch.
- **IP as identity.** Clients behind a shared NAT share a bucket, and a client rotating IPs evades the limit. Token- or credential-based keying would be the next step once authentication exists.
- **In-process only.** Counters live in memory, so they reset on restart and do not coordinate across replicas. A single-instance deployment is the current assumption.

---

## 5. Error Mapping

`ApiError` wraps any error source and maps to `500`:

```rust
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.source, "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { ... })).into_response()
    }
}
```

The error body is always the same single-field shape, so clients never learn internal details:

```json
{ "error": "internal server error" }
```

| Status | Source |
| --- | --- |
| `404` | Fallback handler; no route matched |
| `429` | Rate limiter; over quota |
| `500` | `ApiError`; the use case failed |
| `503` | `/health`; the database ping failed |

Two deliberate properties:

- **No internal leakage.** The source is logged with `tracing`, never serialized into the response. Stack details, connection strings, and SQL stay in the logs.
- **Single error shape.** Every error response is `{ "error": "<message>" }`. Callers can parse one shape.

A limitation worth noting: `ApiError` currently maps every use-case failure to `500`. Once a route needs `404` (missing node) or `400` (malformed path parameter), `ApiError` must become a per-variant enum instead of a generic box. That change has not been made because no route needs it yet.

---

## 6. Server Wiring

`main.rs` builds the server with `into_make_service_with_connect_info`:

```rust
axum::serve(
    listener,
    app.router.into_make_service_with_connect_info::<SocketAddr>(),
)
```

This is **required** for per-IP rate limiting. Without it the `ConnectInfo` extension is never inserted, the middleware takes the shared-bucket path, and every caller shares one counter.

The server binds `0.0.0.0` on `PORT` and shuts down gracefully on `SIGINT`/`SIGTERM`. Shutdown also signals the background importer, which is awaited before the process exits.

---

## 7. Testing Strategy

Two levels of test cover this layer, both in-module.

### Unit tests — the limiter itself

`middleware.rs` tests `RateLimiter::check` directly with explicit `Instant` values, so window expiry is tested without sleeping or a timer:

- allows requests up to the maximum
- rejects once over the maximum, with a non-zero retry delay
- keeps client counters independent
- resets the counter after the window elapses
- uses the shared bucket when the address is unavailable
- prunes expired windows once the map reaches the threshold

### Route tests — full middleware stack

`mod.rs` tests drive the real `Router` through `tower::ServiceExt::oneshot`, which is already available: `tower` sits in `[dev-dependencies]` with the `util` feature.

Tests attach a client address the same way production does:

```rust
Request::builder()
    .uri(path)
    .extension(ConnectInfo(address))
    .body(Body::empty())
```

Covered behaviours:

| Test | Asserts |
| --- | --- |
| `serves_nodes_and_health` | both routes return `200` |
| `unknown_path_returns_404_not_429` | the fallback wins over the limiter |
| `rejects_nodes_over_the_limit_with_retry_after` | `429` carries `Retry-After` |
| `health_is_not_rate_limited` | `/health` stays reachable after `/nodes` is exhausted |
| `limits_per_client_address` | a second client is unaffected by the first |

The test that asserts `/health` survives an exhausted `/nodes` window is the one that protects the monitoring exemption described in §4.

---

## 8. Limitations and Trade-offs

- **Fixed-window limiter** allows a boundary burst of up to 2× the configured maximum.
- **IP-based keying** breaks behind NAT and is evadable by rotating addresses.
- **In-process counters** reset on restart and do not coordinate across replicas.
- **`ApiError` maps everything to `500`**; per-variant status codes await a route that needs them.
- **No authentication, CORS, caching, pagination, or write endpoints.** The API is read-only by scope.
- **Generic `Arc<R>` router state.** Adding a second dependency (another repository, a clock) will require an application state struct, because an Axum router has exactly one state type.
- **`log_request` logs at `info` for every request.** At high volume this is noisy and could warrant `debug` level plus sampling.

---

## 9. References

- [`docs/api.md`](../../../docs/api.md) — endpoint contract, response shapes, and examples.
- `../Architecture.md` — layer boundaries and dependency rule.
- `01-database.md` — read path, health-check query, and data mapping.
- `src/routes/mod.rs` — router assembly and route-level tests.
- `src/routes/middleware.rs` — `RateLimiter`, `rate_limit`, `log_request`, and limiter unit tests.
- `src/routes/nodes.rs` — `/nodes` handler and the scoped rate-limit application.
- `src/routes/health.rs` — `/health` handler, deliberately outside the limiter.
- `src/routes/errors.rs` — `ApiError`, `ErrorResponse`, and the `404` fallback.
- `src/config.rs` — rate-limit configuration and validation.
- `src/main.rs` — `into_make_service_with_connect_info` and graceful shutdown.
