use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::application::list_nodes::{ListError, list_nodes};
use crate::domain::Node;
use crate::repositories::nodes_repository::NodesRepository;

#[derive(Debug, Serialize)]
struct NodeResponse {
    public_key: String,
    alias: String,
    capacity: String,
    first_seen: String,
}

impl From<&Node> for NodeResponse {
    fn from(node: &Node) -> Self {
        Self {
            public_key: node.public_key.as_str().to_owned(),
            alias: node.alias.clone(),
            capacity: node.capacity.as_btc(),
            first_seen: node.first_seen.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: &'static str,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub struct ApiError {
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl ApiError {
    fn internal(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(source),
        }
    }
}

impl From<ListError> for ApiError {
    fn from(error: ListError) -> Self {
        Self::internal(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.source, "request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "internal server error",
            }),
        )
            .into_response()
    }
}

pub fn router<R>(repository: Arc<R>) -> Router
where
    R: NodesRepository + 'static,
{
    Router::new()
        .route("/nodes", get(get_nodes::<R>))
        .route("/health", get(get_health::<R>))
        .fallback(not_found)
        .layer(middleware::from_fn(log_request))
        .with_state(repository)
}

async fn get_nodes<R>(State(repository): State<Arc<R>>) -> Result<Json<Vec<NodeResponse>>, ApiError>
where
    R: NodesRepository,
{
    let nodes = list_nodes(repository.as_ref())
        .await
        .map_err(ApiError::from)?;

    Ok(Json(nodes.iter().map(NodeResponse::from).collect()))
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

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: "not found" }),
    )
}

async fn log_request(request: Request, next: Next) -> Response {
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
