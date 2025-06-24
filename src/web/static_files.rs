//! Static file serving and CSS/JS generation for the Riptide web UI

use std::collections::HashMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

/// Static file handler for CSS, JavaScript, and images.
#[derive(Clone)]
pub struct StaticFileHandler {
    files: HashMap<String, StaticFile>,
}

/// Represents a static file with content and MIME type.
#[derive(Debug, Clone)]
pub struct StaticFile {
    pub content: &'static str,
    pub mime_type: &'static str,
}

impl StaticFileHandler {
    /// Creates new static file handler with built-in assets.
    pub fn new() -> Self {
        let mut files = HashMap::new();

        // CSS files
        files.insert(
            "css/style.css".to_string(),
            StaticFile {
                content: Self::main_css(),
                mime_type: "text/css",
            },
        );

        // JavaScript files
        files.insert(
            "js/app.js".to_string(),
            StaticFile {
                content: Self::main_js(),
                mime_type: "application/javascript",
            },
        );

        Self { files }
    }

    /// Serves static file by path.
    pub fn serve(&self, path: &str) -> Response {
        match self.files.get(path) {
            Some(file) => {
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", file.mime_type)
                    .header("cache-control", "public, max-age=86400") // 1 day cache
                    .body(file.content.into())
                    .unwrap()
            }
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body("File not found".into())
                .unwrap(),
        }
    }

    /// Main CSS stylesheet for the web UI.
    fn main_css() -> &'static str {
        r#"/* Riptide Media Server Web UI Styles */

:root {
    --primary-color: #2563eb;
    --primary-hover: #1d4ed8;
    --secondary-color: #64748b;
    --success-color: #059669;
    --danger-color: #dc2626;
    --warning-color: #d97706;
    --background: #f8fafc;
    --surface: #ffffff;
    --border: #e2e8f0;
    --text-primary: #1e293b;
    --text-secondary: #64748b;
    --text-muted: #94a3b8;
}

* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background-color: var(--background);
    color: var(--text-primary);
    line-height: 1.6;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
    padding: 0 1rem;
}

/* Header */
.header {
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    padding: 1rem 0;
}

.header .container {
    display: flex;
    justify-content: space-between;
    align-items: center;
}

.logo {
    font-size: 1.5rem;
    font-weight: bold;
    color: var(--primary-color);
}

.nav {
    display: flex;
    gap: 2rem;
}

.nav-link {
    text-decoration: none;
    color: var(--text-secondary);
    font-weight: 500;
    padding: 0.5rem 1rem;
    border-radius: 0.375rem;
    transition: all 0.2s;
}

.nav-link:hover,
.nav-link.active {
    color: var(--primary-color);
    background-color: var(--background);
}

/* Main content */
.main {
    padding: 2rem 0;
    min-height: calc(100vh - 140px);
}

/* Buttons */
.btn {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    border: none;
    border-radius: 0.375rem;
    font-weight: 500;
    text-decoration: none;
    cursor: pointer;
    transition: all 0.2s;
}

.btn-primary {
    background: var(--primary-color);
    color: white;
}

.btn-primary:hover {
    background: var(--primary-hover);
}

.btn-secondary {
    background: var(--secondary-color);
    color: white;
}

.btn-secondary:hover {
    background: #475569;
}

.btn-danger {
    background: var(--danger-color);
    color: white;
}

.btn-danger:hover {
    background: #b91c1c;
}

.btn-sm {
    padding: 0.25rem 0.5rem;
    font-size: 0.875rem;
}

/* Dashboard */
.stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
    gap: 1.5rem;
    margin-bottom: 2rem;
}

.stat-card {
    background: var(--surface);
    padding: 1.5rem;
    border-radius: 0.5rem;
    border: 1px solid var(--border);
}

.stat-card h3 {
    color: var(--text-secondary);
    font-size: 0.875rem;
    font-weight: 500;
    margin-bottom: 0.5rem;
}

.stat-value {
    font-size: 2rem;
    font-weight: bold;
    color: var(--primary-color);
}

.recent-activity {
    background: var(--surface);
    padding: 1.5rem;
    border-radius: 0.5rem;
    border: 1px solid var(--border);
}

.recent-activity h2 {
    margin-bottom: 1rem;
    color: var(--text-primary);
}

.activity-list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
}

.activity-item {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 1rem;
    background: var(--background);
    border-radius: 0.375rem;
}

.activity-icon {
    width: 2rem;
    height: 2rem;
    border-radius: 50%;
    background: var(--primary-color);
}

