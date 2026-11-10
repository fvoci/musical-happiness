//! The plumbing around the API: the administration socket (`server/api/admin.go`,
//! `server/server/server.go`), TLS (`server/web/webserver.go`), and the routes a
//! standalone server refuses (`server/api/{channels,statistics,compliance}.go`).
//!
//! The original has no tests of the socket and of TLS; these pin what its server was seen
//! to do. `server/integrationtests/compliance_test.go` runs in the host-application mode
//! only; the tests here are of what a standalone server answers on the same routes.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fvoci_model::{IdKind, new_id};
use fvoci_server::TlsFiles;
use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::common::{Client, PASSWORD, Setup, TestServer};

/// The administration socket. Unix sockets are what it is made of.
#[cfg(unix)]
mod socket {
    use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
    use std::path::{Path, PathBuf};

    use fvoci_model::{IdKind, new_id};
    use reqwest::{Method, StatusCode};
    use serde_json::json;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::UnixStream;

    use crate::common::{Client, PASSWORD, Setup, TestServer, USER1_USERNAME};

    const NEW_PASSWORD: &str = "N3w-Pa$$word";
    const ADMIN_PASSWORD_ROUTE: &str = "/api/v2/admin/users/user1/password";
    const INTERNAL_SERVER_ERROR: &str = r#"{"error":"internal server error","errorCode":500}"#;

