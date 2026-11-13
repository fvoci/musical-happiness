//! Where a session made through a provider came from, and the endings that follow it:
//! the provider removed, its issuer changed, the identity unlinked; and the sessions
//! of before migration 0028.

use fvoci_model::{LoginOrigin, Session, User, new_id};
use fvoci_store::{Identity, Removal, Store, StoredProvider, Unlinked};

use crate::common;

/// The time the sessions are made at, in milliseconds.
const T0: i64 = 1_800_000_000_000;

/// The password hash of the tests' people: one they can unlink an identity with.
const HASH: &str = "a password hash";

/// Stores a person with a password and answers their identifier.
async fn person(store: &Store, username: &str) -> String {
    let mut user = User {
        id: new_id(),
        username: username.to_owned(),
        email: format!("{username}@example.com"),
        password_hash: HASH.to_owned(),
        ..User::default()
    };
    store.create_user(&mut user).await.expect("stored");
    user.id
}

fn provider(issuer: &str) -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: issuer.to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: String::new(),
        auto_join: true,
        enabled: true,
        sso_required: false,
        create_at: 0,
        update_at: 0,
    }
}

/// Stores a session of the user, made through `login` or with a password, and answers
/// the digest it is found by.
async fn open(store: &Store, user_id: &str, login: Option<LoginOrigin>) -> String {
    let digest = new_id();
    let session = Session {
        id: new_id(),
        user_id: user_id.to_owned(),
        created_at: T0,
        last_used_at: T0,
        expires_at: T0 + 1000,
        user_agent: "a browser".to_owned(),
        login,
    };
    assert!(
        store
            .create_session(&session, &digest, HASH)
            .await
            .expect("stored"),
        "the user's password is as read"
    );
    digest
}

async fn alive(store: &Store, digest: &str) -> bool {
    store.get_session(digest, T0, 0).await.is_ok()
}

fn origin(provider: &StoredProvider) -> LoginOrigin {
    LoginOrigin {
        provider_id: provider.id.clone(),
        org_id: provider.org_id.clone(),
        issuer: provider.issuer.clone(),
    }
}

/// A session reads back with the origin it was stored with, and one of a password
/// with none.
#[tokio::test]
async fn a_session_keeps_its_origin() {
    let test = common::setup().await;
    let user = person(&test, "someone").await;
    let stored = test
        .put_oidc_provider_as_read(&provider("https://one.example"), None, &user)
        .await
        .expect("stored")
        .expect("new");
    let through = open(&test, &user, Some(origin(&stored))).await;
    let password = open(&test, &user, None).await;
    let read = test.get_session(&through, T0, 0).await.expect("read");
    let read_password = test.get_session(&password, T0, 0).await.expect("read");
    test.teardown().await;
    assert_eq!(read.login, Some(origin(&stored)));
    assert_eq!(read_password.login, None);
}

/// Removing the provider ends the sessions made through it, of every person; a new
/// issuer ends those of the issuer before; a change that keeps the issuer, and
/// sessions of a password or of another organization's provider of the same issuer,
/// stay.
#[tokio::test]
async fn the_providers_sessions_end_with_its_removal_or_its_issuer() {
    let test = common::setup().await;
    let first = person(&test, "first").await;
    let second = person(&test, "second").await;
    let one = test
        .put_oidc_provider_as_read(&provider("https://one.example"), None, &first)
        .await
        .expect("stored")
        .expect("new");
    let mut elsewhere = origin(&one);
    elsewhere.org_id = new_id();
    let firsts = open(&test, &first, Some(origin(&one))).await;
    let seconds = open(&test, &second, Some(origin(&one))).await;
    let password = open(&test, &first, None).await;
    let of_another_organization = open(&test, &first, Some(elsewhere)).await;

    let mut same_issuer = provider("https://one.example");
    same_issuer.auto_join = false;
    let kept = test
        .put_oidc_provider_as_read(&same_issuer, Some(&one), &first)
        .await
        .expect("stored")
        .expect("as read");
    let after_the_same_issuer = (alive(&test, &firsts).await, alive(&test, &seconds).await);
    let two = test
        .put_oidc_provider_as_read(&provider("https://two.example"), Some(&kept), &first)
        .await
        .expect("stored")
        .expect("as read");
    let after_another_issuer = (alive(&test, &firsts).await, alive(&test, &seconds).await);
    let of_two = open(&test, &second, Some(origin(&two))).await;
    let removal = test.delete_oidc_provider(&first).await.expect("removed");
    let of_two_after = alive(&test, &of_two).await;
    let others = (
        alive(&test, &password).await,
        alive(&test, &of_another_organization).await,
    );
    test.teardown().await;

    assert_eq!(after_the_same_issuer, (true, true), "the issuer stayed");
    assert_eq!(after_another_issuer, (false, false), "everybody's ended");
    assert_eq!(
        removal,
        Removal::Removed {
            issuer: "https://two.example".to_owned()
        }
    );
    assert!(!of_two_after, "ended with the removal");
    assert_eq!(
        others,
        (true, true),
        "a password's and another organization's"
    );
}