.activity-description {
    font-weight: 500;
}

.activity-time {
    color: var(--text-muted);
    font-size: 0.875rem;
}

/* Library */
.library-filters {
    display: flex;
    gap: 1rem;
    margin-bottom: 2rem;
}

.search-input,
.filter-select {
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface);
}

.search-input {
    flex: 1;
    max-width: 400px;
}

.media-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 1.5rem;
}

.media-card {
    background: var(--surface);
    border-radius: 0.5rem;
    border: 1px solid var(--border);
    overflow: hidden;
    transition: transform 0.2s;
}

.media-card:hover {
    transform: translateY(-2px);
}

.media-poster {
    aspect-ratio: 2/3;
    background: var(--background);
    display: flex;
    align-items: center;
    justify-content: center;
}

.media-poster img {
    width: 100%;
    height: 100%;
    object-fit: cover;
}

.placeholder-poster {
    width: 100%;
    height: 100%;
    background: linear-gradient(45deg, var(--border) 25%, transparent 25%),
                linear-gradient(-45deg, var(--border) 25%, transparent 25%),
                linear-gradient(45deg, transparent 75%, var(--border) 75%),
                linear-gradient(-45deg, transparent 75%, var(--border) 75%);
    background-size: 20px 20px;
    background-position: 0 0, 0 10px, 10px -10px, -10px 0px;
}

.media-info {
    padding: 1rem;
}

.media-title {
    font-size: 1rem;
    font-weight: 600;
    margin-bottom: 0.5rem;
    color: var(--text-primary);
}

.media-type,
.media-size,
.media-duration {
    font-size: 0.875rem;
    color: var(--text-secondary);
    margin-bottom: 0.25rem;
}

.media-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
}

/* Torrents table */
.torrents-toolbar {
    display: flex;
    gap: 1rem;
    margin-bottom: 2rem;
}

.table {
    width: 100%;
    background: var(--surface);
    border-radius: 0.5rem;
    border: 1px solid var(--border);
    overflow: hidden;
}

.table th,
.table td {
    padding: 1rem;
    text-align: left;
    border-bottom: 1px solid var(--border);
}

.table th {
    background: var(--background);
    font-weight: 600;
    color: var(--text-secondary);
}

.table tr:last-child td {
    border-bottom: none;
}

.torrent-name {
    font-weight: 500;
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.status {
    padding: 0.25rem 0.75rem;
    border-radius: 9999px;
    font-size: 0.75rem;
    font-weight: 500;
    text-transform: uppercase;
}

.status.downloading {
    background: #dbeafe;
    color: var(--primary-color);
}

.status.seeding {
    background: #dcfce7;
    color: var(--success-color);
}

.status.paused {
    background: #f3f4f6;
    color: var(--text-secondary);
}

.status.error {
    background: #fee2e2;
    color: var(--danger-color);
}

.progress-bar {
    width: 100px;
    height: 20px;
    background: var(--background);
    border-radius: 10px;
    overflow: hidden;
    position: relative;
}

.progress-fill {
    height: 100%;
    background: var(--primary-color);
    transition: width 0.3s;
}

.progress-text {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--text-primary);
}

.torrent-actions {
    display: flex;
    gap: 0.5rem;
}

/* Forms */
.form-group {
    margin-bottom: 1.5rem;
}

.form-group label {
    display: block;
    margin-bottom: 0.5rem;
    font-weight: 500;
    color: var(--text-primary);
}

.form-input {
    width: 100%;
    padding: 0.75rem;
    border: 1px solid var(--border);
    border-radius: 0.375rem;
    background: var(--surface);
    font-size: 1rem;
}

.form-input:focus {
    outline: none;
    border-color: var(--primary-color);
    box-shadow: 0 0 0 3px rgba(37, 99, 235, 0.1);
}

.form-actions {
    display: flex;
    gap: 1rem;
    margin-top: 2rem;
}

.add-torrent-form,
.settings-form {
    background: var(--surface);
    padding: 2rem;
    border-radius: 0.5rem;
    border: 1px solid var(--border);
    margin-bottom: 2rem;
}

.settings-section {
    margin-bottom: 2rem;
}

.settings-section h2 {
    margin-bottom: 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
}

/* Result messages */
.result-message {
    padding: 1rem;
    border-radius: 0.375rem;
    margin-top: 1rem;
}

.result-message.success {
    background: #dcfce7;
    color: var(--success-color);
    border: 1px solid #bbf7d0;
}

