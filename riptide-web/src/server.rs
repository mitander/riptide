//! Web server implementation for the Riptide UI

use std::net::SocketAddr;

use axum::Router;
use axum::response::IntoResponse;
use axum::routing::get;
use tower_http::cors::CorsLayer;

use super::static_files::StaticFileHandler;
use super::{TemplateEngine, WebHandlers, WebUIError};
use riptide_core::config::RiptideConfig;

/// Web server configuration for the UI service.
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    pub bind_address: SocketAddr,
    pub static_dir: String,
    pub enable_cors: bool,
    pub enable_dev_mode: bool,
}

impl WebServerConfig {
    /// Create web server config from Riptide configuration.
    pub fn from_riptide_config(_config: &RiptideConfig) -> Self {
        Self {
            bind_address: "127.0.0.1:3000".parse().unwrap(), // Default web UI port
            static_dir: "static".to_string(),
            enable_cors: true,
            enable_dev_mode: cfg!(debug_assertions),
        }
    }
}

/// Web server for the Riptide UI interface.
pub struct WebServer {
    config: WebServerConfig,
    handlers: WebHandlers,
    template_engine: TemplateEngine,
    static_handler: StaticFileHandler,
    shutdown_sender: Option<tokio::sync::oneshot::Sender<()>>,
}

impl WebServer {
    /// Creates new web server with configuration and handlers.
    pub fn new(
        config: WebServerConfig,
        handlers: WebHandlers,
        template_engine: TemplateEngine,
    ) -> Self {
        Self {
            config,
            handlers,
            template_engine,
            static_handler: StaticFileHandler::new(),
            shutdown_sender: None,
        }
    }

    /// Start the web server.
    ///
    /// # Errors
    /// - `WebUIError::ServerStartFailed` - Failed to bind or start server
    pub async fn start(&self) -> Result<(), WebUIError> {
        let app_state = AppState {
            handlers: self.handlers.clone(),
            template_engine: self.template_engine.clone(),
            static_handler: self.static_handler.clone(),
        };

        let app = Router::new()
            // Main pages
            .route("/", get(home_handler))
            .route("/library", get(library_handler))
            .route("/torrents", get(torrents_handler))
            .route("/add-torrent", get(add_torrent_page_handler))
            .route("/search", get(search_page_handler))
            .route("/settings", get(settings_handler))
            // API routes
            .route("/api/torrents", get(api_torrents_handler))
            .route("/api/torrents/add", get(api_add_torrent_handler))
            .route("/api/stats", get(api_stats_handler))
            .route("/api/library", get(api_library_handler))
            .route("/api/search", get(api_search_handler))
            .route("/api/search/movies", get(api_search_movies_handler))
            .route("/api/search/tv", get(api_search_tv_handler))
            // Built-in static files
            .route("/static/*path", get(static_file_handler))
            .with_state(app_state);

        let app = if self.config.enable_cors {
            app.layer(CorsLayer::permissive())
        } else {
            app
        };

        let listener = tokio::net::TcpListener::bind(self.config.bind_address)
            .await
            .map_err(|e| WebUIError::ServerStartFailed {
                address: self.config.bind_address,
                reason: e.to_string(),
            })?;

        tracing::info!("Web UI server starting on {}", self.config.bind_address);

        // Run the server directly (blocking)
        axum::serve(listener, app)
            .await
            .map_err(|e| WebUIError::ServerStartFailed {
                address: self.config.bind_address,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Stop the web server gracefully.
    pub async fn stop(&mut self) -> Result<(), WebUIError> {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        Ok(())
    }

    /// Get the base URL for the web interface.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.config.bind_address)
    }
}

/// Application state shared across handlers.
#[derive(Clone)]
struct AppState {
    handlers: WebHandlers,
    template_engine: TemplateEngine,
    static_handler: StaticFileHandler,
}

// Page handlers

async fn home_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let stats = state.handlers.get_server_stats().await?;
    let recent_activity = state.handlers.get_recent_activity().await?;

    let context = serde_json::json!({
        "title": "Riptide Media Server",
        "stats": stats,
        "recent_activity": recent_activity,
        "page": "home"
    });

    state.template_engine.render("home", &context)
}

async fn library_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let library_items = state.handlers.get_library_items().await?;

