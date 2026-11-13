//! Revocation reaches open sockets, including a subscription not registered yet.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Block, BlockType, BoardType, OrgRole, WorkspaceRole, new_id};
use fvoci_server::ws::SubscriptionPause;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

use crate::common::{ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer};
use crate::org_admin::{make_workspace, org_of};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;
const WAIT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
enum Moment {
    Registered,
    Validated,
}

async fn subscribe(
    ts: &TestServer,
    socket: &mut Socket,
    command: Value,
    moment: Moment,
) -> Option<SubscriptionPause> {
    let pause = match moment {
        Moment::Registered => None,
        Moment::Validated => Some(ts.app.ws().pause_next_subscription()),
    };
    socket
        .send(Message::Text(command.to_string().into()))
        .await
        .expect("subscription sent");
    if let Some(pause) = &pause {
        tokio::time::timeout(WAIT, pause.reached())
            .await
            .expect("the subscription reaches the registration seam");
    } else {
        barrier(socket).await;
    }
    pause
}

async fn barrier(socket: &mut Socket) {
    socket
        .send(Message::Ping(vec![9].into()))
        .await
        .expect("barrier sent");
    tokio::time::timeout(WAIT, async {
        while let Some(message) = socket.next().await {
            if matches!(
                message.expect("a frame before revocation"),
                Message::Pong(_)
            ) {
                return;
            }
        }
        panic!("the socket closed before subscribing");
    })
    .await
    .expect("the subscription is registered");
}

async fn closed(socket: &mut Socket) {
    tokio::time::timeout(WAIT, async {
        while let Some(message) = socket.next().await {
            if let Message::Close(frame) = message.expect("the close is a protocol frame") {
                let frame = frame.expect("the peer is told why access ended");
                assert_eq!(
                    u16::from(frame.code),
                    1008,
                    "access revocation is a policy close"
                );
                assert!(!frame.reason.is_empty(), "the close explains revocation");
                return;
            }
        }
        panic!("the connection ended without telling the peer why");
    })
    .await
    .expect("revocation promptly closes the connection");
}

#[derive(Clone, Copy, Debug)]
enum EndShare {
    Off,
    Rotate,
    BoardTrash,
    WorkspaceTrash,
    OrganizationOff,
}

impl EndShare {
    const ALL: [Self; 5] = [
        Self::Off,
        Self::Rotate,
        Self::BoardTrash,
        Self::WorkspaceTrash,
        Self::OrganizationOff,
    ];

    async fn apply(self, ts: &TestServer, board: &str) {
        let at = format!("/w/{TEST_WORKSPACE_ID}/boards/{board}");
        match self {
            Self::Off => ts
                .client
                .post(&format!("{at}/sharing"), &json!({"enabled": false}))
                .await
                .check_ok(),
            Self::Rotate => ts
                .client
                .post(
                    &format!("{at}/sharing"),
                    &json!({"enabled": true, "regenerate": true}),
                )
                .await
                .check_ok(),
            Self::BoardTrash => ts.client.delete(&at).await.check_no_content(),
            Self::WorkspaceTrash => {
                let org = org_of(ts).await;
                ts.client
                    .delete(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}"))
                    .await
                    .check_no_content();
            }
            Self::OrganizationOff => {
                let org = org_of(ts).await;
                ts.client
                    .patch(&format!("/org/{org}"), &json!({"publicLinks": false}))
                    .await
                    .check_ok();
            }
        }
    }
}

async fn public_subscription_ends(ending: EndShare, moment: Moment) {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let user = ts.client.me().await.id;
    let board = ts.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let mut card = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    ts.app
        .store()
        .insert_block(&mut card, &user)
        .await
        .expect("a card is stored");
    let token = ts.share(&board.id).await;
    let mut socket = ts.anonymous().as_browser().connect_websocket().await;
    let pause = subscribe(
        &ts,
        &mut socket,
        json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": TEST_WORKSPACE_ID,
            "blockIds": [card.id], "readToken": token,
        }),
        moment,
    )
    .await;
    ending.apply(&ts, &board.id).await;
    if let Some(pause) = &pause {
        pause.resume();
    }
    closed(&mut socket).await;
    ts.teardown().await;
}

