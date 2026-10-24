use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::application::list_nodes::ListError;

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
}

pub struct ApiError {
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl ApiError {
    pub fn internal(source: impl std::error::Error + Send + Sync + 'static) -> Self {
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

pub async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: "not found" }),
    )
}
