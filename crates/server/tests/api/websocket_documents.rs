//! Document-only sockets participate in workspace revocation without sidebar subscriptions.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_server::collab::AdmissionStage;
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::common::{Setup, TEST_WORKSPACE_ID, TestServer};
use crate::org_admin::{make_workspace, org_of};
use crate::rooms::{Incoming, Peer, QUIET, UPDATE, frame, give, paragraphs, table, two_in_a_room};

const WAIT: Duration = Duration::from_secs(5);

async fn server() -> TestServer {
    let ts = Setup::native().start().await.init_basic().await;
    let org = org_of(&ts).await;
    ts.client
        .post_empty(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}/join"))
        .await
        .check_ok();
    ts
}

#[derive(Clone, Copy, Debug)]
enum Revocation {
    Remove,
    Guest,
    Trash,
    WithdrawAdmin,
    DemoteAdmin,
}

impl Revocation {
    const ALL: [Self; 5] = [
        Self::Remove,
        Self::Guest,
        Self::Trash,
        Self::WithdrawAdmin,
        Self::DemoteAdmin,
    ];

    fn client(self, ts: &TestServer) -> &crate::common::Client {
        match self {
            Self::WithdrawAdmin | Self::DemoteAdmin => &ts.client,
            _ => &ts.client2,
        }
    }

    async fn apply(self, ts: &TestServer, user: &str) {
        let member = format!("/w/{TEST_WORKSPACE_ID}/members/{user}");
        match self {
            Self::Remove => ts.client.delete(&member).await.check_no_content(),
            Self::Guest => ts
                .client
                .put(&member, &json!({"role": "guest"}))
                .await
                .check_ok(),
            Self::Trash => {
                let org = org_of(ts).await;
                ts.client
                    .delete(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}"))
                    .await
                    .check_no_content();
            }
            Self::WithdrawAdmin => {
                let org = org_of(ts).await;
                ts.client
                    .post_empty(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}/leave"))
                    .await
                    .check_ok();
            }
            Self::DemoteAdmin => {
                let org = org_of(ts).await;
                let other = ts.client2.me().await.id;
                ts.client
                    .put(
                        &format!("/org/{org}/members/{other}/role"),
                        &json!({"role": "admin"}),
                    )
                    .await
                    .check_no_content();
                ts.client2
                    .put(
                        &format!("/org/{org}/members/{user}/role"),
                        &json!({"role": "member"}),
                    )
                    .await
                    .check_no_content();
            }
        }
    }
}

#[tokio::test]
async fn document_only_sockets_close_when_workspace_access_ends() {
    for ending in Revocation::ALL {
        let ts = server().await;
        let room = table(&ts, &paragraphs(&["initial"])).await;
        let other = ts.client2.me().await.id;
        give(&ts, &room.board_id, &other, "editor").await;
        let client = ending.client(&ts);
        let user = client.me().await.id;
        let mut peer = Peer::connect(client, 202).await;
        peer.joined(TEST_WORKSPACE_ID, &room.card_id).await;
        ending.apply(&ts, &user).await;
        assert_eq!(
            peer.incoming(WAIT).await,
            Incoming::Closed,
            "{ending:?} closes a document-only socket before the periodic recheck"
        );
        ts.teardown().await;
    }
}

#[tokio::test]
async fn document_admission_cannot_miss_concurrent_workspace_revocation() {
    for ending in Revocation::ALL {
        let ts = server().await;
        let room = table(&ts, &paragraphs(&["initial"])).await;
        let other = ts.client2.me().await.id;
        give(&ts, &room.board_id, &other, "editor").await;
        let client = ending.client(&ts);
        let user = client.me().await.id;
        let mut socket = client.connect_websocket().await;
        let (reached, resume) = ts
            .app
            .collab()
            .pause_next_admission(AdmissionStage::AfterCheck);
        socket
            .send(Message::Text(
                json!({"action": "JOIN_DOC", "workspaceId": TEST_WORKSPACE_ID,
                    "cardId": room.card_id})
                .to_string()
                .into(),
            ))
            .await
            .expect("document join sent");
        tokio::time::timeout(WAIT, reached.notified())
            .await
            .expect("document admission reaches the registration seam");
        ending.apply(&ts, &user).await;
        resume.notify_one();
        let message = tokio::time::timeout(WAIT, socket.next())
            .await
            .expect("revocation ends admission promptly")
            .expect("a close frame")
            .expect("a protocol frame");
        let Message::Close(Some(close)) = message else {
            panic!("{ending:?} must close before sending a document: {message:?}");
        };
        assert_eq!(u16::from(close.code), 1008, "policy close for {ending:?}");
        ts.teardown().await;
    }
}

