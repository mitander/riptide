//! Statistics and metrics components

/// Renders a statistics card with value, label, and optional trend.
///
/// Creates a styled card displaying a metric with optional unit, trend indicator,
/// and custom color. Perfect for dashboard statistics and key performance indicators.
pub fn stat_card(
    value: &str,
    label: &str,
    unit: Option<&str>,
    trend: Option<&str>,
    color: Option<&str>,
) -> String {
    let unit_html = unit
        .map(|u| format!(r#"<span class="text-sm text-gray-400 ml-1">{u}</span>"#))
        .unwrap_or_default();

    let trend_html = trend
        .map(|t| format!(r#"<div class="text-xs text-gray-500 mt-1">{t}</div>"#))
        .unwrap_or_default();

    let value_color = color.unwrap_or("text-riptide-500");

    format!(
        r#"<div class="bg-gray-800 border border-gray-700 rounded-lg p-6 text-center">
            <div class="text-2xl font-bold {value_color} mb-1">
                {value}{unit_html}
            </div>
            <div class="text-gray-400 text-sm">{label}</div>
            {trend_html}
        </div>"#
    )
}

/// Renders a progress bar with percentage and optional label.
///
/// Creates animated progress bars with customizable colors and labels.
/// Shows percentage completion with smooth transitions and optional progress text.
pub fn progress_bar(percentage: u32, label: Option<&str>, color: Option<&str>) -> String {
    let progress_color = color.unwrap_or("bg-riptide-500");
    let label_html = label
        .map(|l| {
            format!(
                r#"<div class="flex justify-between text-sm text-gray-400 mb-2">
            <span>{l}</span>
            <span>{percentage}%</span>
        </div>"#
            )
        })
        .unwrap_or_default();

    format!(
        r#"<div>
            {label_html}
            <div class="w-full bg-gray-700 rounded-full h-2 overflow-hidden">
                <div class="{progress_color} h-full rounded-full transition-all duration-300 ease-out" 
                     style="width: {percentage}%"></div>
            </div>
        </div>"#
    )
}

/// Renders a speed/rate indicator with animated bars.
///
/// Creates vertical bar chart style indicators for visualizing rates like download speeds.
/// Bars light up based on current value relative to maximum, with smooth color transitions.
pub fn speed_indicator(value: f64, max_value: f64, label: &str) -> String {
    let percentage = ((value / max_value) * 100.0).min(100.0) as u32;
    let bars = (0..5).map(|i| {
        let bar_active = percentage > (i * 20);
        let bar_class = if bar_active {
            "bg-riptide-500"
        } else {
            "bg-gray-700"
        };

        format!(r#"<div class="w-1 {} rounded-full transition-colors duration-300" style="height: {}px"></div>"#, 
                bar_class, 8 + (i * 3))
    }).collect::<Vec<_>>().join("");

    format!(
        r#"<div class="flex items-center space-x-3">
            <div class="flex items-end space-x-1 h-6">
                {bars}
            </div>
            <div class="text-sm text-gray-400">{label}</div>
        </div>"#
    )
}

/// Renders a stats grid container.
///
/// Creates responsive grid layout for statistics cards with automatic column sizing
/// based on the number of stats. Adapts from 1-2 columns on mobile to 6 on desktop.
pub fn stats_grid(stats: &[String]) -> String {
    let grid_cols = match stats.len() {
        1..=2 => "grid-cols-1 md:grid-cols-2",
        3..=4 => "grid-cols-2 md:grid-cols-4",
        5..=6 => "grid-cols-2 md:grid-cols-3 lg:grid-cols-6",
        _ => "grid-cols-2 md:grid-cols-4 lg:grid-cols-6",
    };

    format!(
        r#"<div class="grid {} gap-6 mb-8">
            {}
        </div>"#,
        grid_cols,
        stats.join("")
    )
}

/// Renders a metric with icon and description.
///
/// Creates metric display cards with icons, values, labels, and optional descriptions.
/// Perfect for system monitoring and detailed statistics with visual context.
pub fn metric_item(icon: &str, value: &str, label: &str, description: Option<&str>) -> String {
    let desc_html = description
        .map(|d| format!(r#"<p class="text-xs text-gray-500 mt-1">{d}</p>"#))
        .unwrap_or_default();

    format!(
        r#"<div class="flex items-center space-x-3 p-4 bg-gray-800 rounded-lg border border-gray-700">
            <div class="text-2xl">{icon}</div>
            <div class="flex-1">
                <div class="text-lg font-semibold text-white">{value}</div>
                <div class="text-sm text-gray-400">{label}</div>
                {desc_html}
            </div>
        </div>"#
    )
}

/// Renders a status indicator dot with label.
///
/// Creates colored status indicators with optional pulsing animation for active states.
/// Supports status types: online/active (green), warning/slow (yellow), error/failed (red).
pub fn status_indicator(status: &str, label: &str) -> String {
    let (color_class, pulse_class) = match status {
        "online" | "active" | "downloading" => ("bg-green-400", "status-pulse"),
        "warning" | "slow" => ("bg-yellow-400", ""),
        "error" | "failed" | "offline" => ("bg-red-400", ""),
        _ => ("bg-gray-400", ""),
    };

    format!(
        r#"<div class="flex items-center space-x-2">
            <div class="w-2 h-2 {color_class} rounded-full {pulse_class}"></div>
            <span class="text-sm text-gray-400">{label}</span>
        </div>"#
    )
}
