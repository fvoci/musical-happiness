//! Ports of `server/api/system_test.go`, `server/app/server_metadata_test.go` and
//! `server/app/clientConfig_test.go`, plus tests of the routing around the API: the
//! request-forgery header, cache headers, the web app fallback and the base path.

use fvoci_model::BoardType;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{Client, PASSWORD, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, USER1_USERNAME};

/// `TestHello/"Returns 'Hello' on success"`
#[tokio::test]
async fn hello_returns_hello_on_success() {
    let th = Setup::native().start().await;

    let response = Client::send(th.client.raw_request(Method::GET, "/api/v1/hello")).await;

    response.check_ok();
    assert_eq!(response.text(), "Hello", "the greeting");
    th.teardown().await;
}

/// `TestPing/"Returns metadata on success"` and `TestGetServerMetadata`
#[tokio::test]
async fn ping_returns_metadata_on_success() {
    let th = Setup::native().start().await;

    let response = Client::send(th.client.raw_request(Method::GET, "/api/v1/ping")).await;

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

/// `TestGetClientConfig/"Test Get Client Config"`. Telemetry is not ported, and the
/// configuration has no members for it.
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
    let response = th.anonymous().get("/client-config").await;

    assert_eq!(
        response.ok_json::<serde_json::Value>(),
        json!({
            "enablePublicSharedBoards": true,
            "teammateNameDisplay": "username",
            "featureFlags": {"newBoardsEditor": "true"},
            "maxFileSize": 1234,
            "maxImportSize": 268_435_456,
        }),
        "the client configuration"
    );
    th.teardown().await;
}

/// Reading needs no header of the request-forgery check: a link or an image tag can
/// point at the API. What changes something does; see the `cookies` module.
#[tokio::test]
async fn reading_needs_no_requested_with_header() {
    let th = Setup::native().start().await.init_basic().await;

    for path in ["/api/v1/client-config", "/api/v1/users/me", "/api/v1/teams"] {
        let request = th
            .client
            .raw_request(Method::GET, path)
            .bearer_auth(&th.client.token);
        Client::send(request).await.check_ok();
    }
    Client::send(th.client.raw_request(Method::GET, "/api/v1/hello"))
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

/// An operation without a result answers 204: no body, and no `Content-Type` for the
/// body that is not there, whichever route it is and whatever else its answer carries
/// (a login and a logout set a cookie).
#[tokio::test]
async fn an_answer_without_a_result_has_no_body_and_no_content_type() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_TEAM_ID, BoardType::Private).await.id;
    let card: Value = th
        .client
        .post(&format!("/boards/{board}/cards"), &json!({"title": "card"}))
        .await
        .created_json();
    let card = card["id"].as_str().expect("the card has an identifier");

    // In an order in which each finds what it needs: the logout ends the session the
    // others are sent with.
    let requests = [
        (
            Method::PATCH,
            format!("/boards/{board}/blocks/{card}"),
            json!({"title": "patched"}),
        ),
        (
            Method::PATCH,
            format!("/boards/{board}/blocks"),
            json!({"blockIds": [card], "blockPatches": [{"title": "patched in a batch"}]}),
        ),
        (
            Method::DELETE,
            format!("/boards/{board}/blocks/{card}"),
            Value::Null,
        ),
        (Method::DELETE, format!("/boards/{board}"), Value::Null),
        (
            Method::POST,
            "/login".to_owned(),
            json!({"username": USER1_USERNAME, "password": PASSWORD}),
        ),
        (Method::POST, "/logout".to_owned(), Value::Null),
    ];
    for (method, path, body) in requests {
        let request = th.client.request(method.clone(), &path);
        let request = if body.is_null() {
            request
        } else {
            request.json(&body)
        };
        let response = Client::send(request).await;

        assert_eq!(
            response.status,
            StatusCode::NO_CONTENT,
            "{method} {path}: {}",
            response.text()
        );
        assert!(
            response.body.is_empty(),
            "{method} {path}: no body, but {:?}",
            response.text()
        );
        assert!(
            !response.headers.contains_key(CONTENT_TYPE),
            "{method} {path}: no content type, but {:?}",
            response.header("content-type")
        );
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

    // The clients already talk to `/boards/api/v1`.
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
    let outside = reqwest::get(format!("{root}/api/v1/client-config"))
        .await
        .expect("the server answers");
    assert_eq!(
        outside.status(),
        StatusCode::NOT_FOUND,
        "outside the base path"
    );
    th.teardown().await;
}

/// A base path that is no absolute path is refused at startup with an error. It would
/// otherwise end in a panic of the router.
#[tokio::test]
async fn startup_refuses_a_malformed_base_path() {
    for base_path in [
        "boards",
        "/boards/{team}",
        "/boards/*rest",
        // The router would read these as patterns, or never match them.
        "/:boards",
        "/boards/:team",
        "/boards?x=1",
        "/boards#top",
        "/a//b",
        "/my boards",
        "/boards\\x",
    ] {
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

    // A trailing slash is fine, as before: the base path is the same without it.
    let th = Setup::native().base_path("/boards/").start().await;
    let hello = format!("{}/api/v1/hello", th.base_url.trim_end_matches('/'));
    assert!(hello.ends_with("/boards/api/v1/hello"), "{hello}");
    let response = Client::send(reqwest::Client::new().get(hello)).await;
    response.check_ok();
    assert_eq!(response.text(), "Hello", "the API, not the web app's page");
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
        let body = json!({"teamId": ROOT_TEAM_ID, "type": "P", "title": title}).to_string();
        let request = format!(
            "POST /api/v1/boards HTTP/1.1\r\nHost: {address}\r\nAuthorization: Bearer {}\r\n\
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
        let boards = th
            .client
            .get(&format!("/teams/{ROOT_TEAM_ID}/boards"))
            .await
            .value();
        let created: Vec<&str> = boards
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|board| board["id"].as_str())
            .collect();
        let categories = th
            .client
            .get(&format!("/teams/{ROOT_TEAM_ID}/categories"))
            .await
            .value();
        let filed: Vec<&str> = categories
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|category| category["boardMetadata"].as_array().into_iter().flatten())
            .filter_map(|metadata| metadata["boardId"].as_str())
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
