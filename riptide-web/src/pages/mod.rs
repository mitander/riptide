//! Full page handlers using component system
//!
//! Pages compose multiple components into complete HTML responses.
//! All pages use the same base layout with HTMX and Tailwind CSS.

pub mod dashboard;
pub mod library;
pub mod search;
pub mod torrents;

// Re-export page handlers
pub use dashboard::dashboard_page;
pub use library::library_page;
pub use search::search_page;
pub use torrents::torrents_page;
