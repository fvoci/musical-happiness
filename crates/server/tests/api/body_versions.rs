//! The versions the server keeps of a card's body: when one is kept, who reads them,
//! and what they say.

use std::time::Duration;

use fvoci_server::app::App;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::collab::{Editing, body_of, card, editing, joined};
use crate::common::{Client, ROOT_WS, Setup, TEST_WORKSPACE_ID, TEST_WS};
use crate::rooms::{PATIENCE, Peer, give, lines_of, paragraphs, table, write_whole};

/// The versions of a card as a client lists them.
pub(crate) async fn versions(client: &Client, card_id: &str) -> Vec<Value> {
    client
        .get(&format!("{TEST_WS}/cards/{card_id}/body-versions"))
        .await
        .ok_json()
}

/// The lines of the body of a version of a card.
pub(crate) async fn lines(client: &Client, card_id: &str, version: &Value) -> Vec<String> {
    let read: Value = client
        .get(&format!(
            "{TEST_WS}/cards/{card_id}/body-versions/{}",
            version["version"]
        ))
        .await
        .ok_json();
    for field in ["version", "keptAt", "editors", "isCurrent"] {
        assert_eq!(read[field], version[field], "{field} as the list has it");
    }
    lines_of(read.get("body").filter(|body| !body.is_null()))
}

/// Has the user's editor write a line into the card's document, and leaves the card:
/// the document is closed as the room of the last connection closes it.
pub(crate) async fn edit_and_leave(e: &Editing, app: &App, user_id: &str, line: &str) {
    let collab = app.collab();
    let (editor, epoch) = joined(collab, app, &e.card_id, 101).await;
    collab
        .take(
            app,
            &e.card_id,
            epoch,
            user_id,
            &editor.append_paragraph(line),
        )
        .await
        .expect("taken");
    collab.close(app, &e.card_id).await.expect("closed");
}

/// A body its editors changed is kept as a version when they leave it, with who wrote
/// it; the body it had before they came is kept first, by who changed the card last.
/// Leaving a body that did not change keeps nothing.
#[tokio::test]
async fn a_body_is_kept_as_a_version_when_its_editors_leave() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let made = card(&th.client, &e.card_id).await;
    assert_eq!(versions(&th.client, &e.card_id).await, Vec::<Value>::new());

    // Read together and left: nothing changed, nothing is kept.
    let collab = e.app.collab();
    joined(collab, &e.app, &e.card_id, 100).await;
    collab.close(&e.app, &e.card_id).await.expect("closed");
    assert_eq!(versions(&th.client, &e.card_id).await, Vec::<Value>::new());

    edit_and_leave(&e, &e.app, &e.user1, "typed").await;
    let kept = versions(&th.client, &e.card_id).await;
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert_eq!(
        kept[1],
        json!({"version": 1, "keptAt": made.update_at, "editors": [e.user1], "isCurrent": false}),
        "the body before the editors came, as of the card's last change"
    );
    assert_eq!(lines(&th.client, &e.card_id, &kept[1]).await, ["first"]);
    assert_eq!(kept[0]["version"], 2);
    assert_eq!(kept[0]["editors"], json!([e.user1]));
    assert_eq!(kept[0]["isCurrent"], true);
    assert!(
        kept[0]["keptAt"].as_i64() > Some(made.update_at),
        "{kept:?}"
    );
    assert_eq!(
        lines(&th.client, &e.card_id, &kept[0]).await,
        ["first", "typed"]
    );
    assert_eq!(
        Some(lines_of(
            body_of(&card(&th.client, &e.card_id).await).as_ref()
        )),
        Some(vec!["first".to_owned(), "typed".to_owned()]),
        "the card holds the newest version's body"
    );

    // Opened and left again without a change.
    joined(collab, &e.app, &e.card_id, 102).await;
    collab.close(&e.app, &e.card_id).await.expect("closed");
    assert_eq!(versions(&th.client, &e.card_id).await, kept);

    // The next change, by another: the next version, theirs.
    edit_and_leave(&e, &e.app, &e.user2, "more").await;
    let kept = versions(&th.client, &e.card_id).await;
    assert_eq!(
        kept.iter()
            .map(|version| (
                &version["version"],
                &version["editors"],
                &version["isCurrent"]
            ))
            .collect::<Vec<_>>(),
        [
            (&json!(3), &json!([e.user2]), &json!(true)),
            (&json!(2), &json!([e.user1]), &json!(false)),
            (&json!(1), &json!([e.user1]), &json!(false)),
        ]
    );
    th.teardown().await;
}

