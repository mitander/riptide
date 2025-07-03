//! Search page - find and add new content

use axum::extract::State;
use axum::response::Html;

use crate::components::layout;
use crate::pages::dashboard::render_page;
use crate::server::AppState;

/// Renders the search page
pub async fn search_page(State(_state): State<AppState>) -> Html<String> {
    let search_form = format!(
        r#"<div class="max-w-2xl mx-auto">
            <form class="space-y-6">
                <div class="flex space-x-4">
                    {}
                    {}
                </div>
                <div class="text-center">
                    <p class="text-gray-400 text-sm">Search for movies, TV shows, and other content</p>
                </div>
            </form>
        </div>"#,
        layout::input(
            "query",
            "Search for movies, TV shows...",
            "text",
            Some("class=\"flex-1\"")
        ),
        layout::button("Search", "primary", Some("type=\"submit\""))
    );

    let content = format!(
        r#"{}
        
        {}
        
        <div class="text-center py-12">
            <div class="text-6xl mb-4">🔍</div>
            <h2 class="text-2xl font-semibold text-white mb-4">Search Coming Soon</h2>
            <p class="text-gray-400 mb-8">Search functionality will be integrated with torrent indexers</p>
            {}
        </div>"#,
        layout::page_header(
            "Search Content",
            Some("Find movies, TV shows, and other media"),
            None
        ),
        layout::card(Some("Search"), &search_form, None),
        layout::button(
            "View Torrents Instead",
            "secondary",
            Some(r#"onclick="window.location.href='/torrents'""#)
        )
    );

    render_page("Search", "search", &content)
}
