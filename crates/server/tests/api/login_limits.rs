//! Limits on login attempts: an address that keeps failing at an account waits, an
//! account that keeps being failed at is locked, an address makes only so many attempts
//! a minute, and the answer in each case is a 429 that says how long to wait.

use fvoci_model::RegisterRequest;
use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::common::{ApiResponse, Client, PASSWORD, Setup, TestServer, USER1_USERNAME};

fn credentials(username: &str, password: &str) -> serde_json::Value {
    json!({"username": username, "password": password})
}

async fn login(th: &TestServer, username: &str, password: &str) -> ApiResponse {
    th.anonymous()
        .post("/login", &credentials(username, password))
        .await
}

/// A login as it arrives through the reverse proxy the server trusts, from `address`.
async fn login_from(th: &TestServer, address: &str, username: &str, password: &str) -> ApiResponse {
    let request = th
        .anonymous()
        .request(Method::POST, "/login")
        .header("X-Forwarded-For", address)
        .json(&credentials(username, password));
    Client::send(request).await
}

/// Checks that the answer is the refusal of one attempt too many, and returns how many
/// seconds it says to wait: the same in the header and in the body.
#[track_caller]
fn retry_after(response: &ApiResponse) -> u64 {
    response.check_error(
        StatusCode::TOO_MANY_REQUESTS,
        "too many attempts; try again later",
    );
    let in_body = response.value()["retry_after"]
        .as_u64()
        .expect("the body says how long to wait");
    assert_eq!(
        response.header("retry-after"),
        in_body.to_string(),
        "the Retry-After header says the same"
    );
    assert!(in_body >= 1, "at least a second");
    in_body
}

/// After the failures an address has for free at an account, each further one makes it
/// wait, twice as long each time. The wait is the same for a wrong password and for an
/// account that does not exist, and a right password does not get through it either.
#[tokio::test]
async fn an_address_that_keeps_failing_at_an_account_is_made_to_wait() {
    let th = Setup::native()
        .configure(|config| {
            config.login_pair_free_failures = 2;
            config.login_pair_base_delay_seconds = 30;
        })
        .start()
        .await
        .init_basic()
        .await;

    for account in [USER1_USERNAME, "nobody-by-this-name"] {
        for attempt in 0..3 {
            let response = login(&th, account, "a wrong password").await;
            response.check_error(StatusCode::UNAUTHORIZED, "incorrect login");
            assert_eq!(
                response.error_reason(),
                "invalid_credentials",
                "{account}, attempt {attempt}: the same refusal whether the account exists"
            );
        }
        // Two were free; the third started the clock.
        let wait = retry_after(&login(&th, account, "a wrong password").await);
        assert!(
            (29..=30).contains(&wait),
            "{account}: about 30 s, was {wait}"
        );
        if account == USER1_USERNAME {
            let wait = retry_after(&login(&th, account, PASSWORD).await);
            assert!(wait <= 30, "the right password waits too: {wait}");
        }
    }
    // The name in other letters is the same account; another account is not held up.
    retry_after(&login(&th, &USER1_USERNAME.to_uppercase(), PASSWORD).await);
    login(&th, "user2", PASSWORD).await.check_no_content();
    // Signed in before the trouble: the session goes on.
    th.client.get("/users/me").await.check_ok();

    th.teardown().await;
}

/// Only who fails waits: the same account from another address is not held up, and a
/// success ends the count of the address that succeeded, not of the one that guesses.
/// Addresses are told apart behind a trusted proxy by what the proxy says; the header
/// of somebody who is no trusted proxy is not believed.
#[tokio::test]
async fn only_the_address_that_fails_waits() {
    let th = Setup::native()
        .configure(|config| {
            config.login_pair_free_failures = 1;
            config.trusted_proxies = vec!["127.0.0.1".to_owned()];
        })
        .start()
        .await
        .init_basic()
        .await;
    let guesser = "203.0.113.7";
    let owner = "198.51.100.1";

    for _ in 0..2 {
        login_from(&th, guesser, USER1_USERNAME, "a guess")
            .await
            .check_unauthorized();
    }
    retry_after(&login_from(&th, guesser, USER1_USERNAME, "a guess").await);

    login_from(&th, owner, USER1_USERNAME, PASSWORD)
        .await
        .check_no_content();
    retry_after(&login_from(&th, guesser, USER1_USERNAME, PASSWORD).await);
    // What the client writes at the left of the header is not what the proxy saw.
    retry_after(
        &login_from(
            &th,
            &format!("{owner}, {guesser}"),
            USER1_USERNAME,
            PASSWORD,
        )
        .await,
    );
    login_from(
        &th,
        &format!("{guesser}, {owner}"),
        USER1_USERNAME,
        PASSWORD,
    )
    .await
    .check_no_content();

    th.teardown().await;
}

