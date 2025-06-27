//! Template rendering engine implementation

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::super::{HtmlResponse, WebUIError};
use super::{base::BASE_TEMPLATE, pages::*};

/// Template rendering engine for server-side rendering.
///
/// Provides HTML template rendering with JSON context injection for the web UI.
/// Uses a simple template system optimized for streaming media server interfaces.
#[derive(Debug, Clone)]
pub struct TemplateEngine {
    templates: HashMap<String, String>,
    base_template: String,
    // TODO: Implement template hot-reloading using this directory
    _template_dir: PathBuf,
}

impl TemplateEngine {
    /// Creates new template engine with templates loaded from directory.
    pub fn new() -> Self {
        Self::from_directory("templates")
    }

    /// Creates template engine loading templates from specified directory.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template directory not found or templates failed to load
    pub fn from_directory<P: AsRef<Path>>(template_dir: P) -> Self {
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
            match fs::read_to_string(&template_path) {
                Ok(content) => {
                    templates.insert(name.to_string(), content);
                }
                Err(_) => {
                    // Fall back to embedded templates if files don't exist
                    templates.insert(name.to_string(), Self::get_embedded_template(name));
                }
            }
        }

        Self {
            templates,
            base_template: BASE_TEMPLATE.to_string(),
            _template_dir: template_dir,
        }
    }

    /// Renders template with provided context data.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template rendering failed or template not found
    pub fn render(&self, template_name: &str, context: Value) -> Result<HtmlResponse, WebUIError> {
        let template_content = self
            .templates
            .get(template_name)
            .ok_or_else(|| WebUIError::TemplateError {
                reason: format!("Template '{template_name}' not found"),
            })?;

        let rendered_content = self.render_template_string(template_content, &context)?;

        // Wrap in base template
        let mut base_context = context;
        if let Value::Object(ref mut map) = base_context {
            map.insert("content".to_string(), Value::String(rendered_content));
        }

        let final_html = self.render_template_string(&self.base_template, &base_context)?;

        Ok(HtmlResponse::from_string(final_html))
    }

    /// Renders template string with context data.
    ///
    /// # Errors
    /// - `WebUIError::TemplateError` - Template parsing or rendering failed
    pub fn render_template_string(
        &self,
        template: &str,
        context: &Value,
    ) -> Result<String, WebUIError> {
        let mut result = template.to_string();

        // Simple template variable replacement
        if let Value::Object(map) = context {
            for (key, value) in map {
                let placeholder = format!("{{{{{key}}}}}");
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Null => String::new(),
                    _ => serde_json::to_string(value).unwrap_or_default(),
                };
                result = result.replace(&placeholder, &replacement);
            }
        }

        Ok(result)
    }

    /// Gets embedded template content for fallback.
    fn get_embedded_template(name: &str) -> String {
        match name {
            "home" => HOME_TEMPLATE.to_string(),
            "library" => LIBRARY_TEMPLATE.to_string(),
            "torrents" => TORRENTS_TEMPLATE.to_string(),
            "add_torrent" => ADD_TORRENT_TEMPLATE.to_string(),
            "search" => SEARCH_TEMPLATE.to_string(),
            "settings" => SETTINGS_TEMPLATE.to_string(),
            _ => format!("<div class=\"error\">Template '{name}' not found</div>"),
        }
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}