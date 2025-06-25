//! Static file serving for the Riptide web UI

mod css;
mod js;

use std::collections::HashMap;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
pub use css::CSS_CONTENT;
pub use js::JS_CONTENT;

/// Static file handler for CSS, JavaScript, and images.
#[derive(Clone)]
pub struct StaticFileHandler {
    files: HashMap<String, StaticFile>,
}

/// Represents a static file with content and MIME type.
#[derive(Debug, Clone)]
pub struct StaticFile {
    pub content: &'static str,
    pub mime_type: &'static str,
}

impl StaticFileHandler {
    /// Creates new static file handler with built-in assets.
    pub fn new() -> Self {
        let mut files = HashMap::new();

        // CSS files
        files.insert(
            "css/style.css".to_string(),
            StaticFile {
                content: CSS_CONTENT,
                mime_type: "text/css",
            },
        );

        // JavaScript files
        files.insert(
            "js/app.js".to_string(),
            StaticFile {
                content: JS_CONTENT,
                mime_type: "application/javascript",
            },
        );

        Self { files }
    }

    /// Serves a static file by path.
    pub fn serve(&self, path: &str) -> Result<Response, StatusCode> {
        if let Some(file) = self.files.get(path) {
            Ok((
                [(axum::http::header::CONTENT_TYPE, file.mime_type)],
                file.content,
            )
                .into_response())
        } else {
            Err(StatusCode::NOT_FOUND)
        }
    }

    /// Lists all available static files.
    pub fn list_files(&self) -> Vec<&String> {
        self.files.keys().collect()
    }
}

impl Default for StaticFileHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_file_handler_creation() {
        let handler = StaticFileHandler::new();
        assert!(!handler.files.is_empty());
        assert!(handler.files.contains_key("css/style.css"));
        assert!(handler.files.contains_key("js/app.js"));
    }

    #[test]
    fn test_css_content() {
        let handler = StaticFileHandler::new();
        let css_file = handler.files.get("css/style.css").unwrap();
        assert!(css_file.content.contains("body"));
        assert_eq!(css_file.mime_type, "text/css");
    }

    #[test]
    fn test_js_content() {
        let handler = StaticFileHandler::new();
        let js_file = handler.files.get("js/app.js").unwrap();
        assert!(js_file.content.contains("function"));
        assert_eq!(js_file.mime_type, "application/javascript");
    }

    #[test]
    fn test_serve_existing_file() {
        let handler = StaticFileHandler::new();
        let response = handler.serve("css/style.css");
        assert!(response.is_ok());
    }

    #[test]
    fn test_serve_nonexistent_file() {
        let handler = StaticFileHandler::new();
        let response = handler.serve("nonexistent.css");
        assert_eq!(response.unwrap_err(), StatusCode::NOT_FOUND);
    }
}
