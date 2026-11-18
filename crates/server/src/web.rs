//! Serving the web app: the files under `static/` and the `index.html` shell.
//!
//! Every path that is neither an API route nor a static file gets `index.html`, so that
//! the web app's client-side routes can be opened directly. The original server adds no
//! cache headers; this one sets them so that the server can sit behind a CDN.

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::header::{
    ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE,
    X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use tower::ServiceExt as _;
use tower_http::services::ServeDir;

/// What the original answers for a path that names nothing.
const NOT_FOUND_BODY: &str = "404 page not found\n";

/// The placeholder in `index.html` that is replaced with the base URL.
const BASE_URL_PLACEHOLDER: &str = "{{.BaseURL}}";

/// The policy of every HTML document under `static/`.
///
/// The one such document is the web app's file viewer frame. The app frames it with
/// `sandbox="allow-scripts"`; this sandboxes it the same way when it is opened on its
/// own, so that it never runs with the app's origin.
const STATIC_DOCUMENT_POLICY: &str = "sandbox allow-scripts";

/// The web app directory (`webpath`) and how it is served.
#[derive(Debug, Clone)]
pub struct WebRoot {
    root: PathBuf,
    base_url: String,
}

impl WebRoot {
    /// Creates the web root for the `webpath` and `serverRoot` settings.
    pub fn new(web_path: impl AsRef<Path>, server_root: &str) -> Self {
        Self {
            root: web_path.as_ref().to_path_buf(),
            base_url: base_url(server_root),
        }
    }

    /// The path the web app is served under: the path part of `serverRoot`.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Serves the file `path` names under `static/`.
    ///
    /// `path` is the request path after `/static/`. Responses carry `Cache-Control`:
    /// a file whose name contains a content hash may be cached forever, any other file
    /// must be revalidated. A `.zst`, `.br` or `.gz` file next to the requested one is
    /// served instead when the client accepts that encoding (the one it prefers; zstd,
    /// then brotli, then gzip on a tie).
    ///
    /// Any origin may read these files: the file viewer's frame has an opaque origin and
    /// loads the app's scripts, style sheets and fonts across origins, and they are the
    /// public build, sent without credentials. An HTML document is sandboxed (it is the
    /// viewer's frame); whether it is one is read from the type the file service gave
    /// it, not from the requested name, which may be percent-encoded.
    pub async fn serve_static(&self, path: &str, request: Request<Body>) -> Response {
        let Ok(uri) = format!("/{path}").parse::<Uri>() else {
            return not_found();
        };
        let (mut parts, body) = request.into_parts();
        parts.uri = uri;

        let files = ServeDir::new(self.root.join("static"))
            .append_index_html_on_directories(false)
            .precompressed_zstd()
            .precompressed_br()
            .precompressed_gzip();
        let mut response = match files.oneshot(Request::from_parts(parts, body)).await {
            Ok(response) => response.map(Body::new),
            Err(never) => match never {},
        };
        if response.status() == StatusCode::NOT_FOUND {
            return not_found();
        }

        let file_name = path.rsplit('/').next().unwrap_or(path);
        let headers = response.headers_mut();
        headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
        let html = headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"));
        if html {
            headers.insert(
                CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(STATIC_DOCUMENT_POLICY),
            );
        }

        let cacheable = matches!(
            response.status(),
            StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED
        );
        if cacheable {
            let policy = if has_content_hash(file_name) {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            response
                .headers_mut()
                .insert(CACHE_CONTROL, HeaderValue::from_static(policy));
        }
        response
    }

    /// Serves `index.html` with the base URL filled in.
    ///
    /// The file is read on every request, so a new build of the web app is picked up
    /// without a restart, and the response must be revalidated by caches.
    pub async fn serve_index(&self) -> Response {
        match tokio::fs::read_to_string(self.root.join("index.html")).await {
            Ok(template) => (
                [
                    (
                        CONTENT_TYPE,
                        HeaderValue::from_static("text/html; charset=utf-8"),
                    ),
                    (CACHE_CONTROL, HeaderValue::from_static("no-cache")),
                ],
                template.replace(BASE_URL_PLACEHOLDER, &self.base_url),
            )
                .into_response(),
            Err(error) => {
                tracing::error!(%error, root = %self.root.display(), "unable to serve the index.html file");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

/// The response for a path that names nothing, worded as the original's.
pub fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            (
                CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            ),
            (X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        ],
        NOT_FOUND_BODY,
    )
        .into_response()
}

/// Returns the path part of the `serverRoot` URL, which is the base URL of the web app.
///
/// The path is taken as it is: a trailing slash stays, and a URL without a path gives the
/// empty string. A `serverRoot` that is not a URL also gives the empty string.
pub fn base_url(server_root: &str) -> String {
    let Some((_, rest)) = server_root.split_once("://") else {
        return String::new();
    };
    let end = rest.find(['?', '#']).unwrap_or(rest.len());
    match rest[..end].find('/') {
        Some(start) => rest[start..end].to_owned(),
        None => String::new(),
    }
}

/// Returns `true` if a file name carries a hash of the file's content, as build tools
/// produce (`main-0123456789abcdef.js`), so the file never changes under that name.
fn has_content_hash(file_name: &str) -> bool {
    file_name.split(['-', '.', '_']).any(|part| {
        part.len() >= 8
            && part.bytes().all(|byte| byte.is_ascii_hexdigit())
            && part.bytes().any(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The base URL cases of `Test_NewServer` in `server/web/webserver_test.go`. The
    /// listen address cases of that test are covered where the listener is created.
    #[test]
    fn base_url_is_the_path_of_the_server_root() {
        let cases = [
            ("https://some-fake-server.com/fake-url", "/fake-url"),
            ("https://localhost:8080/fake-url", "/fake-url"),
            // "should return '/' as base url is not good!"
            ("https://localhost:8080/#!@$@#@", "/"),
        ];
        for (server_root, want) in cases {
            assert_eq!(base_url(server_root), want, "serverRoot {server_root}");
        }
    }

    #[test]
    fn base_url_edge_cases() {
        assert_eq!(base_url("http://localhost:8000"), "");
        assert_eq!(base_url("http://localhost:8000/"), "/");
        assert_eq!(base_url("http://host/boards/sub?x=1"), "/boards/sub");
        assert_eq!(base_url("not a url"), "");
    }

    #[test]
    fn content_hash_detection() {
        for hashed in [
            "main-0123456789abcdef.js",
            "app-1a2b3c4d5e6f7a8b_bg.wasm",
            "style.9f86d081884c7d65.css",
        ] {
            assert!(has_content_hash(hashed), "{hashed} has a content hash");
        }
        for plain in [
            "main.js",
            "favicon.svg",
            "Metropolis-Regular.woff2",
            "feedface.png",
            "a-1234567.js",
        ] {
            assert!(!has_content_hash(plain), "{plain} has no content hash");
        }
    }
}