    /// A path for a socket. The build directory is too long for a socket address, so the
    /// sockets of these tests live in the system's directory for temporary files.
    fn socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("fvoci-{}.socket", new_id(IdKind::None)))
    }

    /// A server with `user1` and `user2` and the administration socket at the returned path.
    async fn start_with_admin_socket() -> (TestServer, PathBuf) {
        let socket = socket_path();
        let th = Setup::native()
            .admin_socket(&socket)
            .start()
            .await
            .init_basic()
            .await;
        (th, socket)
    }

    /// What the server answered on the socket.
    #[derive(Debug)]
    struct SocketResponse {
        status: u16,
        /// The status line and the headers, as sent.
        head: String,
        body: String,
    }

    impl SocketResponse {
        #[track_caller]
        fn check(&self, status: u16, body: &str) {
            assert_eq!(self.status, status, "the status; body: {}", self.body);
            assert_eq!(self.body, body, "the body");
        }

        fn is_json(&self) -> bool {
            self.head
                .to_lowercase()
                .contains("content-type: application/json")
        }
    }

    /// Sends one HTTP request through the Unix socket. No header but the ones the protocol
    /// needs: the administration API asks for no session and no request-forgery header.
    async fn socket_request(
        socket: &Path,
        method: &str,
        target: &str,
        body: &str,
    ) -> SocketResponse {
        let mut stream = UnixStream::connect(socket)
            .await
            .expect("the socket accepts connections");
        let request = format!(
            "{method} {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .expect("the request is sent");
        let mut answer = Vec::new();
        stream
            .read_to_end(&mut answer)
            .await
            .expect("the answer is read");
        let answer = String::from_utf8_lossy(&answer);
        let (head, body) = answer
            .split_once("\r\n\r\n")
            .expect("the answer has a head and a body");
        let status = head
            .split(' ')
            .nth(1)
            .and_then(|status| status.parse().ok())
            .expect("the answer starts with a status line");
        SocketResponse {
            status,
            head: head.to_owned(),
            body: body.to_owned(),
        }
    }

    async fn set_password(socket: &Path, username: &str, body: &str) -> SocketResponse {
        let target = format!("/api/v2/admin/users/{username}/password");
        socket_request(socket, "POST", &target, body).await
    }

    /// Whether `user1` can sign in with the password.
    async fn can_log_in(th: &TestServer, password: &str) -> bool {
        let mut client = th.anonymous();
        let response = client.login(USER1_USERNAME, password).await;
        match response.status {
            StatusCode::OK => true,
            StatusCode::UNAUTHORIZED => {
                assert_eq!(
                    response.error_message(),
                    "incorrect login",
                    "the refusal of a wrong password"
                );
                false
            }
            other => panic!("unexpected status {other} at login: {}", response.text()),
        }
    }

    /// `handleAdminSetPassword` (`server/api/admin.go`): the password is replaced without
    /// asking for the old one, and the answer is an empty object.
    #[tokio::test]
    async fn admin_set_password_changes_the_password() {
        let (th, socket) = start_with_admin_socket().await;

        let body = json!({ "password": NEW_PASSWORD }).to_string();
        let response = set_password(&socket, USER1_USERNAME, &body).await;

        response.check(200, "{}");
        assert!(response.is_json(), "{}", response.head);
        assert!(can_log_in(&th, NEW_PASSWORD).await, "the new password");
        assert!(!can_log_in(&th, PASSWORD).await, "the old password");
        // Preserved quirk: the sessions the user had stay valid.
        assert_eq!(
            th.client.me().await.username,
            USER1_USERNAME,
            "the old session"
        );
        assert!(
            th.login("user2", PASSWORD).await.token.len() > 1,
            "other users keep their passwords"
        );
        th.teardown().await;
    }

    /// The user is named in the path, percent-encoded like any path.
    #[tokio::test]
    async fn admin_set_password_decodes_the_username() {
        let (th, socket) = start_with_admin_socket().await;

        let body = json!({ "password": NEW_PASSWORD }).to_string();
        set_password(&socket, "us%65r1", &body)
            .await
            .check(200, "{}");

        assert!(can_log_in(&th, NEW_PASSWORD).await, "the new password");
        th.teardown().await;
    }

    /// The refusal the original intends at `server/api/admin.go:41` but never gives: its
    /// check cannot fail, and it stores the hash of the empty password instead.
    #[tokio::test]
    async fn admin_set_password_refuses_an_empty_password() {
        let (th, socket) = start_with_admin_socket().await;

        for body in [r#"{"password":""}"#, "{}", r#"{"password":null}"#, "null"] {
            let response = set_password(&socket, USER1_USERNAME, body).await;
            response.check(400, r#"{"error":"password is required","errorCode":400}"#);
            assert!(response.is_json(), "{body}: {}", response.head);
        }

        assert!(can_log_in(&th, PASSWORD).await, "the password is unchanged");
        th.teardown().await;
    }

    /// Apart from that, the original has no rule for the passwords set here: what
    /// registration would refuse as too short or too long is accepted.
    #[tokio::test]
    async fn admin_set_password_applies_no_password_rules() {
        let (th, socket) = start_with_admin_socket().await;

        for password in ["a".to_owned(), "x".repeat(72)] {
            let body = json!({ "password": password }).to_string();
            set_password(&socket, USER1_USERNAME, &body)
                .await
                .check(200, "{}");
            assert!(can_log_in(&th, &password).await, "{} bytes", password.len());
        }
        th.teardown().await;
    }

    /// The original cannot hash more than 72 bytes and drops the connection without an
    /// answer when asked to. The port answers.
    #[tokio::test]
    async fn admin_set_password_refuses_more_than_72_bytes() {
        let (th, socket) = start_with_admin_socket().await;

        let body = json!({ "password": "x".repeat(73) }).to_string();
        let response = set_password(&socket, USER1_USERNAME, &body).await;

        response.check(
            400,
            r#"{"error":"password length exceeds 72 bytes","errorCode":400}"#,
        );
        assert!(can_log_in(&th, PASSWORD).await, "the password is unchanged");
        th.teardown().await;
    }

    /// The store's "user not found" is not one of the errors the original classifies.
    #[tokio::test]
    async fn admin_set_password_of_an_unknown_user_is_a_server_error() {
        let (th, socket) = start_with_admin_socket().await;

        let body = json!({ "password": NEW_PASSWORD }).to_string();
        let response = set_password(&socket, "nobody", &body).await;

        response.check(500, INTERNAL_SERVER_ERROR);
        assert!(response.is_json(), "{}", response.head);
        th.teardown().await;
    }

    /// The original passes the decoding error on unclassified.
    #[tokio::test]
    async fn admin_set_password_with_a_malformed_body_is_a_server_error() {
        let (th, socket) = start_with_admin_socket().await;

        for body in ["", r#"{"password":"#, r#"{"password":5}"#, "5"] {
            set_password(&socket, USER1_USERNAME, body)
                .await
                .check(500, INTERNAL_SERVER_ERROR);
        }

        assert!(can_log_in(&th, PASSWORD).await, "the password is unchanged");
        th.teardown().await;
    }

    /// The socket serves the one administration route and nothing else: not the API, not
    /// `/hello`, not the web app.
    #[tokio::test]
    async fn admin_socket_serves_only_the_admin_route() {
        let (th, socket) = start_with_admin_socket().await;

        for method in ["GET", "PUT", "DELETE"] {
            let response = socket_request(&socket, method, ADMIN_PASSWORD_ROUTE, "").await;
            response.check(405, "");
        }
        for target in [
            "/",
            "/hello",
            "/ping",
            "/api/v2/users/me",
            "/api/v2/admin/users/user1/password/",
            "/api/v2/admin/boards",
        ] {
            let response = socket_request(&socket, "GET", target, "").await;
            response.check(404, "404 page not found\n");
        }
        th.teardown().await;
    }

    /// The TCP listener does not know the administration route: as for any path it does not
    /// know, the answer is the web app, and nothing happens.
    #[tokio::test]
    async fn admin_route_is_not_reachable_over_tcp() {
        let (th, _socket) = start_with_admin_socket().await;
        let body = json!({ "password": NEW_PASSWORD });

        let with_header = th
            .anonymous()
            .post("/admin/users/user1/password", &body)
            .await;
        let without_header = Client::send(
            th.anonymous()
                .raw_request(Method::POST, ADMIN_PASSWORD_ROUTE)
                .json(&body),
        )
        .await;
        let signed_in = th.client.post("/admin/users/user1/password", &body).await;

        for response in [with_header, without_header, signed_in] {
            response.check_ok();
            assert!(
                response.header("content-type").starts_with("text/html"),
                "the web app: {}",
                response.text()
            );
            assert!(response.text().contains("test web app"), "the web app");
        }
        assert!(can_log_in(&th, PASSWORD).await, "the password is unchanged");
        assert!(!can_log_in(&th, NEW_PASSWORD).await, "nothing was set");
        th.teardown().await;
    }

    /// `startLocalModeServer` and `stopLocalModeServer` (`server/server/server.go`): the
    /// socket file is for the server's user only, and is gone after shutdown.
    #[tokio::test]
    async fn admin_socket_is_private_and_removed_on_shutdown() {
        let (th, socket) = start_with_admin_socket().await;

        let metadata = std::fs::symlink_metadata(&socket).expect("the socket file exists");
        assert!(metadata.file_type().is_socket(), "a socket");
        assert_eq!(
            metadata.permissions().mode() & 0o777,
            0o600,
            "only the owner may connect"
        );

        th.teardown().await;
        assert!(!socket.exists(), "the socket file is removed");
    }

    /// A socket file left behind by a server that was killed does not keep the next one
    /// from starting.
    #[tokio::test]
    async fn admin_socket_replaces_the_socket_of_an_earlier_run() {
        let socket = socket_path();
        drop(std::os::unix::net::UnixListener::bind(&socket).expect("a socket is bound"));
        assert!(socket.exists(), "the stale socket file exists");

        let th = Setup::native()
            .admin_socket(&socket)
            .start()
            .await
            .init_basic()
            .await;

        let body = json!({ "password": NEW_PASSWORD }).to_string();
        set_password(&socket, USER1_USERNAME, &body)
            .await
            .check(200, "{}");
        th.teardown().await;
        assert!(!socket.exists(), "the socket file is removed");
    }

    /// `enablelocalmode` is off unless asked for, and the socket has its documented place.
    #[tokio::test]
    async fn admin_socket_is_closed_by_default() {
        let th = Setup::native().start().await;

        let config = th.app.config();
        assert!(!config.enable_local_mode, "off by default");
        assert_eq!(
            config.local_mode_socket_location,
            "/var/tmp/fvoci_local.socket"
        );
        th.teardown().await;
    }
}

/// A certificate for `localhost` and its key, written where the server is told to look.
fn write_certificate(directory: &Path) -> (TlsFiles, reqwest::Certificate) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
            .expect("a certificate is generated");
    std::fs::create_dir_all(directory).expect("the certificate directory is created");
    let tls_files = TlsFiles {
        certificate: directory.join("cert.pem"),
        key: directory.join("key.pem"),
    };
    std::fs::write(&tls_files.certificate, cert.pem()).expect("the certificate is written");
    std::fs::write(&tls_files.key, signing_key.serialize_pem()).expect("the key is written");
    let root = reqwest::Certificate::from_pem(cert.pem().as_bytes())
        .expect("the certificate is read back");
    (tls_files, root)
}

