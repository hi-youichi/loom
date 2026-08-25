//! Static SPA serving — host a built frontend (anureo Desk `packages/web/dist`)
//! on the same origin as the API, replacing the Vite dev proxy in production.

use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    extract::{Query, Request},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use tower_http::services::ServeDir;

const DEFAULT_PWA_APP_NAME: &str = "anureo Desk - AI Coding Assistant";

/// Path prefixes that must never fall through to the SPA `index.html` — they
/// belong to the API surface. Mirrors the Express fallback regex in
/// anureo's `static-routes-runtime.js`.
const RESERVED_PREFIXES: &[&str] = &["/api", "/acp", "/global", "/metrics"];

/// File extensions that must 404 instead of falling back to `index.html` — a
/// missing asset answering with HTML turns a clear 404 into a MIME error.
const ASSET_EXTENSIONS: &[&str] = &[
    "js",
    "mjs",
    "css",
    "map",
    "svg",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "ico",
    "woff",
    "woff2",
    "ttf",
    "eot",
    "json",
    "webmanifest",
    "txt",
    "wasm",
];

/// Router that serves `dist_dir` statically with an SPA fallback to
/// `index.html` for client-side routes.
pub fn static_router(dist_dir: impl Into<PathBuf>) -> Router {
    let dir = dist_dir.into();
    let index = dir.join("index.html");
    let fallback = any(move |req: Request| {
        let index = index.clone();
        async move { spa_fallback(req, index).await }
    });
    let shell_index = dir.join("index.html");
    let shell = any(move || {
        let index = shell_index.clone();
        async move { serve_shell(index).await }
    });
    Router::new()
        // The shell must bypass ServeDir so its cache policy (no-store) holds.
        .route("/", shell.clone())
        .route("/index.html", shell)
        // PWA manifest is generated on the fly (no static file in dist) —
        // mirrors anureo's Express `registerPwaManifestRoute` subset.
        .route("/manifest.webmanifest", get(pwa_manifest))
        // `fallback_service` (not `not_found_service`) keeps the fallback's
        // own status — `not_found_service` would force 404 on the shell.
        .fallback_service(ServeDir::new(dir).fallback(fallback))
}

