//! OpenAPI documentation endpoints.
//!
//! Serves the embedded `openapi.yaml` spec as JSON at `/openapi.json` and a
//! lightweight documentation UI at `/docs`.

use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};

/// Raw OpenAPI YAML bundled at compile time.
pub const OPENAPI_YAML: &str = include_str!("../openapi.yaml");

/// `GET /openapi.json` — return the OpenAPI spec as JSON.
pub async fn openapi_json() -> Response {
    match serde_yaml::from_str::<serde_json::Value>(OPENAPI_YAML) {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(json) => (
                [(
                    header::CONTENT_TYPE,
                    header::HeaderValue::from_static("application/json"),
                )],
                json,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to serialize openapi.json: {e}"),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to parse openapi.yaml: {e}"),
        )
            .into_response(),
    }
}

/// `GET /docs` — Scalar API documentation UI.
pub async fn docs() -> Html<&'static str> {
    Html(DOCS_HTML)
}

const DOCS_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>vico-vee API Reference</title>
    <style>
      html, body { margin: 0; padding: 0; height: 100%; }
      #app { height: 100vh; }
    </style>
  </head>
  <body>
    <div id="app"></div>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.25.0/browser/standalone.min.js"></script>
    <script>
      Scalar.createApiReference('#app', { url: '/openapi.json' });
    </script>
  </body>
</html>
"#;

/// Parse the embedded OpenAPI spec into a JSON value for tests.
pub fn spec_value() -> Result<serde_json::Value, String> {
    serde_yaml::from_str(OPENAPI_YAML).map_err(|e| e.to_string())
}

/// Extract the set of path strings declared in the OpenAPI spec.
pub fn spec_paths() -> Result<std::collections::HashSet<String>, String> {
    let spec = spec_value()?;
    let paths = spec
        .get("paths")
        .and_then(|p| p.as_object())
        .ok_or("OpenAPI spec missing 'paths' object")?;
    Ok(paths.keys().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_yaml_parses() {
        let spec = spec_value().unwrap();
        assert!(spec.get("openapi").is_some());
        assert!(spec.get("paths").is_some());
    }

    #[test]
    fn docs_html_references_spec() {
        assert!(DOCS_HTML.contains("/openapi.json"));
    }
}
