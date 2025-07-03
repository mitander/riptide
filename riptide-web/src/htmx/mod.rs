//! HTMX partial update handlers
//!
//! Provides server-rendered HTML fragments for real-time updates.
//! All responses are optimized for HTMX swapping and minimal bandwidth.

pub mod live_stats;
pub mod notifications;
pub mod torrent_actions;

// Re-export main HTMX handlers
pub use live_stats::{dashboard_activity, dashboard_downloads, dashboard_stats};
pub use notifications::{system_status, toast_notification};
pub use torrent_actions::{add_torrent, torrent_details, torrent_list};
