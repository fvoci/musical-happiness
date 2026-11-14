//! Sign-ins held while an organization moves to another provider, in the store: held
//! only while a former provider is kept and the provider is as it vouched, read and
//! taken by their own organization, once; and carried over to the account the former
//! provider vouches for only if every part still holds, which ties the identity,
//! records it and stores the session in one transaction, or stores nothing.

use std::time::Duration;

use fvoci_model::{AuditAction, Session, new_id};
use fvoci_store::{
    AuditPlace, CarriedOver, CarryOver, CarryOverRefusal, DbType, FlowPurpose, HeldSignIn,
    Identity, LoginFlow, Paging, Store, StoredProvider, Tenant,
};

use crate::common;

const OLD: &str = "https://old.example.test";
const NEW: &str = "https://new.example.test";

/// The time the tests hold and carry over at, and a time before the held sign-in ends.
const THEN: i64 = 1_000;
const SOON: i64 = 1_500;
/// When the held sign-in ends.
const ENDS: i64 = 2_000;

fn provider(issuer: &str, secret: &str) -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: issuer.to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: secret.to_owned(),
        auto_join: true,
        enabled: true,
        sso_required: false,
        revision: 0,
        create_at: 0,
        update_at: 0,
    }
}

/// An organization that moved from [`OLD`] to [`NEW`], with a member whose account the
/// old provider vouches for: the provider as it is, and the member.
async fn moved(store: &Store) -> (StoredProvider, String) {
    let actor = new_id();
    let old = store
        .put_oidc_provider_as_read(&provider(OLD, "one"), None, &actor, Duration::ZERO)
        .await
        .expect("asked")
        .expect("stored");
    let kept = Duration::from_hours(24);
    let new = store
        .put_oidc_provider_as_read(&provider(NEW, "one"), Some(&old), &actor, kept)
        .await
        .expect("asked")
        .expect("stored");
    let member = common::user(store, "member").await;
    store
        .add_identity(&member, at_the_old("member"), "")
        .await
        .expect("tied");
    (new, member)
}

fn at_the_old(subject: &str) -> Identity<'_> {
    Identity {
        issuer: OLD,
        subject,
    }
}

/// A sign-in through `provider` to hold, of the person `subject` at the new issuer.
fn held(provider: &StoredProvider, subject: &str) -> HeldSignIn {
    HeldSignIn {
        held_hash: format!("the digest of the browser of {subject}"),
        org_id: provider.org_id.clone(),
        provider_id: provider.id.clone(),
        revision: provider.revision,
        issuer: NEW.to_owned(),
        subject: subject.to_owned(),
        email: format!("{subject}@new.example.test"),
        email_unverified: false,
        name: "A Person".to_owned(),
        preferred_username: subject.to_owned(),
        create_at: now_millis(),
        expires_at: now_millis() + ENDS,
    }
}

/// Now, in milliseconds since the Unix epoch.
fn now_millis() -> i64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is past the epoch");
    i64::try_from(since.as_millis()).expect("the time fits")
}

/// Carries `held` over as the former provider vouching for `former`, at `after` past
/// the time it was held; the session it would store is under the digest `held_hash`.
async fn carry(
    store: &Store,
    provider_id: &str,
    held: &HeldSignIn,
    former: Identity<'_>,
    after: i64,
) -> Result<CarriedOver, CarryOverRefusal> {
    let mut session = Session {
        id: new_id(),
        created_at: 1,
        expires_at: i64::MAX,
        ..Session::default()
    };
    let carry = CarryOver {
        provider_id,
        held_hash: &held.held_hash,
        former,
        now: held.create_at + after,
    };
    store
        .finish_oidc_carry_over(carry, &mut session, &held.held_hash)
        .await
        .expect("asked")
}

/// What is left of a try: the identities of the new issuer and the sessions.
async fn left(store: &Store) -> (i64, i64) {
    let system = store.system();
    let count = async |rows: &str| system.count_rows(rows).await.expect("counted");
    (
        count(&format!("user_identities WHERE issuer = '{NEW}'")).await,
        count("sessions").await,
    )
}

