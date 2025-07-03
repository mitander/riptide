//! Reusable HTML components for HTMX + Tailwind UI
//!
//! Components are server-rendered HTML fragments that can be used
//! in full pages or as HTMX partial updates. All styling uses Tailwind CSS.

pub mod activity;
pub mod layout;
pub mod stats;
pub mod torrent;

// Re-export main component functions
pub use activity::{activity_feed, activity_item, notification_toast};
pub use layout::{card, nav_bar, page_header};
pub use stats::{progress_bar, speed_indicator, stat_card};
pub use torrent::{
    TorrentDetailsParams, TorrentListItemParams, torrent_form, torrent_list_item,
    torrent_status_badge,
};