fn certificate_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("tls-tests")
        .join(new_id(IdKind::None))
}

/// The port the test server listens on.
fn port_of(th: &TestServer) -> u16 {
    th.base_url
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .expect("the address ends with a port")
}

/// `usessl` with `cert.pem` and `key.pem` (`server/web/webserver.go:113-121`): the server
/// speaks TLS, and only TLS.
#[tokio::test]
async fn use_ssl_serves_over_tls() {
    let directory = certificate_directory();
    let (tls_files, root) = write_certificate(&directory);
    let th = Setup::native()
        .tls_files(tls_files)
        .configure(|config| config.use_ssl = true)
        .start()
        .await;
    let port = port_of(&th);
    let https = reqwest::Client::builder()
        .tls_certs_only([root])
        .timeout(Duration::from_secs(30))
        .build()
        .expect("an HTTPS client can be built");

    let hello = https
        .get(format!("https://localhost:{port}/hello"))
        .send()
        .await
        .expect("the server answers over TLS");
    assert_eq!(hello.status(), StatusCode::OK);
    assert_eq!(hello.text().await.expect("the body is read"), "Hello");

    // The API works as over plain HTTP, here with a body in each direction.
    let register = https
        .post(format!("https://localhost:{port}/api/v2/register"))
        .header("X-Requested-With", "XMLHttpRequest")
        .json(&json!({"username": "user1", "email": "user1@sample.com", "password": PASSWORD}))
        .send()
        .await
        .expect("the server answers over TLS");
    assert_eq!(register.status(), StatusCode::OK);
    assert_eq!(register.text().await.expect("the body is read"), "{}");

    assert!(
        reqwest::Client::new()
            .get(format!("http://localhost:{port}/hello"))
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .is_err(),
        "plain HTTP gets no answer on the TLS port"
    );

    // Shutdown is as graceful as without TLS: the server stops without an error.
    th.teardown().await;
    let _ = std::fs::remove_dir_all(&directory);
}

