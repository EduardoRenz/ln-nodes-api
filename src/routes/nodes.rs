use std::sync::Arc;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::application::list_nodes::list_nodes;
use crate::domain::Node;
use crate::repositories::nodes_repository::NodesRepository;
use crate::routes::errors::ApiError;
use crate::routes::middleware::{RateLimiter, rate_limit};

#[derive(Debug, Serialize)]
pub struct NodeResponse {
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

/// Routes for the `nodes` resource.
///
/// The rate limiter is applied with `route_layer` so it only runs when the
/// request actually matches `/nodes`; an unknown path still reaches the
/// router's `404` fallback instead of being rejected with `429`.
pub fn router<R>(limiter: RateLimiter) -> Router<Arc<R>>
where
    R: NodesRepository + 'static,
{
    Router::new()
        .route("/nodes", get(get_nodes::<R>))
        .route_layer(axum::middleware::from_fn_with_state(limiter, rate_limit))
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