/// A sign-in is held only for the provider as it vouched, on, while a former provider
/// is kept; it is read without being taken, and taken once.
#[tokio::test]
async fn a_sign_in_is_held_while_a_former_provider_is_kept_and_taken_once() {
    let test = common::setup().await;
    let (new, _) = moved(&test).await;
    let asked = held(&new, "ada");
    let stale = HeldSignIn {
        revision: new.revision + 1,
        ..held(&new, "of another revision")
    };

    let of_another_revision = test.hold_oidc_sign_in(&stale).await;
    test.hold_oidc_sign_in(&asked).await.expect("held");
    let read = test
        .oidc_held_sign_in(&asked.held_hash, asked.create_at + SOON)
        .await
        .expect("read");
    let read_past_its_end = test
        .oidc_held_sign_in(&asked.held_hash, asked.expires_at)
        .await
        .expect("read");
    let taken = test
        .take_oidc_held_sign_in(&asked.held_hash, asked.create_at + SOON)
        .await
        .expect("taken");
    let again = test
        .take_oidc_held_sign_in(&asked.held_hash, asked.create_at + SOON)
        .await
        .expect("asked");
    // Past its end it is taken all the same, and is none.
    test.hold_oidc_sign_in(&asked).await.expect("held again");
    let ended = test
        .take_oidc_held_sign_in(&asked.held_hash, asked.expires_at)
        .await
        .expect("asked");
    let rows = test
        .count_rows("oidc_held_sign_ins")
        .await
        .expect("counted");
    // With the keeping ended, nothing is held.
    test.end_oidc_former_provider(&new_id(), former_since(&test).await, now_millis())
        .await
        .expect("ended");
    let without_a_former = test.hold_oidc_sign_in(&asked).await;

    assert!(of_another_revision.is_err_and(|error| error.is_not_found()));
    assert_eq!(read.as_ref(), Some(&asked), "as it was held");
    assert_eq!(read_past_its_end, None);
    assert_eq!((taken, again), (Some(asked), None), "taken once");
    assert_eq!((ended, rows), (None, 0), "ended, and removed by the take");
    assert!(without_a_former.is_err_and(|error| error.is_not_found()));
    test.teardown().await;
}

async fn former_since(store: &Store) -> i64 {
    let kept = store
        .oidc_former_provider(now_millis())
        .await
        .expect("read");
    kept.expect("kept").create_at
}

/// The former provider vouches for a member's account: the held identity is tied to
/// it with the address its provider gave, that is recorded as the member's, the
/// session is theirs and made through the provider at its revision, and the held
/// sign-in is taken.
#[tokio::test]
async fn a_held_sign_in_is_carried_over_to_the_members_account() {
    let test = common::setup().await;
    let (new, member) = moved(&test).await;
    let asked = held(&new, "ada");
    test.hold_oidc_sign_in(&asked).await.expect("held");

    let carried = carry(&test, &new.id, &asked, at_the_old("member"), THEN).await;
    let again = carry(&test, &new.id, &asked, at_the_old("member"), THEN).await;

    assert_eq!(
        carried,
        Ok(CarriedOver {
            user_id: member.clone(),
            issuer: NEW.to_owned()
        })
    );
    assert_eq!(again, Err(CarryOverRefusal::HeldGone), "taken once");
    let tied = test.identities_of_user(&member).await.expect("read");
    let of_the_new: Vec<_> = tied.iter().filter(|tied| tied.issuer == NEW).collect();
    assert_eq!(of_the_new.len(), 1, "{tied:?}");
    assert_eq!(
        (of_the_new[0].subject.as_str(), of_the_new[0].email.as_str()),
        ("ada", "ada@new.example.test")
    );
    let session = test
        .get_session(&asked.held_hash, 0, 0)
        .await
        .expect("the session is stored");
    let login = session.login.expect("made through a provider");
    assert_eq!(session.user_id, member);
    assert_eq!(
        (
            login.provider_id,
            login.issuer,
            login.subject,
            login.revision
        ),
        (
            new.id.clone(),
            NEW.to_owned(),
            Some("ada".to_owned()),
            Some(new.revision)
        )
    );
    let paging = Paging {
        page: 0,
        per_page: 100,
    };
    let records = test
        .audit_records(AuditPlace::Organization, paging)
        .await
        .expect("records");
    let carried: Vec<_> = records
        .iter()
        .filter(|record| record.action == AuditAction::OidcIdentityCarriedOver)
        .collect();
    assert_eq!(carried.len(), 1, "recorded once");
    assert_eq!(carried[0].detail.as_deref(), Some(NEW));
    assert_eq!(
        carried[0].subject.as_ref().map(|person| person.id.as_str()),
        Some(member.as_str())
    );
    test.teardown().await;
}

