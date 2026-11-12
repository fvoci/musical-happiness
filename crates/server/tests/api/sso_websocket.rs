//! The websocket and an organization that takes no password: which connections follow
//! its workspaces, and what becomes of those that did when it begins.

use std::time::Duration;

use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use serde_json::json;

use crate::common::{Client, ROOT_WORKSPACE_ID, Setup, TestServer};
use crate::fake_oidc::FakeProvider;
use crate::oidc::{org_of, server_for, set_provider, with_key_and_private_addresses};
use crate::sso::{
    ADMIN_SUBJECT, Passwords, Two, admin_through_provider, require_sso, through_provider, tie,
    two_organizations,
};
use crate::websocket::{
    Event, PATIENCE, QUIET, Socket, category, follow_block_on, shared_board_with_a_card,
    subscribe_to_workspace,
};

/// Asks to follow the workspace and says whether the connection then hears of it:
/// commands have no acknowledgement, so a message for the user is sent until one
/// arrives, or long enough to say that none does.
async fn follows(th: &TestServer, socket: &mut Socket, workspace_id: &str, user_id: &str) -> bool {
    socket
        .send(json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace_id}))
        .await;
    let probe = category(workspace_id, user_id, "subscription probe");
    for _ in 0..8 {
        th.app.ws().broadcast_category_change(&probe);
        if socket.next_json(Duration::from_millis(100)).await.is_some() {
            while socket.next_json(Duration::from_millis(150)).await.is_some() {}
            return true;
        }
    }
    false
}

/// Whether the server closes the connection, after what was on its way to it.
async fn ends(socket: &mut Socket) -> bool {
    loop {
        match socket.next(PATIENCE).await {
            Event::Closed => return true,
            Event::Nothing => return false,
            Event::Json(_) | Event::Ping => {}
        }
    }
}

/// The connections of the sessions an organization shuts out end when it begins to
/// take no password, and do not follow its workspaces again; the connection of the
/// provider's session goes on, and hears what the others no longer do. Released, the
/// organization is followed with a password again.
#[tokio::test]
async fn a_connection_follows_a_workspace_only_with_a_session_that_opens_it() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    let user1 = ts.client.me().await.id;
    let user2 = ts.client2.me().await.id;
    let mut with_password = Socket::connect_as(&ts.client).await;
    let mut of_another = Socket::connect_as(&ts.client2).await;
    let mut through = Socket::connect_as(&admin).await;
    subscribe_to_workspace(&ts, &mut with_password, ROOT_WORKSPACE_ID, &user1).await;
    subscribe_to_workspace(&ts, &mut of_another, ROOT_WORKSPACE_ID, &user2).await;
    subscribe_to_workspace(&ts, &mut through, ROOT_WORKSPACE_ID, &user1).await;

    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();

    assert!(ends(&mut with_password).await, "the connection is closed");
    assert!(ends(&mut of_another).await, "the connection is closed");
    // The one that stays hears of the workspace; a new one of the password does not.
    let mut again = Socket::connect_as(&ts.client).await;
    assert!(
        !follows(&ts, &mut again, ROOT_WORKSPACE_ID, &user1).await,
        "a password session follows nothing of the organization"
    );
    let probe = category(ROOT_WORKSPACE_ID, &user1, "for the provider's session");
    while through.next_json(QUIET).await.is_some() {}
    ts.app.ws().broadcast_category_change(&probe);
    let heard = through.next_json(PATIENCE).await.expect("it is told");
    assert_eq!(heard["category"]["name"], probe.name, "{heard}");
    assert_eq!(again.next(QUIET).await, Event::Nothing);

    require_sso(&admin, &org_id, &fake, Passwords::Taken)
        .await
        .check_ok();
    assert!(
        follows(&ts, &mut again, ROOT_WORKSPACE_ID, &user1).await,
        "released, the same connection follows it"
    );
    ts.teardown().await;
    fake.stop();
}

/// Whether a connection that asked to follow the block by the board's read token is
/// told of a change of it.
async fn is_told_of(th: &TestServer, socket: &mut Socket, block: &fvoci_model::Block) -> bool {
    for _ in 0..8 {
        th.app
            .ws()
            .broadcast_block_change(ROOT_WORKSPACE_ID, block)
            .await;
        if socket.next_json(Duration::from_millis(100)).await.is_some() {
            while socket.next_json(Duration::from_millis(150)).await.is_some() {}
            return true;
        }
    }
    false
}