#[tokio::test]
async fn public_subscriptions_close_when_their_access_ends() {
    for ending in EndShare::ALL {
        public_subscription_ends(ending, Moment::Registered).await;
    }
}

#[tokio::test]
async fn public_access_ending_between_validation_and_registration_cannot_be_missed() {
    for ending in EndShare::ALL {
        public_subscription_ends(ending, Moment::Validated).await;
    }
}

#[derive(Clone, Copy)]
enum EndMembership {
    Guest,
    Remove,
}

async fn workspace_subscription_ends(ending: EndMembership, moment: Moment) {
    let ts = Setup::native().start().await.init_basic().await;
    let user = ts.client2.me().await.id;
    let mut socket = ts.client2.connect_websocket().await;
    let pause = subscribe(
        &ts,
        &mut socket,
        json!({
            "action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID,
        }),
        moment,
    )
    .await;
    let at = format!("/w/{ROOT_WORKSPACE_ID}/members/{user}");
    match ending {
        EndMembership::Guest => ts
            .client
            .put(&at, &json!({"role": "guest"}))
            .await
            .check_ok(),
        EndMembership::Remove => ts.client.delete(&at).await.check_no_content(),
    }
    if let Some(pause) = &pause {
        pause.resume();
    }
    closed(&mut socket).await;
    ts.teardown().await;
}

#[tokio::test]
async fn removed_and_demoted_members_are_told_their_subscription_ended() {
    for ending in [EndMembership::Guest, EndMembership::Remove] {
        for moment in [Moment::Registered, Moment::Validated] {
            workspace_subscription_ends(ending, moment).await;
        }
    }
}

#[tokio::test]
async fn withdrawing_an_administrators_joining_closes_the_subscription() {
    for prior in [
        None,
        Some(WorkspaceRole::Member),
        Some(WorkspaceRole::Guest),
    ] {
        let ts = Setup::native().start().await.init_basic().await;
        let org = org_of(&ts).await;
        let workspace = make_workspace(&ts, &org, "Joined workspace").await;
        if let Some(role) = prior {
            let user = ts.client.me().await.id;
            ts.app
                .store()
                .add_workspace_member(&workspace, &user, role)
                .await
                .expect("prior membership");
        }
        let at = format!("/org/{org}/workspaces/{workspace}");
        ts.client.post_empty(&format!("{at}/join")).await.check_ok();
        let mut socket = ts.client.connect_websocket().await;
        subscribe(
            &ts,
            &mut socket,
            json!({
                "action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace,
            }),
            Moment::Registered,
        )
        .await;
        ts.client
            .post_empty(&format!("{at}/leave"))
            .await
            .check_ok();
        closed(&mut socket).await;
        ts.teardown().await;
    }
}

#[tokio::test]
async fn demoting_an_organization_administrator_closes_the_joined_subscription() {
    for prior in [
        None,
        Some(WorkspaceRole::Member),
        Some(WorkspaceRole::Guest),
    ] {
        let ts = Setup::native().start().await.init_basic().await;
        let org = org_of(&ts).await;
        let admin = ts.client.me().await.id;
        let other = ts.client2.me().await.id;
        let workspace = make_workspace(&ts, &org, "Joined workspace").await;
        if let Some(role) = prior {
            let user = ts.client.me().await.id;
            ts.app
                .store()
                .add_workspace_member(&workspace, &user, role)
                .await
                .expect("prior membership");
        }
        ts.client
            .post_empty(&format!("/org/{org}/workspaces/{workspace}/join"))
            .await
            .check_ok();
        ts.client
            .put(
                &format!("/org/{org}/members/{other}/role"),
                &json!({"role": "admin"}),
            )
            .await
            .check_no_content();
        let mut socket = ts.client.connect_websocket().await;
        subscribe(
            &ts,
            &mut socket,
            json!({
                "action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace,
            }),
            Moment::Registered,
        )
        .await;
        ts.client2
            .put(
                &format!("/org/{org}/members/{admin}/role"),
                &json!({"role": "member"}),
            )
            .await
            .check_no_content();
        closed(&mut socket).await;
        ts.teardown().await;
    }
}

