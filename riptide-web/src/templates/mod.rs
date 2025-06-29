//! HTML templates and page generation
//!
//! Contains all HTML templates and JavaScript code extracted from the main server module
//! to improve maintainability and reduce file size.

pub mod base;
pub mod dashboard;
pub mod library;
pub mod search;
pub mod torrents;
pub mod video_player;

pub use base::base_template;
pub use dashboard::dashboard_content;
pub use library::library_content;
pub use search::search_content;
pub use torrents::torrents_content;
pub use video_player::video_player_content;
