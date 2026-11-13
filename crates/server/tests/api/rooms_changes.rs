//! Permission changes reach every affected connection without waiting for the timer.

use std::time::Duration;

use tokio::time::Instant;

use serde_json::{Value, json};

use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Peer, give, paragraphs, two_in_a_room};

/// The periodic check cannot account for any notice received by these tests.
fn seldom() -> Setup {
    Setup::native().configure(|config| config.collab.collab_recheck_seconds = 3600)
}

async fn soon(peer: &mut Peer, action: &str, changed_at: Instant) -> Value {
    let message = tokio::time::timeout_at(changed_at + Duration::from_secs(1), peer.text(action))
        .await
        .expect("the change reaches the room within one second");
    assert!(
        changed_at.elapsed() < Duration::from_secs(1),
        "mutation to notice exceeded one second"
    );
    message
}

#[tokio::test]
async fn a_role_change_and_removal_reach_every_connection_immediately() {
    let th = seldom().start().await.init_basic().await;
    let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let mut other_tab = Peer::connect(&th.client2, 103).await;
    other_tab.joined(TEST_WORKSPACE_ID, &table.card_id).await;
    let card: fvoci_model::Card = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/boards/{}/cards", table.board_id),
            &json!({"title": "Another card"}),
        )
        .await
        .created_json();
    let mut another_card = Peer::connect(&th.client2, 104).await;
    another_card.joined(TEST_WORKSPACE_ID, &card.id).await;

    let changed_at = Instant::now();
    give(&th, &table.board_id, &user, "viewer").await;
    for peer in [&mut member, &mut other_tab, &mut another_card] {
        assert_eq!(
            soon(peer, "DOC_MODE", changed_at).await["mode"],
            json!("read")
        );
        peer.write("refused").await;
        assert_eq!(
            peer.text("DOC_NACK").await["reason"],
            json!("document_read_only")
        );
    }
    owner.write_acked("still writing").await;
    let changed_at = Instant::now();
    th.client
        .delete(&format!(
            "/w/{TEST_WORKSPACE_ID}/boards/{}/members/{user}",
            table.board_id
        ))
        .await
        .check_no_content();
    for peer in [&mut member, &mut other_tab, &mut another_card] {
        assert_eq!(
            soon(peer, "DOC_LEFT", changed_at).await["reason"],
            json!("not_found")
        );
    }
    assert_eq!(
        th.app.collab().room_members(&th.app, &table.card_id).await,
        1
    );
    th.teardown().await;
}

#[tokio::test]
async fn removal_from_the_workspace_reaches_the_room_immediately() {
    let th = seldom().start().await.init_basic().await;
    let (_table, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    workspace_admin(&th).await;
    let changed_at = Instant::now();
    th.client
        .delete(&format!("/w/{TEST_WORKSPACE_ID}/members/{user}"))
        .await
        .check_no_content();
    assert_eq!(
        soon(&mut member, "DOC_LEFT", changed_at).await["reason"],
        json!("not_found")
    );
    th.teardown().await;
}

async fn workspace_admin(th: &crate::common::TestServer) {
    let admin = th.client.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'admin' WHERE user_id = '{admin}' \
             AND workspace_id = '{TEST_WORKSPACE_ID}'"
        ))
        .await
        .expect("the first user administers this workspace");
}

async fn template_reader(th: &crate::common::TestServer) -> (String, Peer) {
    let board: fvoci_model::Board = th
        .client
        .create_board(&fvoci_model::Board {
            is_template: true,
            ..crate::boards::new_board(TEST_WORKSPACE_ID, fvoci_model::BoardType::Open, "Template")
        })
        .await
        .created_json();
    let card: fvoci_model::Card = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/boards/{}/cards", board.id),
            &json!({"title": "Notes"}),
        )
        .await
        .created_json();
    let mut reader = Peer::connect(&th.client2, 201).await;
    reader.joined(TEST_WORKSPACE_ID, &card.id).await;
    (board.id, reader)
}

