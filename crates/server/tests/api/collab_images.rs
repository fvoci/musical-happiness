//! The files of the images a body edited together shows, between the update that shows
//! them and the write of the body to the card.

use fvoci_collab::BodyDoc;
use fvoci_model::{Body, FileInfo, new_id};
use serde_json::{Value, json};

use crate::collab::{Editing, body_of, card, editing, joined};
use crate::common::{Setup, TestServer};

/// A file of the card's board, recorded long ago and shown by nothing.
async fn old_file(e: &Editing, th: &TestServer) -> FileInfo {
    let board_id = card(&th.client, &e.card_id).await.board_id;
    let file = FileInfo {
        id: new_id(),
        board_id: board_id.clone(),
        stored_name: format!("7{}.png", new_id()),
        create_at: 1,
        path: format!("boards/{board_id}/image.png"),
        name: "image.png".to_owned(),
        extension: ".png".to_owned(),
        size: 7,
    };
    e.app
        .store()
        .save_file_info(&file)
        .await
        .expect("a file is recorded");
    file
}

/// An update that makes an empty document show an image of `stored_name`.
pub(crate) fn showing(stored_name: &str) -> Vec<u8> {
    let body = Body::parse(json!({"type": "doc", "content": [
        {"type": "image", "attrs": {"fileId": stored_name}}
    ]}))
    .expect("a body")
    .expect("not empty");
    BodyDoc::from_body(Some(&body)).state()
}

/// An update that shows an image whose file the board does not have is refused and not
/// logged, and the document goes on: what another editor typed is taken and written.
#[tokio::test]
async fn an_image_without_a_file_is_refused_and_the_document_goes_on() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &Value::Null).await;
    let collab = e.app.collab();
    let (editor, epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    let typed = editor.append_paragraph("typed");
    collab
        .take(&e.app, &e.card_id, epoch, &e.user1, &typed)
        .await
        .expect("taken");
    let refused = collab
        .take(
            &e.app,
            &e.card_id,
            epoch,
            &e.user2,
            &showing("7nothing.png"),
        )
        .await
        .expect_err("no file of the board");
    assert_eq!(
        refused.reason().map(fvoci_server::error::Reason::as_str),
        Some("update_refused")
    );
    let stored = e
        .app
        .store()
        .get_body_doc(&e.card_id)
        .await
        .expect("read")
        .expect("a document");
    assert_eq!(stored.log_end, 1, "the refused update is not logged");
    collab
        .write_body(&e.app, &e.card_id)
        .await
        .expect("written");
    assert_eq!(
        body_of(&card(&th.client, &e.card_id).await),
        editor.body().map(|body| body.document().clone()),
        "what was typed before is written"
    );
    th.teardown().await;
}

/// An image shown by an update and deleted before the body is written leaves its file
/// shown by nothing once the body is written, as if it had never been shown.
#[tokio::test]
async fn an_image_deleted_before_the_body_is_written_shows_its_file_no_more() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &Value::Null).await;
    let file = old_file(&e, &th).await;
    let collab = e.app.collab();
    let (_, epoch) = joined(collab, &e.app, &e.card_id, 101).await;
    collab
        .take(
            &e.app,
            &e.card_id,
            epoch,
            &e.user1,
            &showing(&file.stored_name),
        )
        .await
        .expect("taken");
    let held = e
        .app
        .store()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("collected");
    assert!(
        held.iter().all(|collected| collected.id != file.id),
        "shown while its update waits to be written"
    );
    let (editor, epoch) = joined(collab, &e.app, &e.card_id, 102).await;
    collab
        .take(&e.app, &e.card_id, epoch, &e.user1, &editor.delete_last())
        .await
        .expect("taken");
    collab
        .write_body(&e.app, &e.card_id)
        .await
        .expect("written");
    let collected = e
        .app
        .store()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("collected");
    assert!(
        collected.iter().any(|collected| collected.id == file.id),
        "shown by nothing once the body is written: {collected:?}"
    );
    th.teardown().await;
}
