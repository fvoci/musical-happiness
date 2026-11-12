//! Signing in through a provider: two first sign-ins at once of an account already
//! known to the issuer.

use std::time::Duration;

use fvoci_model::{Session, new_id};
use fvoci_store::{DbType, OidcAccount, OidcRefusal, Removal, Store, StoredProvider, Tenant};
use sqlx::Connection as _;

use crate::common;

/// One sign-in through the provider ending in the account: what `App::let_in` stores.
fn join_task(
    store: Store,
    provider_id: String,
    user_id: String,
    password_hash: String,
) -> tokio::task::JoinHandle<Result<Result<bool, OidcRefusal>, fvoci_store::StoreError>> {
    tokio::spawn(async move {
        let mut session = Session {
            id: new_id(),
            created_at: 1,
            expires_at: i64::MAX,
            ..Session::default()
        };
        let account = OidcAccount::Known {
            user_id: &user_id,
            password_hash: &password_hash,
        };
        store
            .finish_oidc_sign_in(&provider_id, account, &mut session, &new_id())
            .await
    })
}

/// The end of a sign-in (`Store::finish_oidc_sign_in`) when an identity names an
/// existing account that is not yet in the organization. Two returns may both get that
/// answer. A PostgreSQL SHARE table lock lets their membership reads finish while both
/// inserts wait, making the order independent of sleeps or the speed of the machine.
#[tokio::test]
async fn concurrent_oidc_joins_of_one_account_both_succeed() {
    let test = common::setup().await;
    let account = common::user(&test.scoped(Tenant::Unset), "existing-identity").await;
    let password_hash = test
        .get_user_by_id(&account)
        .await
        .expect("the account")
        .password_hash;
    let provider = test
        .put_oidc_provider(&StoredProvider {
            id: String::new(),
            org_id: String::new(),
            issuer: "https://idp.example.com".to_owned(),
            client_id: "fvoci".to_owned(),
            sealed_client_secret: String::new(),
            auto_join: true,
            enabled: true,
            create_at: 0,
            update_at: 0,
            sso_required: false,
        })
        .await
        .expect("a provider")
        .id;
    let join = || {
        join_task(
            test.store.clone(),
            provider.clone(),
            account.clone(),
            password_hash.clone(),
        )
    };
    assert!(
        !test
            .is_organization_member(&account)
            .await
            .expect("membership read"),
        "the account has not joined this organization",
    );

    let tasks = if common::db_type() == DbType::Postgres {
        let mut connection = common::connect_past_the_store(&test.dbconfig()).await;
        let mut held = connection.begin().await.expect("a test transaction");
        sqlx::query("LOCK TABLE organization_members IN SHARE MODE")
            .execute(&mut *held)
            .await
            .expect("membership inserts wait");
        let tasks = (join(), join());
        let waiting = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM pg_locks WHERE relation = 'organization_members'::regclass \
                     AND mode = 'RowExclusiveLock' AND NOT granted",
                ).fetch_one(&mut *held).await.expect("the waiting inserts are observed");
                if count >= 2 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        held.commit()
            .await
            .expect("both membership inserts may proceed");
        connection
            .close()
            .await
            .expect("the test connection closes");
        assert!(
            waiting.is_ok(),
            "both callbacks reached their first membership insert"
        );
        tasks
    } else {
        (join(), join())
    };
    let (first, second) = tokio::join!(tasks.0, tasks.1);
    let first = first.expect("the first callback task completes");
    let second = second.expect("the second callback task completes");
    let rows = test
        .count_rows("organization_members")
        .await
        .expect("memberships counted");
    let sessions = test.count_rows("sessions").await.expect("sessions counted");
    test.teardown().await;

    assert!(
        matches!(first, Ok(Ok(_))) && matches!(second, Ok(Ok(_))),
        "both valid callbacks must succeed: first={first:?}, second={second:?}"
    );
    assert_ne!(
        first.expect("the first result"),
        second.expect("the second result"),
        "exactly one call newly joins the account"
    );
    assert_eq!(rows, 1, "one organization membership");
    assert_eq!(sessions, 2, "a session for each sign-in");
}

