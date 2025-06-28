//! Simple JSON API server for torrent management

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, Json};
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

pub async fn run_server(
    config: RiptideConfig,
    mode: riptide_core::RuntimeMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let torrent_engine = Arc::new(RwLock::new(TorrentEngine::new(config)));
    let search_service = MediaSearchService::from_runtime_mode(mode);

    let state = AppState {
        torrent_engine,
        search_service,
    };

    let app = Router::new()
        .route("/", get(dashboard_page))
        .route("/torrents", get(torrents_page))
        .route("/library", get(library_page))
        .route("/search", get(search_page))
        .route("/api/stats", get(api_stats))
        .route("/api/torrents", get(api_torrents))
        .route("/api/torrents/add", get(api_add_torrent))
        .route("/api/library", get(api_library))
        .route("/api/search", get(api_search))
        .route("/api/settings", get(api_settings))
        .layer(CorsLayer::permissive())
        .with_state(state);

    println!("Riptide media server running on http://127.0.0.1:3000");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn base_template(title: &str, active_page: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>{title} - Riptide</title>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; 
               background: #0a0a0a; color: #ffffff; line-height: 1.6; }}
        
        /* Navigation */
        nav {{ background: #1a1a1a; border-bottom: 1px solid #333; padding: 0 20px; }}
        .nav-container {{ max-width: 1200px; margin: 0 auto; display: flex; align-items: center; height: 60px; }}
        .logo {{ font-size: 24px; font-weight: bold; color: #4a9eff; margin-right: 40px; }}
        .nav-links {{ display: flex; gap: 30px; flex: 1; }}
        .nav-links a {{ color: #ccc; text-decoration: none; padding: 8px 16px; border-radius: 6px; 
                        transition: all 0.2s; }}
        .nav-links a:hover {{ color: #4a9eff; background: #2a2a2a; }}
        .nav-links a.active {{ color: #4a9eff; background: #2a4a6a; }}
        
        /* Main Content */
        .container {{ max-width: 1200px; margin: 0 auto; padding: 30px 20px; }}
        .page-header {{ margin-bottom: 30px; }}
        .page-header h1 {{ font-size: 32px; margin-bottom: 10px; }}
        .page-header p {{ color: #aaa; font-size: 16px; }}
        
        /* Cards */
        .card {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 20px; margin-bottom: 20px; }}
        .card h3 {{ color: #4a9eff; margin-bottom: 15px; }}
        
        /* Stats Grid */
        .stats-grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 20px; margin-bottom: 30px; }}
        .stat-card {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 20px; text-align: center; }}
        .stat-value {{ font-size: 28px; font-weight: bold; color: #4a9eff; display: block; }}
        .stat-label {{ color: #aaa; margin-top: 5px; }}
        
        /* Forms */
        .search-form {{ margin-bottom: 30px; }}
        .search-form input {{ width: 100%; max-width: 400px; padding: 12px; border: 1px solid #333; 
                           background: #2a2a2a; color: #fff; border-radius: 6px; font-size: 16px; }}
        .search-form button {{ padding: 12px 24px; background: #4a9eff; color: #fff; border: none; 
                             border-radius: 6px; cursor: pointer; margin-left: 10px; font-size: 16px; }}
        .search-form button:hover {{ background: #3a8edf; }}
        
        /* Tables */
        .table {{ width: 100%; background: #1a1a1a; border: 1px solid #333; border-radius: 8px; overflow: hidden; }}
        .table th, .table td {{ padding: 12px; text-align: left; border-bottom: 1px solid #333; }}
        .table th {{ background: #2a2a2a; font-weight: 600; color: #4a9eff; }}
        .table tr:last-child td {{ border-bottom: none; }}
        .table tr:hover {{ background: #2a2a2a; }}
        
        /* Buttons */
        .btn {{ padding: 8px 16px; background: #4a9eff; color: #fff; border: none; border-radius: 4px; 
               cursor: pointer; text-decoration: none; display: inline-block; }}
        .btn:hover {{ background: #3a8edf; }}
        .btn-small {{ padding: 6px 12px; font-size: 14px; }}
        
        /* Grid Layouts */
        .grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 20px; }}
        .grid-item {{ background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 15px; }}
        .grid-item h4 {{ color: #4a9eff; margin-bottom: 10px; }}
        .grid-item p {{ color: #aaa; font-size: 14px; }}
        
        /* Loading States */
        .loading {{ text-align: center; padding: 40px; color: #aaa; }}
        .error {{ background: #4a2a2a; border: 1px solid #664; color: #ffaaaa; padding: 15px; border-radius: 6px; margin: 20px 0; }}
    </style>
</head>
<body>
    <nav>
        <div class="nav-container">
            <div class="logo">Riptide</div>
            <div class="nav-links">
                <a href="/" class="{dashboard_active}">Dashboard</a>
                <a href="/search" class="{search_active}">Search</a>
                <a href="/torrents" class="{torrents_active}">Torrents</a>
                <a href="/library" class="{library_active}">Library</a>
            </div>
        </div>
    </nav>
    
    <div class="container">
        {content}
    </div>
    
    <script>
        // Utility function for API calls
        async function apiCall(endpoint) {{
            try {{
                const response = await fetch(endpoint);
                if (!response.ok) throw new Error(`HTTP ${{response.status}}`);
                return await response.json();
            }} catch (error) {{
                console.error('API call failed:', error);
                throw error;
            }}
        }}
        
        // Auto-refresh stats every 30 seconds
        setInterval(() => {{
            if (window.updateStats) window.updateStats();
        }}, 30000);
    </script>
</body>
</html>"#,
        title = title,
        content = content,
        dashboard_active = if active_page == "dashboard" {
            "active"
        } else {
            ""
        },
        search_active = if active_page == "search" {
            "active"
        } else {
            ""
        },
        torrents_active = if active_page == "torrents" {
            "active"
        } else {
            ""
        },
        library_active = if active_page == "library" {
            "active"
        } else {
            ""
        }
    )
}

async fn dashboard_page(State(state): State<AppState>) -> Html<String> {
    let engine = state.torrent_engine.read().await;
    let stats = engine.get_download_stats().await;
    drop(engine);

    let content = format!(
        r#"
        <div class="page-header">
            <h1>Dashboard</h1>
            <p>Monitor your downloads and server status</p>
        </div>
        
        <div class="stats-grid" id="stats-grid">
            <div class="stat-card">
                <span class="stat-value" id="total-torrents">{}</span>
                <div class="stat-label">Active Torrents</div>
            </div>
            <div class="stat-card">
                <span class="stat-value" id="download-speed">{:.1} MB/s</span>
                <div class="stat-label">Download Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value" id="upload-speed">{:.1} MB/s</span>
                <div class="stat-label">Upload Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value" id="total-downloaded">{:.1} GB</span>
                <div class="stat-label">Total Downloaded</div>
            </div>
        </div>
        
        <div class="card">
            <h3>Quick Actions</h3>
            <p style="margin-bottom: 15px;">Get started with media discovery and torrents</p>
            <a href="/search" class="btn">Search Media</a>
            <a href="/torrents" class="btn" style="margin-left: 10px;">Manage Torrents</a>
            <a href="/library" class="btn" style="margin-left: 10px;">Browse Library</a>
        </div>
        
        <div class="card">
            <h3>Recent Activity</h3>
            <p id="recent-activity">No recent downloads</p>
        </div>
        
        <script>
            window.updateStats = async function() {{
                try {{
                    const stats = await apiCall('/api/stats');
                    document.getElementById('total-torrents').textContent = stats.total_torrents;
                    document.getElementById('download-speed').textContent = stats.download_speed.toFixed(1) + ' MB/s';
                    document.getElementById('upload-speed').textContent = stats.upload_speed.toFixed(1) + ' MB/s';
                    document.getElementById('total-downloaded').textContent = (stats.download_speed * 3600).toFixed(1) + ' GB';
                }} catch (error) {{
                    console.error('Failed to update stats:', error);
                }}
            }};
            
            // Load initial stats
            window.updateStats();
        </script>
    "#,
        stats.active_torrents,
        (stats.bytes_downloaded as f64) / 1_048_576.0,
        (stats.bytes_uploaded as f64) / 1_048_576.0,
        (stats.bytes_downloaded as f64) / 1_073_741_824.0
    );

    Html(base_template("Dashboard", "dashboard", &content))
}

async fn search_page(State(_state): State<AppState>) -> Html<String> {
    let content = r#"
        <div class="page-header">
            <h1>Search Media</h1>
            <p>Find movies and TV shows to download</p>
        </div>
        
        <div class="search-form">
            <input type="text" id="search-input" placeholder="Search for movies, TV shows..." onkeypress="handleSearchKeypress(event)">
            <button onclick="performSearch()">Search</button>
        </div>
        
        <div id="search-results"></div>
        
        <script>
            let searchTimeout;
            
            function handleSearchKeypress(event) {
                if (event.key === 'Enter') {
                    performSearch();
                }
            }
            
            async function performSearch() {
                const query = document.getElementById('search-input').value.trim();
                if (!query) return;
                
                const resultsDiv = document.getElementById('search-results');
                resultsDiv.innerHTML = '<div class="loading">Searching...</div>';
                
                try {
                    const results = await apiCall(`/api/search?q=${encodeURIComponent(query)}`);
                    displaySearchResults(results);
                } catch (error) {
                    resultsDiv.innerHTML = '<div class="loading">No results found</div>';
                }
            }
            
            function displaySearchResults(results) {
                const resultsDiv = document.getElementById('search-results');
                
                if (!results || results.length === 0) {
                    resultsDiv.innerHTML = '<div class="loading">No results found</div>';
                    return;
                }
                
                // Group torrents by media and show poster
                let mediaHtml = '';
                results.forEach(media => {
                    if (media.torrents && media.torrents.length > 0) {
                        const posterImg = media.poster_url ? 
                            `<img src="${media.poster_url}" alt="${media.title}" style="width: 200px; height: 300px; object-fit: cover; border-radius: 8px; flex-shrink: 0;">` :
                            `<div style="width: 200px; height: 300px; background: #333; border-radius: 8px; display: flex; align-items: center; justify-content: center; color: #666; flex-shrink: 0;">No Image</div>`;
                        
                        mediaHtml += `
                            <div class="media-result" style="display: flex; background: #1a1a1a; border: 1px solid #333; border-radius: 8px; padding: 20px; margin-bottom: 20px; gap: 20px;">
                                ${posterImg}
                                <div style="flex: 1; display: flex; flex-direction: column;">
                                    <h2 style="color: #4a9eff; font-size: 24px; font-weight: 600; margin-bottom: 12px;">${media.title} ${media.year ? '(' + media.year + ')' : ''}</h2>
                                    ${media.plot ? `<p style="color: #ccc; margin-bottom: 16px; line-height: 1.5;">${media.plot}</p>` : ''}
                                    <div style="color: #aaa; margin-bottom: 20px; font-size: 14px;">
                                        ${media.genre ? `<span style="margin-right: 20px;"><strong>Genre:</strong> ${media.genre}</span>` : ''}
                                        ${media.rating ? `<span style="margin-right: 20px;"><strong>IMDb:</strong> ${media.rating.toFixed(1)}/10</span>` : ''}
                                        <span><strong>Type:</strong> ${media.media_type || 'Unknown'}</span>
                                    </div>
                                    <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 12px;">
                                        ${media.torrents.map(torrent => `
                                            <div style="background: #2a2a2a; border: 1px solid #404040; border-radius: 6px; padding: 16px;">
                                                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
                                                    <span style="color: #4a9eff; font-weight: 600; font-size: 16px;">${getQualityDisplay(torrent.quality)}</span>
                                                    <button class="btn btn-small" onclick="addTorrent('${torrent.magnet_link}')" style="background: #4a9eff; color: white; border: none; padding: 6px 12px; border-radius: 4px; cursor: pointer; font-size: 13px;">Download</button>
                                                </div>
                                                <div style="color: #aaa; font-size: 13px;">
                                                    ${formatSize(torrent.size)} • ${torrent.seeders || 0} seeds • ${torrent.leechers || 0} peers
                                                </div>
                                            </div>
                                        `).join('')}
                                    </div>
                                </div>
                            </div>
                        `;
                    }
                });
                
                if (!mediaHtml) {
                    resultsDiv.innerHTML = '<div class="loading">No torrents found</div>';
                } else {
                    resultsDiv.innerHTML = mediaHtml;
                }
            }
            
            function formatSize(bytes) {
                if (!bytes) return 'Unknown';
                const gb = 1024 * 1024 * 1024;
                const mb = 1024 * 1024;
                
                if (bytes >= gb) {
                    return (bytes / gb).toFixed(1) + ' GB';
                } else if (bytes >= mb) {
                    return (bytes / mb).toFixed(1) + ' MB';
                } else {
                    return (bytes / 1024).toFixed(1) + ' KB';
                }
            }
            
            function getQualityDisplay(quality) {
                if (!quality) return 'Unknown Quality';
                
                // Handle Rust enum serialization
                if (typeof quality === 'object' && quality !== null) {
                    // Handle variants like {"BluRay1080p": null} or just "BluRay1080p"
                    const keys = Object.keys(quality);
                    if (keys.length > 0) {
                        quality = keys[0];
                    }
                }
                
                // Convert enum names to display names
                switch (quality) {
                    case 'BluRay1080p': return '1080p BluRay';
                    case 'BluRay720p': return '720p BluRay';
                    case 'BluRay4K': return '4K BluRay';
                    case 'WebDl1080p': return '1080p WEB-DL';
                    case 'WebDl720p': return '720p WEB-DL';
                    case 'Hdtv1080p': return '1080p HDTV';
                    case 'Hdtv720p': return '720p HDTV';
                    default: return quality.toString();
                }
            }
            
            async function addTorrent(magnetLink) {
                try {
                    const result = await apiCall(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
                    if (result.success) {
                        alert('Torrent added successfully!');
                    } else {
                        alert('Failed to add torrent: ' + result.message);
                    }
                } catch (error) {
                    alert('Failed to add torrent');
                }
            }
        </script>
    "#;

    Html(base_template("Search", "search", content))
}

async fn torrents_page(State(_state): State<AppState>) -> Html<String> {
    let content = r#"
        <div class="page-header">
            <h1>Torrent Management</h1>
            <p>View and manage your active downloads</p>
        </div>
        
        <div class="card">
            <h3>Add New Torrent</h3>
            <div class="search-form">
                <input type="text" id="magnet-input" placeholder="Paste magnet link here...">
                <button onclick="addMagnetLink()">Add Torrent</button>
            </div>
        </div>
        
        <div class="card">
            <h3>Active Torrents</h3>
            <div id="torrents-list">
                <div class="loading">Loading torrents...</div>
            </div>
        </div>
        
        <script>
            async function loadTorrents() {
                try {
                    const data = await apiCall('/api/torrents');
                    displayTorrents(data.torrents || []);
                } catch (error) {
                    document.getElementById('torrents-list').innerHTML = 
                        '<p>No active torrents</p>';
                }
            }
            
            function displayTorrents(torrents) {
                const listDiv = document.getElementById('torrents-list');
                
                if (torrents.length === 0) {
                    listDiv.innerHTML = '<p>No active torrents</p>';
                    return;
                }
                
                const html = `
                    <table class="table">
                        <thead>
                            <tr>
                                <th>Name</th>
                                <th>Progress</th>
                                <th>Speed</th>
                                <th>Size</th>
                                <th>Actions</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${torrents.map(torrent => `
                                <tr>
                                    <td>${torrent.name || 'Unknown'}</td>
                                    <td>${torrent.progress || '0'}%</td>
                                    <td>${torrent.speed || '0'} KB/s</td>
                                    <td>${torrent.size || 'Unknown'}</td>
                                    <td>
                                        <button class="btn btn-small">Pause</button>
                                        <button class="btn btn-small">Remove</button>
                                    </td>
                                </tr>
                            `).join('')}
                        </tbody>
                    </table>
                `;
                
                listDiv.innerHTML = html;
            }
            
            async function addMagnetLink() {
                const magnetInput = document.getElementById('magnet-input');
                const magnetLink = magnetInput.value.trim();
                
                if (!magnetLink) {
                    alert('Please enter a magnet link');
                    return;
                }
                
                try {
                    const result = await apiCall(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
                    if (result.success) {
                        alert('Torrent added successfully!');
                        magnetInput.value = '';
                        loadTorrents(); // Refresh the list
                    } else {
                        alert('Failed to add torrent: ' + result.message);
                    }
                } catch (error) {
                    alert('Failed to add torrent');
                }
            }
            
            // Load torrents on page load
            loadTorrents();
            
            // Auto-refresh every 10 seconds
            setInterval(loadTorrents, 10000);
        </script>
    "#;

    Html(base_template("Torrents", "torrents", content))
}

async fn library_page(State(_state): State<AppState>) -> Html<String> {
    let content = r#"
        <div class="page-header">
            <h1>Media Library</h1>
            <p>Browse your downloaded movies and TV shows</p>
        </div>
        
        <div id="library-content">
            <div class="loading">Loading library...</div>
        </div>
        
        <script>
            async function loadLibrary() {
                try {
                    const data = await apiCall('/api/library');
                    displayLibrary(data.items || []);
                } catch (error) {
                    document.getElementById('library-content').innerHTML = 
                        '<div class="card"><h3>Library Empty</h3><p>No media found. Start by <a href="/search">searching for content</a> to download.</p></div>';
                }
            }
            
            function displayLibrary(items) {
                const contentDiv = document.getElementById('library-content');
                
                if (items.length === 0) {
                    contentDiv.innerHTML = `
                        <div class="card">
                            <h3>No Media Found</h3>
                            <p>Your library is empty. Start by <a href="/search">searching for media</a> to download.</p>
                        </div>
                    `;
                    return;
                }
                
                const html = `
                    <div class="grid">
                        ${items.map(item => `
                            <div class="grid-item">
                                <h4>${item.title || 'Unknown Title'}</h4>
                                <p>Type: ${item.type || 'Unknown'}</p>
                                <p>Size: ${item.size || 'Unknown'}</p>
                                <p>Added: ${item.added_date || 'Unknown'}</p>
                                <button class="btn btn-small" onclick="streamMedia('${item.id || ''}')">Stream</button>
                            </div>
                        `).join('')}
                    </div>
                `;
                
                contentDiv.innerHTML = html;
            }
            
            function streamMedia(itemId) {
                if (!itemId) {
                    alert('Cannot stream this item');
                    return;
                }
                
                // Future: Open streaming interface
                alert('Streaming feature coming soon!');
            }
            
            // Load library on page load
            loadLibrary();
        </script>
    "#;

    Html(base_template("Library", "library", content))
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

    // For now, show demo data if no real torrents exist
    let demo_torrents = if stats.active_torrents == 0 {
        vec![
            json!({
                "name": "Demo.Movie.2024.1080p.BluRay.x264",
                "progress": 45,
                "speed": 2500,
                "size": "1.5 GB",
                "status": "downloading"
            }),
            json!({
                "name": "Demo.Series.S01E01.720p.WEB-DL.x264",
                "progress": 78,
                "speed": 1800,
                "size": "850 MB",
                "status": "downloading"
            }),
        ]
    } else {
        vec![] // TODO: Return real torrent data from engine
    };

    Json(json!({
        "torrents": demo_torrents,
        "total": if demo_torrents.is_empty() { stats.active_torrents } else { demo_torrents.len() }
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

    match state.search_service.search_with_metadata(query).await {
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

    // Demo library items to show the UI working
    let demo_items = vec![
        json!({
            "id": "movie_1",
            "title": "The Matrix",
            "type": "Movie",
            "year": 1999,
            "size": "1.4 GB",
            "added_date": "2025-06-25",
            "poster_url": null,
            "rating": 8.7
        }),
        json!({
            "id": "series_1",
            "title": "Breaking Bad S01E01",
            "type": "TV Show",
            "year": 2008,
            "size": "720 MB",
            "added_date": "2025-06-26",
            "poster_url": null,
            "rating": 9.5
        }),
        json!({
            "id": "movie_2",
            "title": "Inception",
            "type": "Movie",
            "year": 2010,
            "size": "2.1 GB",
            "added_date": "2025-06-27",
            "poster_url": null,
            "rating": 8.8
        }),
    ];

    Json(json!({
        "items": demo_items,
        "total_size": stats.bytes_downloaded + 4_300_000_000_u64 // Add demo size
    }))
}

async fn api_settings(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "download_dir": "./downloads",
        "max_connections": 50,
        "dht_enabled": true
    }))
}
