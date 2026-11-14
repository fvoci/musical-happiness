//! The provider an organization moved away from, in the store: kept for its time when
//! the issuer changes, read as none past it, ended early by an administrator, removed
//! by the housekeeping when its time ran out, and gone with another move or with the
//! provider; each recorded in the organization's audit log.

use fvoci_model::{AuditAction, AuditRecordActor, new_id};
use fvoci_store::{AuditPlace, FormerProvider, Paging, Removal, Store, StoredProvider};

use crate::common;

const ONE: &str = "https://one.example.test";
const TWO: &str = "https://two.example.test";
const THREE: &str = "https://three.example.test";

/// Thirty days, in milliseconds.
const KEPT: i64 = 30 * 24 * 60 * 60 * 1000;

/// Now, in milliseconds since the Unix epoch.
fn now_millis() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is past the epoch");
    i64::try_from(since.as_millis()).expect("the time fits")
}

fn provider(issuer: &str, client_id: &str) -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: issuer.to_owned(),
        client_id: client_id.to_owned(),
        sealed_client_secret: format!("the sealed secret of {client_id} at {issuer}"),
        auto_join: true,
        enabled: true,
        sso_required: false,
        revision: 0,
        create_at: 0,
        update_at: 0,
    }
}

/// Sets the provider in place of `read`, as `actor`, keeping the one before for `kept`.
async fn put(
    store: &Store,
    next: &StoredProvider,
    read: Option<&StoredProvider>,
    actor: &str,
    kept: i64,
) -> StoredProvider {
    store
        .put_oidc_provider_as_read(next, read, actor, kept)
        .await
        .expect("asked")
        .expect("stored")
}

async fn former(store: &Store, now: i64) -> Option<FormerProvider> {
    store.oidc_former_provider(now).await.expect("read")
}

/// The organization's records of the former provider: what was done, by whom, to
/// which issuer.
async fn records(store: &Store) -> Vec<(AuditAction, AuditRecordActor, String)> {
    let paging = Paging {
        page: 0,
        per_page: 100,
    };
    store
        .audit_records(AuditPlace::Organization, paging)
        .await
        .expect("records")
        .into_iter()
        .filter(|record| record.action.as_str().starts_with("oidc_former_provider_"))
        .map(|record| {
            (
                record.action,
                record.actor,
                record.detail.unwrap_or_default(),
            )
        })
        .collect()
}

/// Whether the records have one of `action` for `issuer`, and who by.
fn recorded(
    records: &[(AuditAction, AuditRecordActor, String)],
    action: AuditAction,
    issuer: &str,
) -> Vec<AuditRecordActor> {
    records
        .iter()
        .filter(|(done, _, detail)| *done == action && detail == issuer)
        .map(|(_, actor, _)| actor.clone())
        .collect()
}

fn is_person(actor: &AuditRecordActor, user_id: &str) -> bool {
    matches!(actor, AuditRecordActor::Person(person) if person.id == user_id)
}

/// Another issuer keeps the provider before, with its client and sealed secret, for
/// the time given, and it is read as none from the moment that time ran out; another
/// client of the same issuer keeps none, and leaves the one kept.
#[tokio::test]
async fn another_issuer_keeps_the_provider_before_for_its_time() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let first = put(&test, &provider(ONE, "client"), None, &actor, KEPT).await;
    let another_client = put(&test, &provider(ONE, "next"), Some(&first), &actor, KEPT).await;
    let none_yet = former(&test, now_millis()).await;

    let moved = put(
        &test,
        &provider(TWO, "client"),
        Some(&another_client),
        &actor,
        KEPT,
    )
    .await;
    let kept = former(&test, now_millis()).await.expect("kept");
    put(&test, &provider(TWO, "next"), Some(&moved), &actor, KEPT).await;
    let still = former(&test, now_millis()).await;

    assert_eq!(none_yet, None, "the issuer did not change");
    assert_eq!(
        (
            kept.issuer.as_str(),
            kept.client_id.as_str(),
            kept.sealed_client_secret.as_str()
        ),
        (ONE, "next", another_client.sealed_client_secret.as_str()),
        "the provider as it was"
    );
    assert_eq!(kept.org_id, moved.org_id);
    assert_eq!(kept.expires_at - kept.create_at, KEPT);
    assert!(former(&test, kept.expires_at - 1).await.is_some());
    assert_eq!(former(&test, kept.expires_at).await, None, "past its time");
    assert_eq!(still, Some(kept), "another client of the issuer leaves it");
    let records = records(&test).await;
    let by = recorded(&records, AuditAction::OidcFormerProviderKept, ONE);
    assert!(by.len() == 1 && is_person(&by[0], &actor), "{records:?}");
    assert_eq!(records.len(), 1, "{records:?}");
    test.teardown().await;
}

