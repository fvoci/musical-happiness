//! A trial of pending settings of another issuer that vouches for an identity tied to
//! no account: tied to the account of the administrator who tries, with the settings
//! applied, in one transaction, only for a session of theirs made through the
//! provider as it is just before; anything else ties nothing and applies nothing.

use std::time::Duration;

use fvoci_model::{AuditAction, OrgRole, Session, User, new_id};
use fvoci_store::{
    AuditPlace, Binding, Identity, LinkingSession, OidcAccount, OidcRefusal, Paging,
    PendingProvider, PendingSettings, Store, StoredProvider, Trial, Tried,
};

use crate::common;

const ONE: &str = "https://one.example.test";
const TWO: &str = "https://two.example.test";

/// When the administrator's session was made, and the time the trial comes back at.
const MADE: i64 = 1_000;
const NOW: i64 = 2_000;

/// The digest of the token of the administrator's session.
const DIGEST: &str = "the digest of the administrator's session";

fn provider(issuer: &str, secret: &str) -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: issuer.to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: secret.to_owned(),
        auto_join: true,
        enabled: true,
        sso_required: true,
        revision: 0,
        create_at: 0,
        update_at: 0,
    }
}

/// An organization that takes no password, with its administrator, their session
/// made through the provider at [`MADE`], and pending settings of `pending_issuer`.
struct Enforced {
    provider: StoredProvider,
    pending: PendingProvider,
    admin: User,
    session_id: String,
}

async fn an_administrator(store: &Store, name: &str) -> User {
    let mut user = User {
        id: new_id(),
        username: name.to_owned(),
        email: format!("{name}@example.com"),
        password_hash: "a password hash".to_owned(),
        ..User::default()
    };
    store.create_user(&mut user).await.expect("stored");
    let admin = format!(
        "UPDATE organization_members SET role = '{}' WHERE user_id = '{}'",
        OrgRole::Admin.as_str(),
        user.id
    );
    store.execute_unchecked(&admin).await.expect("made one");
    user
}

async fn enforced(store: &Store, pending_issuer: &str) -> Enforced {
    let admin = an_administrator(store, "admin").await;
    let provider = store
        .put_oidc_provider_as_read(&provider(ONE, "one"), None, &admin.id, Duration::ZERO)
        .await
        .expect("asked")
        .expect("stored");
    let at_one = Identity {
        issuer: ONE,
        subject: "the administrator at one",
    };
    store
        .add_identity(&admin.id, at_one, "")
        .await
        .expect("tied");
    let mut session = Session {
        id: new_id(),
        created_at: MADE,
        last_used_at: MADE,
        expires_at: i64::MAX,
        ..Session::default()
    };
    let known = OidcAccount::Known {
        user_id: &admin.id,
        password_hash: &admin.password_hash,
        identity: at_one,
    };
    store
        .finish_oidc_sign_in(&provider.id, provider.revision, known, &mut session, DIGEST)
        .await
        .expect("asked")
        .expect("signed in through the provider");
    let aside = PendingSettings {
        issuer: pending_issuer.to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: "two".to_owned(),
    };
    let (provider, pending) = store
        .put_oidc_pending_provider(&aside, &provider, &provider, &admin.id)
        .await
        .expect("asked")
        .expect("saved aside");
    Enforced {
        provider,
        pending,
        admin,
        session_id: session.id,
    }
}

fn at_two() -> Identity<'static> {
    Identity {
        issuer: TWO,
        subject: "the administrator at two",
    }
}

/// What a trial is asked with, beside the organization.
#[derive(Debug, Clone, Copy)]
struct Asked<'a> {
    trier_id: &'a str,
    identity: Identity<'a>,
    session_id: &'a str,
    token_digest: &'a str,
    made_since: i64,
}

async fn try_unbound(
    store: &Store,
    organization: &Enforced,
    asked: Asked<'_>,
) -> Result<fvoci_store::Applied, OidcRefusal> {
    let mut session = Session {
        id: new_id(),
        created_at: NOW,
        expires_at: i64::MAX,
        ..Session::default()
    };
    let trial = Trial {
        provider_id: &organization.provider.id,
        pending_at: organization.pending.create_at,
        trier_id: asked.trier_id,
        keep_former: Duration::from_hours(24),
    };
    let binding = Binding {
        identity: asked.identity,
        provider_email: "admin@two.example.test",
        session: LinkingSession {
            session_id: asked.session_id,
            token_digest: asked.token_digest,
            now: NOW,
            idle_since: 0,
        },
        made_since: asked.made_since,
    };
    store
        .finish_oidc_trial(
            trial,
            Tried::Unbound(binding),
            &mut session,
            "the digest of the trial's session",
        )
        .await
        .expect("asked")
}

