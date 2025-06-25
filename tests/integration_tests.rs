//! Integration tests for Riptide web service and component interaction.

use std::sync::Arc;

use riptide::config::RiptideConfig;
use riptide::streaming::DirectStreamingService;
use riptide::torrent::TorrentEngine;
use riptide::web::handlers::WebHandlers;
use tokio::sync::RwLock;

/// Test that web handlers can be created and provide basic functionality.
#[tokio::test]
async fn test_web_service_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    // Test basic server stats
    let stats = handlers.get_server_stats().await.unwrap();
    assert_eq!(stats.total_torrents, 0);
    assert_eq!(stats.active_downloads, 0);

    // Test library items
    let library = handlers.get_library_items().await.unwrap();
    assert_eq!(library.len(), 2); // Mock data returns 2 items
    assert_eq!(library[0].title, "Big Buck Bunny");

    // Test torrent list
    let torrents = handlers.get_torrent_list().await.unwrap();
    assert_eq!(torrents.len(), 2); // Mock data returns 2 torrents

    // Test server settings
    let settings = handlers.get_server_settings().await.unwrap();
    assert_eq!(settings.streaming_port, 8080);
    assert_eq!(settings.web_ui_port, 3000);
}

/// Test media search functionality works end-to-end.
#[tokio::test]
async fn test_media_search_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    // Test general search
    let results = handlers.search_media("test").await.unwrap();
    assert!(!results.is_empty());

    // Test movie search
    let movie_results = handlers.search_movies("test movie").await.unwrap();
    assert!(!movie_results.is_empty());

    // Test TV search
    let tv_results = handlers.search_tv_shows("test series").await.unwrap();
    assert!(!tv_results.is_empty());
}

/// Test template engine integration with external files.
#[tokio::test]
async fn test_template_engine_integration() {
    use riptide::web::templates::TemplateEngine;
    use serde_json::json;

    let engine = TemplateEngine::new();

    let context = json!({
        "title": "Test Page",
        "page": "home",
        "stats": {
            "total_torrents": 5,
            "active_streams": 2
        }
    });

    // Test template rendering works
    let result = engine.render("home", &context);
    assert!(result.is_ok());

    let html = result.unwrap().0;
    assert!(html.contains("Test Page"));
    assert!(html.contains("Riptide"));
}

/// Test error handling across service boundaries.
#[tokio::test]
async fn test_error_handling_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    // Test invalid magnet link
    let result = handlers.add_torrent("invalid-magnet-link").await.unwrap();
    assert!(!result.success);
    assert!(result.message.contains("Failed to add torrent"));

    // Test empty search query
    let search_result = handlers.search_media("").await;
    assert!(search_result.is_err() || search_result.unwrap().is_empty());
}

/// Test concurrent access to shared resources.
#[tokio::test]
async fn test_concurrent_access_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    // Spawn multiple concurrent tasks
    let mut tasks = Vec::new();

    for i in 0..10 {
        let handlers_clone = handlers.clone();
        tasks.push(tokio::spawn(async move {
            let stats = handlers_clone.get_server_stats().await.unwrap();
            let library = handlers_clone.get_library_items().await.unwrap();
            let torrents = handlers_clone.get_torrent_list().await.unwrap();
            
            // Basic consistency checks
            assert_eq!(stats.total_torrents, 0);
            assert_eq!(library.len(), 2);
            assert_eq!(torrents.len(), 2);
            
            i // Return task number for verification
        }));
    }

    // Wait for all tasks to complete
    for (i, task) in tasks.into_iter().enumerate() {
        let result = task.await.unwrap();
        assert_eq!(result, i);
    }
}

/// Test the full streaming workflow integration.
#[tokio::test]
async fn test_streaming_workflow_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    // Test adding a magnet link
    let magnet = "magnet:?xt=urn:btih:1234567890abcdef1234567890abcdef12345678";
    let add_result = handlers.add_torrent(magnet).await.unwrap();
    
    // Should fail with mock implementation but error handling should work
    assert!(!add_result.success);
    assert!(!add_result.message.is_empty());
}

/// Test configuration and settings integration.
#[tokio::test]
async fn test_configuration_integration() {
    let config = RiptideConfig::default();
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config.clone())));
    let streaming_service = Arc::new(RwLock::new(DirectStreamingService::new(config)));

    let handlers = WebHandlers::new(torrent_engine, streaming_service);

    let settings = handlers.get_server_settings().await.unwrap();

    // Verify settings structure
    assert!(settings.streaming_port > 0);
    assert!(settings.web_ui_port > 0);
    assert!(settings.max_connections > 0);
    assert!(!settings.download_directory.is_empty());
    
    // Verify default configuration values
    assert_eq!(settings.streaming_port, 8080);
    assert_eq!(settings.web_ui_port, 3000);
    assert_eq!(settings.max_connections, 50);
}