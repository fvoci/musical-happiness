//! Going back to a version of a card's body: what is written, what is kept of the
//! body before, what the room is sent, and who may.

use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::body_versions::{lines, said, versions};
use crate::collab::{body_of, card};
use crate::common::{Client, ROOT_WS, Setup, TEST_WS};
use crate::rooms::{
    Incoming, QUIET, give, lines_of, paragraphs, table, two_in_a_room, write_whole,
};

/// The path that restores a version of a card.
fn restore_path(card_id: &str, version: i64) -> String {
    format!("{TEST_WS}/cards/{card_id}/body-versions/{version}/restore")
}

/// Restores a version of a card as the client's user, and gives the card's lines.
async fn restore(client: &Client, card_id: &str, version: i64) -> Vec<String> {
    let restored: Value = client
        .post_empty(&restore_path(card_id, version))
        .await
        .ok_json();
    assert_eq!(restored["id"], json!(card_id), "the card restored");
    lines_of(restored.get("body").filter(|body| !body.is_null()))
}

/// A version restored while the card is edited together: what the editors typed since
/// is kept as a version of theirs first, the version's body is the card's and its
/// newest version, by who restored it, and everyone in the room is sent it as a new
/// document they go on in. The versions before stay.
#[tokio::test]
async fn a_version_restored_is_sent_to_the_room_and_keeps_what_was_typed() {
    let th = Setup::native().start().await.init_basic().await;
    let (t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let (user1, user2) = (th.client.me().await.id, th.client2.me().await.id);
    one.write_acked("typed").await;
    two.settle().await;
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(kept.len(), 1, "the body before the editors came: {kept:?}");

    assert_eq!(restore(&th.client2, &t.card_id, 1).await, ["first"]);
    for peer in [&mut one, &mut two] {
        let reset = peer.text("DOC_RESET").await;
        assert_eq!(
            reset,
            json!({"action": "DOC_RESET", "doc": peer.doc, "epoch": 2})
        );
        peer.settle().await;
        assert_eq!(
            peer.lines(),
            ["first"],
            "the version, in place of the document"
        );
    }
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(
        said(&kept),
        [
            (3, vec![user2.as_str()], true),
            (2, vec![user1.as_str()], false),
            (1, Vec::new(), false)
        ]
    );
    assert_eq!(
        lines(&th.client, &t.card_id, &kept[1]).await,
        ["first", "typed"],
        "what was typed can be gone back to"
    );

    // The room goes on in the restored body, and what it types is the next version.
    two.write_acked("after").await;
    one.settle().await;
    assert_eq!(one.lines(), ["first", "after"]);
    assert_eq!(restore(&th.client, &t.card_id, 2).await, ["first", "typed"]);
    for peer in [&mut one, &mut two] {
        peer.text("DOC_RESET").await;
        peer.settle().await;
        assert_eq!(peer.lines(), ["first", "typed"]);
    }
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(
        said(&kept)[..3],
        [
            (5, vec![user1.as_str()], true),
            (4, vec![user2.as_str()], false),
            (3, vec![user2.as_str()], false)
        ]
    );
    assert_eq!(
        lines(&th.client, &t.card_id, &kept[1]).await,
        ["first", "after"]
    );
    one.leave().await;
    two.leave().await;
    crate::rooms::no_rooms(&th).await;
    assert_eq!(
        versions(&th.client, &t.card_id).await,
        kept,
        "left as it was"
    );
    th.teardown().await;
}

/// A version restored with nobody in the room is the card's body, and no body is
/// restored as none. Restoring the body the card has changes nothing: not the card,
/// not its versions, and the room is sent nothing.
#[tokio::test]
async fn a_version_restored_is_the_cards_body_and_the_same_body_again_is_nothing() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let user1 = th.client.me().await.id;
    write_whole(&th.client, &t.card_id, &paragraphs(&["second"])).await;
    let read = card(&th.client, &t.card_id).await;
    th.client
        .patch(
            &format!("{TEST_WS}/cards/{}", t.card_id),
            &json!({"body": null, "readAt": read.update_at}),
        )
        .await
        .check_ok();

    assert_eq!(restore(&th.client, &t.card_id, 1).await, ["first"]);
    assert_eq!(
        Some(lines_of(
            body_of(&card(&th.client, &t.card_id).await).as_ref()
        )),
        Some(vec!["first".to_owned()])
    );
    assert_eq!(
        said(&versions(&th.client, &t.card_id).await),
        [
            (4, vec![user1.as_str()], true),
            (3, vec![user1.as_str()], false),
            (2, vec![user1.as_str()], false),
            (1, Vec::new(), false)
        ],
        "the body before was a version already; the body restored is the newest"
    );
    assert_eq!(
        restore(&th.client, &t.card_id, 3).await,
        Vec::<String>::new(),
        "no body"
    );
    assert_eq!(body_of(&card(&th.client, &t.card_id).await), None);

    // The body the card has, by its newest version and by an older one.
    let before = card(&th.client, &t.card_id).await;
    let kept = versions(&th.client, &t.card_id).await;
    assert_eq!(kept.len(), 5, "{kept:?}");
    let mut reader = crate::rooms::Peer::connect(&th.client, 101).await;
    reader
        .joined(crate::common::TEST_WORKSPACE_ID, &t.card_id)
        .await;
    for same in [5, 3] {
        assert_eq!(
            restore(&th.client, &t.card_id, same).await,
            Vec::<String>::new()
        );
    }
    assert_eq!(versions(&th.client, &t.card_id).await, kept);
    assert_eq!(
        card(&th.client, &t.card_id).await.update_at,
        before.update_at,
        "the card is not written"
    );
    assert_eq!(reader.incoming(QUIET).await, Incoming::Nothing);
    th.teardown().await;
}