/// A change of the provider decided from what was read is stored only if the provider
/// is still as it was read: another change of the same provider meanwhile (its secret,
/// within the same millisecond as often as not) makes it read again. Without one, it is
/// stored.
#[tokio::test]
async fn a_change_of_the_provider_made_from_an_old_reading_is_not_stored() {
    let test = common::setup().await;
    let provider = |client_id: &str, secret: &str| StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: "https://idp.example.com".to_owned(),
        client_id: client_id.to_owned(),
        sealed_client_secret: secret.to_owned(),
        auto_join: true,
        enabled: true,
        create_at: 0,
        update_at: 0,
        sso_required: false,
    };
    let none_read = test
        .put_oidc_provider_as_read(&provider("first", ""), None)
        .await
        .expect("stored")
        .expect("none was there, as read");
    // Read by one request; changed by another meanwhile, within the same millisecond
    // as often as not: the time of the change is later all the same.
    let later = test
        .put_oidc_provider(&provider("first", "a secret"))
        .await
        .expect("changed");
    assert_eq!(later.id, none_read.id, "the same provider");
    assert!(
        later.update_at > none_read.update_at,
        "{} after {}",
        later.update_at,
        none_read.update_at
    );
    let stale = test
        .put_oidc_provider_as_read(&provider("first", ""), Some(&none_read))
        .await
        .expect("asked");
    let stored = test.oidc_provider().await.expect("read").expect("stored");
    let fresh = test
        .put_oidc_provider_as_read(&provider("third", ""), Some(&later))
        .await
        .expect("asked");
    let none_stale = test
        .put_oidc_provider_as_read(&provider("fourth", ""), None)
        .await
        .expect("asked");
    test.teardown().await;
    assert!(stale.is_none(), "not stored from an old reading: {stale:?}");
    assert_eq!(
        (
            stored.client_id.as_str(),
            stored.sealed_client_secret.as_str()
        ),
        ("first", "a secret"),
        "the change meanwhile stays"
    );
    assert!(fresh.is_some(), "stored from the reading of what is there");
    assert!(none_stale.is_none(), "one was there, not none");
}

/// A provider removed after a change of it was read does not come back: the change,
/// decided from the reading before the removal, is not stored; nor when the provider was
/// set again meanwhile, at whatever time, as it is another. (That a removal and a
/// change are one after the other, not interleaved, is the lock's; this test runs them
/// one after the other.)
#[tokio::test]
async fn a_removed_provider_is_not_brought_back_by_a_change_read_before() {
    let test = common::setup().await;
    let provider = StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: "https://idp.example.com".to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: "sealed".to_owned(),
        auto_join: true,
        enabled: false,
        create_at: 0,
        update_at: 0,
        sso_required: false,
    };
    let read = test.put_oidc_provider(&provider).await.expect("stored");
    assert_eq!(
        test.delete_oidc_provider().await.expect("removed"),
        Removal::Removed
    );
    let change = test
        .put_oidc_provider_as_read(&provider, Some(&read))
        .await
        .expect("asked");
    let after = test.oidc_provider().await.expect("read");
    // Set again, by another request; the reading of the one removed has, at worst,
    // the time of the new one (the same millisecond, or the clock went back).
    let again = test.put_oidc_provider(&provider).await.expect("set again");
    let of_the_removed = StoredProvider {
        update_at: again.update_at,
        ..read.clone()
    };
    let change_again = test
        .put_oidc_provider_as_read(&provider, Some(&of_the_removed))
        .await
        .expect("asked");
    test.teardown().await;
    assert!(change.is_none(), "{change:?}");
    assert!(after.is_none(), "{after:?}");
    assert_ne!(again.id, read.id, "another provider");
    assert!(change_again.is_none(), "{change_again:?}");
}

fn provider_that(sso_required: Enforcement) -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: "https://idp.example.com".to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: String::new(),
        auto_join: true,
        enabled: true,
        create_at: 0,
        update_at: 0,
        sso_required: sso_required == Enforcement::On,
    }
}

/// Whether the organization takes only sessions made through its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enforcement {
    On,
    Off,
}