/// What a refused trial must leave: the identities of the other issuer, the sessions,
/// the provider's issuer, and whether settings are still pending.
async fn left(store: &Store) -> (i64, i64, String, bool) {
    let system = store.system();
    let count = async |rows: &str| system.count_rows(rows).await.expect("counted");
    let provider = store.oidc_provider().await.expect("read").expect("one");
    let pending = store.oidc_pending_provider().await.expect("read");
    (
        count(&format!("user_identities WHERE issuer = '{TWO}'")).await,
        count("sessions").await,
        provider.issuer,
        pending.is_some(),
    )
}

/// From a session made through the provider no earlier than asked, the identity is
/// tied to the administrator, with the address its provider gave, the settings are
/// applied, the provider before is kept, and each is recorded as theirs.
#[tokio::test]
async fn a_session_made_just_now_binds_the_identity_and_applies_the_settings() {
    let test = common::setup().await;
    let organization = enforced(&test, TWO).await;
    let admin = &organization.admin;

    let applied = try_unbound(
        &test,
        &organization,
        Asked {
            trier_id: &admin.id,
            identity: at_two(),
            session_id: &organization.session_id,
            token_digest: DIGEST,
            made_since: MADE,
        },
    )
    .await
    .expect("applied");

    assert_eq!(applied.provider.issuer, TWO);
    let tied = test.identities_of_user(&admin.id).await.expect("read");
    let of_two: Vec<_> = tied.iter().filter(|tied| tied.issuer == TWO).collect();
    assert_eq!(of_two.len(), 1, "{tied:?}");
    assert_eq!(
        (of_two[0].subject.as_str(), of_two[0].email.as_str()),
        ("the administrator at two", "admin@two.example.test")
    );
    let session = test
        .get_session("the digest of the trial's session", 0, 0)
        .await
        .expect("the trial's session is stored");
    let login = session.login.expect("made through the provider");
    assert_eq!(session.user_id, admin.id);
    assert_eq!(
        (login.issuer.as_str(), login.revision),
        (TWO, Some(applied.provider.revision)),
        "it proves the provider as it is now"
    );
    let kept = test.oidc_former_provider(NOW).await.expect("read");
    assert_eq!(kept.map(|kept| kept.issuer).as_deref(), Some(ONE));
    let paging = Paging {
        page: 0,
        per_page: 100,
    };
    let records = test
        .audit_records(AuditPlace::Organization, paging)
        .await
        .expect("records");
    let bound: Vec<_> = records
        .iter()
        .filter(|record| record.action == AuditAction::OidcIdentityBound)
        .collect();
    assert_eq!(bound.len(), 1, "recorded once");
    assert_eq!(bound[0].detail.as_deref(), Some(TWO));
    assert_eq!(
        bound[0].subject.as_ref().map(|person| person.id.as_str()),
        Some(admin.id.as_str())
    );
    assert!(
        records
            .iter()
            .any(|record| record.action == AuditAction::OidcProviderPendingApplied)
    );
    test.teardown().await;
}

/// Why a trial that vouches for an identity tied to no account binds nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lacking {
    /// The session was made earlier than asked.
    MadeEarlier,
    /// The browser came back with another session than the one the trial started from.
    AnotherToken,
    /// The trial names another session than the administrator's.
    AnotherSession,
    /// The session is another administrator's than the one who tries.
    AnothersSession,
    /// The session was made at another revision of the provider.
    AnotherRevision,
    /// The session was made with a password, through no provider.
    NoProvider,
    /// The session ended.
    Ended,
    /// The administrator who tries administers the organization no more.
    NoAdministrator,
    /// The identity was tied to another account meanwhile.
    TiedToAnother,
    /// The identity is of another issuer than the pending settings'.
    AnotherIssuer,
}