#[tokio::test]
async fn an_unrelated_revocation_does_not_end_a_validating_subscription() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let user = ts.client.me().await.id;
    let board = ts.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let unrelated = ts.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    ts.share(&unrelated.id).await;
    let mut card = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    ts.app
        .store()
        .insert_block(&mut card, &user)
        .await
        .expect("card stored");
    let token = ts.share(&board.id).await;
    let mut socket = ts.anonymous().as_browser().connect_websocket().await;
    let pause = subscribe(
        &ts,
        &mut socket,
        json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": TEST_WORKSPACE_ID,
            "blockIds": [card.id], "readToken": token,
        }),
        Moment::Validated,
    )
    .await
    .expect("the seam is installed");
    EndShare::Off.apply(&ts, &unrelated.id).await;
    pause.resume();
    barrier(&mut socket).await;
    ts.app
        .ws()
        .broadcast_block_change(TEST_WORKSPACE_ID, &card)
        .await;
    let message = tokio::time::timeout(WAIT, socket.next())
        .await
        .expect("valid subscription receives its change")
        .expect("socket stays open")
        .expect("a frame");
    let Message::Text(text) = message else {
        panic!("expected a change, got {message:?}");
    };
    let value: Value = serde_json::from_str(&text).expect("JSON update");
    assert_eq!(
        value["block"]["id"], card.id,
        "unrelated revocation preserves the subscription"
    );
    ts.teardown().await;
}

#[tokio::test]
async fn trashing_a_workspace_tells_its_subscribed_members() {
    let ts = Setup::native().start().await.init_basic().await;
    let mut socket = ts.client2.connect_websocket().await;
    subscribe(
        &ts,
        &mut socket,
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": TEST_WORKSPACE_ID}),
        Moment::Registered,
    )
    .await;
    let org = org_of(&ts).await;
    ts.client
        .delete(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}"))
        .await
        .check_no_content();
    closed(&mut socket).await;
    ts.teardown().await;
}

#[tokio::test]
async fn joining_as_an_administrator_ends_a_subscription_made_under_the_prior_role() {
    for role in [WorkspaceRole::Member, WorkspaceRole::Guest] {
        let ts = Setup::native().start().await.init_basic().await;
        let org = org_of(&ts).await;
        let user = ts.client.me().await.id;
        let workspace = make_workspace(&ts, &org, "Joined after subscribing").await;
        ts.app
            .store()
            .add_workspace_member(&workspace, &user, role)
            .await
            .expect("prior membership");
        let mut socket = ts.client.connect_websocket().await;
        subscribe(
            &ts,
            &mut socket,
            json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace}),
            Moment::Registered,
        )
        .await;
        ts.client
            .post_empty(&format!("/org/{org}/workspaces/{workspace}/join"))
            .await
            .check_ok();
        closed(&mut socket).await;
        ts.teardown().await;
    }
}

