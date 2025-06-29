//! Torrents page template

/// Generates the torrents page content
pub fn torrents_content() -> String {
    include_str!("../../templates/torrents.html").to_string()
}
