//! Template rendering and interpolation logic

use serde_json::Value;

use crate::WebUIError;

/// Interpolates template variables with context values.
///
/// # Errors
/// - `WebUIError::TemplateRenderError` - Template syntax error or invalid context
pub fn interpolate_template(template: &str, context: &Value) -> Result<String, WebUIError> {
    let mut result = template.to_string();

    // Find all {{...}} placeholders using simple string search
    let mut start = 0;
    while let Some(begin) = result[start..].find("{{") {
        let begin = start + begin;
        if let Some(end) = result[begin..].find("}}") {
            let end = begin + end + 2; // Include the }}
            let full_placeholder = &result[begin..end];
            let property_path = &result[begin + 2..end - 2].trim();

            if let Some(value) = get_nested_value(context, property_path) {
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Array(_) | Value::Object(_) => {
                        // For complex values, render as JSON for JavaScript consumption
                        serde_json::to_string(value).unwrap_or_default()
                    }
                    Value::Null => String::new(),
                };
                result = result.replace(full_placeholder, &replacement);
                start = begin + replacement.len();
            } else {
                start = end;
            }
        } else {
            break;
        }
    }

    Ok(result)
}

/// Get nested value from JSON using dot notation (e.g., "stats.total_torrents").
fn get_nested_value<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        current = current.get(part)?;
    }

    Some(current)
}

/// Wraps content in base template layout.
///
/// # Errors
/// - `WebUIError::TemplateRenderError` - Base template processing error
pub fn wrap_in_base(
    base_template: &str,
    content: &str,
    context: &Value,
) -> Result<String, WebUIError> {
    let title = context
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("Riptide Media Server");

    let page = context
        .get("page")
        .and_then(|p| p.as_str())
        .unwrap_or("home");

    let result = base_template
        .replace("{{title}}", title)
        .replace("{{content}}", content)
        .replace("{{page}}", page);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn test_variable_interpolation() {
        let template = "Hello {{name}}, you have {{count}} items.";
        let context = json!({
            "name": "Alice",
            "count": 42
        });

        let result = interpolate_template(template, &context).unwrap();
        assert_eq!(result, "Hello Alice, you have 42 items.");
    }

    #[test]
    fn test_nested_value_access() {
        let context = json!({
            "user": {
                "profile": {
                    "name": "Bob"
                }
            }
        });

        let value = get_nested_value(&context, "user.profile.name");
        assert_eq!(value.unwrap().as_str().unwrap(), "Bob");
    }

    #[test]
    fn test_wrap_in_base() {
        let base = "<html><title>{{title}}</title><body>{{content}}</body></html>";
        let content = "<p>Test content</p>";
        let context = json!({"title": "Test Page"});

        let result = wrap_in_base(base, content, &context).unwrap();
        assert_eq!(
            result,
            "<html><title>Test Page</title><body><p>Test content</p></body></html>"
        );
    }
}
