//! Simple JSON API server for torrent management

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::get;
use riptide_core::config::RiptideConfig;
use riptide_core::torrent::TorrentEngine;
use riptide_search::MediaSearchService;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct AppState {
    torrent_engine: Arc<RwLock<TorrentEngine>>,
    search_service: MediaSearchService,
}

#[derive(Serialize)]
pub struct Stats {
    pub total_torrents: u32,
    pub active_downloads: u32,
    pub upload_speed: f64,
    pub download_speed: f64,
}

#[derive(Deserialize)]
pub struct AddTorrentQuery {
    magnet: String,
}

pub async fn run_server(config: RiptideConfig) -> Result<(), Box<dyn std::error::Error>> {
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config)));
    let search_service = MediaSearchService::new();

    let state = AppState {
        torrent_engine,
        search_service,
    };

    let app = Router::new()
        .route("/api/stats", get(api_stats))
        .route("/api/torrents", get(api_torrents))
        .route("/api/torrents/add", get(api_add_torrent))
        .route("/api/library", get(api_library))
        .route("/api/search", get(api_search))
        .route("/api/settings", get(api_settings))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("API server starting on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn api_stats(State(state): State<AppState>) -> Json<Stats> {
    let engine = state.torrent_engine.read().await;
    let stats = engine.get_download_stats().await;

    Json(Stats {
        total_torrents: stats.active_torrents as u32,
        active_downloads: stats.active_torrents as u32,
        upload_speed: (stats.bytes_uploaded as f64) / 1_048_576.0,
        download_speed: (stats.bytes_downloaded as f64) / 1_048_576.0,
    })
}

async fn api_torrents(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.torrent_engine.read().await;
    let stats = engine.get_download_stats().await;

    Json(json!({
        "torrents": [],
        "total": stats.active_torrents
    }))
}

async fn api_add_torrent(
    State(state): State<AppState>,
    Query(params): Query<AddTorrentQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if params.magnet.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut engine = state.torrent_engine.write().await;
    match engine.add_magnet(&params.magnet).await {
        Ok(info_hash) => Ok(Json(json!({
            "success": true,
            "message": "Torrent added",
            "info_hash": info_hash.to_string()
        }))),
        Err(e) => Ok(Json(json!({
            "success": false,
            "message": format!("Failed: {}", e)
        }))),
    }
}

async fn api_search(
    State(state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Json<serde_json::Value> {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");

    if query.is_empty() {
        return Json(json!([]));
    }

    match state.search_service.search_all(query).await {
        Ok(results) => Json(json!(results)),
        Err(_) => Json(json!([
            {"title": format!("Movie: {}", query), "type": "movie"},
            {"title": format!("Show: {}", query), "type": "tv"}
        ])),
    }
}

async fn api_library(State(state): State<AppState>) -> Json<serde_json::Value> {
    let engine = state.torrent_engine.read().await;
    let stats = engine.get_download_stats().await;

    Json(json!({
        "items": [],
        "total_size": stats.bytes_downloaded
    }))
}

async fn api_settings(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "download_dir": "./downloads",
        "max_connections": 50,
        "dht_enabled": true
    }))
}
