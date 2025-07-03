//! Library page - browse downloaded content

use axum::extract::State;
use axum::response::Html;

use crate::components::layout;
use crate::pages::dashboard::render_page;
use crate::server::AppState;

/// Renders the library page
pub async fn library_page(State(_state): State<AppState>) -> Html<String> {
    let content = format!(
        r#"{}
        
        <div class="text-center py-12">
            <div class="text-6xl mb-4">📚</div>
            <h2 class="text-2xl font-semibold text-white mb-4">Library Coming Soon</h2>
            <p class="text-gray-400 mb-8">Browse and stream your downloaded content</p>
            {}
        </div>"#,
        layout::page_header(
            "Media Library",
            Some("Browse and stream your downloaded content"),
            None
        ),
        layout::button(
            "Browse Torrents",
            "primary",
            Some(r#"onclick="window.location.href='/torrents'""#)
        )
    );

    render_page("Library", "library", &content)
}
