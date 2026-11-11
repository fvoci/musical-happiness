//! Independent failure cases for the boundary between object storage and its records.

#[path = "api/common.rs"]
mod common;

use fvoci_model::{BoardType, Body, Card, FileInfo};
use fvoci_server::app::ChangeNotification;
use fvoci_server::auth::now_millis;

use common::{Setup, TEST_WORKSPACE_ID, TestServer};

async fn recorded(ts: &TestServer) -> FileInfo {
    let board = ts.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;
    let name = ts
        .app
        .save_file(&mut &b"keep these bytes"[..], &board.id, "kept.txt")
        .await
        .expect("the file is stored");
    ts.app
        .store()
        .get_file_info(&board.id, &name)
        .await
        .expect("the file is recorded")
}

/// The HTTP handlers `check_body_files` before `create_card` starts its transaction.
/// Run those actual application steps with the collector in the intervening gap:
/// a request that was valid when checked must not commit a dangling image.
#[tokio::test]
async fn a_body_checked_before_collection_cannot_commit_a_missing_image() {
    let ts = Setup::native().start().await.init_basic().await;
    let user_id = ts.client.me().await.id;
    let file = recorded(&ts).await;
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE file_info SET unreferenced_at = 1 WHERE id = '{}'",
            file.id
        ))
        .await
        .expect("the unreferenced file has already outlived its grace period");
    let body: Body = serde_json::from_value(serde_json::json!({
        "type": "doc", "content": [{
            "type": "image", "attrs": {"fileId": file.stored_name}
        }]
    }))
    .expect("a valid image body");
    ts.app
        .check_body_files(&file.board_id, Some(&body))
        .await
        .expect("the request's image is valid before collection");
    // The normal housekeeping deadline, for a file already eligible when checked.
    let before = now_millis().saturating_sub(ts.app.config().trash_retention_millis());
    let collected = ts
        .app
        .housekeeping()
        .store()
        .collect_unreferenced_files(before)
        .await
        .expect("collected");
    assert_eq!(collected.len(), 1);
    ts.app
        .housekeeping()
        .remove_doomed_objects(now_millis())
        .await
        .expect("the collected object's bytes are removed");
    let created = ts
        .app
        .create_card(
            Card {
                body: Some(body),
                ..Card::default()
            },
            &file.board_id,
            &user_id,
            ChangeNotification::default(),
        )
        .await;
    let still_present = ts.app.files().exists(&file.path).await.expect("read");
    ts.teardown().await;
    assert!(
        created.is_err() || still_present,
        "a successful card creation must retain the validated image: {created:?}"
    );
}
