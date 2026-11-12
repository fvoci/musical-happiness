//! Signing in through a provider: two first sign-ins at once of an account already
//! known to the issuer.

use std::time::Duration;

use fvoci_model::{Session, new_id};
use fvoci_store::{DbType, OidcAccount, OidcRefusal, Store, StoredProvider, Tenant};
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
