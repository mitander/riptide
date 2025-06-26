//! Riptide Web - Web UI and API server
//!
//! Provides HTTP-based interface for managing torrents and streaming media.

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

pub mod handlers;
pub mod server;
pub mod static_files;
pub mod templates;

// Re-export main types
pub use handlers::WebHandlers;
pub use server::{WebServer, WebServerConfig};
pub use templates::TemplateEngine;

/// HTML response type for web handlers
pub type HtmlResponse = Html<String>;

/// Web UI specific errors.
#[derive(Debug, thiserror::Error)]
pub enum WebUIError {
    #[error("Template error: {reason}")]
    TemplateError { reason: String },

    #[error("Handler error: {reason}")]
    HandlerError { reason: String },

    #[error("Internal error: {reason}")]
    InternalError { reason: String },

    #[error("Server failed to start on {address}: {reason}")]
    ServerStartFailed {
        address: std::net::SocketAddr,
        reason: String,
    },
}

pub type Result<T> = std::result::Result<T, WebUIError>;

impl IntoResponse for WebUIError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            WebUIError::TemplateError { reason } => (StatusCode::INTERNAL_SERVER_ERROR, reason),
            WebUIError::HandlerError { reason } => (StatusCode::BAD_REQUEST, reason),
            WebUIError::InternalError { reason } => (StatusCode::INTERNAL_SERVER_ERROR, reason),
            WebUIError::ServerStartFailed { reason, .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, reason)
            }
        };

        let body = format!("Error: {error_message}");
        (status, body).into_response()
    }
}