/// A board's public link is followed without a session as before, by a reader who is
/// told nobody's identifier. A connection that comes with a session that does not open
/// the board's organization follows no block by the token: the hub tells a member of
/// the board who wrote a block, and that session is no member's there. One that did
/// before ends with the others.
#[tokio::test]
async fn a_public_link_is_followed_by_readers_and_not_by_a_session_that_opens_nothing() {
    let fake = FakeProvider::start().await;
    let ts = Setup::local_mode()
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(with_key_and_private_addresses)
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    let user1 = ts.client.me().await.id;
    let (_board, block, token) = shared_board_with_a_card(&ts, &user1).await;
    let subscription = json!({
        "action": "SUBSCRIBE_BLOCKS",
        "workspaceId": ROOT_WORKSPACE_ID,
        "readToken": token,
        "blockIds": [block.id],
    });
    let (mut reader, _) = follow_block_on(&ts, Socket::connect(&ts).await, &block, &token).await;
    let (mut with_password, as_member) =
        follow_block_on(&ts, Socket::connect_as(&ts.client).await, &block, &token).await;
    assert_eq!(as_member["block"]["createdBy"], json!(user1), "{as_member}");

    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();

    assert!(ends(&mut with_password).await, "the connection is closed");
    ts.app
        .ws()
        .broadcast_block_change(ROOT_WORKSPACE_ID, &block)
        .await;
    let read = reader.next_json(PATIENCE).await.expect("a reader is told");
    assert_eq!(read["block"]["createdBy"], json!(""), "{read}");
    while reader.next_json(QUIET).await.is_some() {}

    let mut again = Socket::connect_as(&ts.client).await;
    again.send(subscription.clone()).await;
    assert!(
        !is_told_of(&ts, &mut again, &block).await,
        "a password session is told nothing by the token"
    );
    let mut through = Socket::connect_as(&admin).await;
    through.send(subscription).await;
    assert!(
        is_told_of(&ts, &mut through, &block).await,
        "the provider's session follows the link as a member does"
    );
    assert!(
        is_told_of(&ts, &mut reader, &block).await,
        "and a reader goes on"
    );
    ts.teardown().await;
    fake.stop();
}

/// The session of another organization's provider follows nothing of an organization
/// that takes no password, and goes on following its own.
#[tokio::test]
async fn the_session_of_another_organizations_provider_follows_nothing() {
    if db_type_of_the_tests() != DbType::Postgres {
        // An SQLite database holds one organization.
        return;
    }
    let fake = FakeProvider::start().await;
    let Two {
        ts,
        ours,
        theirs,
        their_workspace,
    } = two_organizations(&fake).await;
    tie(&ts, &ts.client, &fake, ADMIN_SUBJECT).await;
    let through_ours: Client = through_provider(&ts, &fake, &ours, ADMIN_SUBJECT).await;
    let through_theirs = through_provider(&ts, &fake, &theirs, ADMIN_SUBJECT).await;
    let user1 = ts.client.me().await.id;
    let mut theirs_before = Socket::connect_as(&through_theirs).await;
    subscribe_to_workspace(&ts, &mut theirs_before, ROOT_WORKSPACE_ID, &user1).await;

    require_sso(&through_ours, &ours, &fake, Passwords::Refused)
        .await
        .check_ok();

    assert!(ends(&mut theirs_before).await, "the connection is closed");
    let mut of_theirs = Socket::connect_as(&through_theirs).await;
    assert!(
        !follows(&ts, &mut of_theirs, ROOT_WORKSPACE_ID, &user1).await,
        "not ours, with the session of their provider"
    );
    assert!(
        follows(&ts, &mut of_theirs, &their_workspace, &user1).await,
        "theirs, as before"
    );
    let mut of_ours = Socket::connect_as(&through_ours).await;
    assert!(follows(&ts, &mut of_ours, ROOT_WORKSPACE_ID, &user1).await);
    assert!(
        follows(&ts, &mut of_ours, &their_workspace, &user1).await,
        "theirs takes any session"
    );
    ts.teardown().await;
    fake.stop();
}
