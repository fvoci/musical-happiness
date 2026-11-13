//! Review of password recovery: what a reset leaves of the limits on signing in.

use reqwest::StatusCode;
use serde_json::json;

use crate::common::{PASSWORD, Setup};

/// Who proves the mailbox by a reset link chose the only password that now opens the
/// account, yet the lock that others' wrong guesses put on the old one still refuses it.
#[tokio::test]
async fn a_reset_password_signs_in_although_others_locked_the_account() {
    let th = Setup::native()
        .configure(|config| config.login_account_max_failures = 2)
        .start()
        .await
        .init_basic()
        .await;
    for _ in 0..2 {
        th.anonymous()
            .login("user1", "not-the-password")
            .await
            .check_status(StatusCode::UNAUTHORIZED);
    }
    th.anonymous()
        .login("user1", PASSWORD)
        .await
        .check_status(StatusCode::TOO_MANY_REQUESTS);

    let url = th
        .app
        .password_reset_link("user1")
        .await
        .expect("link issued");
    let token = url.split_once('#').expect("a fragment").1;
    th.anonymous()
        .post(
            "/password-reset/complete",
            &json!({"token": token, "password": "New-password-1"}),
        )
        .await
        .check_no_content();

    th.anonymous()
        .login("user1", "New-password-1")
        .await
        .check_no_content();
    th.teardown().await;
}