#[derive(Debug, Clone, Default)]
struct Reads(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl fvoci_store::Observer for Reads {
    fn called(&self, call: fvoci_store::Call) {
        if call.access == fvoci_store::Access::Read {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn waited_for_connection(&self, _: fvoci_store::DbType, _: Duration) {}
}

async fn updated(socket: &mut Socket, phase: &str) {
    tokio::time::timeout(WAIT, async {
        while let Some(frame) = socket.next().await {
            if let Message::Text(text) = frame.expect("update frame") {
                let message: Value = serde_json::from_str(&text).expect("JSON update");
                assert_eq!(message["action"], "UPDATE_BLOCK", "{phase}: a block update");
                return;
            }
        }
        panic!("socket ended before update");
    })
    .await
    .unwrap_or_else(|error| panic!("{phase}: an authorized update arrives: {error}"));
}

async fn stored_card(ts: &TestServer, board: &str, user: &str) -> Block {
    let mut block = Block {
        id: new_id(),
        board_id: board.to_owned(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    ts.app
        .store()
        .insert_block(&mut block, user)
        .await
        .expect("another card stored");
    block
}

/// Two hundred dispatches with the cache warm read nothing of the store; how long they
/// take is printed for the report.
#[expect(clippy::print_stdout, reason = "the measurement is for the report")]
async fn warm_dispatch_reads_nothing(
    ts: &TestServer,
    reads: &Reads,
    block: &Block,
    [member, reader]: [&mut Socket; 2],
) {
    use std::sync::atomic::Ordering;
    reads.0.store(0, Ordering::Relaxed);
    let started = std::time::Instant::now();
    let mut dispatch_time = Duration::ZERO;
    for _ in 0..200 {
        let dispatch = std::time::Instant::now();
        ts.app
            .ws()
            .broadcast_block_change(ROOT_WORKSPACE_ID, block)
            .await;
        dispatch_time += dispatch.elapsed();
        updated(member, "member").await;
        updated(reader, "reader").await;
    }
    let count = reads.0.load(Ordering::Relaxed);
    println!(
        "dispatch measurement: db={:?}, messages=200, recipients=2, reads={count}, dispatch_us={}, elapsed_us={}",
        ts.app.store().db_type(),
        dispatch_time.as_micros(),
        started.elapsed().as_micros()
    );
    assert_eq!(count, 0, "warm dispatch performs no database round trips");
}

/// Another organization's authorization write, and a user renaming themselves, leave
/// this organization's revision, and a dispatch after them reads nothing of the store.
async fn writes_elsewhere_read_nothing(
    ts: &TestServer,
    revision: fvoci_store::VisibilityVersion,
    reads: &Reads,
    block: &Block,
    [member, reader]: [&mut Socket; 2],
) {
    use std::sync::atomic::Ordering;
    ts.app
        .store()
        .for_org(&new_id())
        .expect("an identifier")
        .execute_unchecked(&format!(
            "UPDATE boards SET title = title WHERE id = '{}'",
            new_id()
        ))
        .await
        .expect("a write of another organization");
    assert_eq!(
        revision,
        ts.app.store().visibility_version().await,
        "another organization's write is not this one's revision"
    );
    ts.client
        .patch("/users/me", &json!({"displayName": "renamed"}))
        .await
        .check_ok();
    assert_eq!(
        revision,
        ts.app.store().visibility_version().await,
        "a user's own row is no authorization"
    );
    reads.0.store(0, Ordering::Relaxed);
    ts.app
        .ws()
        .broadcast_block_change(ROOT_WORKSPACE_ID, block)
        .await;
    updated(member, "member").await;
    updated(reader, "reader").await;
    assert_eq!(
        reads.0.load(Ordering::Relaxed),
        0,
        "dispatch after another organization's write reads nothing"
    );
}

#[tokio::test]
async fn dispatch_reuses_authorization_reads_until_visibility_changes() {
    use std::sync::atomic::Ordering;
    let ts = Setup::local_mode().start().await.init_basic().await;
    let user = ts.client.me().await.id;
    let board = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Private).await;
    assert!(
        ts.app.finish_background(WAIT).await,
        "fixture announcements finish before subscribing"
    );
    let mut block = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    ts.app
        .store()
        .insert_block(&mut block, &user)
        .await
        .expect("card stored");
    let token = ts.share(&board.id).await;
    let mut member = ts.client.connect_websocket().await;
    let mut reader = ts.anonymous().as_browser().connect_websocket().await;
    subscribe(
        &ts,
        &mut member,
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID}),
        Moment::Registered,
    )
    .await;
    subscribe(&ts, &mut reader, json!({"action": "SUBSCRIBE_BLOCKS", "workspaceId": ROOT_WORKSPACE_ID, "blockIds": [block.id], "readToken": token}), Moment::Registered).await;
    let reads = Reads::default();
    assert!(
        ts.app.store().observe(Box::new(reads.clone())),
        "observer installed once"
    );
    ts.app
        .ws()
        .broadcast_block_change(ROOT_WORKSPACE_ID, &block)
        .await;
    updated(&mut member, "member").await;
    updated(&mut reader, "reader").await;
    assert!(
        reads.0.load(Ordering::Relaxed) > 0,
        "cold dispatch reads permissions"
    );
    let revision = ts.app.store().visibility_version().await;
    assert!(
        revision.cacheable(),
        "normal setup leaves a cacheable revision"
    );
    // A real card write, including its history, must not flush permission caches.
    let _next = stored_card(&ts, &board.id, &user).await;
    assert_eq!(revision, ts.app.store().visibility_version().await);
    warm_dispatch_reads_nothing(&ts, &reads, &block, [&mut member, &mut reader]).await;
    writes_elsewhere_read_nothing(&ts, revision, &reads, &block, [&mut member, &mut reader]).await;
    ts.app.store().execute_unchecked(&format!("DELETE FROM workspace_members WHERE user_id = '{user}' AND workspace_id = '{ROOT_WORKSPACE_ID}'")).await.expect("membership revoked below app hooks");
    assert_ne!(revision, ts.app.store().visibility_version().await);
    // User-directed messages must also recheck, not only board updates.
    ts.app
        .ws()
        .broadcast_category_reorder(ROOT_WORKSPACE_ID, &user, &[])
        .await;
    closed(&mut member).await;
    ts.app
        .ws()
        .broadcast_block_change(ROOT_WORKSPACE_ID, &block)
        .await;
    updated(&mut reader, "reader").await;
    ts.app
        .store()
        .set_sharing(&board.id, fvoci_store::SharingChange::Off, "", &user)
        .await
        .expect("sharing revoked below app hooks");
    ts.app
        .ws()
        .broadcast_block_change(ROOT_WORKSPACE_ID, &block)
        .await;
    closed(&mut reader).await;
    ts.teardown().await;
}

#[tokio::test]
async fn revocation_between_dispatch_check_and_queue_is_rechecked() {
    for public in [false, true] {
        let ts = Setup::local_mode().start().await.init_basic().await;
        let user = ts.client.me().await.id;
        let board = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Private).await;
        assert!(
            ts.app.finish_background(WAIT).await,
            "fixture announcements finish before subscribing"
        );
        let mut block = Block {
            id: new_id(),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            ..Block::default()
        };
        ts.app
            .store()
            .insert_block(&mut block, &user)
            .await
            .expect("stored card");
        let mut socket = if public {
            ts.anonymous().as_browser().connect_websocket().await
        } else {
            ts.client.connect_websocket().await
        };
        let command = if public {
            let token = ts.share(&board.id).await;
            json!({"action": "SUBSCRIBE_BLOCKS", "workspaceId": ROOT_WORKSPACE_ID, "blockIds": [block.id], "readToken": token})
        } else {
            json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID})
        };
        subscribe(&ts, &mut socket, command, Moment::Registered).await;
        let pause = ts.app.ws().pause_next_dispatch();
        let app = ts.app.clone();
        let sending = tokio::spawn(async move {
            app.ws()
                .broadcast_block_change(ROOT_WORKSPACE_ID, &block)
                .await;
        });
        tokio::time::timeout(WAIT, pause.reached())
            .await
            .expect("dispatch passed authorization");
        if public {
            ts.app
                .store()
                .set_sharing(&board.id, fvoci_store::SharingChange::Off, "", &user)
                .await
                .expect("sharing ends without connection hook");
        } else {
            ts.app
                .store()
                .execute_unchecked(&format!(
                    "DELETE FROM board_members WHERE board_id = '{}' AND user_id = '{user}'",
                    board.id
                ))
                .await
                .expect("board membership ends without connection hook");
        }
        pause.resume();
        sending.await.expect("dispatch ends");
        if public {
            closed(&mut socket).await;
        } else {
            ts.app
                .ws()
                .broadcast_category_reorder(ROOT_WORKSPACE_ID, &user, &[])
                .await;
            tokio::time::timeout(WAIT, async {
                while let Some(frame) = socket.next().await {
                    if let Message::Text(text) = frame.expect("frame") {
                        let message: Value = serde_json::from_str(&text).expect("JSON");
                        assert_eq!(
                            message["action"], "REORDER_CATEGORIES",
                            "the stale private block was not queued"
                        );
                        return;
                    }
                }
                panic!("the workspace connection remains usable");
            })
            .await
            .expect("category barrier");
        }
        ts.teardown().await;
    }
}

