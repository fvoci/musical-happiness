//! Serving the web app directory. The original's only test of this code checks the base
//! URL and listen address (ported next to the code); these pin the responses.

use std::path::PathBuf;

use axum::body::{Body, to_bytes};
use axum::http::header::{
    ACCEPT_ENCODING, ACCESS_CONTROL_ALLOW_ORIGIN, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_RANGE,
    CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_NONE_MATCH, RANGE, VARY,
};
use axum::http::{Request, StatusCode};
use axum::response::Response;
use fvoci_model::new_id;
use fvoci_server::web::WebRoot;

struct Fixture {
    dir: PathBuf,
    web: WebRoot,
}

impl Fixture {
    fn new(server_root: &str) -> Self {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join("web-tests")
            .join(new_id());
        std::fs::create_dir_all(dir.join("static/sub")).expect("the fixture directory");
        let write = |name: &str, content: &str| {
            std::fs::write(dir.join(name), content).expect("a fixture file");
        };
        write(
            "index.html",
            "<script>window.baseURL = '{{.BaseURL}}'</script><script src=\"{{.BaseURL}}/static/main.js\"></script>",
        );
        write("static/main.js", "console.log('plain')");
        write("static/app-0123456789abcdef.js", "console.log('hashed')");
        write("static/app-0123456789abcdef.js.br", "BROTLI-BYTES");
        write("static/app-0123456789abcdef.js.gz", "GZIP-BYTES");
        write("static/app-0123456789abcdef.js.zst", "ZSTD-BYTES");
        write("static/sub/nested.css", "body{}");
        write(
            "static/viewer-frame.html",
            "<!doctype html><title>frame</title>",
        );
        write("secret.txt", "outside static");
        let web = WebRoot::new(&dir, server_root);
        Self { dir, web }
    }

    async fn get(&self, path: &str, headers: &[(axum::http::HeaderName, &str)]) -> Response {
        let mut request = Request::get(format!("/static/{path}"));
        for (name, value) in headers {
            request = request.header(name, *value);
        }
        let request = request.body(Body::empty()).expect("a valid request");
        self.web.serve_static(path, request).await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn body_text(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a readable body");
    String::from_utf8(bytes.to_vec()).expect("a UTF-8 body")
}

fn header<'r>(response: &'r Response, name: &axum::http::HeaderName) -> &'r str {
    response
        .headers()
        .get(name)
        .unwrap_or_else(|| panic!("the response has no {name} header"))
        .to_str()
        .expect("an ASCII header value")
}

#[tokio::test]
async fn static_file_without_a_content_hash_must_be_revalidated() {
    let fixture = Fixture::new("http://localhost:8000");

    let response = fixture.get("main.js", &[]).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, &CACHE_CONTROL), "no-cache");
    assert!(
        header(&response, &CONTENT_TYPE).contains("javascript"),
        "a script content type"
    );
    let etag = header(&response, &ETAG).to_owned();
    assert_eq!(body_text(response).await, "console.log('plain')");

    // The validator from the first response makes the second one a 304.
    let again = fixture.get("main.js", &[(IF_NONE_MATCH, &etag)]).await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(header(&again, &CACHE_CONTROL), "no-cache");
}

/// The file viewer's frame has an opaque origin: it reads the app's files across origins.
#[tokio::test]
async fn static_files_may_be_read_from_any_origin() {
    let fixture = Fixture::new("http://localhost:8000");

    for path in ["main.js", "app-0123456789abcdef.js", "sub/nested.css"] {
        let response = fixture.get(path, &[]).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            header(&response, &ACCESS_CONTROL_ALLOW_ORIGIN),
            "*",
            "{path}"
        );
        assert!(
            response.headers().get(CONTENT_SECURITY_POLICY).is_none(),
            "{path} is not sandboxed"
        );
    }
}

/// Opened on its own, the viewer's frame is sandboxed as it is in the app's iframe, by
/// whatever name reaches it.
#[tokio::test]
async fn the_viewer_frame_is_sandboxed() {
    let fixture = Fixture::new("http://localhost:8000");

    for path in [
        "viewer-frame.html",
        "viewer%2Dframe.html",
        "viewer-frame%2Ehtml",
    ] {
        let response = fixture.get(path, &[]).await;

        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            header(&response, &CONTENT_SECURITY_POLICY),
            "sandbox allow-scripts",
            "{path}"
        );
        assert_eq!(
            header(&response, &ACCESS_CONTROL_ALLOW_ORIGIN),
            "*",
            "{path}"
        );
        assert_eq!(header(&response, &CACHE_CONTROL), "no-cache", "{path}");
    }
}

