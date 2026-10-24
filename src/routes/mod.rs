mod errors;
mod health;
mod middleware;
mod nodes;

pub use middleware::RateLimiter;

use std::sync::Arc;

use axum::Router;

use crate::repositories::nodes_repository::NodesRepository;

/// Build the HTTP router for the service.
///
/// Sub-routers are merged into one, so the ordering here matters: the
/// `fallback` is declared once (merging two routers that each carry one
/// panics) and the logging layer is applied last so it covers every route
/// registered above.
pub fn router<R>(repository: Arc<R>, limiter: RateLimiter) -> Router
where
    R: NodesRepository + 'static,
{
    nodes::router::<R>(limiter)
        .merge(health::router::<R>())
        .fallback(errors::not_found)
        .layer(axum::middleware::from_fn(middleware::log_request))
        .with_state(repository)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::body::Body;
    use axum::extract::{ConnectInfo, Request};
    use axum::http::{StatusCode, header};
    use tower::ServiceExt;

    use super::*;
    use crate::repositories::mock_nodes_repository::MockNodesRepository;

    /// Build a request with a client address attached, the same way
    /// `into_make_service_with_connect_info` does in production.
    fn request_from(path: &str, ip: Ipv4Addr) -> Request {
        let address = SocketAddr::new(IpAddr::V4(ip), 40_000);
        Request::builder()
            .uri(path)
            .extension(ConnectInfo(address))
            .body(Body::empty())
            .expect("request")
    }

    fn app(max_requests: u32) -> Router {
        let repository = Arc::new(MockNodesRepository::new());
        let limiter = RateLimiter::new(max_requests, std::time::Duration::from_secs(60));
        router(repository, limiter)
    }

    #[tokio::test]
    async fn serves_nodes_and_health() {
        let app = app(10);

        let response = app
            .clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("nodes");
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(request_from("/health", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("health");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_path_returns_404_not_429() {
        let response = app(1)
            .oneshot(request_from("/nope", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("fallback");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rejects_nodes_over_the_limit_with_retry_after() {
        let app = app(2);

        for _ in 0..2 {
            let response = app
                .clone()
                .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
                .await
                .expect("within quota");
            assert_eq!(response.status(), StatusCode::OK);
        }

        let response = app
            .clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("over quota");

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(
            response.headers().contains_key(header::RETRY_AFTER),
            "429 must carry a Retry-After header"
        );
    }

    #[tokio::test]
    async fn health_is_not_rate_limited() {
        let app = app(1);

        // Exhaust the shared window through /nodes first.
        app.clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("within quota");
        app.clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("within quota");

        // /health must stay reachable so monitoring is never blocked.
        for _ in 0..5 {
            let response = app
                .clone()
                .oneshot(request_from("/health", Ipv4Addr::new(127, 0, 0, 1)))
                .await
                .expect("health");
            assert_eq!(response.status(), StatusCode::OK);
        }
    }

    #[tokio::test]
    async fn limits_per_client_address() {
        let app = app(1);

        app.clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("within quota");

        let blocked = app
            .clone()
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 1)))
            .await
            .expect("over quota");
        assert_eq!(blocked.status(), StatusCode::TOO_MANY_REQUESTS);

        // A different client has its own counter.
        let other = app
            .oneshot(request_from("/nodes", Ipv4Addr::new(127, 0, 0, 2)))
            .await
            .expect("other client");
        assert_eq!(other.status(), StatusCode::OK);
    }
}
