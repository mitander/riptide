//! Dashboard page template

use crate::handlers::DownloadStats;

/// Generates the dashboard page content
pub fn dashboard_content(stats: &DownloadStats) -> String {
    const DASHBOARD_TEMPLATE: &str = include_str!("../../templates/dashboard.html");

    DASHBOARD_TEMPLATE
        .replace("{{ active_torrents }}", &stats.active_torrents.to_string())
        .replace(
            "{{ download_speed }}",
            &format!("{:.1}", (stats.bytes_downloaded as f64) / 1_048_576.0),
        )
        .replace(
            "{{ upload_speed }}",
            &format!("{:.1}", (stats.bytes_uploaded as f64) / 1_048_576.0),
        )
        .replace(
            "{{ downloaded_gb }}",
            &format!("{:.1}", (stats.bytes_downloaded as f64) / 1_073_741_824.0),
        )
}
