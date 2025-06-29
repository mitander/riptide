//! Dashboard page template

use crate::server::DownloadStats;

/// Generates the dashboard page content
pub fn dashboard_content(stats: &DownloadStats) -> String {
    format!(
        r#"
        <div class="page-header">
            <h1>Dashboard</h1>
            <p>Monitor your downloads and server status</p>
        </div>
        
        <div class="stats-grid">
            <div class="stat-card">
                <span class="stat-value">{}</span>
                <div class="stat-label">Active Torrents</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1} MB/s</span>
                <div class="stat-label">Download Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1} MB/s</span>
                <div class="stat-label">Upload Speed</div>
            </div>
            <div class="stat-card">
                <span class="stat-value">{:.1} GB</span>
                <div class="stat-label">Downloaded</div>
            </div>
        </div>
        
        <div class="card">
            <h3>Quick Actions</h3>
            <div style="display: flex; gap: 15px; margin-top: 15px;">
                <a href="/search" class="btn">Search Content</a>
                <a href="/torrents" class="btn">View Downloads</a>
                <a href="/library" class="btn">Browse Library</a>
            </div>
        </div>
        
        <div class="card">
            <h3>Recent Activity</h3>
            <p style="color: #aaa;">Server running in demo mode with simulated data</p>
        </div>
        "#,
        stats.active_torrents,
        (stats.bytes_downloaded as f64) / 1_048_576.0,
        (stats.bytes_uploaded as f64) / 1_048_576.0,
        (stats.bytes_downloaded as f64) / 1_073_741_824.0
    )
}
