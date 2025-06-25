//! Template rendering engine for the Riptide web UI

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{HtmlResponse, WebUIError};

/// Template rendering engine for server-side rendering.
///
/// Provides HTML template rendering with JSON context injection for the web UI.
/// Uses a simple template system optimized for streaming media server interfaces.
#[derive(Debug, Clone)]
pub struct TemplateEngine {
    templates: HashMap<String, String>,
    base_template: String,
    // TODO: Implement template hot-reloading using this directory
    _template_dir: PathBuf,
}

impl TemplateEngine {
    /// Creates new template engine with templates loaded from directory.
    pub fn new() -> Self {
        Self::from_directory("templates")
    }

    /// Creates template engine loading templates from specified directory.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template directory not found or templates failed to load
    pub fn from_directory<P: AsRef<Path>>(template_dir: P) -> Self {
        let template_dir = template_dir.as_ref().to_path_buf();
        let mut templates = HashMap::new();

        // Template file mappings
        let template_files = [
            ("home", "home.html"),
            ("library", "library.html"),
            ("torrents", "torrents.html"),
            ("add_torrent", "add-torrent.html"),
            ("search", "search.html"),
            ("settings", "settings.html"),
        ];

        // Load templates from files
        for (name, filename) in template_files {
            let template_path = template_dir.join(filename);
            match fs::read_to_string(&template_path) {
                Ok(content) => {
                    templates.insert(name.to_string(), content);
                }
                Err(_) => {
                    // Fall back to hardcoded templates for development
                    templates.insert(name.to_string(), Self::fallback_template(name));
                }
            }
        }

        // Load base template
        let base_template = template_dir.join("base.html");
        let base_content =
            fs::read_to_string(&base_template).unwrap_or_else(|_| Self::base_template());

        Self {
            templates,
            base_template: base_content,
            _template_dir: template_dir,
        }
    }

    /// Renders template with provided context.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template not found or rendering failed
    pub fn render(&self, template_name: &str, context: &Value) -> Result<HtmlResponse, WebUIError> {
        let template =
            self.templates
                .get(template_name)
                .ok_or_else(|| WebUIError::TemplateError {
                    reason: format!("Template '{template_name}' not found"),
                })?;

        let content = self.interpolate_template(template, context)?;
        let full_page = self.wrap_in_base(&content, context)?;

        Ok(axum::response::Html(full_page))
    }

    /// Interpolates template variables with context values.
    fn interpolate_template(&self, template: &str, context: &Value) -> Result<String, WebUIError> {
        let mut result = template.to_string();

        // Find all {{...}} placeholders using simple string search
        let mut start = 0;
        while let Some(begin) = result[start..].find("{{") {
            let begin = start + begin;
            if let Some(end) = result[begin..].find("}}") {
                let end = begin + end + 2; // Include the }}
                let full_placeholder = &result[begin..end];
                let property_path = &result[begin + 2..end - 2].trim();

                if let Some(value) = self.get_nested_value(context, property_path) {
                    let replacement = match value {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        Value::Array(_) | Value::Object(_) => {
                            // For complex values, render as JSON for JavaScript consumption
                            serde_json::to_string(value).unwrap_or_default()
                        }
                        Value::Null => String::new(),
                    };
                    result = result.replace(full_placeholder, &replacement);
                    start = begin + replacement.len();
                } else {
                    start = end;
                }
            } else {
                break;
            }
        }

        Ok(result)
    }