.result-message.error {
    background: #fee2e2;
    color: var(--danger-color);
    border: 1px solid #fecaca;
}

/* Footer */
.footer {
    background: var(--surface);
    border-top: 1px solid var(--border);
    padding: 1rem 0;
    text-align: center;
    color: var(--text-muted);
}

/* Responsive design */
@media (max-width: 768px) {
    .header .container {
        flex-direction: column;
        gap: 1rem;
    }
    
    .nav {
        justify-content: center;
        flex-wrap: wrap;
    }
    
    .stats-grid {
        grid-template-columns: 1fr;
    }
    
    .media-grid {
        grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    }
    
    .library-filters {
        flex-direction: column;
    }
    
    .table {
        font-size: 0.875rem;
    }
    
    .torrent-actions {
        flex-direction: column;
    }
}"#
    }

    /// Main JavaScript for interactive functionality.
    fn main_js() -> &'static str {
        r#"// Riptide Media Server Web UI JavaScript

document.addEventListener('DOMContentLoaded', function() {
    console.log('Riptide Web UI loaded');
    
    // Initialize components
    initializeNavigation();
    initializeAutoRefresh();
    initializeNotifications();
});

// Navigation handling
function initializeNavigation() {
    const currentPage = document.body.getAttribute('data-page') || 
                       window.location.pathname.split('/')[1] || 'home';
    
    const navLinks = document.querySelectorAll('.nav-link');
    navLinks.forEach(link => {
        const href = link.getAttribute('href');
        if ((href === '/' && currentPage === 'home') || 
            href === `/${currentPage}`) {
            link.classList.add('active');
        }
    });
}

// Auto-refresh for dynamic content
function initializeAutoRefresh() {
    if (window.location.pathname === '/torrents') {
        setInterval(refreshTorrents, 30000); // Refresh every 30 seconds
    }
    
    if (window.location.pathname === '/') {
        setInterval(refreshDashboard, 60000); // Refresh every minute
    }
}

// Refresh torrent list
async function refreshTorrents() {
    try {
        const response = await fetch('/api/torrents');
        const data = await response.json();
        
        if (data.torrents) {
            updateTorrentTable(data.torrents);
        }
    } catch (error) {
        console.error('Failed to refresh torrents:', error);
    }
}