#[tokio::test]
async fn a_revoked_document_connection_cannot_write_or_receive_another_update() {
    let ts = server().await;
    let (_, mut one, mut two) = two_in_a_room(&ts, &paragraphs(&["initial"])).await;
    // Sidebar subscriptions have a separate lifetime from the document.
    for action in ["SUBSCRIBE_WORKSPACE", "UNSUBSCRIBE_WORKSPACE"] {
        two.send_json(json!({"action": action, "workspaceId": TEST_WORKSPACE_ID}))
            .await;
    }
    two.write_acked("before removal").await;
    one.settle().await;
    let late = two.editor().append_paragraph("must not be stored");
    let user = ts.client2.me().await.id;
    Revocation::Remove.apply(&ts, &user).await;
    two.try_send_binary(frame(two.doc, two.epoch, UPDATE, &late))
        .await;
    one.write_acked("after removal").await;
    assert_eq!(
        two.incoming(WAIT).await,
        Incoming::Closed,
        "no later document update"
    );
    one.settle().await;
    assert_eq!(
        one.lines(),
        ["initial", "before removal", "after removal"],
        "revoked writes are dropped"
    );
    ts.teardown().await;
}

#[tokio::test]
async fn leaving_a_document_removes_its_workspace_watch() {
    let ts = server().await;
    let room = table(&ts, &paragraphs(&["initial"])).await;
    let user = ts.client2.me().await.id;
    give(&ts, &room.board_id, &user, "editor").await;
    let mut peer = Peer::connect(&ts.client2, 202).await;
    peer.joined(TEST_WORKSPACE_ID, &room.card_id).await;
    peer.leave().await;
    let refused = peer.join("no-workspace", "no-card").await;
    assert_eq!(
        refused["action"], "DOC_REFUSED",
        "later command is answered"
    );
    Revocation::Remove.apply(&ts, &user).await;
    assert_eq!(
        peer.incoming(QUIET).await,
        Incoming::Nothing,
        "no active document was revoked"
    );
    ts.teardown().await;
}

#[tokio::test]
async fn malformed_and_server_evicted_documents_leave_no_workspace_watch() {
    let ts = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let org = org_of(&ts).await;
    ts.client
        .post_empty(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}/join"))
        .await
        .check_ok();
    let room = table(&ts, &paragraphs(&["initial"])).await;
    let user = ts.client2.me().await.id;
    give(&ts, &room.board_id, &user, "editor").await;
    let mut peer = Peer::connect(&ts.client2, 202).await;
    peer.joined(TEST_WORKSPACE_ID, &room.card_id).await;
    ts.client
        .delete(&format!(
            "/w/{TEST_WORKSPACE_ID}/boards/{}/members/{user}",
            room.board_id
        ))
        .await
        .check_no_content();
    let left = peer.text("DOC_LEFT").await;
    assert_eq!(
        left["reason"], "not_found",
        "the board's room evicts the member"
    );
    peer.send_json(
        json!({"action": "JOIN_DOC", "workspaceId": TEST_WORKSPACE_ID,
        "cardId": room.card_id, "doc": null}),
    )
    .await;
    peer.barrier().await;
    Revocation::Remove.apply(&ts, &user).await;
    assert_eq!(
        peer.incoming(QUIET).await,
        Incoming::Nothing,
        "no stale watch after eviction or malformed input"
    );
    ts.teardown().await;
}

