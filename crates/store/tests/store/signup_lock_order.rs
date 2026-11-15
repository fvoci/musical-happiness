//! A registration that replaces the one of its browser, and an acceptance that does,
//! each hold two links: whichever goes first holds both before the other holds any
//! (`crates/store/src/link_locks.rs`). On PostgreSQL the test stops the registration
//! once it holds its first link, starts the acceptance, and lets the registration go
//! once the acceptance waits for it; on SQLite, whose writers take turns, the two run
//! one after the other.

use fvoci_model::new_id;
use fvoci_store::{Acceptor, DbType, SIGNUP_LIFETIME_MILLIS, Signup, SignupStart, Store};

use crate::common::{self, TestStore};
use crate::invitations::{MakerFound, NOW, link, newcomer};
use crate::lock_gate::{Gate, ended};

/// When the registration again and the acceptance happen: after the registration that
/// ends first has ended.
const AFTER: i64 = NOW + 10;

/// A registration of `username` from the browser whose marker has the digest
/// `browser`, waiting until `expires_at`.
fn registration(username: &str, browser: &str, expires_at: i64) -> Signup {
    Signup {
        id: new_id(),
        username: username.to_owned(),
        email: format!("{username}@example.test"),
        display_name: String::new(),
        password_hash: "hash".to_owned(),
        language: String::new(),
        start: SignupStart::Organization("Ours".to_owned()),
        browser_hash: browser.to_owned(),
        create_at: NOW,
        expires_at,
    }
}

/// Registers by the link of `token_hash`, as the server does: its use is taken, and
/// the registration's start is the link.
async fn register_by(store: &Store, token_hash: &str, signup: &Signup) {
    let maker = MakerFound::of(store, token_hash, NOW)
        .await
        .expect("the maker is found");
    let reserved = store
        .reserve_invitation(token_hash, signup, None, NOW, maker.inviter())
        .await
        .expect("registered");
    assert!(reserved.accepted().is_some(), "the link lets them register");
}

/// How often the link of `token_hash` counts as used.
async fn uses_of(test: &TestStore, token_hash: &str) -> i64 {
    test.unended_invitation(token_hash, AFTER)
        .await
        .expect("the link is read")
        .expect("the link")
        .uses
}

/// The registration from the browser `one` replaces its own, made by the second link,
/// after the clean-up before it removes one by the first link that ended; the
/// acceptance of the second link from the browser `two` replaces that browser's
/// registration, made by the first link. Each holds one of the links and then the
/// other, in the other order.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_registration_again_and_an_acceptance_again_take_turns() {
    let test = common::setup().await;
    let admin = common::user(&test, "admin").await;
    link(&test, &admin, "first", None, None).await;
    link(&test, &admin, "second", None, None).await;
    let waiting = NOW + SIGNUP_LIFETIME_MILLIS;
    register_by(&test, "first", &registration("ended", "gone", NOW + 1)).await;
    register_by(&test, "first", &registration("tess", "two", waiting)).await;
    register_by(&test, "second", &registration("olga", "one", waiting)).await;
    let again = registration("olga", "one", AFTER + SIGNUP_LIFETIME_MILLIS);
    let maker = MakerFound::of(&test, "second", AFTER)
        .await
        .expect("the maker is found");
    let mut account = newcomer("tess");

    if common::db_type() == DbType::Postgres {
        let mut gate = Gate::before(&test, "DELETE", "signups", "OLD.browser_hash = 'gone'").await;
        let acceptor = gate.second_store(&test).await;
        let registrar = test.store.clone();
        let registering =
            tokio::spawn(async move { registrar.create_signup(&again, Some("one"), AFTER).await });
        let holding = gate.stopped(&registering).await;
        let accepting = tokio::spawn(async move {
            let replacing = Acceptor::Replacing {
                user: &mut account,
                browser: "two",
            };
            acceptor
                .accept_invitation("second", replacing, AFTER, maker.inviter())
                .await
        });
        gate.waits_for(holding, &accepting).await;
        gate.open().await;
        ended(registering).await.expect("registered again");
        let accepted = ended(accepting).await.expect("accepted");
        assert!(accepted.accepted().is_some(), "let in");
    } else {
        test.create_signup(&again, Some("one"), AFTER)
            .await
            .expect("registered again");
        let replacing = Acceptor::Replacing {
            user: &mut account,
            browser: "two",
        };
        let accepted = test
            .accept_invitation("second", replacing, AFTER, maker.inviter())
            .await
            .expect("accepted");
        assert!(accepted.accepted().is_some(), "let in");
    }
    assert_eq!(
        uses_of(&test, "first").await,
        0,
        "both its registrations went"
    );
    assert_eq!(
        uses_of(&test, "second").await,
        1,
        "its registration went, and its acceptance counts"
    );
    test.teardown().await;
}