/// The provider an organization takes no other session than stays: its removal is
/// refused, until the organization is released, which leaves the provider as it was
/// but for that, under a later time.
#[tokio::test]
async fn an_enforced_provider_is_not_removed_until_it_is_released() {
    let test = common::setup().await;
    assert_eq!(
        test.delete_oidc_provider().await.expect("asked"),
        Removal::NoneThere
    );
    assert_eq!(test.release_sso().await.expect("asked"), None);
    let enforced = test
        .put_oidc_provider(&provider_that(Enforcement::On))
        .await
        .expect("stored");
    assert!(enforced.sso_required);

    let refused = test.delete_oidc_provider().await.expect("asked");
    let still = test.oidc_provider().await.expect("read");
    let released = test.release_sso().await.expect("released");
    let again = test.release_sso().await.expect("asked");
    let removed = test.delete_oidc_provider().await.expect("asked");
    test.teardown().await;

    assert_eq!(refused, Removal::Enforced);
    assert_eq!(still.as_ref(), Some(&enforced), "as it was");
    let released = released.expect("it was enforced");
    assert!(!released.sso_required);
    assert!(
        released.update_at > enforced.update_at,
        "another state of it"
    );
    assert_eq!(
        StoredProvider {
            sso_required: true,
            update_at: enforced.update_at,
            ..released
        },
        enforced,
        "nothing else of it changed"
    );
    assert_eq!(again, None, "nothing left to release");
    assert_eq!(removed, Removal::Removed);
}

/// What a session must be made through to open an organization is answered to who has
/// a place in it, by the organization or by a workspace of it, and to nobody else; and
/// only while the organization takes no other session.
#[tokio::test]
async fn the_required_provider_is_told_to_who_has_a_place_in_the_organization() {
    let test = common::setup().await;
    let nobodys = test.scoped(Tenant::Unset);
    let member = common::user(&test, "member").await;
    let guest = common::user(&nobodys, "guest").await;
    let stranger = common::user(&nobodys, "stranger").await;
    test.add_workspace_member(
        &test.workspace_id,
        &guest,
        fvoci_model::WorkspaceRole::Guest,
    )
    .await
    .expect("a guest of the workspace, no member of the organization");
    let required = async |user_id: &str| {
        test.required_provider_for(user_id, &test.org_id)
            .await
            .expect("asked")
    };
    let entered = async |user_id: &str| {
        test.resolve_workspace(user_id, &test.workspace_id)
            .await
            .expect("asked")
            .map(|access| access.required_provider)
    };
    let listed = async |user_id: &str, through: Option<&str>| {
        test.workspaces_of_user(user_id, through)
            .await
            .expect("listed")
            .iter()
            .map(|workspace| workspace.sso_required)
            .collect::<Vec<_>>()
    };

    let open = test
        .put_oidc_provider(&provider_that(Enforcement::Off))
        .await
        .expect("stored");
    let while_open = (
        required(&member).await,
        entered(&member).await,
        listed(&member, None).await,
    );
    let provider = test
        .put_oidc_provider(&provider_that(Enforcement::On))
        .await
        .expect("stored");
    let told = [
        required(&member).await,
        required(&guest).await,
        required(&stranger).await,
    ];
    let at_the_door = [
        entered(&member).await,
        entered(&guest).await,
        entered(&stranger).await,
    ];
    let lists = [
        listed(&member, None).await,
        listed(&member, Some(&provider.id)).await,
        listed(&member, Some(&new_id())).await,
        listed(&guest, None).await,
        listed(&stranger, None).await,
    ];
    let elsewhere = test
        .required_provider_for(&member, &new_id())
        .await
        .expect("asked");
    test.teardown().await;

    assert_eq!(open.id, provider.id, "the same provider");
    assert_eq!(while_open, (None, Some(None), vec![false]));
    let ours = Some(provider.id.clone());
    assert_eq!(told, [ours.clone(), ours.clone(), None]);
    assert_eq!(at_the_door, [Some(ours.clone()), Some(ours), None]);
    assert_eq!(
        lists,
        [vec![true], vec![false], vec![true], vec![true], vec![]]
    );
    assert_eq!(elsewhere, None, "an organization that is not there");
}
