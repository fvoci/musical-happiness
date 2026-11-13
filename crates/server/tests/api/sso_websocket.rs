//! The websocket and an organization that takes no password: which connections follow
//! its workspaces, and what becomes of those that did when it begins.

use std::time::Duration;

use fvoci_model::{Block, new_id};
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use serde_json::{Value, json};

use crate::common::{Client, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer};
use crate::fake_oidc::FakeProvider;
use crate::oidc::{org_of, set_provider, with_key_and_private_addresses};
use crate::rooms::{Peer, paragraphs, table};
use crate::sso::{
    ADMIN_SUBJECT, Passwords, Two, admin_through_provider, require_sso, through_provider, tie,
    two_organizations,
};
use crate::websocket::{
    Event, PATIENCE, QUIET, Socket, category, follow_block_on, shared_board_with_a_card,
    subscribe_to_workspace,
};

/// How often a negative is asked before it is believed: about a second.
const DOUBT: usize = 8;
/// How often a positive is waited for: about `PATIENCE`.
const HOPE: usize = 50;

/// Asks to follow the workspace and says whether the connection then hears of it:
/// commands have no acknowledgement, so a message for the user is sent, `tries` times,
/// until it arrives.
async fn follows(
    th: &TestServer,
    socket: &mut Socket,
    workspace_id: &str,
    user_id: &str,
    tries: usize,
) -> bool {
    socket
        .send(json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace_id}))
        .await;
    let probe = category(workspace_id, user_id, &format!("probe {}", new_id()));
    for _ in 0..tries {
        th.app.ws().broadcast_category_change(&probe);
        while let Some(message) = socket.next_json(Duration::from_millis(100)).await {
            if message["category"]["name"] == probe.name.as_str() {
                while socket.next_json(Duration::from_millis(150)).await.is_some() {}
                return true;
            }
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

/// A server that reaches the fake provider, shares boards by link and keeps metrics,
/// with its organization's provider set and the administrator's session made through
/// it.
async fn server(fake: &FakeProvider) -> (TestServer, String, Client) {
    let ts = Setup::local_mode()
        .metrics_at("127.0.0.1:0")
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(with_key_and_private_addresses)
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, fake).await;
    let admin = admin_through_provider(&ts, fake, &org_id).await;
    (ts, org_id, admin)
}

/// The connections that follow a workspace with a session its organization shuts out
/// end when it begins to take no password, counted as that, and do not follow the
/// workspace again; the connection of the provider's session goes on, and hears what
/// the others no longer do. Released, the organization is followed with a password
/// again.
#[tokio::test]
async fn a_connection_follows_a_workspace_only_with_a_session_that_opens_it() {
    use crate::metrics::{sample, scrape_until};

    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = server(&fake).await;
    let user1 = ts.client.me().await.id;
    let user2 = ts.client2.me().await.id;
    let mut with_password = Socket::connect_as(&ts.client).await;
    let mut of_user2 = Socket::connect_as(&ts.client2).await;
    let mut through = Socket::connect_as(&admin).await;
    let mut following_nothing = Socket::connect_as(&ts.client).await;
    subscribe_to_workspace(&ts, &mut with_password, ROOT_WORKSPACE_ID, &user1).await;
    subscribe_to_workspace(&ts, &mut of_user2, ROOT_WORKSPACE_ID, &user2).await;
    subscribe_to_workspace(&ts, &mut through, ROOT_WORKSPACE_ID, &user1).await;

    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();

    assert!(ends(&mut with_password).await, "the password's is closed");
    assert!(ends(&mut of_user2).await, "and another member's");
    let closed = |body: &str| {
        sample(
            body,
            "fvoci_websocket_disconnections_total",
            &[("reason", "sso_required")],
        )
    };
    let body = scrape_until(&ts, |body| closed(body) == Some(2.0)).await;
    assert_eq!(closed(&body), Some(2.0), "the two, and for that: {body}");
    // The one that followed nothing of the organization stays, and follows none now.
    assert!(
        !follows(
            &ts,
            &mut following_nothing,
            ROOT_WORKSPACE_ID,
            &user1,
            DOUBT
        )
        .await,
        "a password session follows nothing of the organization"
    );
    assert!(
        follows(&ts, &mut through, ROOT_WORKSPACE_ID, &user1, HOPE).await,
        "the provider's session goes on hearing of it"
    );
    assert_eq!(following_nothing.next(QUIET).await, Event::Nothing);

    require_sso(&admin, &org_id, &fake, Passwords::Taken)
        .await
        .check_ok();
    assert!(
        follows(&ts, &mut following_nothing, ROOT_WORKSPACE_ID, &user1, HOPE).await,
        "released, the same connection follows it"
    );
    ts.teardown().await;
    fake.stop();
}

/// What the connection is told of a change of the block, which it asked to follow by
/// the board's read token.
async fn told_of(th: &TestServer, socket: &mut Socket, block: &Block) -> Value {
    // What it was told before is not the answer.
    while socket.next_json(QUIET).await.is_some() {}
    for _ in 0..HOPE {
        th.app
            .ws()
            .broadcast_block_change(ROOT_WORKSPACE_ID, block)
            .await;
        while let Some(message) = socket.next_json(Duration::from_millis(100)).await {
            if message["block"]["id"] == block.id.as_str() {
                while socket.next_json(Duration::from_millis(150)).await.is_some() {}
                return message;
            }
        }
    }
    panic!("the connection was never told of the block");
}

/// A board's public link is followed as before when its organization takes no
/// password: by anybody, as a reader, who is told nobody's identifier. A member of the
/// board who follows it so is told what a member is told only with a session that
/// opens the organization; with another, also one that followed before, what a reader
/// is. No such connection is closed: it follows no workspace.
#[tokio::test]
async fn a_public_link_is_followed_as_a_reader_by_a_session_that_opens_nothing() {
    let fake = FakeProvider::start().await;
    let (ts, org_id, admin) = server(&fake).await;
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

    let wrote = |message: Value| message["block"]["createdBy"].clone();
    assert_eq!(wrote(told_of(&ts, &mut reader, &block).await), json!(""));
    assert_eq!(
        wrote(told_of(&ts, &mut with_password, &block).await),
        json!(""),
        "the member's password session, which followed before, reads as anybody now"
    );
    let mut again = Socket::connect_as(&ts.client).await;
    again.send(subscription.clone()).await;
    assert_eq!(
        wrote(told_of(&ts, &mut again, &block).await),
        json!(""),
        "and so does one that follows since"
    );
    let mut through = Socket::connect_as(&admin).await;
    through.send(subscription).await;
    assert_eq!(
        wrote(told_of(&ts, &mut through, &block).await),
        json!(user1),
        "the provider's session is told what a member is"
    );

    require_sso(&admin, &org_id, &fake, Passwords::Taken)
        .await
        .check_ok();
    assert_eq!(
        wrote(told_of(&ts, &mut with_password, &block).await),
        json!(user1),
        "released, the same connection is a member's again"
    );
    ts.teardown().await;
    fake.stop();
}

/// The session of another organization's provider follows nothing of an organization
/// that takes no password, and goes on following its own; a connection that follows
/// only another organization is left alone.
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
    let mut only_theirs = Socket::connect_as(&ts.client).await;
    subscribe_to_workspace(&ts, &mut only_theirs, &their_workspace, &user1).await;

    require_sso(&through_ours, &ours, &fake, Passwords::Refused)
        .await
        .check_ok();

    assert!(ends(&mut theirs_before).await, "the connection is closed");
    let mut of_theirs = Socket::connect_as(&through_theirs).await;
    assert!(
        !follows(&ts, &mut of_theirs, ROOT_WORKSPACE_ID, &user1, DOUBT).await,
        "not ours, with the session of their provider"
    );
    assert!(
        follows(&ts, &mut of_theirs, &their_workspace, &user1, HOPE).await,
        "theirs, as before"
    );
    assert!(
        follows(&ts, &mut only_theirs, &their_workspace, &user1, HOPE).await,
        "who followed theirs alone with a password still does"
    );
    let mut of_ours = Socket::connect_as(&through_ours).await;
    assert!(
        follows(&ts, &mut of_ours, ROOT_WORKSPACE_ID, &user1, HOPE).await,
        "ours, with the session of our provider"
    );
    assert!(
        follows(&ts, &mut of_ours, &their_workspace, &user1, HOPE).await,
        "theirs takes any session"
    );
    ts.teardown().await;
    fake.stop();
}