/// The versions are for who reads the card, whatever their role; nobody else is told
/// that there is a card: not who is no member of its board, not through another
/// workspace, and not a reader by the board's link.
#[tokio::test]
async fn the_versions_are_for_who_reads_the_card() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    let mut writer = Peer::connect(&th.client, 101).await;
    writer.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    writer.write_acked("typed").await;
    writer.leave().await;
    let kept = kept_after(&th.client, &t.card_id, 2).await;
    let list = format!("{TEST_WS}/cards/{}/body-versions", t.card_id);
    let one = format!("{list}/2");

    for path in [&list, &one] {
        th.client2
            .get(path)
            .await
            .check_error(StatusCode::NOT_FOUND, "card not found");
    }
    let token = th.share(&t.board_id).await;
    for path in [&list, &one] {
        th.anonymous()
            .get(&format!("{path}?readToken={token}"))
            .await
            .check_unauthorized();
        th.client2
            .get(&format!("{path}?readToken={token}"))
            .await
            .check_error(StatusCode::NOT_FOUND, "card not found");
    }
    // Through another workspace the card is not there, also for who reads it.
    for path in [&list, &one] {
        th.client
            .get(&path.replace(TEST_WS, ROOT_WS))
            .await
            .check_error(StatusCode::NOT_FOUND, "card not found");
    }

    give(&th, &t.board_id, &user2, "viewer").await;
    assert_eq!(versions(&th.client2, &t.card_id).await, kept, "a viewer");
    assert_eq!(
        lines(&th.client2, &t.card_id, &kept[0]).await,
        ["first", "typed"]
    );
    for missing in ["9", "0", "-1", "two", "2.5"] {
        th.client
            .get(&format!("{list}/{missing}"))
            .await
            .check_error(StatusCode::NOT_FOUND, "version not found");
    }
    th.teardown().await;
}

/// The last writer of a room leaves it to its readers: what was typed is kept as a
/// version then, not only when the room closes.
#[tokio::test]
async fn a_room_left_to_its_readers_keeps_what_its_writers_typed() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    give(&th, &t.board_id, &user2, "viewer").await;
    let mut writer = Peer::connect(&th.client, 101).await;
    let mut reader = Peer::connect(&th.client2, 102).await;
    writer.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    reader.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    writer.write_acked("typed").await;
    writer.leave().await;

    let kept = kept_after(&th.client, &t.card_id, 2).await;
    assert_eq!(
        lines(&th.client, &t.card_id, &kept[0]).await,
        ["first", "typed"]
    );
    // The reader is still in the room; its leaving, which closes the room, keeps nothing.
    reader.leave().await;
    crate::rooms::no_rooms(&th).await;
    assert_eq!(versions(&th.client, &t.card_id).await, kept);
    th.teardown().await;
}

/// What the versions of a card say besides when they were kept, the newest first:
/// the number, who wrote, and whether it is the card's body now.
fn said(kept: &[Value]) -> Vec<(i64, Vec<&str>, bool)> {
    kept.iter()
        .map(|version| {
            let editors = version["editors"]
                .as_array()
                .expect("editors")
                .iter()
                .map(|editor| editor.as_str().expect("an identifier"))
                .collect();
            (
                version["version"].as_i64().expect("a number"),
                editors,
                version["isCurrent"].as_bool().expect("a flag"),
            )
        })
        .collect()
}