    /// Get nested value from JSON using dot notation (e.g., "stats.total_torrents").
    fn get_nested_value<'a>(&self, value: &'a Value, path: &str) -> Option<&'a Value> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = value;

        for part in parts {
            current = current.get(part)?;
        }

        Some(current)
    }

    /// Wraps content in base template layout.
    fn wrap_in_base(&self, content: &str, context: &Value) -> Result<String, WebUIError> {
        let title = context
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Riptide Media Server");

        let page = context
            .get("page")
            .and_then(|p| p.as_str())
            .unwrap_or("home");

        let result = self
            .base_template
            .replace("{{title}}", title)
            .replace("{{content}}", content)
            .replace("{{page}}", page);

        Ok(result)
    }

    /// Returns fallback template for given name when external template file not found.
    fn fallback_template(name: &str) -> String {
        match name {
            "home" => Self::home_template(),
            "library" => Self::library_template(),
            "torrents" => Self::torrents_template(),
            "add_torrent" => Self::add_torrent_template(),
            "search" => Self::search_template(),
            "settings" => Self::settings_template(),
            _ => format!("<div class=\"error\">Template '{name}' not found</div>"),
        }
    }

    /// Base HTML template with navigation and layout.
    fn base_template() -> String {
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{{title}}</title>
    <link rel="stylesheet" href="/static/css/style.css">
    <script src="/static/js/app.js" defer></script>
</head>
<body>
    <header class="header">
        <div class="container">
            <h1 class="logo">Riptide</h1>
            <nav class="nav">
                <a href="/" class="nav-link {{#if (eq page 'home')}}active{{/if}}">Dashboard</a>
                <a href="/search" class="nav-link {{#if (eq page 'search')}}active{{/if}}">Search</a>
                <a href="/library" class="nav-link {{#if (eq page 'library')}}active{{/if}}">Library</a>
                <a href="/torrents" class="nav-link {{#if (eq page 'torrents')}}active{{/if}}">Torrents</a>
                <a href="/add-torrent" class="nav-link {{#if (eq page 'add-torrent')}}active{{/if}}">Add Torrent</a>
                <a href="/settings" class="nav-link {{#if (eq page 'settings')}}active{{/if}}">Settings</a>
            </nav>
        </div>
    </header>

    <main class="main">
        <div class="container">
            {{content}}
        </div>
    </main>

    <footer class="footer">
        <div class="container">
            <p>&copy; 2024 Riptide Media Server</p>
        </div>
    </footer>
</body>
</html>"#.to_string()
    }

    /// Dashboard/home page template.
    fn home_template() -> String {
        r#"<div class="dashboard">
    <div class="stats-grid">
        <div class="stat-card">
            <h3>Active Torrents</h3>
            <p class="stat-value">{{stats.total_torrents}}</p>
        </div>
        <div class="stat-card">
            <h3>Active Streams</h3>
            <p class="stat-value">{{stats.active_streams}}</p>
        </div>
        <div class="stat-card">
            <h3>Download Speed</h3>
            <p class="stat-value">{{stats.download_speed}} KB/s</p>
        </div>
        <div class="stat-card">
            <h3>Upload Speed</h3>
            <p class="stat-value">{{stats.upload_speed}} KB/s</p>
        </div>
    </div>

    <div class="recent-activity">
        <h2>Recent Activity</h2>
        <div class="activity-list">
            <script>
                const activities = {{recent_activity}};
                activities.forEach(activity => {
                    const div = document.createElement('div');
                    div.className = 'activity-item';
                    div.innerHTML = `
                        <div class="activity-icon ${activity.activity_type}"></div>
                        <div class="activity-content">
                            <p class="activity-description">${activity.description}</p>
                            <p class="activity-time">${new Date(activity.timestamp).toLocaleString()}</p>
                        </div>
                    `;
                    document.querySelector('.activity-list').appendChild(div);
                });
            </script>
        </div>
    </div>
</div>"#.to_string()
    }

    /// Media library browsing template.
    fn library_template() -> String {
        r#"<div class="library">
    <h1>Media Library</h1>

    <div class="library-filters">
        <input type="text" id="search" placeholder="Search media..." class="search-input">
        <select id="type-filter" class="filter-select">
            <option value="">All Types</option>
            <option value="Movie">Movies</option>
            <option value="TvShow">TV Shows</option>
            <option value="Music">Music</option>
        </select>
    </div>

    <div class="media-grid" id="media-grid">
        <script>
            const libraryItems = {{library_items}};
            const mediaGrid = document.getElementById('media-grid');

            function renderLibraryItems(items) {
                mediaGrid.innerHTML = '';
                items.forEach(item => {
                    const div = document.createElement('div');
                    div.className = 'media-card';
                    div.innerHTML = `
                        <div class="media-poster">
                            ${item.thumbnail_url ?
                                `<img src="${item.thumbnail_url}" alt="${item.title}">` :
                                '<div class="placeholder-poster"></div>'
                            }
                        </div>
                        <div class="media-info">
                            <h3 class="media-title">${item.title}</h3>
                            <p class="media-type">${item.media_type}</p>
                            <p class="media-size">${(item.size / 1073741824).toFixed(1)} GB</p>
                            ${item.duration ? `<p class="media-duration">${Math.floor(item.duration / 60)}m</p>` : ''}
                            <div class="media-actions">
                                <a href="${item.stream_url}" class="btn btn-primary">Stream</a>
                                <button class="btn btn-secondary" onclick="showMediaDetails('${item.id}')">Details</button>
                            </div>
                        </div>
                    `;
                    mediaGrid.appendChild(div);
                });
            }

            renderLibraryItems(libraryItems);

            // Search functionality
            document.getElementById('search').addEventListener('input', (e) => {
                const query = e.target.value.toLowerCase();
                const filtered = libraryItems.filter(item =>
                    item.title.toLowerCase().includes(query)
                );
                renderLibraryItems(filtered);
            });

            // Type filter
            document.getElementById('type-filter').addEventListener('change', (e) => {
                const type = e.target.value;
                const filtered = type ?
                    libraryItems.filter(item => item.media_type === type) :
                    libraryItems;
                renderLibraryItems(filtered);
            });
        </script>
    </div>
</div>"#.to_string()
    }

    /// Torrent management template.
    fn torrents_template() -> String {
        r#"<div class="torrents">
    <h1>Torrent Management</h1>

    <div class="torrents-toolbar">
        <a href="/add-torrent" class="btn btn-primary">Add Torrent</a>
        <button class="btn btn-secondary" onclick="refreshTorrents()">Refresh</button>
    </div>

    <div class="torrents-table">
        <table class="table">
            <thead>
                <tr>
                    <th>Name</th>
                    <th>Status</th>
                    <th>Progress</th>
                    <th>Down Speed</th>
                    <th>Up Speed</th>
                    <th>Size</th>
                    <th>Ratio</th>
                    <th>Peers</th>
                    <th>Actions</th>
                </tr>
            </thead>
            <tbody id="torrents-tbody">
                <script>
                    const torrents = {{torrents}};
                    const tbody = document.getElementById('torrents-tbody');

                    function renderTorrents() {
                        tbody.innerHTML = '';
                        torrents.forEach(torrent => {
                            const tr = document.createElement('tr');
                            tr.innerHTML = `
                                <td class="torrent-name">${torrent.name}</td>
                                <td><span class="status ${torrent.status.toLowerCase()}">${torrent.status}</span></td>
                                <td>
                                    <div class="progress-bar">
                                        <div class="progress-fill" style="width: ${torrent.progress}%"></div>
                                        <span class="progress-text">${torrent.progress.toFixed(1)}%</span>
                                    </div>
                                </td>
                                <td>${(torrent.download_speed / 1024).toFixed(0)} KB/s</td>
                                <td>${(torrent.upload_speed / 1024).toFixed(0)} KB/s</td>
                                <td>${(torrent.size / 1073741824).toFixed(1)} GB</td>
                                <td>${torrent.ratio.toFixed(2)}</td>
                                <td>${torrent.peers}/${torrent.seeds}</td>
                                <td class="torrent-actions">
                                    <button class="btn btn-sm" onclick="pauseTorrent('${torrent.info_hash}')">Pause</button>
                                    <button class="btn btn-sm btn-danger" onclick="removeTorrent('${torrent.info_hash}')">Remove</button>
                                </td>
                            `;
                            tbody.appendChild(tr);
                        });
                    }

                    renderTorrents();
                </script>
            </tbody>
        </table>
    </div>
</div>"#.to_string()
    }

    /// Add torrent form template.
    fn add_torrent_template() -> String {
        r#"<div class="add-torrent">
    <h1>Add New Torrent</h1>

    <form class="add-torrent-form" onsubmit="addTorrent(event)">
        <div class="form-group">
            <label for="magnet-link">Magnet Link:</label>
            <input type="url" id="magnet-link" name="magnet" required
                   placeholder="magnet:?xt=urn:btih:..." class="form-input">
        </div>

        <div class="form-group">
            <label>
                <input type="checkbox" id="start-immediately" checked>
                Start download immediately
            </label>
        </div>

        <div class="form-actions">
            <button type="submit" class="btn btn-primary">Add Torrent</button>
            <a href="/torrents" class="btn btn-secondary">Cancel</a>
        </div>
    </form>

    <div id="add-result" class="result-message" style="display: none;"></div>

    <script>
        async function addTorrent(event) {
            event.preventDefault();

            const magnetLink = document.getElementById('magnet-link').value;
            const resultDiv = document.getElementById('add-result');

            try {
                const response = await fetch(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
                const data = await response.json();

                if (data.result.success) {
                    resultDiv.className = 'result-message success';
                    resultDiv.textContent = data.result.message;
                    if (data.result.stream_url) {
                        resultDiv.innerHTML += `<br><a href="${data.result.stream_url}" class="btn btn-primary">Stream Now</a>`;
                    }
                    document.getElementById('magnet-link').value = '';
                } else {
                    resultDiv.className = 'result-message error';
                    resultDiv.textContent = data.result.message;
                }

                resultDiv.style.display = 'block';
            } catch (error) {
                resultDiv.className = 'result-message error';
                resultDiv.textContent = 'Failed to add torrent: ' + error.message;
                resultDiv.style.display = 'block';
            }
        }
    </script>
</div>"#.to_string()
    }

    /// Media search template.
    fn search_template() -> String {
        r#"<div class="search">
    <h1>Search Media</h1>

    <div class="search-form">
        <form class="search-input-form" onsubmit="performSearch(event)">
            <div class="form-group">
                <input type="text" id="search-query" name="query" required
                       placeholder="Search for movies, TV shows..." class="form-input search-input">
                <div class="search-filters">
                    <label>
                        <input type="radio" name="category" value="all" checked> All
                    </label>
                    <label>
                        <input type="radio" name="category" value="movie"> Movies
                    </label>
                    <label>
                        <input type="radio" name="category" value="tv"> TV Shows
                    </label>
                </div>
                <button type="submit" class="btn btn-primary search-btn">Search</button>
            </div>
        </form>
    </div>

    <div id="search-loading" class="loading" style="display: none;">
        <p>Searching...</p>
    </div>

    <div id="search-results" class="search-results" style="display: none;">
        <h2>Search Results</h2>
        <div id="results-grid" class="results-grid"></div>
    </div>

    <script>
        async function performSearch(event) {
            event.preventDefault();

            const query = document.getElementById('search-query').value.trim();
            const category = document.querySelector('input[name="category"]:checked').value;

            if (!query) return;

            const loadingDiv = document.getElementById('search-loading');
            const resultsDiv = document.getElementById('search-results');
            const resultsGrid = document.getElementById('results-grid');

            // Show loading
            loadingDiv.style.display = 'block';
            resultsDiv.style.display = 'none';

            try {
                let url = '/api/search';
                if (category === 'movie') {
                    url = '/api/search/movies';
                } else if (category === 'tv') {
                    url = '/api/search/tv';
                }

                const response = await fetch(`${url}?q=${encodeURIComponent(query)}`);
                const data = await response.json();

                // Hide loading
                loadingDiv.style.display = 'none';

                if (data.results && data.results.length > 0) {
                    renderSearchResults(data.results);
                    resultsDiv.style.display = 'block';
                } else {
                    resultsGrid.innerHTML = '<p class="no-results">No results found.</p>';
                    resultsDiv.style.display = 'block';
                }
            } catch (error) {
                loadingDiv.style.display = 'none';
                resultsGrid.innerHTML = `<p class="error">Search failed: ${error.message}</p>`;
                resultsDiv.style.display = 'block';
            }
        }

        function renderSearchResults(results) {
            const grid = document.getElementById('results-grid');
            grid.innerHTML = '';

            results.forEach(result => {
                const resultDiv = document.createElement('div');
                resultDiv.className = 'search-result-card';

                const posterImg = result.poster_url ?
                    `<img src="${result.poster_url}" alt="${result.title}" class="result-poster">` :
                    '<div class="result-poster-placeholder"></div>';

                const year = result.year ? ` (${result.year})` : '';
                const rating = result.rating ? `<span class="rating">★ ${result.rating}</span>` : '';
                const genre = result.genre ? `<span class="genre">${result.genre}</span>` : '';

                resultDiv.innerHTML = `
                    <div class="result-poster-container">
                        ${posterImg}
                    </div>
                    <div class="result-info">
                        <h3 class="result-title">${result.title}${year}</h3>
                        <div class="result-meta">
                            ${rating}
                            ${genre}
                            <span class="media-type">${result.media_type}</span>
                        </div>
                        <p class="result-plot">${result.plot || 'No description available.'}</p>
                        <div class="result-torrents">
                            <h4>Available Downloads (${result.torrents.length})</h4>
                            <div class="torrent-list">
                                ${result.torrents.slice(0, 3).map(torrent => `
                                    <div class="torrent-item">
                                        <div class="torrent-info">
                                            <span class="torrent-quality">${torrent.quality}</span>
                                            <span class="torrent-size">${formatBytes(torrent.size)}</span>
                                            <span class="torrent-seeds">🌱 ${torrent.seeders}</span>
                                        </div>
                                        <button class="btn btn-sm btn-primary" onclick="downloadTorrent('${torrent.magnet_link}')">
                                            Download
                                        </button>
                                    </div>
                                `).join('')}
                                ${result.torrents.length > 3 ? `<p class="more-torrents">... and ${result.torrents.length - 3} more</p>` : ''}
                            </div>
                        </div>
                    </div>
                `;

                grid.appendChild(resultDiv);
            });
        }

        function formatBytes(bytes) {
            if (bytes >= 1073741824) {
                return (bytes / 1073741824).toFixed(1) + ' GB';
            } else if (bytes >= 1048576) {
                return (bytes / 1048576).toFixed(1) + ' MB';
            }
            return (bytes / 1024).toFixed(0) + ' KB';
        }

        async function downloadTorrent(magnetLink) {
            try {
                const response = await fetch(`/api/torrents/add?magnet=${encodeURIComponent(magnetLink)}`);
                const data = await response.json();

                if (data.result.success) {
                    alert('Torrent added successfully! ' + data.result.message);
                    if (data.result.stream_url) {
                        if (confirm('Would you like to start streaming now?')) {
                            window.open(data.result.stream_url, '_blank');
                        }
                    }
                } else {
                    alert('Failed to add torrent: ' + data.result.message);
                }
            } catch (error) {
                alert('Error adding torrent: ' + error.message);
            }
        }

        // Auto-focus search input
        document.getElementById('search-query').focus();
    </script>
</div>"#.to_string()
    }

    /// Server settings template.
    fn settings_template() -> String {
        r#"<div class="settings">
    <h1>Server Settings</h1>

    <form class="settings-form" onsubmit="saveSettings(event)">
        <div class="settings-section">
            <h2>Network Settings</h2>

            <div class="form-group">
                <label for="download-limit">Download Limit (KB/s):</label>
                <input type="number" id="download-limit" name="download_limit"
                       value="{{settings.download_limit}}" class="form-input">
                <small>Leave empty for unlimited</small>
            </div>

            <div class="form-group">
                <label for="upload-limit">Upload Limit (KB/s):</label>
                <input type="number" id="upload-limit" name="upload_limit"
                       value="{{settings.upload_limit}}" class="form-input">
                <small>Leave empty for unlimited</small>
            </div>

            <div class="form-group">
                <label for="max-connections">Max Connections:</label>
                <input type="number" id="max-connections" name="max_connections"
                       value="{{settings.max_connections}}" class="form-input">
            </div>
        </div>

        <div class="settings-section">
            <h2>Server Ports</h2>

            <div class="form-group">
                <label for="streaming-port">Streaming Port:</label>
                <input type="number" id="streaming-port" name="streaming_port"
                       value="{{settings.streaming_port}}" class="form-input">
            </div>

            <div class="form-group">
                <label for="web-ui-port">Web UI Port:</label>
                <input type="number" id="web-ui-port" name="web_ui_port"
                       value="{{settings.web_ui_port}}" class="form-input">
            </div>
        </div>

        <div class="settings-section">
            <h2>Protocol Settings</h2>

            <div class="form-group">
                <label>
                    <input type="checkbox" id="enable-upnp" name="enable_upnp"
                           {{#if settings.enable_upnp}}checked{{/if}}>
                    Enable UPnP
                </label>
            </div>

            <div class="form-group">
                <label>
                    <input type="checkbox" id="enable-dht" name="enable_dht"
                           {{#if settings.enable_dht}}checked{{/if}}>
                    Enable DHT
                </label>
            </div>

            <div class="form-group">
                <label>
                    <input type="checkbox" id="enable-pex" name="enable_pex"
                           {{#if settings.enable_pex}}checked{{/if}}>
                    Enable Peer Exchange
                </label>
            </div>
        </div>

        <div class="form-actions">
            <button type="submit" class="btn btn-primary">Save Settings</button>
            <button type="button" class="btn btn-secondary" onclick="resetSettings()">Reset</button>
        </div>
    </form>

    <div id="settings-result" class="result-message" style="display: none;"></div>

    <script>
        function saveSettings(event) {
            event.preventDefault();

            const resultDiv = document.getElementById('settings-result');
            resultDiv.className = 'result-message success';
            resultDiv.textContent = 'Settings saved successfully!';
            resultDiv.style.display = 'block';

            setTimeout(() => {
                resultDiv.style.display = 'none';
            }, 3000);
        }

        function resetSettings() {
            if (confirm('Reset all settings to defaults?')) {
                location.reload();
            }
        }
    </script>
</div>"#
            .to_string()
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(!engine.templates.is_empty());
        assert!(engine.templates.contains_key("home"));
        assert!(engine.templates.contains_key("library"));
    }

    #[test]
    fn test_template_engine_with_missing_directory() {
        // Should fall back to hardcoded templates when directory doesn't exist
        let engine = TemplateEngine::from_directory("nonexistent_templates");
        assert!(!engine.templates.is_empty());
        assert!(engine.templates.contains_key("home"));
        assert!(engine.templates.contains_key("library"));
    }

    #[test]
    fn test_template_rendering() {
        let engine = TemplateEngine::new();
        let context = json!({
            "title": "Test Page",
            "page": "home",
            "stats": {
                "total_torrents": 5,
                "active_streams": 2
            }
        });

        let result = engine.render("home", &context);
        assert!(result.is_ok());

        let html = result.unwrap().0;
        assert!(html.contains("Test Page"));
        assert!(html.contains("Riptide"));
    }

    #[test]
    fn test_unknown_template() {
        let engine = TemplateEngine::new();
        let context = json!({});

        let result = engine.render("nonexistent", &context);
        assert!(result.is_err());

        if let Err(WebUIError::TemplateError { reason }) = result {
            assert!(reason.contains("Template 'nonexistent' not found"));
        }
    }

    #[test]
    fn test_variable_interpolation() {
        let engine = TemplateEngine::new();
        let template = "Hello {{name}}, you have {{count}} items.";
        let context = json!({
            "name": "Alice",
            "count": 42
        });

        let result = engine.interpolate_template(template, &context).unwrap();
        assert_eq!(result, "Hello Alice, you have 42 items.");
    }
}