#[tokio::test]
async fn static_file_with_a_content_hash_is_immutable() {
    let fixture = Fixture::new("http://localhost:8000");

    let response = fixture.get("app-0123456789abcdef.js", &[]).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        header(&response, &CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert!(
        response.headers().get(CONTENT_ENCODING).is_none(),
        "no encoding was asked for"
    );
    assert_eq!(body_text(response).await, "console.log('hashed')");
}

#[tokio::test]
async fn precompressed_variants_follow_accept_encoding() {
    let fixture = Fixture::new("http://localhost:8000");

    let zstd = fixture
        .get(
            "app-0123456789abcdef.js",
            &[(ACCEPT_ENCODING, "gzip, deflate, br, zstd")],
        )
        .await;
    assert_eq!(header(&zstd, &CONTENT_ENCODING), "zstd", "zstd on a tie");
    assert_eq!(body_text(zstd).await, "ZSTD-BYTES");
    let preferred = fixture
        .get(
            "app-0123456789abcdef.js",
            &[(ACCEPT_ENCODING, "zstd;q=0.5, br")],
        )
        .await;
    assert_eq!(
        header(&preferred, &CONTENT_ENCODING),
        "br",
        "the client's preference"
    );

    let brotli = fixture
        .get("app-0123456789abcdef.js", &[(ACCEPT_ENCODING, "gzip, br")])
        .await;
    assert_eq!(header(&brotli, &CONTENT_ENCODING), "br");
    assert!(
        header(&brotli, &VARY)
            .to_lowercase()
            .contains("accept-encoding"),
        "Vary names the header"
    );
    assert!(
        header(&brotli, &CONTENT_TYPE).contains("javascript"),
        "the type of the original file"
    );
    assert_eq!(
        header(&brotli, &CACHE_CONTROL),
        "public, max-age=31536000, immutable"
    );
    assert_eq!(body_text(brotli).await, "BROTLI-BYTES");

    let gzip = fixture
        .get("app-0123456789abcdef.js", &[(ACCEPT_ENCODING, "gzip")])
        .await;
    assert_eq!(header(&gzip, &CONTENT_ENCODING), "gzip");
    assert_eq!(body_text(gzip).await, "GZIP-BYTES");

    // A file without a compressed variant is served as it is.
    let plain = fixture
        .get("main.js", &[(ACCEPT_ENCODING, "gzip, br")])
        .await;
    assert!(
        plain.headers().get(CONTENT_ENCODING).is_none(),
        "there is no variant to serve"
    );
    assert_eq!(body_text(plain).await, "console.log('plain')");
}

#[tokio::test]
async fn range_requests_get_partial_content() {
    let fixture = Fixture::new("http://localhost:8000");

    let response = fixture.get("main.js", &[(RANGE, "bytes=0-6")]).await;

    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(header(&response, &CONTENT_RANGE), "bytes 0-6/20");
    assert_eq!(body_text(response).await, "console");
}

#[tokio::test]
async fn what_is_not_a_file_under_static_is_not_found() {
    let fixture = Fixture::new("http://localhost:8000");

    for path in [
        "missing.js",
        "sub",
        "sub/",
        "",
        "../secret.txt",
        "..%2Fsecret.txt",
        "sub/../../secret.txt",
    ] {
        let response = fixture.get(path, &[]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "path {path:?}");
        assert!(
            response.headers().get(CACHE_CONTROL).is_none(),
            "a 404 carries no cache policy"
        );
        assert_eq!(
            body_text(response).await,
            "404 page not found\n",
            "path {path:?}"
        );
    }

    let nested = fixture.get("sub/nested.css", &[]).await;
    assert_eq!(nested.status(), StatusCode::OK);
}

#[tokio::test]
async fn index_gets_the_base_url_and_must_be_revalidated() {
    let fixture = Fixture::new("https://example.com/boards");
    assert_eq!(fixture.web.base_url(), "/boards");

    let response = fixture.web.serve_index().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(header(&response, &CONTENT_TYPE), "text/html; charset=utf-8");
    assert_eq!(header(&response, &CACHE_CONTROL), "no-cache");
    assert_eq!(
        body_text(response).await,
        "<script>window.baseURL = '/boards'</script><script src=\"/boards/static/main.js\"></script>"
    );
}

#[tokio::test]
async fn missing_index_is_a_server_error() {
    let fixture = Fixture::new("http://localhost:8000");
    std::fs::remove_file(fixture.dir.join("index.html")).expect("the index can be removed");

    let response = fixture.web.serve_index().await;

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body_text(response).await, "");
}
