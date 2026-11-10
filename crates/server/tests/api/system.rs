//! Ports of `server/api/system_test.go`, `server/app/server_metadata_test.go` and
//! `server/app/clientConfig_test.go`, plus tests of the routing around the API: the
//! request-forgery header, cache headers, the web app fallback and the base path.

use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::common::{Client, Setup};

/// `TestHello/"Returns 'Hello' on success"`
#[tokio::test]
async fn hello_returns_hello_on_success() {
    let th = Setup::native().start().await;

    let response = Client::send(th.client.raw_request(Method::GET, "/hello")).await;

    response.check_ok();
    assert_eq!(response.text(), "Hello", "the greeting");
    th.teardown().await;
}

/// `TestPing/"Returns metadata on success"` and `TestGetServerMetadata`
#[tokio::test]
async fn ping_returns_metadata_on_success() {
    let th = Setup::native().start().await;

    let response = Client::send(th.client.raw_request(Method::GET, "/ping")).await;

    let metadata = response.ok_json::<serde_json::Value>();
    assert_eq!(metadata["version"], json!("8.0.0"), "{metadata}");
    assert_eq!(metadata["sku"], json!("personal_server"), "{metadata}");
    assert_eq!(metadata["edition"], json!("dev"), "{metadata}");
    assert_eq!(metadata["build_number"], json!(""), "{metadata}");
    let db_type = metadata["db_type"].as_str().unwrap_or_default();
    assert!(db_type == "sqlite3" || db_type == "postgres", "{metadata}");
    assert!(
        !metadata["db_version"]
            .as_str()
            .unwrap_or_default()
            .is_empty(),
        "the database reports a version: {metadata}"
    );
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(metadata["os_type"], json!("linux"), "{metadata}");
        assert_eq!(metadata["os_arch"], json!("amd64"), "{metadata}");
    }
    let mut keys: Vec<&str> = metadata
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys().map(String::as_str))
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "build_date",
            "build_number",
            "commit",
            "db_type",
            "db_version",
            "edition",
            "os_arch",
            "os_type",
            "sku",
            "version"
        ],
        "the members of the original"
    );
    th.teardown().await;
}

/// `TestPing/"Sets SKU to 'personal_desktop' when in single-user mode"`. The case
/// `"Sets SKU to 'suite' when in plugin mode"` has no counterpart: there is no such mode.
#[tokio::test]
async fn ping_sets_sku_to_personal_desktop_when_in_single_user_mode() {
    let th = Setup::single_user().start().await;

    // No token: `/ping` needs none.
    let response = Client::send(th.anonymous().raw_request(Method::GET, "/ping")).await;

    assert_eq!(
        response.ok_json::<serde_json::Value>()["sku"],
        json!("personal_desktop"),
        "sku"
    );
    th.teardown().await;
}

/// `TestGetClientConfig/"Test Get Client Config"`. Telemetry is not ported; its two
/// members are still sent, switched off, for the original's web app.
#[tokio::test]
async fn get_client_config() {
    let th = Setup::local_mode()
        .configure(|config| {
            config.teammate_name_display = "username".to_owned();
            config
                .feature_flags
                .insert("newBoardsEditor".to_owned(), "true".to_owned());
            config.max_file_size = 1234;
        })
        .start()
        .await;

    // Without a session: the login page needs the configuration.
    let response = th.anonymous().get("/clientConfig").await;

    assert_eq!(
        response.ok_json::<serde_json::Value>(),
        json!({
            "telemetry": false,
            "telemetryid": "",
            "enablePublicSharedBoards": true,
            "teammateNameDisplay": "username",
            "featureFlags": {"newBoardsEditor": "true"},
            "maxFileSize": 1234,
        }),
        "the client configuration"
    );
    th.teardown().await;
}