async fn spa_fallback(req: Request, index: PathBuf) -> Response {
    let path = req.uri().path();
    let reserved = RESERVED_PREFIXES
        .iter()
        .any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")));
    let is_asset = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            ASSET_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
        .unwrap_or(false);

    if reserved || is_asset {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    serve_shell(index).await
}

async fn serve_shell(index: PathBuf) -> Response {
    match tokio::fs::read(&index).await {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            // Never let a stale shell outlive a redeploy; hashed assets are
            // immutable by URL and safe to cache normally.
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(body))
            .expect("static response parts are valid"),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "static files not found — build the frontend first",
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ManifestQuery {
    pwa_name: Option<String>,
    #[serde(alias = "appName")]
    app_name: Option<String>,
    orientation: Option<String>,
}

/// Generate `/manifest.webmanifest` on the fly (query overrides only; the
/// Express version also merges settings + recent-session shortcuts).
async fn pwa_manifest(Query(query): Query<ManifestQuery>) -> Response {
    let name = query
        .pwa_name
        .or(query.app_name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_PWA_APP_NAME.to_string());
    let short_name: String = name.chars().take(30).collect();
    let orientation = match query.orientation.as_deref().map(str::trim) {
        Some("portrait") => Some("portrait-primary"),
        Some("landscape") => Some("landscape-primary"),
        _ => None,
    };

    let mut manifest = serde_json::json!({
        "name": name,
        "short_name": short_name,
        "description": "Web interface companion for anureo AI coding agent",
        "id": "/",
        "start_url": "/",
        "scope": "/",
        "display": "standalone",
        "display_override": ["window-controls-overlay"],
        "background_color": "#151313",
        "theme_color": "#edb449",
        "icons": [
            { "src": "/pwa-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any" },
            { "src": "/pwa-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any" },
            { "src": "/pwa-maskable-192.png", "sizes": "192x192", "type": "image/png", "purpose": "any maskable" },
            { "src": "/pwa-maskable-512.png", "sizes": "512x512", "type": "image/png", "purpose": "any maskable" },
            { "src": "/apple-touch-icon-180x180.png", "sizes": "180x180", "type": "image/png", "purpose": "any" },
            { "src": "/apple-touch-icon-152x152.png", "sizes": "152x152", "type": "image/png", "purpose": "any" },
            { "src": "/favicon-32.png", "sizes": "32x32", "type": "image/png" },
            { "src": "/favicon-16.png", "sizes": "16x16", "type": "image/png" }
        ],
        "shortcuts": [
            {
                "name": "Appearance Settings",
                "short_name": "Settings",
                "description": "Open appearance settings",
                "url": "/?settings=appearance",
                "icons": [{ "src": "/pwa-192.png", "sizes": "192x192", "type": "image/png" }]
            }
        ],
        "categories": ["developer", "tools", "productivity"],
        "lang": "en"
    });
    if let Some(orientation) = orientation {
        manifest["orientation"] = serde_json::json!(orientation);
    }

    (
        [
            (header::CONTENT_TYPE, "application/manifest+json"),
            (header::CACHE_CONTROL, "no-store, must-revalidate"),
        ],
        Json(manifest),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    fn make_dist() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dist = tmp.path().to_path_buf();
        std::fs::write(dist.join("index.html"), "<html>shell</html>").expect("write index");
        std::fs::create_dir_all(dist.join("assets")).expect("mkdir assets");
        std::fs::write(dist.join("assets/app.js"), "console.log(1)").expect("write asset");
        (tmp, dist)
    }

    async fn send(router: Router, path: &str) -> Response {
        router
            .oneshot(
                HttpRequest::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("oneshot")
    }

    #[tokio::test]
    async fn serves_index_assets_and_spa_fallback() {
        let (_tmp, dist) = make_dist();
        let router = static_router(dist);

        let root = send(router.clone(), "/").await;
        assert_eq!(root.status(), StatusCode::OK);
        assert!(root
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("text/html")));
        assert_eq!(
            root.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );
        let body = to_bytes(root.into_body(), usize::MAX).await.expect("body");
        assert_eq!(&body[..], b"<html>shell</html>");

        let asset = send(router.clone(), "/assets/app.js").await;
        assert_eq!(asset.status(), StatusCode::OK);
        assert!(asset
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("javascript")));

        // Deep client-side route falls back to the SPA shell.
        let deep = send(router.clone(), "/session/abc-123").await;
        assert_eq!(deep.status(), StatusCode::OK);
        let deep_body = to_bytes(deep.into_body(), usize::MAX).await.expect("body");
        assert_eq!(&deep_body[..], b"<html>shell</html>");
    }

    #[tokio::test]
    async fn reserved_prefixes_and_missing_assets_return_404() {
        let (_tmp, dist) = make_dist();
        let router = static_router(dist);

        for path in ["/api/unknown", "/acp/sub", "/global/x", "/metrics", "/api"] {
            let res = send(router.clone(), path).await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "path: {path}");
        }

        let missing_asset = send(router.clone(), "/assets/missing.js").await;
        assert_eq!(missing_asset.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_dist_reports_missing_build() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let router = static_router(tmp.path());
        let res = send(router, "/").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(res.into_body(), usize::MAX).await.expect("body");
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("build the frontend first"));
    }

    #[tokio::test]
    async fn pwa_manifest_generated_with_query_overrides() {
        let (_tmp, dist) = make_dist();
        let router = static_router(dist);

        let res = send(router.clone(), "/manifest.webmanifest?orientation=portrait").await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/manifest+json")
        );
        let body = to_bytes(res.into_body(), usize::MAX).await.expect("body");
        let manifest: serde_json::Value = serde_json::from_slice(&body).expect("manifest json");
        assert_eq!(manifest["name"], DEFAULT_PWA_APP_NAME);
        assert_eq!(manifest["orientation"], "portrait-primary");

        let res = send(router, "/manifest.webmanifest?pwa_name=My%20Chamber").await;
        let body = to_bytes(res.into_body(), usize::MAX).await.expect("body");
        let manifest: serde_json::Value = serde_json::from_slice(&body).expect("manifest json");
        assert_eq!(manifest["name"], "My Chamber");
        assert!(manifest.get("orientation").is_none());
    }
}
