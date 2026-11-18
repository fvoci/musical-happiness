//! Registrations whose address is not confirmed yet hold their name and address against
//! accounts, and accounts against them, also when both are made at once.

use std::sync::Arc;

use fvoci_model::{User, new_id};
use fvoci_store::{
    SIGNUP_LIFETIME_MILLIS, Signup, SignupLookup, SignupStart, Store, StoreError, Taken,
};
use tokio::sync::Barrier;

use crate::common;

fn signup(username: &str, email: &str, now: i64) -> Signup {
    Signup {
        id: new_id(),
        username: username.to_owned(),
        email: email.to_owned(),
        display_name: String::new(),
        password_hash: "hash".to_owned(),
        language: String::new(),
        start: SignupStart::Organization("Ours".to_owned()),
        browser_hash: String::new(),
        create_at: now,
        expires_at: now + SIGNUP_LIFETIME_MILLIS,
    }
}

fn account(username: &str, email: &str) -> User {
    User {
        id: new_id(),
        username: username.to_owned(),
        email: email.to_owned(),
        password_hash: "hash".to_owned(),
        ..User::default()
    }
}

fn taken<T: std::fmt::Debug>(result: Result<T, StoreError>) -> Taken {
    match result {
        Err(StoreError::Conflict(taken)) => taken,
        other => panic!("a conflict, not {other:?}"),
    }
}

#[tokio::test]
async fn a_registration_and_an_account_hold_their_names_against_each_other() {
    let test = common::setup().await;
    let store = &test.store;
    let now = now_millis();
    store
        .create_signup(&signup("Pia", "pia@example.test", now), None, now)
        .await
        .expect("stored");
    assert_eq!(
        taken(
            store
                .create_user(&mut account("PIA", "other@example.test"))
                .await
        ),
        Taken::Username
    );
    assert_eq!(
        taken(
            store
                .create_user(&mut account("other", "PIA@example.test"))
                .await
        ),
        Taken::Email
    );
    assert_eq!(
        taken(
            store
                .create_signup(&signup("pia", "x@example.test", now), None, now)
                .await
        ),
        Taken::Username,
        "one registration a name"
    );
    common::user(store, "quinn").await;
    let quinns = store
        .get_user_by_username("quinn")
        .await
        .expect("the account");
    assert_eq!(
        taken(
            store
                .create_signup(&signup("QUINN", "q@example.test", now), None, now)
                .await
        ),
        Taken::Username
    );
    assert_eq!(
        taken(
            store
                .create_signup(&signup("q", &quinns.email.to_uppercase(), now), None, now)
                .await
        ),
        Taken::Email
    );

    // Ended, it holds nothing: an account takes its name, and it is removed.
    let later = now + SIGNUP_LIFETIME_MILLIS + 1;
    assert!(
        store
            .live_signup(SignupLookup::Username("pia"), later)
            .await
            .expect("read")
            .is_none()
    );
    assert_eq!(store.remove_ended_signups(later).await.expect("removed"), 1);
    store
        .create_user(&mut account("pia", "pia@example.test"))
        .await
        .expect("the name is free");
    test.teardown().await;
}

/// An account and a registration of one name made at once: one of them is.
#[tokio::test]
async fn of_an_account_and_a_registration_of_one_name_at_once_one_is_made() {
    let test = common::setup().await;
    let store = &test.store;
    for round in 0..8 {
        let name = format!("race{round}");
        let email = format!("{name}@example.test");
        let barrier = Arc::new(Barrier::new(2));
        let (made_account, made_signup) = tokio::join!(
            make_account(store, &barrier, &name, &email),
            make_signup(store, &barrier, &name, &email),
        );
        assert!(
            made_account != made_signup,
            "round {round}: one of the two, not {made_account} and {made_signup}"
        );
    }
    test.teardown().await;
}

async fn make_account(store: &Store, barrier: &Barrier, name: &str, email: &str) -> bool {
    barrier.wait().await;
    match store.create_user(&mut account(name, email)).await {
        Ok(()) => true,
        Err(StoreError::Conflict(_)) => false,
        Err(error) => panic!("the account: {error}"),
    }
}

async fn make_signup(store: &Store, barrier: &Barrier, name: &str, email: &str) -> bool {
    barrier.wait().await;
    let now = now_millis();
    match store
        .create_signup(&signup(name, email, now), None, now)
        .await
    {
        Ok(()) => true,
        Err(StoreError::Conflict(_)) => false,
        Err(error) => panic!("the registration: {error}"),
    }
}

fn now_millis() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_millis(),
    )
    .expect("a time in range")
}

/// The migration of the registrations makes `audit_log` again on SQLite, and changes its
/// checks on PostgreSQL: the records it finds keep every column, the server's own and
/// those of a provider's move with their issuer and identity, and the operator's new
/// action is taken.
#[tokio::test]
async fn the_migration_of_the_registrations_keeps_every_record_of_the_audit_log() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_signups(&database).await;
    let (org, lapsed, carried, actor) = (new_id(), new_id(), new_id(), new_id());
    common::execute_raw(
        &common::raw_connection(&database),
        &format!(
            "INSERT INTO organizations (id, name, settings, create_at, update_at) \
             VALUES ('{org}', 'Org', '{{}}', 1, 1); \
             INSERT INTO audit_log (id, org_id, actor_kind, action, target_id, create_at, detail) \
             VALUES ('{lapsed}', '{org}', 'server', 'oidc_former_provider_lapsed', '{org}', 7, \
             'https://old.example.test'); \
             INSERT INTO audit_log (id, org_id, actor_id, action, target_id, subject_id, \
             create_at, detail, identity) VALUES ('{carried}', '{org}', '{actor}', \
             'oidc_identity_carried_over', '{org}', '{actor}', 8, 'https://new.example.test', \
             'who@example.test');"
        ),
    )
    .await;
    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrated");
    let ours = store.for_org(&org).expect("organization");
    let records = ours
        .audit_records(
            fvoci_store::AuditPlace::Organization,
            fvoci_store::Paging {
                page: 0,
                per_page: 100,
            },
        )
        .await
        .expect("records");
    let of = |id: &str| {
        records
            .iter()
            .find(|record| record.id == id)
            .unwrap_or_else(|| panic!("the record {id} is kept: {records:?}"))
    };
    let lapsed = of(&lapsed);
    assert_eq!(
        lapsed.actor,
        fvoci_model::AuditRecordActor::Server,
        "{lapsed:?}"
    );
    assert_eq!(
        lapsed.action,
        fvoci_model::AuditAction::OidcFormerProviderLapsed
    );
    assert_eq!(lapsed.detail.as_deref(), Some("https://old.example.test"));
    let carried = of(&carried);
    assert_eq!(
        carried.action,
        fvoci_model::AuditAction::OidcIdentityCarriedOver
    );
    assert_eq!(carried.detail.as_deref(), Some("https://new.example.test"));
    assert_eq!(carried.identity.as_deref(), Some("who@example.test"));
    assert_eq!(carried.create_at, 8, "{carried:?}");

    // The operator's confirmation is a record the log takes now.
    common::execute_raw(
        &common::raw_connection(&database),
        &format!(
            "INSERT INTO audit_log (id, org_id, actor_kind, action, target_id, subject_id, \
             create_at) VALUES ('{}', '{org}', 'server_operator', 'email_verified_by_operator', \
             '{org}', '{actor}', 9);",
            new_id()
        ),
    )
    .await;
    store.close().await;
    database.remove().await;
}