/// A client that does not know the certificate does not get through, and the server
/// keeps serving the ones that do.
#[tokio::test]
async fn use_ssl_failed_handshakes_do_not_disturb_the_server() {
    let directory = certificate_directory();
    let (tls_files, root) = write_certificate(&directory);
    let th = Setup::native()
        .tls_files(tls_files)
        .configure(|config| config.use_ssl = true)
        .start()
        .await;
    let port = port_of(&th);
    let url = format!("https://localhost:{port}/hello");

    let stranger = reqwest::Client::builder()
        .tls_certs_only([])
        .build()
        .expect("an HTTPS client can be built");
    assert!(
        stranger.get(&url).send().await.is_err(),
        "an unknown certificate is not trusted"
    );
    // A connection that never starts the handshake does not hold up the others.
    let _silent = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .expect("the port accepts connections");

    let https = reqwest::Client::builder()
        .tls_certs_only([root])
        .build()
        .expect("an HTTPS client can be built");
    let hello = https.get(&url).send().await.expect("the server answers");
    assert_eq!(hello.text().await.expect("the body is read"), "Hello");

    th.teardown().await;
    let _ = std::fs::remove_dir_all(&directory);
}

/// Preserved quirk of `server/web/webserver.go:113`: with `usessl` but without the
/// certificate files the server serves plain HTTP.
#[tokio::test]
async fn use_ssl_without_certificate_files_serves_plain_http() {
    let directory = certificate_directory();
    let tls_files = TlsFiles {
        certificate: directory.join("cert.pem"),
        key: directory.join("key.pem"),
    };
    let th = Setup::native()
        .tls_files(tls_files)
        .configure(|config| config.use_ssl = true)
        .start()
        .await;

    let response = Client::send(th.client.raw_request(Method::GET, "/hello")).await;

    response.check_ok();
    assert_eq!(response.text(), "Hello", "over plain HTTP");
    th.teardown().await;
}

const NO_SESSION: &str = r#"{"error":"no session token","errorCode":401}"#;
const NOT_IN_STANDALONE_MODE: &str =
    r#"{"error":"not permitted in standalone mode","errorCode":501}"#;