/// Unlinking an identity ends the user's sessions made through a provider of its
/// issuer, and only those.
#[tokio::test]
async fn unlinking_ends_the_users_sessions_of_that_issuer() {
    let test = common::setup().await;
    let user = person(&test, "linked").await;
    let other = person(&test, "other").await;
    let stored = test
        .put_oidc_provider_as_read(&provider("https://one.example"), None, &user)
        .await
        .expect("stored")
        .expect("new");
    test.add_identity(
        &user,
        Identity {
            issuer: "https://one.example",
            subject: "subject",
        },
        "",
    )
    .await
    .expect("tied");
    let mut of_another_issuer = origin(&stored);
    of_another_issuer.issuer = "https://two.example".to_owned();
    let through = open(&test, &user, Some(origin(&stored))).await;
    let password = open(&test, &user, None).await;
    let another_issuer = open(&test, &user, Some(of_another_issuer)).await;
    let someone_elses = open(&test, &other, Some(origin(&stored))).await;
    let unlinked = test
        .unlink_identity(
            &user,
            HASH,
            Identity {
                issuer: "https://one.example",
                subject: "subject",
            },
        )
        .await
        .expect("asked");
    let alive_after = [
        alive(&test, &through).await,
        alive(&test, &password).await,
        alive(&test, &another_issuer).await,
        alive(&test, &someone_elses).await,
    ];
    test.teardown().await;
    assert_eq!(unlinked, Unlinked::Removed);
    assert_eq!(
        alive_after,
        [false, true, true, true],
        "only the user's session of that issuer ends"
    );
}

/// Migration 0028: a session made through a provider that is still there takes its
/// organization and issuer; one whose provider is gone has no origin to answer for,
/// and ends; a password's stays as it was.
#[tokio::test]
async fn the_migration_fills_the_origin_or_ends_the_session() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_session_origin(&database).await;
    let (org, user, provider) = (new_id(), new_id(), new_id());
    let [present, gone, password] = [new_id(), new_id(), new_id()];
    let session = |id: &str, digest: &str, provider_id: &str| {
        format!(
            "INSERT INTO sessions (id, token_hash, user_id, created_at, last_used_at, \
             expires_at, user_agent, login_provider_id) \
             VALUES ('{id}', '{digest}', '{user}', {T0}, {T0}, {}, 'a browser', {provider_id});",
            T0 + 1000
        )
    };
    common::execute_raw(
        &common::raw_dbconfig(&database),
        &[
            format!(
                "INSERT INTO organizations (id, name, settings, create_at, update_at) \
                 VALUES ('{org}', 'Org', '{{}}', 1, 1);"
            ),
            format!(
                "INSERT INTO users (id, username, email, password_hash, create_at, update_at) \
                 VALUES ('{user}', 'someone', 'someone@example.com', '', 1, 1);"
            ),
            format!(
                "INSERT INTO oidc_providers (id, org_id, issuer, client_id, client_secret, \
                 create_at, update_at) \
                 VALUES ('{provider}', '{org}', 'https://one.example', 'client', '', 1, 1);"
            ),
            session(&present, "digest-present", &format!("'{provider}'")),
            session(&gone, "digest-gone", &format!("'{}'", new_id())),
            session(&password, "digest-password", "NULL"),
        ]
        .concat(),
    )
    .await;
    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("migrated");
    let present = store.get_session("digest-present", T0, 0).await;
    let gone = store.get_session("digest-gone", T0, 0).await;
    let password = store.get_session("digest-password", T0, 0).await;
    store.close().await;
    database.remove().await;

    assert_eq!(
        present.expect("kept").login,
        Some(LoginOrigin {
            provider_id: provider,
            org_id: org,
            issuer: "https://one.example".to_owned(),
        })
    );
    assert!(gone.is_err(), "its provider is gone: ended");
    assert_eq!(password.expect("kept").login, None);
}
