//! OpenAPI documentation routes for `vico-vee`.
//!
//! Serves the hand-written `openapi.yaml` spec as JSON at `/openapi.json` and a
//! lightweight documentation page at `/docs`.

use axum::{
    response::{Html, IntoResponse, Response},
    Json,
};

/// The OpenAPI 3.1 spec in YAML form, embedded at compile time.
pub const OPENAPI_YAML: &str = include_str!("../openapi.yaml");

/// Serve the OpenAPI spec as JSON.
pub async fn openapi_json() -> Response {
    match serde_yaml::from_str::<serde_json::Value>(OPENAPI_YAML) {
        Ok(value) => Json(value).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "failed to parse openapi.yaml");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("OpenAPI spec is invalid: {}", e),
            )
                .into_response()
        }
    }
}

/// Serve a simple documentation page that loads the spec.
pub async fn docs() -> Html<&'static str> {
    Html(DOCS_HTML)
}

const DOCS_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>vico-vee API Documentation</title>
  <style>
    body { font-family: system-ui, -apple-system, sans-serif; margin: 2rem; }
    pre { background: #f4f4f4; padding: 1rem; overflow: auto; }
  </style>
</head>
<body>
  <h1>vico-vee API Documentation</h1>
  <p>OpenAPI spec: <a href="/openapi.json">/openapi.json</a></p>
  <p>Service: <code>vico-vee</code> — ViCo Execution Environment</p>
  <hr />
  <pre id="spec">Loading spec...</pre>
  <script>
    fetch('/openapi.json')
      .then(r => r.json())
      .then(spec => {
        document.getElementById('spec').textContent = JSON.stringify(spec, null, 2);
      })
      .catch(e => {
        document.getElementById('spec').textContent = 'Failed to load spec: ' + e;
      });
  </script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openapi_yaml_is_valid_yaml() {
        let value: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).unwrap();
        assert_eq!(value.get("openapi").and_then(|v| v.as_str()), Some("3.1.0"));
    }

    #[test]
    fn openapi_spec_covers_all_registered_routes() {
        let value: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).unwrap();
        let paths = value
            .get("paths")
            .expect("paths object")
            .as_object()
            .unwrap();

        let registered = crate::openapi::registered_routes();
        for (method, route) in &registered {
            let path_entry = paths.get(*route).unwrap_or_else(|| {
                panic!(
                    "OpenAPI spec missing route {} {}",
                    method.to_uppercase(),
                    route
                )
            });
            assert!(
                path_entry.get(method.to_lowercase()).is_some(),
                "OpenAPI spec missing method {} for route {}",
                method.to_uppercase(),
                route
            );
        }
    }

    #[test]
    fn openapi_spec_has_required_fields() {
        let value: serde_json::Value = serde_yaml::from_str(OPENAPI_YAML).unwrap();
        assert!(value.get("info").is_some());
        assert!(value.get("info").unwrap().get("title").is_some());
        assert!(value.get("info").unwrap().get("version").is_some());
    }
}

/// Returns the list of HTTP methods and paths registered by the service router.
///
/// Kept in sync manually with `crate::server::router`. Tests use this to verify
/// that the OpenAPI spec covers every registered route.
pub fn registered_routes() -> Vec<(&'static str, &'static str)> {
    vec![
        ("post", "/health"),
        ("post", "/vee/submit"),
        ("post", "/vee/status"),
        ("post", "/vee/cancel"),
        ("post", "/vee/list"),
        ("post", "/vee/artifacts"),
        ("post", "/vee/dashboard"),
        ("post", "/vee/patterns"),
        ("post", "/vee/audit"),
        ("post", "/vee/checkpoints"),
        ("post", "/vee/odin/health"),
        ("post", "/vee/odin/model"),
        ("post", "/vee/diff"),
        ("post", "/vee/merge"),
        ("post", "/vee/reject"),
        ("get", "/openapi.json"),
        ("get", "/docs"),
    ]
}
