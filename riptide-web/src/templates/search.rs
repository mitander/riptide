//! Search page template

/// Generates the search page content
pub fn search_content() -> String {
    include_str!("../../templates/search.html").to_string()
}
