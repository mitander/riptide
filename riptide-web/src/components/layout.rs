//! Layout components - headers, cards, containers, navigation

/// Renders a page header with title and optional subtitle.
///
/// Creates a responsive header section with title, optional subtitle, and action buttons.
/// Used at the top of main content areas to establish page context.
pub fn page_header(title: &str, subtitle: Option<&str>, actions: Option<&str>) -> String {
    let subtitle_html = subtitle
        .map(|s| format!(r#"<p class="text-gray-400 mt-2">{s}</p>"#))
        .unwrap_or_default();

    let actions_html = actions
        .map(|a| format!(r#"<div class="flex items-center space-x-4">{a}</div>"#))
        .unwrap_or_default();

    format!(
        r#"<div class="flex items-start justify-between mb-8">
            <div>
                <h1 class="text-3xl font-bold text-white">{title}</h1>
                {subtitle_html}
            </div>
            {actions_html}
        </div>"#
    )
}

/// Renders a card container with optional header and actions.
///
/// Creates a styled container with consistent padding, borders, and optional header section.
/// Perfect for grouping related content with optional title and action buttons.
pub fn card(title: Option<&str>, content: &str, actions: Option<&str>) -> String {
    let header_html = title
        .map(|t| {
            let actions_html = actions
                .map(|a| format!(r#"<div class="flex items-center space-x-2">{a}</div>"#))
                .unwrap_or_default();

            format!(
                r#"<div class="flex items-center justify-between mb-6">
                <h3 class="text-lg font-semibold text-white">{t}</h3>
                {actions_html}
            </div>"#
            )
        })
        .unwrap_or_default();

    format!(
        r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-6 mb-6">
            {header_html}
            {content}
        </div>"#
    )
}

/// Renders the main navigation bar.
///
/// Creates responsive navigation with brand, page links, and connection status.
/// Highlights the active page based on the provided page identifier.
pub fn nav_bar(active_page: &str) -> String {
    let nav_item = |href: &str, label: &str, page: &str| {
        let active_class = if page == active_page {
            "nav-active text-riptide-500 bg-riptide-500 bg-opacity-10"
        } else {
            "text-gray-300 hover:text-riptide-500 hover:bg-gray-700"
        };

        format!(
            r#"<a href="{href}" class="px-3 py-2 rounded-md text-sm font-medium transition-colors {active_class}">{label}</a>"#
        )
    };

    format!(
        r#"<nav class="bg-gray-800 border-b border-gray-700 sticky top-0 z-50">
            <div class="max-w-7xl mx-auto px-4">
                <div class="flex items-center justify-between h-16">
                    <div class="flex items-center space-x-8">
                        <div class="text-2xl font-bold text-riptide-500">Riptide</div>
                        <div class="hidden md:flex space-x-6">
                            {}
                            {}
                            {}
                            {}
                        </div>
                    </div>
                    
                    <!-- Connection status -->
                    <div class="flex items-center space-x-4">
                        <div class="flex items-center space-x-2 text-sm text-gray-400">
                            <div class="w-2 h-2 bg-green-400 rounded-full status-pulse"></div>
                            <span>Live</span>
                        </div>
                    </div>
                </div>
            </div>
        </nav>"#,
        nav_item("/", "Dashboard", "dashboard"),
        nav_item("/search", "Search", "search"),
        nav_item("/torrents", "Torrents", "torrents"),
        nav_item("/library", "Library", "library")
    )
}

/// Renders a grid container for responsive layouts.
///
/// Creates a CSS Grid container with specified column configuration and gap spacing.
/// Use Tailwind grid column classes like "grid-cols-3" or "grid-cols-1 lg:grid-cols-3".
pub fn grid(columns: &str, content: &str) -> String {
    format!(r#"<div class="grid {columns} gap-6">{content}</div>"#)
}

/// Renders a button with Tailwind styling.
///
/// Creates styled buttons with predefined variants (primary, secondary, danger, ghost).
/// Supports additional HTML attributes for custom behavior like onclick handlers.
pub fn button(text: &str, variant: &str, attributes: Option<&str>) -> String {
    let base_classes = "px-4 py-2 rounded-lg font-medium transition-colors focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-offset-gray-900";

    let variant_classes = match variant {
        "primary" => "bg-riptide-500 hover:bg-riptide-600 text-white focus:ring-riptide-500",
        "secondary" => "bg-gray-700 hover:bg-gray-600 text-white focus:ring-gray-500",
        "danger" => "bg-red-600 hover:bg-red-700 text-white focus:ring-red-500",
        "ghost" => "text-gray-300 hover:text-white hover:bg-gray-700 focus:ring-gray-500",
        _ => "bg-gray-600 hover:bg-gray-700 text-white focus:ring-gray-500",
    };

    let attrs = attributes.unwrap_or("");

    format!(r#"<button class="{base_classes} {variant_classes}" {attrs}>{text}</button>"#)
}

/// Renders an input field with Tailwind styling.
///
/// Creates form input elements with consistent styling and focus states.
/// Supports all HTML input types and additional attributes for validation or behavior.
pub fn input(name: &str, placeholder: &str, input_type: &str, attributes: Option<&str>) -> String {
    let attrs = attributes.unwrap_or("");

    format!(
        r#"<input type="{input_type}" name="{name}" placeholder="{placeholder}" 
                  class="w-full px-4 py-2 bg-gray-700 border border-gray-600 rounded-lg text-white placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-riptide-500 focus:border-transparent"
                  {attrs} />"#
    )
}
