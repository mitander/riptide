//! Web UI module for library browsing and torrent management
//!
//! Provides a modern web interface for managing torrents, browsing the media library,
//! and monitoring streaming activity. Built with server-side rendering for optimal
//! performance and SEO compatibility.

pub mod handlers;
pub mod server;
pub mod static_files;
pub mod templates;

use std::sync::Arc;

use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
pub use handlers::WebHandlers;
pub use server::{WebServer, WebServerConfig};
pub use templates::TemplateEngine;
use tokio::sync::RwLock;

use crate::config::RiptideConfig;
use crate::streaming::DirectStreamingService;
use crate::torrent::TorrentEngine;

/// Complete web UI service providing library management and monitoring interface.
///
/// Integrates with the streaming service and torrent engine to provide a comprehensive
/// web-based interface for managing the Riptide media server.
pub struct WebUIService {
    web_server: WebServer,
    template_engine: TemplateEngine,
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    streaming_service: Arc<RwLock<DirectStreamingService>>,
}

impl WebUIService {
    /// Creates new web UI service with configuration.
    pub fn new(
        config: RiptideConfig,
        torrent_engine: Arc<RwLock<TorrentEngine>>,
        streaming_service: Arc<RwLock<DirectStreamingService>>,
    ) -> Self {
        let web_config = WebServerConfig::from_riptide_config(&config);
        let template_engine = TemplateEngine::new();
        let handlers =
            WebHandlers::new(Arc::clone(&torrent_engine), Arc::clone(&streaming_service));
        let web_server = WebServer::new(web_config, handlers, template_engine.clone());

        Self {
            web_server,
            template_engine,
            torrent_engine,
            streaming_service,
        }
    }

    /// Start the web UI server.
    ///
    /// # Errors
    /// - `WebUIError::ServerStartFailed` - Failed to bind to port or start server
    pub async fn start(&self) -> Result<(), WebUIError> {
        self.web_server.start().await
    }

    /// Stop the web UI server gracefully.
    pub async fn stop(&mut self) -> Result<(), WebUIError> {
        self.web_server.stop().await
    }

    /// Get the base URL for the web interface.
    pub fn base_url(&self) -> String {
        self.web_server.base_url()
    }
}

/// Web UI service errors.
#[derive(Debug, thiserror::Error)]
pub enum WebUIError {
    #[error("Failed to start web server on {address}: {reason}")]
    ServerStartFailed {
        address: std::net::SocketAddr,
        reason: String,
    },

    #[error("Template rendering error: {reason}")]
    TemplateError { reason: String },

    #[error("Static file not found: {path}")]
    StaticFileNotFound { path: String },

    #[error("Internal server error: {reason}")]
    InternalError { reason: String },
}

impl IntoResponse for WebUIError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            WebUIError::ServerStartFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            WebUIError::TemplateError { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            WebUIError::StaticFileNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            WebUIError::InternalError { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        (status, error_message).into_response()
    }
}

/// HTML response type alias for cleaner signatures.
pub type HtmlResponse = Html<String>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_web_ui_service_creation() {
        let config = RiptideConfig::default();
        let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
        let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config.clone())));

        let web_ui = WebUIService::new(config, torrent_engine, streaming_service);

        assert!(web_ui.base_url().contains("http://"));
    }

    #[test]
    fn test_web_ui_error_display() {
        let error = WebUIError::TemplateError {
            reason: "Missing template".to_string(),
        };

        assert!(error.to_string().contains("Template rendering error"));
    }
}