/// What is left of a user's subscriptions after their roles change is asked of the
/// store as it is now, also of a workspace of another organization than the one whose
/// change asks: a membership that ended there is not answered from what this
/// organization's revision keeps.
#[tokio::test]
async fn a_role_change_asks_another_organizations_workspace_anew() {
    // An SQLite database holds one organization.
    if fvoci_store::testing::db_type_of_the_tests() != fvoci_store::DbType::Postgres {
        return;
    }
    let ts = Setup::local_mode().start().await.init_basic().await;
    let user = ts.client.me().await.id;
    let (theirs, their_workspace, _board) = crate::workspaces::another_organization(&ts).await;
    theirs
        .add_organization_member(&user, OrgRole::Member)
        .await
        .expect("the user joins their organization");
    theirs
        .add_workspace_member(&their_workspace, &user, WorkspaceRole::Member)
        .await
        .expect("the user is let into their workspace");
    let mut socket = ts.client.connect_websocket().await;
    subscribe(
        &ts,
        &mut socket,
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": their_workspace}),
        Moment::Registered,
    )
    .await;
    // Asked from this organization while the user is still in their workspace.
    ts.app.ws().follow_only_own_workspaces(&user).await;
    barrier(&mut socket).await;
    theirs
        .execute_unchecked(&format!(
            "DELETE FROM workspace_members WHERE user_id = '{user}' \
             AND workspace_id = '{their_workspace}'"
        ))
        .await
        .expect("the membership ends there, below the app's hooks");
    ts.app.ws().follow_only_own_workspaces(&user).await;
    closed(&mut socket).await;
    ts.teardown().await;
}