#[tokio::test]
async fn becoming_a_guest_closes_an_uninvited_template_room_immediately() {
    let th = seldom().start().await.init_basic().await;
    workspace_admin(&th).await;
    let (_board, mut reader) = template_reader(&th).await;
    let user = th.client2.me().await.id;
    let changed_at = Instant::now();
    th.client
        .put(
            &format!("/w/{TEST_WORKSPACE_ID}/members/{user}"),
            &json!({"role": "guest"}),
        )
        .await
        .check_ok();
    assert_eq!(
        soon(&mut reader, "DOC_LEFT", changed_at).await["reason"],
        json!("not_found")
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_template_closed_to_the_workspace_puts_out_its_uninvited_readers() {
    let th = seldom().start().await.init_basic().await;
    let (board, mut reader) = template_reader(&th).await;
    let changed_at = Instant::now();
    th.client
        .patch(
            &format!("/w/{TEST_WORKSPACE_ID}/boards/{board}"),
            &json!({"type": "P"}),
        )
        .await
        .check_ok();
    assert_eq!(
        soon(&mut reader, "DOC_LEFT", changed_at).await["reason"],
        json!("not_found")
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_batch_closing_a_template_puts_out_its_uninvited_readers() {
    let th = seldom().start().await.init_basic().await;
    let (board, mut reader) = template_reader(&th).await;
    let changed_at = Instant::now();
    th.client.patch(&format!("/w/{TEST_WORKSPACE_ID}/boards-and-blocks"), &json!({
        "boardIds": [board], "boardPatches": [{"type": "P"}], "blockIds": [], "blockPatches": [],
    })).await.check_ok();
    assert_eq!(
        soon(&mut reader, "DOC_LEFT", changed_at).await["reason"],
        json!("not_found")
    );
    th.teardown().await;
}

#[tokio::test]
async fn trashing_a_board_or_workspace_empties_its_rooms_immediately() {
    for trash_workspace in [false, true] {
        let th = seldom().start().await.init_basic().await;
        let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
        let path = if trash_workspace {
            let org = crate::org_admin::org_of(&th).await;
            format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}")
        } else {
            format!("/w/{TEST_WORKSPACE_ID}/boards/{}", table.board_id)
        };
        let changed_at = Instant::now();
        th.client.delete(&path).await.check_no_content();
        for peer in [&mut owner, &mut member] {
            assert_eq!(
                soon(peer, "DOC_LEFT", changed_at).await["reason"],
                json!("not_found")
            );
        }
        crate::rooms::no_rooms(&th).await;
        th.teardown().await;
    }
}

#[tokio::test]
async fn ending_an_organization_administrators_participation_puts_them_out() {
    for ending in ["leave", "organization", "instance"] {
        let th = seldom().start().await.init_basic().await;
        let table = crate::rooms::table(&th, &paragraphs(&["first"])).await;
        let user = th.client2.me().await.id;
        let org = crate::org_admin::org_of(&th).await;
        give(&th, &table.board_id, &user, "editor").await;
        crate::workspace_removal::remove_from_the_test_workspace(&th, &user).await;
        th.client
            .put(
                &format!("/org/{org}/members/{user}/role"),
                &json!({"role": "admin"}),
            )
            .await
            .check_no_content();
        th.client2
            .post_empty(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}/join"))
            .await
            .check_ok();
        if ending == "instance" {
            th.client
                .put(&format!("/users/{user}/admin"), &json!({"isAdmin": true}))
                .await
                .check_ok();
        }
        let mut peer = Peer::connect(&th.client2, 301).await;
        peer.joined(TEST_WORKSPACE_ID, &table.card_id).await;
        let changed_at = Instant::now();
        if ending == "instance" {
            th.client
                .put(&format!("/users/{user}/admin"), &json!({"isAdmin": false}))
                .await
                .check_ok();
        } else if ending == "organization" {
            th.client
                .put(
                    &format!("/org/{org}/members/{user}/role"),
                    &json!({"role": "member"}),
                )
                .await
                .check_no_content();
        } else {
            th.client2
                .post_empty(&format!("/org/{org}/workspaces/{TEST_WORKSPACE_ID}/leave"))
                .await
                .check_ok();
        }
        assert_eq!(
            soon(&mut peer, "DOC_LEFT", changed_at).await["reason"],
            json!("not_found")
        );
        th.teardown().await;
    }
}

/// A periodic answer read before a change cannot restore the old write permission.
#[tokio::test]
async fn an_old_periodic_answer_does_not_undo_an_immediate_demotion() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let (table, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let (waiting, go) = th.app.collab().pause_next_recheck();
    tokio::time::timeout(Duration::from_secs(3), waiting.notified())
        .await
        .expect("the periodic read pauses");
    let changed_at = Instant::now();
    give(&th, &table.board_id, &user, "viewer").await;
    assert_eq!(
        soon(&mut member, "DOC_MODE", changed_at).await["mode"],
        json!("read")
    );
    go.notify_one();
    assert_eq!(
        member.incoming(crate::rooms::QUIET).await,
        crate::rooms::Incoming::Nothing
    );
    member.write("not allowed by the old answer").await;
    assert_eq!(
        member.text("DOC_NACK").await["reason"],
        json!("document_read_only")
    );
    th.teardown().await;
}

/// A stuck immediate read cannot hold the room and all future checks forever.
#[tokio::test]
async fn an_unanswered_immediate_check_has_a_deadline() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let (table, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let waiting = th.app.collab().stall_next_access_check();
    tokio::time::timeout(Duration::from_secs(3), async {
        give(&th, &table.board_id, &user, "viewer").await;
        waiting.notified().await;
        assert_eq!(
            member.text("DOC_LEFT").await["reason"],
            json!("document_unavailable")
        );
    })
    .await
    .expect("the unanswered permission read expires before the mutation answers");
    give(&th, &table.board_id, &user, "editor").await;
    member.joined(TEST_WORKSPACE_ID, &table.card_id).await;
    member
        .write_acked("the next check and room still work")
        .await;
    th.teardown().await;
}
