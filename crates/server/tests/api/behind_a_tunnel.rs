//! Requests as a tunnel such as cloudflared brings them: from a proxy the server trusts,
//! with the person's address at the right of `X-Forwarded-For` and in
//! `CF-Connecting-IP`. The limits of sign-up, login and password reset count each person
//! by the address the proxy added, never by the proxy's own; the header the server reads
//! is `X-Forwarded-For` alone, and from no peer but a trusted proxy.

use fvoci_model::RegisterRequest;
use fvoci_server::config::Configuration;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, PASSWORD, Setup, TestServer};
use crate::fake_smtp::FakeSmtp;

/// The headers of one request: what `X-Forwarded-For` and `CF-Connecting-IP` say.
#[derive(Debug, Clone, Copy)]
struct Headers<'a> {
    forwarded_for: &'a str,
    connecting_ip: &'a str,
}

/// A person's request as the tunnel brings it.
fn of(address: &str) -> Headers<'_> {
    Headers {
        forwarded_for: address,
        connecting_ip: address,
    }
}

async fn post(th: &TestServer, path: &str, body: &Value, headers: Headers<'_>) -> ApiResponse {
    let request = th
        .anonymous()
        .request(Method::POST, path)
        .header("X-Forwarded-For", headers.forwarded_for)
        .header("CF-Connecting-IP", headers.connecting_ip)
        .json(body);
    Client::send(request).await
}

fn registration(username: &str) -> Value {
    serde_json::to_value(RegisterRequest {
        username: username.to_owned(),
        email: format!("{username}@sample.com"),
        password: PASSWORD.to_owned(),
        display_name: String::new(),
        organization_name: format!("{username}'s firm"),
        language: String::new(),
    })
    .expect("a registration is JSON")
}

fn login(username: &str) -> Value {
    json!({"username": username, "password": PASSWORD})
}

fn reset(username: &str) -> Value {
    json!({"email": format!("{username}@sample.com"), "language": "en"})
}

const REGISTER: &str = "/register";
const LOGIN: &str = "/login";
const RESET: &str = "/password-reset/request";

/// A server that sends mail (a reset needs it), with what `trust` sets: an address may
/// make two attempts a minute, and the proxies it trusts.
async fn server(smtp: &FakeSmtp, trust: fn(&mut Configuration)) -> TestServer {
    Setup::native()
        .smtp(smtp.settings())
        .configure(trust)
        .start()
        .await
}

/// Two people behind the same tunnel: one who has used up their attempts waits, the
/// other does not, and what the first writes into the headers does not change that.
#[tokio::test]
async fn behind_a_trusted_tunnel_each_person_has_their_own_limits() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = server(&smtp, |config| {
        config.login_address_attempts_per_minute = 2;
        config.trusted_proxies = vec!["127.0.0.1".to_owned(), "::1".to_owned()];
    })
    .await;
    let alice = "203.0.113.10";
    let bob = "198.51.100.20";

    post(&th, REGISTER, &registration("alice"), of(alice))
        .await
        .check_created();
    post(&th, LOGIN, &login("alice"), of(alice))
        .await
        .check_no_content();
    post(&th, LOGIN, &login("alice"), of(alice))
        .await
        .check_status(StatusCode::TOO_MANY_REQUESTS);

    // The server holds one organization (SQLite): the second person signs in to the
    // first one's account, which their own address's count allows twice.
    for _ in 0..2 {
        post(&th, LOGIN, &login("alice"), of(bob))
            .await
            .check_no_content();
    }

    // What a person writes at the left of X-Forwarded-For stays there: the tunnel adds
    // the address it saw at the right. CF-Connecting-IP is not read at all. The address
    // claimed is one with attempts left, which a server believing it would let through.
    let claimed = "192.0.2.77";
    let forged = format!("{claimed}, {alice}");
    for headers in [
        Headers {
            forwarded_for: &forged,
            connecting_ip: claimed,
        },
        Headers {
            forwarded_for: alice,
            connecting_ip: claimed,
        },
    ] {
        post(&th, LOGIN, &login("alice"), headers)
            .await
            .check_status(StatusCode::TOO_MANY_REQUESTS);
    }
    post(&th, LOGIN, &login("alice"), of(claimed))
        .await
        .check_no_content();

    // A reset has a count of its own, by the same address.
    for _ in 0..2 {
        post(&th, RESET, &reset("alice"), of(alice))
            .await
            .check_no_content();
    }
    post(&th, RESET, &reset("alice"), of(alice))
        .await
        .check_status(StatusCode::TOO_MANY_REQUESTS);
    post(&th, RESET, &reset("alice"), of(bob))
        .await
        .check_no_content();

    th.teardown().await;
    smtp.stop().await;
}

/// A server that trusts some proxy, but not the peer the requests come from: whatever
/// the headers say, every request is the peer's, and the peer's attempts run out.
#[tokio::test]
async fn from_a_peer_that_is_no_trusted_proxy_the_headers_are_not_believed() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = server(&smtp, |config| {
        config.login_address_attempts_per_minute = 2;
        config.trusted_proxies = vec!["10.0.0.0/8".to_owned()];
    })
    .await;

    post(&th, REGISTER, &registration("carol"), of("203.0.113.1"))
        .await
        .check_created();
    post(&th, LOGIN, &login("carol"), of("203.0.113.2"))
        .await
        .check_no_content();
    post(&th, LOGIN, &login("carol"), of("203.0.113.3"))
        .await
        .check_status(StatusCode::TOO_MANY_REQUESTS);

    for address in ["203.0.113.4", "203.0.113.5"] {
        post(&th, RESET, &reset("carol"), of(address))
            .await
            .check_no_content();
    }
    post(&th, RESET, &reset("carol"), of("203.0.113.6"))
        .await
        .check_status(StatusCode::TOO_MANY_REQUESTS);

    th.teardown().await;
    smtp.stop().await;
}
