//! End-to-end test over a real HTTP server.
//!
//! Unlike the route tests in `src/routes/mod.rs`, which drive the `Router`
//! in memory through `tower::ServiceExt::oneshot`, this test boots the actual
//! server and speaks HTTP over a socket. That matters because
//! `into_make_service_with_connect_info` — which populates the client address
//! the rate limiter keys on — only runs on a real connection. The in-memory
//! tests have to inject that extension by hand, so they cannot prove the
//! production path works.
//!
//! The test follows the ambient configuration without overriding it. Whatever
//! `PERSISTENCE` selects is what gets exercised: the repository comes from
//! `ConfiguredNodesRepositoryFactory`, so Postgres, SQLite, and in-memory are
//! all covered by the same code path. If the required variables are missing,
//! `Config::from_env` fails and the test fails with that error.
//!
//! No import is performed. The gateway is built by its factory, which
//! validates the configuration wiring, but nothing is fetched and the
//! configured database is left untouched. As a result `/nodes` returns `[]`
//! on a fresh database or the existing snapshot otherwise — both are valid.
//!
//! Ignored by default because it needs a configured environment. Run with
//! `make test-live` or `cargo test --test e2e_live -- --ignored`.

use std::net::SocketAddr;
use std::time::Duration;

use axum::http::{StatusCode, header};
use rust_lightning_service::composition::build_app;
use rust_lightning_service::config::Config;
use rust_lightning_service::gateways::nodes_gateway_factory::{
    MempoolNodesGatewayFactory, NodesGatewayFactory,
};
use rust_lightning_service::repositories::nodes_repository_factory::{
    ConfiguredNodesRepositoryFactory, NodesRepositoryFactory,
};

/// Largest configured request count the exhaustion tests will drive.
///
/// Exceeding this means the test would issue more than a few hundred requests
/// just to prove a `429`, which is not worth the runtime.
const MAX_PRACTICAL_REQUESTS: u32 = 200;

/// Shortest configured window the exhaustion tests will drive.
///
/// With a very short window the counter can reset mid-test, turning a
/// deterministic assertion into a flake. The behaviour is still covered by the
/// unit tests in `src/routes/middleware.rs`, which control time explicitly.
const MIN_PRACTICAL_WINDOW: Duration = Duration::from_secs(10);