/// Without a trusted proxy the header is nobody's to set: all attempts come from the
/// address of the connection, whatever they claim.
#[tokio::test]
async fn the_forwarded_address_is_believed_from_trusted_proxies_only() {
    let th = Setup::native()
        .configure(|config| config.login_pair_free_failures = 1)
        .start()
        .await
        .init_basic()
        .await;

    for claimed in ["203.0.113.1", "203.0.113.2"] {
        login_from(&th, claimed, USER1_USERNAME, "a guess")
            .await
            .check_unauthorized();
    }
    retry_after(&login_from(&th, "203.0.113.3", USER1_USERNAME, "a guess").await);

    th.teardown().await;
}

/// Failures in a row from anywhere lock the account for a while, for everybody: that
/// is what stops guessing from many addresses. Other accounts are not touched.
#[tokio::test]
async fn an_account_is_locked_after_too_many_failures_from_anywhere() {
    let th = Setup::native()
        .configure(|config| {
            config.login_account_max_failures = 4;
            config.login_account_lock_seconds = 600;
            config.trusted_proxies = vec!["127.0.0.1".to_owned()];
        })
        .start()
        .await
        .init_basic()
        .await;

    // Four addresses, one failure each: none of them has anything to wait for.
    for host in 1..=4 {
        login_from(&th, &format!("203.0.113.{host}"), USER1_USERNAME, "a guess")
            .await
            .check_unauthorized();
    }

    let wait = retry_after(&login_from(&th, "198.51.100.1", USER1_USERNAME, PASSWORD).await);
    assert!(
        (599..=600).contains(&wait),
        "locked for ten minutes: {wait}"
    );
    login_from(&th, "198.51.100.1", "user2", PASSWORD)
        .await
        .check_no_content();

    th.teardown().await;
}

/// An address makes only so many attempts a minute, successes and registrations
/// included, whatever the accounts: that is what stops one password tried on many
/// accounts.
#[tokio::test]
async fn an_address_makes_only_so_many_attempts_a_minute() {
    let th = Setup::native()
        .configure(|config| config.login_address_attempts_per_minute = 4)
        .start()
        .await;
    let register = |username: &str| RegisterRequest {
        username: username.to_owned(),
        email: format!("{username}@sample.com"),
        password: PASSWORD.to_owned(),
        token: String::new(),
    };

    // A registration, a success, and two failures at accounts that do not exist.
    th.anonymous()
        .post("/register", &register("first"))
        .await
        .check_created();
    login(&th, "first", PASSWORD).await.check_no_content();
    login(&th, "one", "password1").await.check_unauthorized();
    login(&th, "two", "password1").await.check_unauthorized();

    let wait = retry_after(&login(&th, "three", "password1").await);
    assert!(wait <= 60, "room again within the minute: {wait}");
    // The right password and a registration are attempts like any other.
    retry_after(&login(&th, "first", PASSWORD).await);
    let signup_token = th
        .app
        .root_team()
        .await
        .expect("the root team exists")
        .signup_token;
    let late = RegisterRequest {
        token: signup_token,
        ..register("late")
    };
    retry_after(&th.anonymous().post("/register", &late).await);
    // A registration that is refused for what it says costs no hash and is no attempt:
    // it is answered as what it is.
    th.anonymous()
        .post("/register", &register("without-a-token"))
        .await
        .check_error(StatusCode::FORBIDDEN, "registration needs the signup token");
    // What needs no password is not an attempt.
    th.anonymous().get("/client-config").await.check_ok();

    th.teardown().await;
}

/// The queue of password work is bounded: what does not fit is told to come back, at
/// once, without a hash being spent on it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn logins_beyond_the_queue_of_password_work_are_told_to_come_back() {
    let th = Setup::native()
        .configure(|config| {
            config.password_hash_concurrency = 1;
            config.password_hash_queue = 1;
            config.password_hash_queue_seconds = 2;
        })
        .start()
        .await
        .init_basic()
        .await;

    // Many at once: one is hashed, one waits, the rest are refused or wait their turn
    // in vain. None fails otherwise, and the server is as it was afterwards.
    let attempts = (0..24).map(|index| {
        let client = th.anonymous();
        async move {
            client
                .post(
                    "/login",
                    &credentials(&format!("nobody-{index}"), "a wrong password"),
                )
                .await
        }
    });
    let answers = futures_util::future::join_all(attempts).await;

    let refused: Vec<&ApiResponse> = answers
        .iter()
        .filter(|answer| answer.status == StatusCode::TOO_MANY_REQUESTS)
        .collect();
    assert!(
        !refused.is_empty(),
        "24 logins at once do not all fit a queue of one"
    );
    for answer in &refused {
        assert_eq!(retry_after(answer), 2, "come back when the queue has moved");
    }
    for answer in &answers {
        assert!(
            matches!(
                answer.status,
                StatusCode::UNAUTHORIZED | StatusCode::TOO_MANY_REQUESTS
            ),
            "refused as a wrong login or as one too many: {}",
            answer.text()
        );
    }
    login(&th, USER1_USERNAME, PASSWORD)
        .await
        .check_no_content();

    th.teardown().await;
}
