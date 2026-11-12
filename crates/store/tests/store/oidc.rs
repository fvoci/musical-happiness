//! Signing in through a provider: two first sign-ins at once of an account already
//! known to the issuer. First written by an independent review of the branch
//! (`fvoci/review-oidc-i1`).

use std::time::Duration;

use fvoci_store::{DbType, Store, Tenant};
use sqlx::Connection as _;

use crate::common;

fn join_task(
    store: Store,
    user_id: String,
) -> tokio::task::JoinHandle<Result<bool, fvoci_store::StoreError>> {
    tokio::spawn(async move { store.join_organization_through_oidc(&user_id).await })
}

/// This is the operation `App::let_in` calls when an identity names an existing
/// account that is not yet in the organization. Both callbacks may get that answer.
/// A PostgreSQL SHARE table lock lets their membership reads finish while both
/// inserts wait, making the order independent of sleeps or the speed of the machine.
#[tokio::test]
async fn concurrent_oidc_joins_of_one_account_both_succeed() {
    let test = common::setup().await;
    let account = common::user(&test.scoped(Tenant::Unset), "existing-identity").await;
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
        let tasks = (
            join_task(test.store.clone(), account.clone()),
            join_task(test.store.clone(), account.clone()),
        );
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
        (
            join_task(test.store.clone(), account.clone()),
            join_task(test.store.clone(), account.clone()),
        )
    };
    let (first, second) = tokio::join!(tasks.0, tasks.1);
    let first = first.expect("the first callback task completes");
    let second = second.expect("the second callback task completes");
    let rows = test
        .count_rows("organization_members")
        .await
        .expect("memberships counted");
    test.teardown().await;

    assert!(
        first.is_ok() && second.is_ok(),
        "both valid callbacks must succeed: first={first:?}, second={second:?}"
    );
    assert_ne!(
        first.expect("the first result"),
        second.expect("the second result"),
        "exactly one call newly joins the account"
    );
    assert_eq!(rows, 1, "one organization membership");
}