/// A body written whole over HTTP is a version, by its writer, and the body it wrote
/// over is kept before it if it was none yet. A body written again as it is, and a
/// patch that leaves the body, keep nothing; no body is a version too.
#[tokio::test]
async fn a_body_written_whole_is_a_version_and_so_is_the_one_before() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let (user1, user2) = (th.client.me().await.id, th.client2.me().await.id);
    give(&th, &t.board_id, &user2, "editor").await;
    let made = card(&th.client, &t.card_id).await;
    assert_eq!(
        versions(&th.client, &t.card_id).await,
        Vec::<Value>::new(),
        "a card as it was made has no version"
    );

    write_whole(&th.client2, &t.card_id, &paragraphs(&["second"])).await;
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(
        said(&kept),
        [
            (2, vec![user2.as_str()], true),
            (1, vec![user1.as_str()], false)
        ]
    );
    assert_eq!(
        kept[1]["keptAt"], made.update_at,
        "the body before, as of the card's last change"
    );
    assert_eq!(lines(&th.client, &t.card_id, &kept[1]).await, ["first"]);
    assert_eq!(lines(&th.client, &t.card_id, &kept[0]).await, ["second"]);

    write_whole(&th.client, &t.card_id, &paragraphs(&["second"])).await;
    th.client
        .patch(
            &format!("{TEST_WS}/cards/{}", t.card_id),
            &json!({"title": "renamed"}),
        )
        .await
        .check_ok();
    assert_eq!(versions(&th.client, &t.card_id).await, kept, "no new body");

    let read = card(&th.client, &t.card_id).await;
    th.client
        .patch(
            &format!("{TEST_WS}/cards/{}", t.card_id),
            &json!({"body": null, "readAt": read.update_at}),
        )
        .await
        .check_ok();
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(said(&kept)[0], (3, vec![user1.as_str()], true));
    assert_eq!(
        lines(&th.client, &t.card_id, &kept[0]).await,
        Vec::<String>::new()
    );
    th.teardown().await;
}

/// A body written whole while editors type: what they typed is kept as a version of
/// theirs before the body that is written over it, by its writer. A write that is
/// refused keeps what they typed too. Of a batch, the cards whose bodies changed.
#[tokio::test]
async fn a_body_written_whole_over_what_editors_typed_keeps_both() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let other: Value = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", t.board_id),
            &json!({"title": "Beside it", "body": paragraphs(&["other"])}),
        )
        .await
        .created_json();
    let other_id = other["id"].as_str().expect("an identifier");
    let (user1, user2) = (th.client.me().await.id, th.client2.me().await.id);
    give(&th, &t.board_id, &user2, "editor").await;
    let mut typist = Peer::connect(&th.client2, 101).await;
    typist.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    let read_before = card(&th.client, &t.card_id).await;
    typist.write_acked("typed").await;

    let path = format!("{TEST_WS}/cards/{}", t.card_id);
    th.client
        .patch(
            &path,
            &json!({"body": paragraphs(&["whole"]), "readAt": read_before.update_at}),
        )
        .await
        .check_status(StatusCode::CONFLICT);
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(
        said(&kept),
        [
            (2, vec![user2.as_str()], true),
            (1, vec![user1.as_str()], false)
        ],
        "what was typed is kept before the write that was refused"
    );
    assert_eq!(
        lines(&th.client, &t.card_id, &kept[0]).await,
        ["first", "typed"]
    );

    write_whole(&th.client, &t.card_id, &paragraphs(&["whole"])).await;
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(said(&kept)[0], (3, vec![user1.as_str()], true));
    assert_eq!(kept.len(), 3, "{kept:?}");
    // The typist's document is the body written whole; leaving it keeps nothing more.
    typist.leave().await;
    crate::rooms::no_rooms(&th).await;
    assert_eq!(versions(&th.client, &t.card_id).await, kept);

    // A batch over two cards: one body changes, the other is written as it is.
    let (first, second) = (
        card(&th.client, &t.card_id).await,
        card(&th.client, other_id).await,
    );
    th.client
        .patch(
            &format!("{TEST_WS}/boards/{}/blocks", t.board_id),
            &json!({
                "blockIds": [t.card_id, other_id],
                "blockPatches": [
                    {"body": paragraphs(&["batched"]), "readAt": first.update_at},
                    {"body": paragraphs(&["other"]), "readAt": second.update_at},
                ],
            }),
        )
        .await
        .check_no_content();
    assert_eq!(said(&versions(&th.client, &t.card_id).await)[0].0, 4);
    assert_eq!(
        said(&versions(&th.client, other_id).await),
        [(1, vec![user1.as_str()], true)],
        "the body it had is kept, once"
    );
    th.teardown().await;
}

/// Waits until the card has `count` versions, and gives them.
pub(crate) async fn kept_after(client: &Client, card_id: &str, count: usize) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let kept = versions(client, card_id).await;
        if kept.len() == count {
            return kept;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the card has the versions {kept:?}, not {count}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
