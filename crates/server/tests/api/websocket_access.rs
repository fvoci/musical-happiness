//! Revocation reaches open sockets, including a subscription not registered yet.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{Block, BlockType, BoardType, WorkspaceRole, new_id};
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
