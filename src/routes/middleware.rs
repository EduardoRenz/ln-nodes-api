use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::{ConnectInfo, Extension, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::routes::errors::ErrorResponse;

pub async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = Instant::now();

    let response = next.run(request).await;

    tracing::info!(
        %method,
        %path,
        status = %response.status(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "http request"
    );

    response
}

/// Fallback bucket used when the client address is unavailable.
///
/// `ConnectInfo` is only populated when the server is built with
/// `into_make_service_with_connect_info`. Rather than rejecting the request
/// with a `500` when that is missing, the limiter falls back to a single
/// shared bucket so the limit still applies.
const SHARED_BUCKET: IpAddr = IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

/// Number of tracked clients above which expired windows are pruned.
///
/// Without pruning the map would grow without bound as distinct clients
/// arrive. Pruning only runs once the map is large enough that the cost
/// is justified.
const PRUNE_THRESHOLD: usize = 10_000;

/// Fixed-window counter for a single client.
#[derive(Debug, Clone, Copy)]
struct Window {
    count: u32,
    reset_at: Instant,
}

/// Per-client fixed-window rate limiter.
///
/// The counters live behind an `Arc<Mutex<_>>` so the limiter can be cloned
/// into each middleware invocation while sharing one set of counters.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    windows: Arc<Mutex<HashMap<IpAddr, Window>>>,
    max_requests: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            windows: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window,
        }
    }

    /// Record one request for `client` and report whether it is allowed.
    ///
    /// Returns `Ok(())` while the client is within quota. Once the client
    /// exceeds `max_requests` in the current window it returns the time to
    /// wait until the window resets.
    fn check(&self, client: IpAddr, now: Instant) -> Result<(), Duration> {
        let mut windows = self.windows.lock().expect("rate limiter mutex poisoned");

        if windows.len() >= PRUNE_THRESHOLD {
            windows.retain(|_, window| window.reset_at > now);
        }

        let window = windows.entry(client).or_insert(Window {
            count: 0,
            reset_at: now + self.window,
        });

        if now >= window.reset_at {
            window.count = 0;
            window.reset_at = now + self.window;
        }

        window.count += 1;

        if window.count > self.max_requests {
            return Err(window.reset_at.saturating_duration_since(now));
        }

        Ok(())
    }
}

pub async fn rate_limit(
    State(limiter): State<RateLimiter>,
    // Read the address through `Extension` rather than `ConnectInfo` so the
    // extractor can be wrapped in `Option`. `ConnectInfo` itself does not
    // implement `OptionalFromRequestParts`, so `Option<ConnectInfo<_>>` does
    // not compile. `Extension` does, and its rejection is `Infallible`.
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    request: Request,
    next: Next,
) -> Response {
    let client = connect_info
        .map(|Extension(ConnectInfo(address))| address.ip())
        .unwrap_or(SHARED_BUCKET);

    match limiter.check(client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            tracing::warn!(%client, "rate limit exceeded");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [(
                    header::RETRY_AFTER,
                    retry_after.as_secs().max(1).to_string(),
                )],
                Json(ErrorResponse {
                    error: "too many requests",
                }),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, last))
    }

    #[test]
    fn allows_requests_up_to_the_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let now = Instant::now();

        for _ in 0..3 {
            assert_eq!(limiter.check(ip(1), now), Ok(()));
        }
    }

    #[test]
    fn rejects_the_request_over_the_limit() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        let now = Instant::now();

        limiter.check(ip(1), now).expect("within quota");
        limiter.check(ip(1), now).expect("within quota");

        let retry_after = limiter.check(ip(1), now).expect_err("over quota");
        assert!(retry_after > Duration::ZERO);
        assert!(retry_after <= Duration::from_secs(60));
    }

    #[test]
    fn keeps_clients_independent() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let now = Instant::now();

        limiter.check(ip(1), now).expect("within quota");
        assert!(limiter.check(ip(1), now).is_err());

        // A different client has its own counter and is unaffected.
        limiter
            .check(ip(2), now)
            .expect("other client within quota");
    }

    #[test]
    fn resets_count_after_the_window_elapses() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let start = Instant::now();

        limiter.check(ip(1), start).expect("within quota");
        assert!(limiter.check(ip(1), start).is_err());

        let later = start + Duration::from_secs(61);
        limiter.check(ip(1), later).expect("after window reset");
    }

    #[test]
    fn falls_back_to_a_shared_bucket_without_connect_info() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        let now = Instant::now();

        limiter.check(SHARED_BUCKET, now).expect("within quota");
        assert!(limiter.check(SHARED_BUCKET, now).is_err());
    }

    #[test]
    fn prunes_expired_windows_once_the_map_is_large() {
        let limiter = RateLimiter::new(5, Duration::from_millis(1));
        let start = Instant::now();

        for host in 0..PRUNE_THRESHOLD {
            let address = IpAddr::V4(std::net::Ipv4Addr::new(
                10,
                0,
                (host >> 8) as u8,
                host as u8,
            ));
            limiter.check(address, start).expect("within quota");
        }

        assert_eq!(
            limiter.windows.lock().expect("lock").len(),
            PRUNE_THRESHOLD,
            "map is at the pruning threshold"
        );

        // An expired window triggers pruning, which drops the stale entries
        // and leaves room for the new client.
        let later = start + Duration::from_secs(5);
        limiter.check(ip(250), later).expect("within quota");

        assert!(limiter.windows.lock().expect("lock").len() < PRUNE_THRESHOLD);
    }
}