/// Every API route refuses a request without the header a cross-site page cannot set.
#[tokio::test]
async fn api_requests_need_the_requested_with_header() {
    let th = Setup::native().start().await.init_basic().await;

    for path in ["/api/v2/clientConfig", "/api/v2/users/me", "/api/v2/teams"] {
        let request = th
            .client
            .raw_request(Method::GET, path)
            .bearer_auth(&th.client.token);
        let response = Client::send(request).await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(
            response.value(),
            json!({"error": "checkCSRFToken FAILED", "errorCode": 400}),
            "{path}"
        );
    }

    // Outside the API the header is not needed.
    Client::send(th.client.raw_request(Method::GET, "/hello"))
        .await
        .check_ok();
    th.teardown().await;
}

/// API answers must not be kept by a cache; that is a deliberate difference.
#[tokio::test]
async fn api_responses_are_not_cacheable() {
    let th = Setup::native().start().await.init_basic().await;

    let ok = th.client.get("/users/me").await;
    assert_eq!(
        ok.header("cache-control"),
        "no-store",
        "a successful answer"
    );
    assert_eq!(ok.header("content-type"), "application/json", "is JSON");

    let refused = th.anonymous().get("/users/me").await;
    refused.check_unauthorized();
    assert_eq!(
        refused.header("cache-control"),
        "no-store",
        "an error answer"
    );
    assert_eq!(
        refused.header("content-type"),
        "application/json",
        "is JSON too"
    );
    th.teardown().await;
}

/// Preserved quirk: what is no API route, and a known route asked with another method,
/// gets the web app's page with status 200.
#[tokio::test]
async fn unknown_paths_get_the_web_app() {
    let th = Setup::native().start().await;

    let paths = [
        (Method::GET, "/"),
        (Method::GET, "/board/some-board"),
        (Method::GET, "/api/v2/no-such-route"),
        (Method::DELETE, "/api/v2/login"),
        (Method::POST, "/hello"),
    ];
    for (method, path) in paths {
        let request = th
            .client
            .raw_request(method.clone(), path)
            .header("X-Requested-With", "XMLHttpRequest");
        let response = Client::send(request).await;
        assert_eq!(response.status, StatusCode::OK, "{method} {path}");
        assert!(
            response.text().contains("test web app"),
            "{method} {path}: {}",
            response.text()
        );
        assert_eq!(
            response.header("cache-control"),
            "no-cache",
            "{method} {path}"
        );
    }

    let file = Client::send(th.client.raw_request(Method::GET, "/static/main.js")).await;
    file.check_ok();
    assert_eq!(file.text(), "console.log('test');", "a static file");
    for path in ["/static", "/static/", "/static/missing.js", "/staticx/y"] {
        let response = Client::send(th.client.raw_request(Method::GET, path)).await;
        assert_eq!(response.status, StatusCode::NOT_FOUND, "{path}");
    }
    th.teardown().await;
}

/// With a base path, everything lives under it and the page's base address names it.
#[tokio::test]
async fn base_path_prefixes_every_route() {
    let th = Setup::native()
        .base_path("/boards")
        .configure(|config| config.server_root = "https://example.com/boards".to_owned())
        .start()
        .await
        .init_basic()
        .await;

    // The clients already talk to `/boards/api/v2`.
    th.client.get("/users/me").await.check_ok();
    let index = Client::send(th.client.raw_request(Method::GET, "/board/x")).await;
    index.check_ok();
    assert!(
        index.text().contains("<base href=\"/boards/\">"),
        "{}",
        index.text()
    );

    // Outside the prefix there is nothing.
    let root = th.base_url.trim_end_matches("/boards");
    let outside = reqwest::get(format!("{root}/api/v2/clientConfig"))
        .await
        .expect("the server answers");
    assert_eq!(
        outside.status(),
        StatusCode::NOT_FOUND,
        "outside the base path"
    );
    th.teardown().await;
}