function updateTorrentTable(torrents) {
    const tbody = document.getElementById('torrents-tbody');
    if (!tbody) return;
    
    tbody.innerHTML = '';
    torrents.forEach(torrent => {
        const tr = document.createElement('tr');
        tr.innerHTML = `
            <td class="torrent-name">${escapeHtml(torrent.name)}</td>
            <td><span class="status ${torrent.status.toLowerCase()}">${torrent.status}</span></td>
            <td>
                <div class="progress-bar">
                    <div class="progress-fill" style="width: ${torrent.progress}%"></div>
                    <span class="progress-text">${torrent.progress.toFixed(1)}%</span>
                </div>
            </td>
            <td>${formatSpeed(torrent.download_speed)}</td>
            <td>${formatSpeed(torrent.upload_speed)}</td>
            <td>${formatSize(torrent.size)}</td>
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

// Refresh dashboard
async function refreshDashboard() {
    try {
        const response = await fetch('/api/stats');
        const data = await response.json();
        
        updateDashboardStats(data);
    } catch (error) {
        console.error('Failed to refresh dashboard:', error);
    }
}

function updateDashboardStats(stats) {
    const statCards = document.querySelectorAll('.stat-card');
    if (statCards.length >= 4) {
        statCards[0].querySelector('.stat-value').textContent = stats.total_torrents;
        statCards[1].querySelector('.stat-value').textContent = stats.active_streams;
        statCards[2].querySelector('.stat-value').textContent = formatSpeed(stats.download_speed);
        statCards[3].querySelector('.stat-value').textContent = formatSpeed(stats.upload_speed);
    }
}

// Torrent actions
async function pauseTorrent(infoHash) {
    try {
        const response = await fetch(`/api/torrents/${infoHash}/pause`, { method: 'POST' });
        if (response.ok) {
            showNotification('Torrent paused', 'success');
            refreshTorrents();
        } else {
            showNotification('Failed to pause torrent', 'error');
        }
    } catch (error) {
        showNotification('Error: ' + error.message, 'error');
    }
}

async function removeTorrent(infoHash) {
    if (!confirm('Are you sure you want to remove this torrent?')) {
        return;
    }
    
    try {
        const response = await fetch(`/api/torrents/${infoHash}`, { method: 'DELETE' });
        if (response.ok) {
            showNotification('Torrent removed', 'success');
            refreshTorrents();
        } else {
            showNotification('Failed to remove torrent', 'error');
        }
    } catch (error) {
        showNotification('Error: ' + error.message, 'error');
    }
}

// Media details modal
function showMediaDetails(mediaId) {
    // Simple implementation - could be enhanced with modal dialogs
    console.log('Show details for media:', mediaId);
    // For now, just log - could implement modal or navigate to detail page
}

// Notification system
function initializeNotifications() {
    // Create notification container if it doesn't exist
    if (!document.getElementById('notifications')) {
        const container = document.createElement('div');
        container.id = 'notifications';
        container.style.cssText = `
            position: fixed;
            top: 20px;
            right: 20px;
            z-index: 1000;
            max-width: 400px;
        `;
        document.body.appendChild(container);
    }
}

function showNotification(message, type = 'info') {
    const container = document.getElementById('notifications');
    if (!container) return;
    
    const notification = document.createElement('div');
    notification.className = `notification notification-${type}`;
    notification.style.cssText = `
        background: ${type === 'success' ? '#dcfce7' : type === 'error' ? '#fee2e2' : '#dbeafe'};
        color: ${type === 'success' ? '#059669' : type === 'error' ? '#dc2626' : '#2563eb'};
        border: 1px solid ${type === 'success' ? '#bbf7d0' : type === 'error' ? '#fecaca' : '#93c5fd'};
        padding: 1rem;
        border-radius: 0.375rem;
        margin-bottom: 0.5rem;
        box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        transform: translateX(100%);
        transition: transform 0.3s ease-in-out;
    `;
    notification.textContent = message;
    
    container.appendChild(notification);
    
    // Animate in
    setTimeout(() => {
        notification.style.transform = 'translateX(0)';
    }, 10);
    
    // Auto-remove after 5 seconds
    setTimeout(() => {
        notification.style.transform = 'translateX(100%)';
        setTimeout(() => {
            if (notification.parentNode) {
                notification.parentNode.removeChild(notification);
            }
        }, 300);
    }, 5000);
}

// Utility functions
function formatSpeed(bytesPerSecond) {
    if (bytesPerSecond < 1024) {
        return bytesPerSecond + ' B/s';
    } else if (bytesPerSecond < 1024 * 1024) {
        return (bytesPerSecond / 1024).toFixed(1) + ' KB/s';
    } else {
        return (bytesPerSecond / (1024 * 1024)).toFixed(1) + ' MB/s';
    }
}

function formatSize(bytes) {
    if (bytes < 1024) {
        return bytes + ' B';
    } else if (bytes < 1024 * 1024) {
        return (bytes / 1024).toFixed(1) + ' KB';
    } else if (bytes < 1024 * 1024 * 1024) {
        return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
    } else {
        return (bytes / (1024 * 1024 * 1024)).toFixed(1) + ' GB';
    }
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Global functions for template usage
window.refreshTorrents = refreshTorrents;
window.pauseTorrent = pauseTorrent;
window.removeTorrent = removeTorrent;
window.showMediaDetails = showMediaDetails;
window.showNotification = showNotification;"#
    }
}

impl Default for StaticFileHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl IntoResponse for StaticFile {
    fn into_response(self) -> Response {
        Response::builder()
            .header("content-type", self.mime_type)
            .body(self.content.into())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_file_handler_creation() {
        let handler = StaticFileHandler::new();
        assert!(!handler.files.is_empty());
        assert!(handler.files.contains_key("css/style.css"));
        assert!(handler.files.contains_key("js/app.js"));
    }

    #[test]
    fn test_serve_existing_file() {
        let handler = StaticFileHandler::new();
        let response = handler.serve("css/style.css");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_serve_nonexistent_file() {
        let handler = StaticFileHandler::new();
        let response = handler.serve("nonexistent.txt");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_css_content() {
        let css = StaticFileHandler::main_css();
        assert!(css.contains("Riptide Media Server"));
        assert!(css.contains(".header"));
        assert!(css.contains(".media-grid"));
    }

    #[test]
    fn test_js_content() {
        let js = StaticFileHandler::main_js();
        assert!(js.contains("Riptide Web UI"));
        assert!(js.contains("refreshTorrents"));
        assert!(js.contains("showNotification"));
    }
}
