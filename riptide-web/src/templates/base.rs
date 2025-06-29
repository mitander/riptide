//! Base HTML template with navigation and common styles

use std::include_str;

/// Generates the base HTML template with navigation and external assets
pub fn base_template(title: &str, active_page: &str, content: &str) -> String {
    const BASE_TEMPLATE: &str = include_str!("../../templates/base.html");

    BASE_TEMPLATE
        .replace("{{ title }}", title)
        .replace("{{ content }}", content)
        .replace(
            "{{ dashboard_active }}",
            if active_page == "dashboard" {
                "active"
            } else {
                ""
            },
        )
        .replace(
            "{{ search_active }}",
            if active_page == "search" {
                "active"
            } else {
                ""
            },
        )
        .replace(
            "{{ torrents_active }}",
            if active_page == "torrents" {
                "active"
            } else {
                ""
            },
        )
        .replace(
            "{{ library_active }}",
            if active_page == "library" {
                "active"
            } else {
                ""
            },
        )
}
