//! Personal API tokens: stored by their digest, opening the workspaces they name while
//! their owner works in them, and ended with what ends the owner's sessions.

use fvoci_model::{ApiTokenScope, OrgRole, Session, Workspace, WorkspaceRole, new_id};
use fvoci_store::{
    DbType, GrantedWorkspace, Identity, Issuance, Store, TokenOrigin, TokenToIssue, Unlinked,
    WorkspaceTrashing,
};

use crate::common::{self, TestStore};

/// A time in milliseconds the tests count from, before any token ends.
const T0: i64 = 1_800_000_000_000;

/// When the tokens of the tests end.
const ENDS: i64 = T0 + 90 * 24 * 60 * 60 * 1000;

/// Stores a write token of `user_id` under `digest` that reaches `workspaces` of the
/// handle's organization.
async fn issue(
    store: &Store,
    user_id: &str,
    digest: &str,
    workspaces: &[&str],
    origin: Option<TokenOrigin<'_>>,
) -> Issuance {
    let org_id = store
        .tenant()
        .org_id()
        .expect("a handle of an organization");
    let granted: Vec<GrantedWorkspace<'_>> = workspaces
        .iter()
        .map(|workspace_id| GrantedWorkspace {
            workspace_id,
            org_id: &org_id,
        })
        .collect();
    let id = new_id();
    store
        .issue_api_token(&TokenToIssue {
            id: &id,
            user_id,
            name: "agent",
            token_prefix: "fvoci_pat_abcdef",
            token_digest: digest,
            scope: ApiTokenScope::Write,
            workspaces: &granted,
            allow_personal_space: false,
            notify_self: true,
            origin,
            created_at: T0,
            expires_at: ENDS,
        })
        .await
        .expect("the token is stored")
}

/// The identifier of a token stored as [`issue`] stores it.
async fn issued(store: &Store, user_id: &str, digest: &str, workspaces: &[&str]) -> String {
    match issue(store, user_id, digest, workspaces, None).await {
        Issuance::Issued(token) => token.id,
        refused @ Issuance::NoLongerWorksIn(_) => panic!("{refused:?}"),
    }
}

/// Whether the token stored under `digest` opens anything at [`T0`].
async fn is_live(store: &Store, digest: &str) -> bool {
    store
        .live_api_token(digest, T0 + 1)
        .await
        .expect("looked up")
        .is_some()
}

/// Whether the token reaches the workspace, asked of the handle's organization.
async fn reaches(store: &Store, token_id: &str, workspace_id: &str) -> bool {
    store
        .api_token_reach(token_id, workspace_id)
        .await
        .expect("asked")
        .is_some()
}

/// An administrator of the test's organization.
async fn org_admin(test: &TestStore, username: &str) -> String {
    let user_id = common::user(test, username).await;
    for statement in [
        format!(
            "UPDATE organization_members SET role = '{}' WHERE user_id = '{user_id}'",
            OrgRole::Admin.as_str()
        ),
        format!("UPDATE users SET is_admin = TRUE WHERE id = '{user_id}'"),
    ] {
        test.execute_unchecked(&statement)
            .await
            .expect("the user administers the organization");
    }
    user_id
}