/// Checks what a route answers to a request without a session and to one of `user1`.
async fn check_refusal(th: &TestServer, path: &str, status: StatusCode, body: &str) {
    let anonymous = th.anonymous().get(path).await;
    anonymous.check_unauthorized();
    assert_eq!(anonymous.text(), NO_SESSION, "{path} without a session");

    let signed_in = th.client.get(path).await;
    signed_in.check_status(status);
    assert_eq!(signed_in.text(), body, "{path} with a session");
    assert_eq!(
        signed_in.header("content-type"),
        "application/json",
        "{path}"
    );
}

/// `handleGetChannel` (`server/api/channels.go:53-56`): a standalone server has no
/// channels.
#[tokio::test]
async fn get_channel_is_not_implemented_in_standalone_mode() {
    let th = Setup::native().start().await.init_basic().await;

    check_refusal(
        &th,
        "/teams/0/channels/some-channel",
        StatusCode::NOT_IMPLEMENTED,
        NOT_IN_STANDALONE_MODE,
    )
    .await;
    th.teardown().await;
}

/// `handleStatistics` (`server/api/statistics.go:36-39`).
#[tokio::test]
async fn statistics_are_not_implemented_in_standalone_mode() {
    let th = Setup::native().start().await.init_basic().await;

    check_refusal(
        &th,
        "/statistics",
        StatusCode::NOT_IMPLEMENTED,
        NOT_IN_STANDALONE_MODE,
    )
    .await;
    th.teardown().await;
}

/// The single user of single-user mode gets the same answers.
#[tokio::test]
async fn standalone_refusals_are_the_same_in_single_user_mode() {
    let th = Setup::single_user().start().await;

    for path in ["/teams/0/channels/some-channel", "/statistics"] {
        let response = th.client.get(path).await;
        response.check_not_implemented();
        assert_eq!(response.text(), NOT_IN_STANDALONE_MODE, "{path}");
    }
    let response = th.client.get("/admin/boards").await;
    response.check_unauthorized();
    assert_eq!(
        response.error_message(),
        "access denied Compliance Export getAllBoards"
    );
    th.teardown().await;
}

/// `TestGetBoardsForCompliance/"a non authenticated user should be rejected"` and
/// `"a user without manage_system permission should be rejected"`, as a standalone
/// server answers them: nobody has the permission there
/// (`server/api/compliance.go:73-78`).
#[tokio::test]
async fn get_boards_for_compliance_is_refused_in_standalone_mode() {
    let th = Setup::native().start().await.init_basic().await;

    for path in ["/admin/boards", "/admin/boards?team_id=nope&page=x"] {
        check_refusal(
            &th,
            path,
            StatusCode::UNAUTHORIZED,
            r#"{"error":"access denied Compliance Export getAllBoards","errorCode":401}"#,
        )
        .await;
    }
    th.teardown().await;
}

/// `TestGetBoardsComplianceHistory`, the same two cases on a standalone server. The
/// required parameter is checked before the permission (`server/api/compliance.go:203-213`).
#[tokio::test]
async fn get_boards_compliance_history_is_refused_in_standalone_mode() {
    let th = Setup::native().start().await.init_basic().await;

    check_refusal(
        &th,
        "/admin/boards_history",
        StatusCode::BAD_REQUEST,
        r#"{"error":"`modified_since` parameter required","errorCode":400}"#,
    )
    .await;
    check_refusal(
        &th,
        "/admin/boards_history?modified_since=1",
        StatusCode::UNAUTHORIZED,
        r#"{"error":"access denied Compliance Export getBoardsHistory","errorCode":401}"#,
    )
    .await;
    th.teardown().await;
}

/// `TestGetBlocksComplianceHistory`, the same two cases on a standalone server
/// (`server/api/compliance.go:352-362`).
#[tokio::test]
async fn get_blocks_compliance_history_is_refused_in_standalone_mode() {
    let th = Setup::native().start().await.init_basic().await;

    check_refusal(
        &th,
        "/admin/blocks_history",
        StatusCode::BAD_REQUEST,
        r#"{"error":"`modified_since` parameter required","errorCode":400}"#,
    )
    .await;
    check_refusal(
        &th,
        "/admin/blocks_history?modified_since=1&board_id=nope",
        StatusCode::UNAUTHORIZED,
        r#"{"error":"access denied Compliance Export getBlocksHistory","errorCode":401}"#,
    )
    .await;
    th.teardown().await;
}