/// A subscription checked again as often as access is taken back elsewhere is given
/// up after a bound, and the page is told to connect again (1013), not that its
/// access ended.
#[tokio::test]
async fn a_subscription_checked_too_often_is_given_up_as_retryable() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let mut socket = ts.client.connect_websocket().await;
    let mut pause = subscribe(
        &ts,
        &mut socket,
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID}),
        Moment::Validated,
    )
    .await
    .expect("the first check is paused");
    // Eight checks in all, each overtaken by somebody else's access being taken back.
    for _ in 1..8 {
        let next = ts.app.ws().pause_next_subscription();
        ts.app.ws().close_user(&new_id());
        pause.resume();
        tokio::time::timeout(WAIT, next.reached())
            .await
            .expect("the subscription is checked again");
        pause = next;
    }
    ts.app.ws().close_user(&new_id());
    pause.resume();
    let frame = tokio::time::timeout(WAIT, async {
        while let Some(message) = socket.next().await {
            if let Message::Close(frame) = message.expect("the close is a protocol frame") {
                return frame;
            }
        }
        None
    })
    .await
    .expect("the connection is closed");
    let frame = frame.expect("the page is told why");
    assert_eq!(u16::from(frame.code), 1013, "a retryable close");
    assert_eq!(frame.reason.as_str(), "try again");
    ts.teardown().await;
}
