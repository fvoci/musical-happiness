//! Trash snapshots and room lifetimes agree, including writes and restores in flight.

use std::time::Duration;

use serde_json::json;

use crate::collab::{body_of, card};
use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Peer, paragraphs, two_in_a_room};

#[tokio::test]
async fn deleting_cards_boards_and_batches_keeps_the_last_room_body_for_restore() {
    for target in ["card", "board", "batch"] {
        let th = Setup::native().start().await.init_basic().await;
        let (table, mut writer, mut other) = two_in_a_room(&th, &paragraphs(&["first"])).await;
        writer.write_acked("last acknowledged edit").await;
        let wanted = writer.body();
        assert_ne!(body_of(&card(&th.client, &table.card_id).await), wanted);
        let base = format!("/w/{TEST_WORKSPACE_ID}");
        let block = format!("{base}/boards/{}/blocks/{}", table.board_id, table.card_id);
        let board = format!("{base}/boards/{}", table.board_id);
        match target {
            "card" => th.client.delete(&block).await.check_no_content(),
            "board" => th.client.delete(&board).await.check_no_content(),
            _ => th
                .client
                .delete_json(
                    &format!("{base}/boards-and-blocks"),
                    &json!({"boards": [table.board_id], "blocks": [table.card_id]}),
                )
                .await
                .check_no_content(),
        }
        for peer in [&mut writer, &mut other] {
            assert_eq!(peer.text("DOC_LEFT").await["reason"], json!("not_found"));
        }
        assert_eq!(th.app.collab().rooms(), 0);
        let trashed = th
            .app
            .store()
            .get_deleted_block(&table.card_id)
            .await
            .expect("in the trash");
        assert_eq!(
            trashed
                .block
                .body
                .as_ref()
                .map(|body| body.document().clone()),
            wanted
        );
        th.app
            .collab()
            .write_body(&th.app, &table.card_id)
            .await
            .expect("no pending projection remains");
        assert!(
            th.app.store().get_block(&table.card_id).await.is_err(),
            "no write resurrects the card"
        );
        let restore = if target == "card" { &block } else { &board };
        th.client
            .post_empty(&format!("{restore}/undelete"))
            .await
            .check_ok();
        let mut reopened = Peer::connect(&th.client, 203).await;
        reopened.joined(TEST_WORKSPACE_ID, &table.card_id).await;
        assert_eq!(
            reopened.body(),
            wanted,
            "{target}: restore includes the last edit"
        );
        th.teardown().await;
    }
}

#[tokio::test]
async fn deletion_waits_for_a_projection_already_in_progress() {
    let th = Setup::native().start().await.init_basic().await;
    let (table, mut writer, _other) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    writer.write_acked("written before deletion").await;
    let wanted = writer.body();
    let (waiting, go) = th.app.collab().pause_next_projection();
    let projection = tokio::spawn({
        let (app, id) = (th.app.clone(), table.card_id.clone());
        async move { app.collab().write_body(&app, &id).await }
    });
    waiting.notified().await;
    let path = format!(
        "/w/{TEST_WORKSPACE_ID}/boards/{}/blocks/{}",
        table.board_id, table.card_id
    );
    let mut deletion = tokio::spawn({
        let client = th.client.clone();
        async move { client.delete(&path).await }
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut deletion)
            .await
            .is_err(),
        "deletion cannot overtake a held projection"
    );
    assert!(th.app.store().get_block(&table.card_id).await.is_ok());
    go.notify_one();
    projection
        .await
        .expect("projection task")
        .expect("written before deletion");
    deletion.await.expect("deletion task").check_no_content();
    assert_eq!(writer.text("DOC_LEFT").await["reason"], json!("not_found"));
    let trashed = th
        .app
        .store()
        .get_deleted_block(&table.card_id)
        .await
        .expect("in the trash");
    assert_eq!(
        trashed
            .block
            .body
            .as_ref()
            .map(|body| body.document().clone()),
        wanted
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_deleted_rooms_late_input_cannot_reach_the_restored_cards_new_room() {
    let th = Setup::native().start().await.init_basic().await;
    let (table, mut old, _other) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    old.write_acked("before deletion").await;
    let wanted = old.body();
    let path = format!(
        "/w/{TEST_WORKSPACE_ID}/boards/{}/blocks/{}",
        table.board_id, table.card_id
    );
    let (waiting, go) = th.app.collab().pause_next_deletion();
    let deletion = tokio::spawn({
        let (client, path) = (th.client.clone(), path.clone());
        async move { client.delete(&path).await }
    });
    waiting.notified().await;
    th.client
        .post_empty(&format!("{path}/undelete"))
        .await
        .check_ok();
    let mut fresh = Peer::connect(&th.client, 303).await;
    fresh.joined(TEST_WORKSPACE_ID, &table.card_id).await;
    assert_eq!(fresh.body(), wanted);
    old.send_binary(crate::rooms::frame(
        old.doc,
        old.epoch,
        crate::rooms::STEP_1,
        &old.editor().state_vector(),
    ))
    .await;
    assert_eq!(
        old.incoming(crate::rooms::QUIET).await,
        crate::rooms::Incoming::Nothing,
        "the deleted room cannot read the restored document either"
    );
    old.write("must not reach the restored card").await;
    assert_eq!(old.text("DOC_NACK").await["reason"], json!("not_found"));
    fresh.write_acked("in the new room").await;
    go.notify_one();
    deletion.await.expect("deletion task").check_no_content();
    assert_eq!(old.text("DOC_LEFT").await["reason"], json!("not_found"));
    fresh
        .write_acked("the old room did not close this one")
        .await;
    th.app
        .collab()
        .write_body(&th.app, &table.card_id)
        .await
        .expect("the new body is written");
    assert_eq!(
        body_of(&card(&th.client, &table.card_id).await),
        fresh.body()
    );
    assert!(
        !serde_json::to_string(&fresh.body())
            .expect("JSON")
            .contains("must not reach")
    );
    assert_eq!(
        th.app.collab().room_members(&th.app, &table.card_id).await,
        1
    );
    th.teardown().await;
}
