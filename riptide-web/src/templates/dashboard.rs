//! Dashboard page template

use crate::handlers::DownloadStats;

/// Generates the dashboard page content
pub fn dashboard_content(stats: &DownloadStats) -> String {
    const DASHBOARD_TEMPLATE: &str = include_str!("../../templates/dashboard.html");

    DASHBOARD_TEMPLATE
        .replace("{{ active_torrents }}", &stats.active_torrents.to_string())
        .replace(
            "{{ download_speed }}",
            &format!("{:.1}", stats.download_speed),
        )
        .replace("{{ upload_speed }}", &format!("{:.1}", stats.upload_speed))
        .replace(
            "{{ downloaded_gb }}",
            &format!("{:.1}", (stats.downloaded_size as f64) / 1_073_741_824.0),
        )
}