/// A base path that is no absolute path, and an empty single-user token, are refused at
/// startup with an error. The first would otherwise end in a panic of the router, the
/// second would let every request without a token in.
#[tokio::test]
async fn startup_refuses_a_malformed_base_path_and_an_empty_single_user_token() {
    for base_path in ["boards", "/boards/{team}", "/boards/*rest"] {
        let error = Setup::native()
            .base_path(base_path)
            .try_start()
            .await
            .err()
            .unwrap_or_else(|| panic!("the base path {base_path:?} must be refused"));
        assert!(
            error.to_string().contains("FVOCI_HTTP_SERVER_BASEPATH"),
            "{base_path:?}: {error}"
        );
    }

    let error = Setup::single_user_with_token("")
        .try_start()
        .await
        .err()
        .expect("an empty single-user token must be refused");
    assert!(error.to_string().contains("single-user token"), "{error}");

    // A trailing slash is fine, as before.
    let th = Setup::native().base_path("/boards/").start().await;
    Client::send(th.client.raw_request(Method::GET, "/hello"))
        .await
        .check_ok();
    th.teardown().await;
}

/// A handler finishes what it started when its client goes away. The framework would
/// otherwise drop it at its next pause: a board would exist without being filed under
/// its creator's default category. The original's handlers always run to their end.
#[tokio::test]
async fn a_request_is_finished_when_its_client_disconnects() {
    use tokio::io::AsyncWriteExt as _;

    let th = Setup::native().start().await.init_basic().await;
    let address = th.base_url.trim_start_matches("http://").to_owned();

    for round in 0..20 {
        let title = format!("abandoned {round}");
        let body = json!({"teamId": "0", "type": "P", "title": title}).to_string();
        let request = format!(
            "POST /api/v2/boards HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\n\
             X-Requested-With: XMLHttpRequest\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            th.client.token,
            body.len()
        );
        let mut stream = tokio::net::TcpStream::connect(&address)
            .await
            .expect("the server accepts the connection");
        stream
            .write_all(request.as_bytes())
            .await
            .expect("the request is sent");
        // Gone before the answer.
        drop(stream);
    }

    // A request whose connection closed before its handler began does nothing at all.
    // Every board that was created must also have been filed under the default
    // category. Wait until nothing changes any more.
    let mut settled = (0, 0, 0);
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let boards = th.client.get("/teams/0/boards").await.value();
        let created: Vec<&str> = boards
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|board| board["id"].as_str())
            .collect();
        let categories = th.client.get("/teams/0/categories").await.value();
        let filed: Vec<&str> = categories
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|category| category["boardMetadata"].as_array().into_iter().flatten())
            .filter_map(|metadata| metadata["boardID"].as_str())
            .collect();
        let counts = (
            created.len(),
            created.iter().filter(|id| filed.contains(id)).count(),
            categories.as_array().map_or(0, Vec::len),
        );
        if counts == settled && counts.0 > 0 {
            break;
        }
        settled = counts;
    }
    let (created, filed, categories) = settled;
    assert!(created > 0, "some of the abandoned requests were started");
    assert_eq!(filed, created, "every created board was filed");
    assert_eq!(categories, 1, "under the one default category");
    th.teardown().await;
}

/// Static file names are percent-decoded once: a name with a space or a percent sign
/// is found.
#[tokio::test]
async fn static_file_names_are_decoded_once() {
    let th = Setup::native().start().await;
    let directory = std::path::Path::new(&th.app.config().web_path).join("static");
    std::fs::write(directory.join("with space.js"), "spaced").expect("a file is written");
    std::fs::write(directory.join("100%.txt"), "percent").expect("a file is written");

    let spaced = Client::send(
        th.client
            .raw_request(Method::GET, "/static/with%20space.js"),
    )
    .await;
    spaced.check_ok();
    assert_eq!(spaced.text(), "spaced", "the name with a space");
    let percent = Client::send(th.client.raw_request(Method::GET, "/static/100%25.txt")).await;
    percent.check_ok();
    assert_eq!(percent.text(), "percent", "the name with a percent sign");

    // Leaving the static directory stays impossible.
    let path = "/static/..%2Findex.html";
    let response = Client::send(th.client.raw_request(Method::GET, path)).await;
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{path}");
    th.teardown().await;
}
