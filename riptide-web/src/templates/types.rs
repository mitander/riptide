//! Template engine types and core structures

use std::collections::HashMap;
use std::path::PathBuf;

use crate::{HtmlResponse, WebUIError};

/// Template rendering engine for server-side rendering.
///
/// Provides HTML template rendering with JSON context injection for the web UI.
/// Uses a simple template system optimized for streaming media server interfaces.
#[derive(Debug, Clone)]
pub struct TemplateEngine {
    pub(super) templates: HashMap<String, String>,
    pub(super) base_template: String,
    // TODO: Implement template hot-reloading using this directory
    pub(super) _template_dir: PathBuf,
}

impl TemplateEngine {
    /// Creates new template engine with templates loaded from default directory.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template files cannot be read from riptide-web/templates/
    pub fn new() -> Result<Self, crate::WebUIError> {
        Self::from_directory("riptide-web/templates")
    }

    /// Creates template engine loading templates from specified directory.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template file cannot be read or directory does not exist
    pub fn from_directory<P: AsRef<std::path::Path>>(
        template_dir: P,
    ) -> Result<Self, crate::WebUIError> {
        use std::fs;

        let template_dir = template_dir.as_ref().to_path_buf();
        let mut templates = HashMap::new();

        // Template file mappings
        let template_files = [
            ("home", "home.html"),
            ("library", "library.html"),
            ("torrents", "torrents.html"),
            ("add_torrent", "add-torrent.html"),
            ("search", "search.html"),
            ("settings", "settings.html"),
        ];

        // Load templates from files
        for (name, filename) in template_files {
            let template_path = template_dir.join(filename);
            let content = fs::read_to_string(&template_path).map_err(|e| {
                crate::WebUIError::TemplateError {
                    reason: format!("Failed to load template {filename}: {e}"),
                }
            })?;
            templates.insert(name.to_string(), content);
        }

        // Load base template
        let base_template_path = template_dir.join("base.html");
        let base_content = fs::read_to_string(&base_template_path).map_err(|e| {
            crate::WebUIError::TemplateError {
                reason: format!("Failed to load base template: {e}"),
            }
        })?;

        Ok(Self {
            templates,
            base_template: base_content,
            _template_dir: template_dir,
        })
    }

    /// Renders template with provided context.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template not found or rendering failed
    pub fn render(
        &self,
        template_name: &str,
        context: &serde_json::Value,
    ) -> Result<HtmlResponse, WebUIError> {
        let template =
            self.templates
                .get(template_name)
                .ok_or_else(|| WebUIError::TemplateError {
                    reason: format!("Template '{template_name}' not found"),
                })?;

        let content = super::rendering::interpolate_template(template, context)?;
        let full_page = super::rendering::wrap_in_base(&self.base_template, &content, context)?;

        Ok(axum::response::Html(full_page))
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new_minimal()
    }
}

impl TemplateEngine {
    /// Creates a minimal template engine for testing.
    ///
    /// Uses empty templates - suitable for unit tests that don't need actual HTML.
    pub fn new_minimal() -> Self {
        let mut templates = HashMap::new();

        // Add minimal templates for testing
        let template_names = [
            "home",
            "library",
            "torrents",
            "add_torrent",
            "search",
            "settings",
        ];
        for name in template_names {
            templates.insert(
                name.to_string(),
                format!("<div class=\"{name}\">Test {name} template</div>"),
            );
        }

        let base_template =
            "<html><head><title>{{title}}</title></head><body>{{content}}</body></html>"
                .to_string();

        Self {
            templates,
            base_template,
            _template_dir: std::path::PathBuf::from("test"),
        }
    }
}
