use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    Unauthorized,
    NotFound,
    BadRequest(String),
    Conflict(String),
    Internal(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Belum login".to_string()),
            AppError::NotFound => (StatusCode::NOT_FOUND, "Data tidak ditemukan".to_string()),
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::Internal(e) => {
                eprintln!("error internal: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Terjadi kesalahan di server".to_string())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            if db.is_unique_violation() {
                return AppError::Conflict("Data sudah ada".to_string());
            }
        }
        AppError::Internal(e.into())
    }
}

/// Mengubah error dari library (sesi, hashing) menjadi error internal.
pub fn internal<E: std::fmt::Display>(e: E) -> AppError {
    AppError::Internal(anyhow::anyhow!("{e}"))
}
