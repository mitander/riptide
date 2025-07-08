//! Activity feed and notification components

/// Renders an activity feed with real-time updates
pub fn activity_feed(activities: &[ActivityItem]) -> String {
    let items_html: String = activities
        .iter()
        .map(|activity| {
            activity_item(
                &activity.icon,
                &activity.title,
                &activity.time,
                activity.description.as_deref(),
            )
        })
        .collect();

    if activities.is_empty() {
        r#"<div class="text-center py-8">
                <div class="text-4xl mb-2">•</div>
                <p class="text-gray-400">No recent activity</p>
                <p class="text-gray-500 text-sm mt-1">Activity will appear here as you use Riptide</p>
            </div>"#.to_string()
    } else {
        format!(
            r#"<div class="space-y-1">
                {items_html}
            </div>"#
        )
    }
}

/// Renders a single activity item
pub fn activity_item(icon: &str, title: &str, time: &str, description: Option<&str>) -> String {
    let desc_html = description
        .map(|d| format!(r#"<p class="text-gray-500 text-sm">{d}</p>"#))
        .unwrap_or_default();

    format!(
        r#"<div class="flex items-start space-x-3 p-3 hover:bg-gray-700 hover:bg-opacity-50 rounded-lg transition-colors">
            <div class="flex-shrink-0 w-8 h-8 bg-gray-700 rounded-full flex items-center justify-center text-sm">
                {icon}
            </div>
            <div class="flex-1 min-w-0">
                <p class="text-white text-sm font-medium">{title}</p>
                {desc_html}
                <p class="text-gray-400 text-xs mt-1">{time}</p>
            </div>
        </div>"#
    )
}

/// Renders a notification toast
pub fn notification_toast(message: &str, toast_type: &str, dismissible: bool) -> String {
    let (bg_class, border_class, icon) = match toast_type {
        "success" => ("bg-green-800", "border-green-600", "✓"),
        "error" => ("bg-red-800", "border-red-600", "✗"),
        "warning" => ("bg-yellow-800", "border-yellow-600", "!"),
        "info" => ("bg-blue-800", "border-blue-600", "i"),
        _ => ("bg-gray-800", "border-gray-600", "•"),
    };

    let dismiss_button = if dismissible {
        r#"<button class="ml-4 text-gray-400 hover:text-white" onclick="this.parentElement.remove()">
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
            </svg>
        </button>"#
    } else {
        ""
    };

    format!(
        r#"<div class="flex items-center p-4 border rounded-lg {bg_class} {border_class} fadeInDown">
            <span class="mr-3">{icon}</span>
            <span class="flex-1 text-sm text-white">{message}</span>
            {dismiss_button}
        </div>"#
    )
}

/// Renders a live stats ticker
pub fn live_ticker(stats: &[TickerStat]) -> String {
    let stats_html: String = stats
        .iter()
        .map(|stat| {
            format!(
                r#"<div class="flex items-center space-x-2 px-4 py-2 bg-gray-800 rounded-lg">
                    <span class="text-lg">{}</span>
                    <div>
                        <p class="text-white font-semibold text-sm">{}</p>
                        <p class="text-gray-400 text-xs">{}</p>
                    </div>
                </div>"#,
                stat.icon, stat.value, stat.label
            )
        })
        .collect();

    format!(
        r#"<div class="flex space-x-4 overflow-x-auto pb-2" hx-get="/api/ticker" hx-trigger="every 2s" hx-swap="innerHTML">
            {stats_html}
        </div>"#
    )
}

/// Renders a status banner
pub fn status_banner(message: &str, status: &str, dismissible: bool) -> String {
    let (bg_class, text_class) = match status {
        "success" => ("bg-green-600", "text-green-100"),
        "error" => ("bg-red-600", "text-red-100"),
        "warning" => ("bg-yellow-600", "text-yellow-100"),
        "info" => ("bg-blue-600", "text-blue-100"),
        _ => ("bg-gray-600", "text-gray-100"),
    };

    let dismiss_button = if dismissible {
        r#"<button class="ml-4 text-white hover:text-gray-200" onclick="this.parentElement.style.display='none'">
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
            </svg>
        </button>"#
    } else {
        ""
    };

    format!(
        r#"<div class="flex items-center justify-center p-3 {bg_class} {text_class}">
            <span class="text-sm font-medium">{message}</span>
            {dismiss_button}
        </div>"#
    )
}

// Helper structs for component data

/// Activity item displayed in the activity feed
#[derive(Debug)]
pub struct ActivityItem {
    pub icon: String,
    pub title: String,
    pub description: Option<String>,
    pub time: String,
}

/// Statistic displayed in the ticker component
#[derive(Debug)]
pub struct TickerStat {
    pub icon: String,
    pub value: String,
    pub label: String,
}