/// The room of a card's body is behind the wall, as the card is: when its organization
/// begins to take no password, the connection of a password session is out of the
/// room by the time the request answers (the room's own asking is an hour away), as
/// for a card that is not there: what it sends then goes to nobody. It comes into no
/// room of it again, nor does a new one; the connection of the provider's session
/// stays and writes. Released, a password connection comes in again.
#[tokio::test]
async fn the_room_of_a_body_is_behind_the_wall() {
    let fake = FakeProvider::start().await;
    let ts = Setup::native()
        .trusting_oidc_provider(&fake.certificate_der)
        .configure(|config| {
            with_key_and_private_addresses(config);
            config.collab.collab_recheck_seconds = 3600;
        })
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let t = table(&ts, &paragraphs(&["first"])).await;
    let admin = admin_through_provider(&ts, &fake, &org_id).await;
    let mut with_password = Peer::connect(&ts.client, 101).await;
    let mut through = Peer::connect(&admin, 102).await;
    with_password.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    through.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    with_password.write_acked("with a password").await;

    require_sso(&admin, &org_id, &fake, Passwords::Refused)
        .await
        .check_ok();
    with_password.write("after the wall").await;
    let left = with_password.text("DOC_LEFT").await;
    assert_eq!(left["reason"], json!("not_found"), "{left}");
    with_password.settle().await;
    assert!(
        with_password
            .unread()
            .iter()
            .all(|message| message["action"] != json!("DOC_ACK")),
        "nothing is taken after the wall: {:?}",
        with_password.unread()
    );
    let mut late = Peer::connect(&ts.client, 103).await;
    for peer in [&mut with_password, &mut late] {
        let refused = peer.join(TEST_WORKSPACE_ID, &t.card_id).await;
        assert_eq!(refused["action"], json!("DOC_REFUSED"), "{refused}");
        assert_eq!(refused["reason"], json!("not_found"), "{refused}");
    }
    through.write_acked("through the provider").await;
    assert_eq!(
        through.lines(),
        ["first", "with a password", "through the provider"]
    );

    require_sso(&admin, &org_id, &fake, Passwords::Taken)
        .await
        .check_ok();
    late.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    ts.teardown().await;
    fake.stop();
}