/// Serializes the tests in this binary.
///
/// Every test boots its own server but shares the configured database, and
/// booting runs migrations. In parallel, two tests can both observe a
/// migration as pending and both execute it, which fails with "table nodes
/// already exists". Production runs migrations once at startup, so the tests
/// serialize to mirror that. This also keeps each test's rate-limit counters
/// from being consumed by a sibling.
static BOOT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Boot the application on an ephemeral port using the ambient configuration.
///
/// Returns the bound address and a guard that must be held for the test's
/// duration; binding it keeps the boot lock, which is what serializes the
/// tests. The server task keeps running for the life of the test's runtime,
/// so no handle needs to be retained.
async fn boot() -> (SocketAddr, Config, tokio::sync::MutexGuard<'static, ()>) {
    let guard = BOOT_LOCK.lock().await;

    let config = Config::from_env().expect(
        "configuration must be valid; this test follows the ambient setup and does not override it",
    );

    let gateway = MempoolNodesGatewayFactory
        .create(&config)
        .expect("build the mempool gateway from configuration");

    let repository = ConfiguredNodesRepositoryFactory
        .create(&config)
        .await
        .expect("create the configured repository, with migrations applied");

    let app = build_app(gateway, repository, &config);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let address = listener.local_addr().expect("read the bound address");

    // `Serve` implements `IntoFuture` rather than `Future`, so it needs an
    // async block before it can be spawned. The handle is intentionally
    // dropped: the task keeps running for the life of the test runtime.
    tokio::spawn(async move {
        axum::serve(
            listener,
            app.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .expect("serve HTTP");
    });

    (address, config, guard)
}

fn url(address: SocketAddr, path: &str) -> String {
    format!("http://{address}{path}")
}

#[tokio::test]
#[ignore = "needs a configured environment; run with make test-live"]
async fn nodes_returns_a_valid_json_array() {
    let (address, _config, _guard) = boot().await;

    let response = reqwest::get(url(address, "/nodes"))
        .await
        .expect("GET /nodes over a real socket");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("decode the body as JSON");
    assert!(
        body.is_array(),
        "GET /nodes must return an array, got: {body}"
    );
}

#[tokio::test]
#[ignore = "needs a configured environment; run with make test-live"]
async fn health_reports_database_availability() {
    let (address, _config, _guard) = boot().await;

    let response = reqwest::get(url(address, "/health"))
        .await
        .expect("GET /health over a real socket");

    // The repository factory only returns once the backend is reachable and
    // migrations have applied, so a configured backend reports healthy here.
    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("decode the body as JSON");
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
#[ignore = "needs a configured environment; run with make test-live"]
async fn unknown_path_returns_404() {
    let (address, _config, _guard) = boot().await;

    let response = reqwest::get(url(address, "/definitely-not-a-route"))
        .await
        .expect("GET an unknown path over a real socket");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs a configured environment; run with make test-live"]
async fn health_stays_available_after_the_nodes_limit_is_exhausted() {
    let (address, config, _guard) = boot().await;

    if !exhaustion_is_practical(&config) {
        eprintln!(
            "skipping exhaustion assertions: configured limit of {} per {:?} is impractical to drive",
            config.rate_limit_max_requests, config.rate_limit_window
        );
        return;
    }

    let client = reqwest::Client::new();

    // The budget allows exactly `max_requests` successful requests, so drive
    // that many and then one more, which must be rejected.
    for _ in 0..config.rate_limit_max_requests {
        let response = client
            .get(url(address, "/nodes"))
            .send()
            .await
            .expect("GET /nodes over a real socket");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "/nodes must succeed while the budget lasts"
        );
    }

    let rejected = client
        .get(url(address, "/nodes"))
        .send()
        .await
        .expect("GET /nodes over a real socket");
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        rejected.headers().contains_key(header::RETRY_AFTER),
        "a 429 must carry Retry-After"
    );

    // /health is deliberately outside the limiter so monitoring survives an
    // abusive burst. This is the assertion that protects that exemption on the
    // real server, where the client address comes from the socket.
    for _ in 0..5 {
        let response = client
            .get(url(address, "/health"))
            .send()
            .await
            .expect("GET /health over a real socket");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "/health must not be rate limited"
        );
    }
}

#[tokio::test]
#[ignore = "needs a configured environment; run with make test-live"]
async fn unknown_paths_stay_404_when_the_nodes_limit_is_exhausted() {
    let (address, config, _guard) = boot().await;

    if !exhaustion_is_practical(&config) {
        eprintln!(
            "skipping exhaustion assertions: configured limit of {} per {:?} is impractical to drive",
            config.rate_limit_max_requests, config.rate_limit_window
        );
        return;
    }

    let client = reqwest::Client::new();

    // Exhaust the budget. Statuses are not asserted here; the test below this
    // loop's counterpart proves the limit behaves. What matters is that the
    // budget is spent before the unknown path is requested.
    for _ in 0..=config.rate_limit_max_requests {
        let _ = client
            .get(url(address, "/nodes"))
            .send()
            .await
            .expect("GET /nodes over a real socket");
    }

    let rejected = client
        .get(url(address, "/nodes"))
        .send()
        .await
        .expect("GET /nodes over a real socket");
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);

    // The limiter is attached with route_layer, so it only runs when the
    // request matches a route. An unknown path must still reach the 404
    // fallback rather than being rejected with a 429.
    let response = client
        .get(url(address, "/definitely-not-a-route"))
        .send()
        .await
        .expect("GET an unknown path over a real socket");
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "an unmatched path must return 404 even when the limiter has rejected /nodes"
    );
}

/// Whether driving the configured limit is fast enough to be worth doing.
///
/// Both the request count and the window matter: a large budget means many
/// requests, and a short window lets the counter reset mid-test.
fn exhaustion_is_practical(config: &Config) -> bool {
    config.rate_limit_max_requests <= MAX_PRACTICAL_REQUESTS
        && config.rate_limit_window >= MIN_PRACTICAL_WINDOW
}
