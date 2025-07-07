//! Integration tests for development mode simulation loop.

use std::time::Duration;

use riptide_core::config::RiptideConfig;
use riptide_web::server::run_server;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[tokio::test]
async fn test_development_mode_full_simulation_loop() {
    // 1. Create a temporary movie file for the test
    let temp_dir = tempfile::tempdir().unwrap();
    let movies_dir = temp_dir.path().join("movies");
    fs::create_dir(&movies_dir).await.unwrap();
    let movie_path = movies_dir.join("test_movie.mp4");
    let mut file = fs::File::create(&movie_path).await.unwrap();
    file.write_all(&vec![0u8; 1024 * 1024]).await.unwrap(); // 1MB file
    drop(file); // Close the file

    // 2. Start the server in a separate thread to avoid Send requirement
    let movies_dir_clone = movies_dir.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let config = RiptideConfig::default();
            // Create development components for simulation
            let components =
                riptide_sim::create_development_components(config.clone(), Some(movies_dir_clone))
                    .await
                    .unwrap();
            // Create search service for dependency injection
            use riptide_core::RuntimeMode;
            use riptide_search::MediaSearchService;
            let search_service = MediaSearchService::from_runtime_mode(RuntimeMode::Development);

            // The server will run in the background. We don't need to await its completion
            let _ = run_server(config, components, search_service).await;
        });
    });

    // Give the server a moment to start and scan files
    tokio::time::sleep(Duration::from_secs(5)).await;

    // 3. Use an HTTP client to interact with the running server's API
    let client = reqwest::Client::new();

    // 4. Get the list of torrents (which should include our local movie)
    let response = client
        .get("http://127.0.0.1:3000/api/torrents")
        .send()
        .await
        .expect("Failed to get torrents");
    assert!(response.status().is_success());

    let body: serde_json::Value = response.json().await.expect("Failed to parse JSON");
    println!("Initial torrents response: {:?}", body);
    let torrents = body["torrents"].as_array().unwrap();
    assert_eq!(torrents.len(), 1, "Expected one torrent from local movie");
    let info_hash = torrents[0]["info_hash"].as_str().unwrap();
    println!("Found torrent with info_hash: {}", info_hash);
    println!("Initial torrent state: {:?}", torrents[0]);

    // 5. Poll for progress and verify it increases (download started automatically in development mode)
    let mut last_progress = 0.0;
    let mut progress_increased = false;
    for i in 0..20 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let response = client
            .get("http://127.0.0.1:3000/api/torrents")
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = response.json().await.unwrap();
        println!("Poll {}: Full torrent state: {:?}", i, body["torrents"][0]);
        let progress = body["torrents"][0]["progress"].as_f64().unwrap() as f32;

        if progress > last_progress {
            progress_increased = true;
            println!("Progress increased: {:.1}%", progress);
        }
        last_progress = progress;
        if progress >= 100.0 {
            break;
        }
    }

    // 6. Assert that progress was made, proving the simulation loop is active
    assert!(
        progress_increased,
        "Download progress did not increase, indicating the simulation loop is not running."
    );
}