/// What changes between holding and carrying over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meanwhile {
    /// The held sign-in ended.
    HeldEnded,
    /// The provider was given another secret: another revision of the same row.
    AnotherSecret,
    /// The sign-in names another provider than the organization's.
    AnotherProvider,
    /// The former provider vouches under another issuer than the one kept.
    AnotherFormerIssuer,
    /// The keeping of the former provider ended.
    KeepingEnded,
    /// The former provider vouches for nobody with an account.
    NobodyKnown,
    /// The account it vouches for was deleted.
    AccountDeleted,
    /// The account it vouches for is no member of the organization any more.
    NoMember,
    /// The held identity was tied to another account.
    TiedToAnother,
}

/// Every part that does not hold any more refuses, and nothing is stored: no identity
/// of the new issuer, no session. The held sign-in is taken by the try.
#[tokio::test]
async fn what_does_not_hold_any_more_carries_nothing_over() {
    use CarryOverRefusal as Refusal;
    for (meanwhile, refusal) in [
        (Meanwhile::HeldEnded, Refusal::HeldGone),
        (Meanwhile::AnotherSecret, Refusal::HeldGone),
        (Meanwhile::AnotherProvider, Refusal::ProviderGone),
        (Meanwhile::AnotherFormerIssuer, Refusal::FormerGone),
        (Meanwhile::KeepingEnded, Refusal::FormerGone),
        (Meanwhile::NobodyKnown, Refusal::NoAccount),
        (Meanwhile::AccountDeleted, Refusal::NoAccount),
        (Meanwhile::NoMember, Refusal::NotAMember),
        (Meanwhile::TiedToAnother, Refusal::IdentityTaken),
    ] {
        let test = common::setup().await;
        let (new, member) = moved(&test).await;
        let asked = held(&new, "ada");
        test.hold_oidc_sign_in(&asked).await.expect("held");
        let mut provider_id = new.id.clone();
        let mut former = at_the_old("member");
        let mut after = THEN;
        let before = match meanwhile {
            Meanwhile::HeldEnded => {
                after = ENDS;
                left(&test).await
            }
            Meanwhile::AnotherSecret => {
                let again = provider(NEW, "another");
                test.put_oidc_provider_as_read(&again, Some(&new), &member, Duration::ZERO)
                    .await
                    .expect("asked")
                    .expect("stored");
                left(&test).await
            }
            Meanwhile::AnotherProvider => {
                provider_id = new_id();
                left(&test).await
            }
            Meanwhile::AnotherFormerIssuer => {
                former = Identity {
                    issuer: "https://elsewhere.example.test",
                    subject: "member",
                };
                test.add_identity(&member, former, "").await.expect("tied");
                left(&test).await
            }
            Meanwhile::KeepingEnded => {
                let since = former_since(&test).await;
                test.end_oidc_former_provider(&member, since, now_millis())
                    .await
                    .expect("ended");
                left(&test).await
            }
            Meanwhile::NobodyKnown => {
                former = at_the_old("known to nobody");
                left(&test).await
            }
            Meanwhile::AccountDeleted => {
                let delete = format!("UPDATE users SET delete_at = 1 WHERE id = '{member}'");
                test.execute_unchecked(&delete).await.expect("deleted");
                left(&test).await
            }
            Meanwhile::NoMember => {
                let leave = format!("DELETE FROM organization_members WHERE user_id = '{member}'");
                test.execute_unchecked(&leave).await.expect("left");
                left(&test).await
            }
            Meanwhile::TiedToAnother => {
                let another = common::user(&test, "another").await;
                let taken = Identity {
                    issuer: NEW,
                    subject: "ada",
                };
                test.add_identity(&another, taken, "").await.expect("tied");
                left(&test).await
            }
        };

        let carried = carry(&test, &provider_id, &asked, former, after).await;

        assert_eq!(carried, Err(refusal), "{meanwhile:?}");
        assert_eq!(left(&test).await, before, "{meanwhile:?}: nothing stored");
        let rows = test.count_rows("oidc_held_sign_ins").await;
        assert_eq!(rows.expect("counted"), 0, "{meanwhile:?}: taken by the try");
        test.teardown().await;
    }
}