/// A time of zero keeps none.
#[tokio::test]
async fn no_time_keeps_none() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let first = put(&test, &provider(ONE, "client"), None, &actor, 0).await;

    put(&test, &provider(TWO, "client"), Some(&first), &actor, 0).await;

    assert_eq!(former(&test, now_millis()).await, None);
    assert_eq!(records(&test).await, []);
    test.teardown().await;
}

/// An administrator ends the keeping before its time, once: it is recorded as theirs.
#[tokio::test]
async fn it_is_ended_before_its_time_once() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let first = put(&test, &provider(ONE, "client"), None, &actor, KEPT).await;
    put(&test, &provider(TWO, "client"), Some(&first), &actor, KEPT).await;

    let ended = test
        .end_oidc_former_provider(&actor, now_millis())
        .await
        .expect("asked");
    let again = test
        .end_oidc_former_provider(&actor, now_millis())
        .await
        .expect("asked");

    assert!(ended && !again);
    assert_eq!(former(&test, now_millis()).await, None);
    let records = records(&test).await;
    let by = recorded(&records, AuditAction::OidcFormerProviderEnded, ONE);
    assert!(by.len() == 1 && is_person(&by[0], &actor), "{records:?}");
    test.teardown().await;
}

/// One whose time ran out is not an administrator's to end: the housekeeping removes
/// it, once, and records it as the server's own doing; one still kept stays.
#[tokio::test]
async fn one_whose_time_ran_out_is_removed_by_the_housekeeping() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let first = put(&test, &provider(ONE, "client"), None, &actor, KEPT).await;
    put(&test, &provider(TWO, "client"), Some(&first), &actor, KEPT).await;
    let kept = former(&test, now_millis()).await.expect("kept");
    let system = test.system();

    let too_early = system
        .end_lapsed_oidc_former_providers(kept.expires_at - 1)
        .await
        .expect("asked");
    let by_an_administrator = test
        .end_oidc_former_provider(&actor, kept.expires_at)
        .await
        .expect("asked");
    let removed = system
        .end_lapsed_oidc_former_providers(kept.expires_at)
        .await
        .expect("asked");
    let again = system
        .end_lapsed_oidc_former_providers(kept.expires_at)
        .await
        .expect("asked");

    assert_eq!((too_early, removed, again), (0, 1, 0));
    assert!(!by_an_administrator, "its time ran out: not theirs to end");
    let rows = test.count_rows("oidc_former_providers").await;
    assert_eq!(rows.expect("counted"), 0);
    let records = records(&test).await;
    assert_eq!(
        recorded(&records, AuditAction::OidcFormerProviderLapsed, ONE),
        [AuditRecordActor::Server],
        "{records:?}"
    );
    assert_eq!(
        recorded(&records, AuditAction::OidcFormerProviderEnded, ONE),
        []
    );
    test.teardown().await;
}

/// Another move puts the provider before in the place of the one kept, which is
/// recorded as ended by who moved; and the removal of the provider ends what is kept.
#[tokio::test]
async fn another_move_and_the_removal_of_the_provider_end_it() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let mover = new_id();
    let first = put(&test, &provider(ONE, "client"), None, &actor, KEPT).await;
    let second = put(&test, &provider(TWO, "client"), Some(&first), &actor, KEPT).await;

    put(
        &test,
        &provider(THREE, "client"),
        Some(&second),
        &mover,
        KEPT,
    )
    .await;
    let after_the_move = former(&test, now_millis()).await.expect("kept");
    let removal = test.delete_oidc_provider(&actor).await.expect("asked");
    let after_the_removal = former(&test, now_millis()).await;

    assert_eq!(after_the_move.issuer, TWO);
    assert!(matches!(removal, Removal::Removed { .. }));
    assert_eq!(after_the_removal, None);
    let records = records(&test).await;
    let ended_one = recorded(&records, AuditAction::OidcFormerProviderEnded, ONE);
    let kept_two = recorded(&records, AuditAction::OidcFormerProviderKept, TWO);
    let ended_two = recorded(&records, AuditAction::OidcFormerProviderEnded, TWO);
    assert!(
        ended_one.len() == 1 && is_person(&ended_one[0], &mover),
        "{records:?}"
    );
    assert!(
        kept_two.len() == 1 && is_person(&kept_two[0], &mover),
        "{records:?}"
    );
    assert!(
        ended_two.len() == 1 && is_person(&ended_two[0], &actor),
        "{records:?}"
    );
    test.teardown().await;
}
