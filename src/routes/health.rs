use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::repositories::nodes_repository::NodesRepository;

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn router<R>() -> Router<Arc<R>>
where
    R: NodesRepository + 'static,
{
    Router::new().route("/health", get(get_health::<R>))
}

async fn get_health<R>(State(repository): State<Arc<R>>) -> (StatusCode, Json<HealthResponse>)
where
    R: NodesRepository,
{
    match repository.ping().await {
        Ok(()) => (StatusCode::OK, Json(HealthResponse { status: "ok" })),
        Err(error) => {
            tracing::error!(%error, "database health check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "unavailable",
                }),
            )
        }
    }
}