/// The held identity that is the account's already (tied by another way meanwhile) is
/// left as it is, and the account is signed in.
#[tokio::test]
async fn an_identity_that_is_the_accounts_already_is_left_as_it_is() {
    let test = common::setup().await;
    let (new, member) = moved(&test).await;
    let asked = held(&new, "ada");
    test.hold_oidc_sign_in(&asked).await.expect("held");
    let theirs = Identity {
        issuer: NEW,
        subject: "ada",
    };
    test.add_identity(&member, theirs, "as it was tied")
        .await
        .expect("tied");

    let carried = carry(&test, &new.id, &asked, at_the_old("member"), THEN).await;

    assert_eq!(carried.map(|carried| carried.user_id), Ok(member.clone()));
    let tied = test.identities_of_user(&member).await.expect("read");
    let of_the_new: Vec<_> = tied.iter().filter(|tied| tied.issuer == NEW).collect();
    assert_eq!(of_the_new.len(), 1, "{tied:?}");
    assert_eq!(of_the_new[0].email, "as it was tied");
    test.teardown().await;
}

/// A sign-in to carry over starts only for a held sign-in that is there, the held
/// sign-ins that ended go with the sign-ins that ended, and on PostgreSQL another
/// organization neither reads nor takes what is held for this one.
#[tokio::test]
async fn a_held_sign_in_is_its_organizations_and_goes_when_it_ended() {
    let test = common::setup().await;
    let (new, _) = moved(&test).await;
    let asked = held(&new, "ada");
    test.hold_oidc_sign_in(&asked).await.expect("held");
    let flow = |held_hash: &str| LoginFlow {
        state_hash: format!("the state for {held_hash}"),
        browser_hash: "a browser".to_owned(),
        nonce: String::new(),
        pkce_verifier: String::new(),
        org_id: new.org_id.clone(),
        provider_id: new.id.clone(),
        create_at: 0,
        expires_at: i64::MAX,
        purpose: FlowPurpose::CarryOver {
            held_hash: held_hash.to_owned(),
        },
    };

    let for_nothing_held = test.create_oidc_login_flow(&flow("no such digest")).await;
    test.create_oidc_login_flow(&flow(&asked.held_hash))
        .await
        .expect("a sign-in to carry over is stored");
    if common::db_type() == DbType::Postgres {
        let (other_org, _) = common::organization(&test.scoped(Tenant::Unset), "other").await;
        let theirs = test.for_org(&other_org).expect("an identifier");
        let soon = asked.create_at + SOON;
        let read = theirs.oidc_held_sign_in(&asked.held_hash, soon).await;
        let taken = theirs.take_oidc_held_sign_in(&asked.held_hash, soon).await;
        assert_eq!(read.expect("asked"), None, "not another organization's");
        assert_eq!(taken.expect("asked"), None, "nor theirs to take");
    }
    let still = test
        .count_rows("oidc_held_sign_ins")
        .await
        .expect("counted");
    let before_its_end = test
        .delete_ended_oidc_login_flows(asked.expires_at - 1)
        .await
        .expect("cleaned");
    let kept = test
        .count_rows("oidc_held_sign_ins")
        .await
        .expect("counted");
    test.delete_ended_oidc_login_flows(asked.expires_at)
        .await
        .expect("cleaned");
    let gone = test
        .count_rows("oidc_held_sign_ins")
        .await
        .expect("counted");

    assert!(for_nothing_held.is_err_and(|error| error.is_not_found()));
    assert_eq!((still, before_its_end, kept, gone), (1, 0, 1, 0));
    test.teardown().await;
}