#[tokio::test]
async fn a_token_opens_the_workspaces_it_names_until_it_is_taken_back() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let named = common::workspace_of(store, &[&alice]).await;
    let other = common::workspace_of(store, &[&alice]).await;
    let token_id = issued(store, &alice, "digest-alice", &[&named]).await;

    let live = store
        .live_api_token("digest-alice", T0 + 1)
        .await
        .expect("looked up")
        .expect("the token is live");
    assert_eq!(
        (live.id.as_str(), live.user_id.as_str()),
        (token_id.as_str(), alice.as_str())
    );
    assert_eq!(live.scope, ApiTokenScope::Write);
    assert!(
        reaches(store, &token_id, &named).await,
        "the workspace it names"
    );
    assert!(
        !reaches(store, &token_id, &other).await,
        "not one it does not name"
    );
    assert!(
        store
            .live_api_token("digest-alice", ENDS)
            .await
            .expect("looked up")
            .is_none(),
        "past its end it opens nothing"
    );
    let listed = store.api_tokens_of_user(&alice).await.expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0]
            .workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.as_str())
            .collect::<Vec<_>>(),
        [named.as_str()]
    );

    let bob = common::user(store, "bob").await;
    assert!(
        !store
            .revoke_api_token(&bob, &token_id, T0 + 2)
            .await
            .expect("asked"),
        "nobody takes back another's token"
    );
    assert!(
        store
            .revoke_api_token(&alice, &token_id, T0 + 2)
            .await
            .expect("taken back")
    );
    assert!(
        !is_live(store, "digest-alice").await,
        "taken back, it opens nothing"
    );
    assert!(
        store
            .api_tokens_of_user(&alice)
            .await
            .expect("listed")
            .is_empty(),
        "and is not listed"
    );

    test.teardown().await;
}

#[tokio::test]
async fn a_token_is_given_no_workspace_its_maker_does_not_work_in() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let theirs = common::workspace_of(store, &[&alice]).await;
    let not_theirs = common::workspace(store).await;
    let as_guest = common::workspace(store).await;
    store
        .add_workspace_member(&as_guest, &alice, WorkspaceRole::Guest)
        .await
        .expect("a guest of it");

    for (refused, digest) in [(&not_theirs, "digest-none"), (&as_guest, "digest-guest")] {
        let issuance = issue(store, &alice, digest, &[&theirs, refused], None).await;
        assert_eq!(issuance, Issuance::NoLongerWorksIn(refused.clone()));
        assert!(!is_live(store, digest).await, "nothing of the token stays");
    }
    let listed = store.api_tokens_of_user(&alice).await.expect("listed");
    assert_eq!(listed, [], "no token was made");

    test.teardown().await;
}

#[tokio::test]
async fn what_ends_the_sessions_of_an_account_ends_its_tokens() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let bob = common::user(store, "bob").await;
    let workspace = common::workspace_of(store, &[&alice, &bob]).await;
    issued(store, &alice, "digest-alice", &[&workspace]).await;
    issued(store, &bob, "digest-bob", &[&workspace]).await;
    let kept = Session {
        id: new_id(),
        user_id: alice.clone(),
        created_at: T0,
        last_used_at: T0,
        expires_at: ENDS,
        ..Session::default()
    };
    assert!(
        store
            .create_session(&kept, "alice-here", "")
            .await
            .expect("the session is stored")
    );

    let set = store
        .change_user_password(&alice, "", "new-hash", &kept.id, "alice-here-anew")
        .await
        .expect("the password is set");
    assert!(set.is_some(), "the hash was the one seen");
    assert!(
        !is_live(store, "digest-alice").await,
        "a password set ends the tokens, the session it was set through stays"
    );
    assert!(is_live(store, "digest-bob").await, "another's stay");

    store
        .request_account_deletion(&bob, "", T0 + 1, T0 + 2)
        .await
        .expect("asked");
    assert!(
        !is_live(store, "digest-bob").await,
        "an account put out of use"
    );

    test.teardown().await;
}

