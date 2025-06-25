//! Static file serving for web UI assets

use std::path::PathBuf;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Simple static file handler for serving web assets
#[derive(Debug, Clone)]
pub struct StaticFileHandler {
    static_dir: PathBuf,
}

impl StaticFileHandler {
    /// Create new static file handler
    pub fn new() -> Self {
        Self {
            static_dir: PathBuf::from("static"),
        }
    }

    /// Serve a static file
    pub fn serve(&self, _path: &str) -> Result<Response, StatusCode> {
        // Simple placeholder implementation
        // In production, this would read and serve actual files
        Err(StatusCode::NOT_FOUND)
    }

    /// Serve a static file (async version)
    pub async fn serve_file(&self, path: &str) -> Response {
        match self.serve(path) {
            Ok(response) => response,
            Err(status) => (status, "File not found").into_response(),
        }
    }
}

impl Default for StaticFileHandler {
    fn default() -> Self {
        Self::new()
    }
}