/// Only who may edit the card restores a version of it: not a viewer or a commenter
/// of its board (403), not who is no member of it or asks through another workspace
/// (no such card), and not a number the card has no version of.
#[tokio::test]
async fn only_who_edits_the_card_restores_a_version() {
    let th = Setup::native().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    write_whole(&th.client, &t.card_id, &paragraphs(&["second"])).await;
    let kept = versions(&th.client, &t.card_id).await;
    let path = restore_path(&t.card_id, 1);

    th.client2
        .post_empty(&path)
        .await
        .check_error(StatusCode::NOT_FOUND, "card not found");
    th.anonymous().post_empty(&path).await.check_unauthorized();
    th.client
        .post_empty(&path.replace(TEST_WS, ROOT_WS))
        .await
        .check_error(StatusCode::NOT_FOUND, "card not found");
    for role in ["viewer", "commenter"] {
        give(&th, &t.board_id, &user2, role).await;
        th.client2.post_empty(&path).await.check_forbidden();
        assert_eq!(
            versions(&th.client2, &t.card_id).await,
            kept,
            "a {role} reads them"
        );
    }
    for missing in ["9", "0", "two"] {
        th.client
            .post_empty(&path.replace("/1/restore", &format!("/{missing}/restore")))
            .await
            .check_error(StatusCode::NOT_FOUND, "version not found");
    }
    assert_eq!(
        versions(&th.client, &t.card_id).await,
        kept,
        "nothing restored"
    );

    give(&th, &t.board_id, &user2, "editor").await;
    assert_eq!(restore(&th.client2, &t.card_id, 1).await, ["first"]);
    assert_eq!(
        said(&versions(&th.client, &t.card_id).await)[0],
        (3, vec![user2.as_str()], true)
    );

    // Not of a card in the trash.
    th.client
        .delete(&crate::blocks::block_path(&t.board_id, &t.card_id))
        .await
        .check_no_content();
    th.client
        .post_empty(&path)
        .await
        .check_error(StatusCode::NOT_FOUND, "card not found");
    th.teardown().await;
}
