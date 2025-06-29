//! Library page template

/// Generates the library page content
pub fn library_content() -> String {
    include_str!("../../templates/library.html").to_string()
}
