use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

// converts any error to HTTP response
pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("Error: {:?}", self.0);

        // Return generic 500 to client
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

// Auto-convert any error type into AppError
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error> + Send + Sync,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[derive(Deserialize)]
pub struct StatusQuery {
    #[serde(rename = "relativePath")]
    pub relative_path: String,
}

// hash path for safe directory name
pub fn hash_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());

    // Return first 16 chars (64 bits) for shorter directory names
    // astronomically unlikely to collide
    format!("{:x}", hasher.finalize())[..16].to_string()
}