/// Each of them refuses, and leaves everything as it was: no identity tied, no
/// session made, the provider and its pending settings as they were.
#[tokio::test]
async fn anything_less_binds_nothing_and_applies_nothing() {
    use OidcRefusal::{NotFresh, NotTheTrier};
    for (lacking, refusal) in [
        (Lacking::MadeEarlier, NotFresh),
        (Lacking::AnotherToken, NotFresh),
        (Lacking::AnotherSession, NotFresh),
        (Lacking::AnothersSession, NotFresh),
        (Lacking::AnotherRevision, NotFresh),
        (Lacking::NoProvider, NotFresh),
        (Lacking::Ended, NotFresh),
        (Lacking::NoAdministrator, NotTheTrier),
        (Lacking::TiedToAnother, NotTheTrier),
        (Lacking::AnotherIssuer, NotTheTrier),
    ] {
        let test = common::setup().await;
        let organization = enforced(&test, TWO).await;
        let another = an_administrator(&test, "another").await;
        let mut asked = Asked {
            trier_id: &organization.admin.id,
            identity: at_two(),
            session_id: &organization.session_id,
            token_digest: DIGEST,
            made_since: MADE,
        };
        let change = |column: &str| {
            format!(
                "UPDATE sessions SET {column} WHERE id = '{}'",
                organization.session_id
            )
        };
        let other_session = new_id();
        match lacking {
            Lacking::MadeEarlier => asked.made_since = MADE + 1,
            Lacking::AnotherToken => asked.token_digest = "another digest",
            Lacking::AnotherSession => asked.session_id = &other_session,
            Lacking::AnothersSession => asked.trier_id = &another.id,
            Lacking::AnotherRevision => {
                let sql = change("login_revision = login_revision + 1");
                test.execute_unchecked(&sql).await.expect("changed");
            }
            Lacking::NoProvider => {
                let sql = change(
                    "login_provider_id = NULL, login_org_id = NULL, login_issuer = NULL, \
                     login_subject = NULL, login_revision = NULL",
                );
                test.execute_unchecked(&sql).await.expect("changed");
            }
            Lacking::Ended => {
                let sql = change(&format!("expires_at = {NOW}"));
                test.execute_unchecked(&sql).await.expect("changed");
            }
            Lacking::NoAdministrator => {
                let sql = format!(
                    "UPDATE organization_members SET role = '{}' WHERE user_id = '{}'",
                    OrgRole::Member.as_str(),
                    organization.admin.id
                );
                test.execute_unchecked(&sql).await.expect("changed");
            }
            Lacking::TiedToAnother => {
                test.add_identity(&another.id, at_two(), "")
                    .await
                    .expect("tied");
            }
            Lacking::AnotherIssuer => {
                asked.identity = Identity {
                    issuer: "https://elsewhere.example.test",
                    subject: "the administrator elsewhere",
                };
            }
        }
        let before = left(&test).await;

        let tried = try_unbound(&test, &organization, asked).await;

        assert_eq!(tried.err(), Some(refusal), "{lacking:?}");
        assert_eq!(left(&test).await, before, "{lacking:?}: nothing changed");
        let elsewhere = test
            .system()
            .count_rows("user_identities WHERE issuer = 'https://elsewhere.example.test'")
            .await;
        assert_eq!(elsewhere.expect("counted"), 0, "{lacking:?}");
        test.teardown().await;
    }
}

/// Pending settings of the same issuer bind nothing, also from a session made just
/// now: another person of the provider is not the administrator under a new name.
#[tokio::test]
async fn settings_of_the_same_issuer_bind_nothing() {
    let test = common::setup().await;
    let organization = enforced(&test, ONE).await;
    let before = left(&test).await;

    let tried = try_unbound(
        &test,
        &organization,
        Asked {
            trier_id: &organization.admin.id,
            identity: Identity {
                issuer: ONE,
                subject: "another person at one",
            },
            session_id: &organization.session_id,
            token_digest: DIGEST,
            made_since: MADE,
        },
    )
    .await;

    assert_eq!(tried.err(), Some(OidcRefusal::NotTheTrier));
    assert_eq!(left(&test).await, before, "nothing changed");
    let theirs = test
        .identities_of_user(&organization.admin.id)
        .await
        .expect("read");
    assert_eq!(theirs.len(), 1, "only the one they had: {theirs:?}");
    test.teardown().await;
}
