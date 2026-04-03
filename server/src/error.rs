use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("device not found")]
    DeviceNotFound,
    #[error("job not found")]
    JobNotFound,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("device_locked")]
    DeviceLocked { held_by_job_id: String },
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("conflict: job is already {0}")]
    JobNotCancellable(String),
    #[error("internal server error: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    held_by_job_id: Option<String>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::DeviceNotFound | Self::JobNotFound => StatusCode::NOT_FOUND,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::DeviceLocked { .. } | Self::Conflict(_) | Self::JobNotCancellable(_) => {
                StatusCode::CONFLICT
            }
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let body = ErrorBody {
            error: self.to_string(),
            held_by_job_id: match self {
                Self::DeviceLocked { held_by_job_id } => Some(held_by_job_id),
                _ => None,
            },
        };

        (status, Json(body)).into_response()
    }
}