#[tokio::test]
async fn unrelated_admin_withdrawal_does_not_close_pending_document_admission() {
    let ts = server().await;
    let room = table(&ts, &paragraphs(&["initial"])).await;
    let org = org_of(&ts).await;
    let other = make_workspace(&ts, &org, "Unrelated").await;
    let at = format!("/org/{org}/workspaces/{other}");
    ts.client.post_empty(&format!("{at}/join")).await.check_ok();
    let mut socket = ts.client.connect_websocket().await;
    let (reached, resume) = ts
        .app
        .collab()
        .pause_next_admission(AdmissionStage::BeforeCheck);
    socket
        .send(Message::Text(
            json!({"action": "JOIN_DOC", "workspaceId": TEST_WORKSPACE_ID,
        "cardId": room.card_id})
            .to_string()
            .into(),
        ))
        .await
        .expect("join sent");
    tokio::time::timeout(WAIT, reached.notified())
        .await
        .expect("admission is pending");
    ts.client
        .post_empty(&format!("{at}/leave"))
        .await
        .check_ok();
    resume.notify_one();
    let message = tokio::time::timeout(WAIT, socket.next())
        .await
        .expect("admission resumes")
        .expect("connection stays open")
        .expect("protocol frame");
    let Message::Text(text) = message else {
        panic!("expected document admission, got {message:?}");
    };
    let joined: serde_json::Value = serde_json::from_str(&text).expect("JSON message");
    assert_eq!(
        joined["action"], "DOC_JOINED",
        "unrelated withdrawal preserves admission"
    );
    ts.teardown().await;
}

#[tokio::test]
async fn an_unchecked_admission_uses_permissions_after_admin_revocation() {
    for demote in [false, true] {
        let ts = server().await;
        let org = org_of(&ts).await;
        let workspace = make_workspace(&ts, &org, "Joined only").await;
        let at = format!("/org/{org}/workspaces/{workspace}");
        ts.client.post_empty(&format!("{at}/join")).await.check_ok();
        let board = ts
            .create_board(&workspace, fvoci_model::BoardType::Private)
            .await;
        let card: fvoci_model::Card = ts
            .client
            .post(
                &format!("/w/{workspace}/boards/{}/cards", board.id),
                &json!({"title": "Joined card", "body": paragraphs(&["initial"])}),
            )
            .await
            .created_json();
        if demote {
            let other = ts.client2.me().await.id;
            ts.client
                .put(
                    &format!("/org/{org}/members/{other}/role"),
                    &json!({"role": "admin"}),
                )
                .await
                .check_no_content();
        }
        let mut socket = ts.client.connect_websocket().await;
        let (reached, resume) = ts
            .app
            .collab()
            .pause_next_admission(AdmissionStage::BeforeCheck);
        socket
            .send(Message::Text(
                json!({"action": "JOIN_DOC", "workspaceId": workspace,
            "cardId": card.id})
                .to_string()
                .into(),
            ))
            .await
            .expect("join sent");
        tokio::time::timeout(WAIT, reached.notified())
            .await
            .expect("admission has not checked access");
        if demote {
            let user = ts.client.me().await.id;
            ts.client2
                .put(
                    &format!("/org/{org}/members/{user}/role"),
                    &json!({"role": "member"}),
                )
                .await
                .check_no_content();
        } else {
            ts.client
                .post_empty(&format!("{at}/leave"))
                .await
                .check_ok();
        }
        resume.notify_one();
        let message = tokio::time::timeout(WAIT, socket.next())
            .await
            .expect("admission resumes")
            .expect("a refusal")
            .expect("protocol frame");
        let Message::Text(text) = message else {
            panic!("expected a refusal, got {message:?}");
        };
        let refused: serde_json::Value = serde_json::from_str(&text).expect("JSON message");
        assert_eq!(
            refused["action"], "DOC_REFUSED",
            "unadmitted connection receives no document"
        );
        assert_eq!(
            refused["reason"], "not_found",
            "the former administrator cannot enter"
        );
        ts.teardown().await;
    }
}