#[tokio::test]
async fn a_token_stops_reaching_a_workspace_its_owner_no_longer_works_in() {
    let test = common::setup().await;
    let store = &test.store;
    let admin = org_admin(&test, "admin").await;
    let alice = common::user(store, "alice").await;
    let left = common::workspace_of(store, &[&admin, &alice]).await;
    let guest_of = common::workspace_of(store, &[&admin, &alice]).await;
    let trashed = Workspace {
        id: new_id(),
        title: "trashed".to_owned(),
        ..Workspace::default()
    };
    store
        .create_named_workspace(&trashed, &admin)
        .await
        .expect("the workspace is made");
    store
        .add_workspace_member(&trashed.id, &alice, WorkspaceRole::Member)
        .await
        .expect("a member of it");
    let kept = common::workspace_of(store, &[&alice]).await;
    let token_id = issued(
        store,
        &alice,
        "digest",
        &[&left, &guest_of, &trashed.id, &kept],
    )
    .await;

    store
        .remove_workspace_member(&left, &alice, &admin)
        .await
        .expect("removed");
    store
        .change_workspace_role(
            &guest_of,
            &alice,
            (WorkspaceRole::Member, WorkspaceRole::Guest),
            &admin,
        )
        .await
        .expect("made a guest");
    assert_eq!(
        store
            .trash_workspace(&trashed.id, &admin)
            .await
            .expect("trashed"),
        WorkspaceTrashing::Done
    );

    for gone in [&left, &guest_of, &trashed.id] {
        assert!(!reaches(store, &token_id, gone).await, "{gone}");
    }
    assert!(
        reaches(store, &token_id, &kept).await,
        "the one still worked in"
    );
    assert!(is_live(store, "digest").await, "the token itself stays");

    test.teardown().await;
}

#[tokio::test]
async fn a_token_made_through_a_provider_ends_with_an_identity_unlinked() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let workspace = common::workspace_of(store, &[&alice]).await;
    let provider_id = new_id();
    let through = TokenOrigin {
        provider_id: &provider_id,
        revision: 3,
    };
    let Issuance::Issued(_) =
        issue(store, &alice, "digest-sso", &[&workspace], Some(through)).await
    else {
        panic!("the token is stored");
    };
    issued(store, &alice, "digest-plain", &[&workspace]).await;
    let live = store
        .live_api_token("digest-sso", T0 + 1)
        .await
        .expect("looked up")
        .expect("live");
    assert_eq!(
        live.origin_provider_id.as_deref(),
        Some(provider_id.as_str())
    );

    let (kept, unlinked) = (
        Identity {
            issuer: "https://one.example.com",
            subject: "alice",
        },
        Identity {
            issuer: "https://two.example.com",
            subject: "alice",
        },
    );
    for identity in [kept, unlinked] {
        store
            .add_identity(&alice, identity, "")
            .await
            .expect("an identity is tied");
    }
    let outcome = store
        .unlink_identity(&alice, "", unlinked)
        .await
        .expect("asked");
    assert!(matches!(outcome, Unlinked::Removed(_)), "{outcome:?}");
    assert!(
        !is_live(store, "digest-sso").await,
        "made through the provider"
    );
    assert!(is_live(store, "digest-plain").await, "made otherwise");

    test.teardown().await;
}

#[tokio::test]
async fn an_administrator_sees_and_takes_back_the_tokens_of_their_organization() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let workspace = common::workspace_of(store, &[&alice]).await;
    let token_id = issued(store, &alice, "digest", &[&workspace]).await;
    let listed = store
        .api_tokens_of_organization(T0 + 1)
        .await
        .expect("listed");
    assert_eq!(
        listed
            .iter()
            .map(|token| token.id.as_str())
            .collect::<Vec<_>>(),
        [token_id.as_str()]
    );
    // An SQLite database holds one organization: the wall is PostgreSQL's to keep.
    if common::db_type() == DbType::Postgres {
        let system = test.system();
        let (elsewhere, _) = common::organization(&system, "elsewhere").await;
        let elsewhere = system.for_org(&elsewhere).expect("an identifier");
        assert!(
            elsewhere
                .api_tokens_of_organization(T0 + 1)
                .await
                .expect("listed")
                .is_empty(),
            "another organization sees none of them"
        );
        assert!(
            !elsewhere
                .revoke_api_token_of_organization(&token_id, T0 + 2)
                .await
                .expect("asked"),
            "nor takes one back"
        );
        assert!(!reaches(&elsewhere, &token_id, &workspace).await);
    }
    assert!(
        store
            .revoke_api_token_of_organization(&token_id, T0 + 2)
            .await
            .expect("taken back")
    );
    assert!(!is_live(store, "digest").await);

    test.teardown().await;
}