    let context = serde_json::json!({
        "title": "Media Library - Riptide",
        "library_items": library_items,
        "page": "library"
    });

    state.template_engine.render("library", &context)
}

async fn torrents_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let torrents = state.handlers.get_torrent_list().await?;

    let context = serde_json::json!({
        "title": "Torrents - Riptide",
        "torrents": torrents,
        "page": "torrents"
    });

    state.template_engine.render("torrents", &context)
}

async fn add_torrent_page_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let context = serde_json::json!({
        "title": "Add Torrent - Riptide",
        "page": "add-torrent"
    });

    state.template_engine.render("add_torrent", &context)
}

async fn settings_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let settings = state.handlers.get_server_settings().await?;

    let context = serde_json::json!({
        "title": "Settings - Riptide",
        "settings": settings,
        "page": "settings"
    });

    state.template_engine.render("settings", &context)
}

async fn search_page_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<super::HtmlResponse, WebUIError> {
    let context = serde_json::json!({
        "title": "Search Media - Riptide",
        "page": "search"
    });

    state.template_engine.render("search", &context)
}

// API handlers

async fn api_torrents_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let torrents = state.handlers.get_torrent_list().await?;
    Ok(axum::Json(serde_json::json!({ "torrents": torrents })))
}

async fn api_add_torrent_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let magnet_link = params
        .get("magnet")
        .ok_or_else(|| WebUIError::InternalError {
            reason: "Missing magnet parameter".to_string(),
        })?;

    let result = state.handlers.add_torrent(magnet_link).await?;
    Ok(axum::Json(serde_json::json!({ "result": result })))
}

async fn api_stats_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let stats = state.handlers.get_server_stats().await?;
    Ok(axum::Json(serde_json::to_value(stats).unwrap()))
}

async fn api_library_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let library = state.handlers.get_library_items().await?;
    Ok(axum::Json(serde_json::json!({ "library": library })))
}

async fn api_search_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let query = params.get("q").ok_or_else(|| WebUIError::InternalError {
        reason: "Missing query parameter 'q'".to_string(),
    })?;

    let results = state.handlers.search_media(query).await?;
    Ok(axum::Json(serde_json::json!({ "results": results })))
}

async fn api_search_movies_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let query = params.get("q").ok_or_else(|| WebUIError::InternalError {
        reason: "Missing query parameter 'q'".to_string(),
    })?;

    let results = state.handlers.search_movies(query).await?;
    Ok(axum::Json(serde_json::json!({ "results": results })))
}

async fn api_search_tv_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<axum::Json<serde_json::Value>, WebUIError> {
    let query = params.get("q").ok_or_else(|| WebUIError::InternalError {
        reason: "Missing query parameter 'q'".to_string(),
    })?;

    let results = state.handlers.search_tv_shows(query).await?;
    Ok(axum::Json(serde_json::json!({ "results": results })))
}

// Static file handler

async fn static_file_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> axum::response::Response {
    match state.static_handler.serve(&path) {
        Ok(response) => response,
        Err(status_code) => (status_code, "File not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::RwLock;

    use super::*;
    use crate::streaming::DirectStreamingService;
    use crate::torrent::TorrentEngine;

    #[test]
    fn test_web_server_config_from_riptide() {
        let config = RiptideConfig::default();
        let web_config = WebServerConfig::from_riptide_config(&config);

        assert_eq!(web_config.bind_address.port(), 3000);
        assert!(web_config.enable_cors);
        assert_eq!(web_config.static_dir, "static");
    }

    #[test]
    fn test_web_server_base_url() {
        let config = WebServerConfig::from_riptide_config(&RiptideConfig::default());
        let handlers = WebHandlers::new(
            Arc::new(RwLock::new(TorrentEngine::new(RiptideConfig::default()))),
            Arc::new(RwLock::new(DirectStreamingService::new(
                RiptideConfig::default(),
            ))),
        );
        let template_engine = TemplateEngine::new();

        let server = WebServer::new(config, handlers, template_engine);

        assert_eq!(server.base_url(), "http://127.0.0.1:3000");
    }
}
