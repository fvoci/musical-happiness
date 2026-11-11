//! Who may change, move, delete and bring back a comment: its author from the
//! commenter's role up, and somebody else's the board's admins only. Not in the
//! original, where comments are blocks like any other (`server/api/blocks.go`).

use fvoci_model::{Block, BlockType};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::blocks::{
    NO_CHANGES, QUIET, Socket, batch, block_path, blocks_of, blocks_path, boards_with_roles,
    card_with_content, child_of, delete, find, insert_one, new_block, title_patch, titles,
    undelete_path,
};
use crate::common::{Setup, TEST_TEAM_ID};

/// Not in the original, where every editor can delete anyone's comment
/// (`server/api/blocks.go:372`; on the original, an editor's `DELETE` of another user's
/// comment answers 200). A comment is deleted by its author or by an admin.
#[tokio::test]
async fn delete_comment_of_another_user_needs_the_permission_to_delete_others_comments() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;

    // The comment was written by the commenter.
    let response = boards
        .editor
        .delete(&block_path(board_id, &content.comment.id))
        .await;
    response.check_forbidden();
    assert_eq!(
        response.error_message(),
        "access denied to delete other users' comments",
        "an editor is refused"
    );
    let stored = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&stored).contains(&"comment"),
        "the comment is still there: {:?}",
        titles(&stored)
    );

    // Other blocks of other users are deleted by an editor as before.
    delete(&boards.editor, &content.text).await;
}

/// The cases that stay allowed: an editor deletes a comment of their own, and an admin
/// deletes the comment of another user.
#[tokio::test]
async fn delete_comment_by_its_author_or_by_an_admin_works() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;

    let own = child_of(&content.card, BlockType::Comment, "the editor's comment");
    let own = insert_one(&boards.editor, &own).await;
    delete(&boards.editor, &own).await;

    // The commenter's comment, deleted by the board's admin.
    delete(&th.client, &content.comment).await;

    let stored = blocks_of(&th.client, board_id).await;
    assert_eq!(titles(&stored), ["card", "text"], "both comments are gone");
}

/// Not in the original, where deleting needs the editor's role whatever the block is
/// (`server/api/blocks.go:372`; user decision 2026-11-11 22:13 KST). A commenter
/// deletes their own comment and brings it back, and does neither to anything else: not
/// to somebody else's comment, not to a card, and not to their comment that went with a
/// card, since the card decides what comes back.
#[tokio::test]
async fn a_commenter_deletes_and_brings_back_their_own_comment() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let own = &content.comment;

    delete(&boards.commenter, own).await;
    assert!(!titles(&blocks_of(&th.client, board_id).await).contains(&"comment"));
    boards
        .commenter
        .post(&undelete_path(board_id, &own.id), &json!({}))
        .await
        .check_ok();
    assert!(titles(&blocks_of(&th.client, board_id).await).contains(&"comment"));

    let editors = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Comment, "the editor's"),
    )
    .await;
    boards
        .commenter
        .delete(&block_path(board_id, &editors.id))
        .await
        .check_error(
            StatusCode::FORBIDDEN,
            "access denied to delete other users' comments",
        );
    boards
        .commenter
        .delete(&block_path(board_id, &content.card.id))
        .await
        .check_error(StatusCode::FORBIDDEN, NO_CHANGES);

    // The card, deleted by the editor with the commenter's comment on it.
    delete(&boards.editor, &content.card).await;
    for id in [&content.card.id, &own.id] {
        boards
            .commenter
            .post(&undelete_path(board_id, id), &json!({}))
            .await
            .check_error(StatusCode::FORBIDDEN, NO_CHANGES);
    }
    assert!(
        blocks_of(&th.client, board_id).await.is_empty(),
        "all still in the trash"
    );
}

/// Not in the original (user decision 2026-11-11 22:13 KST). Deleting a comment takes
/// the replies below it, so a reply of somebody else keeps the author of the comment
/// from deleting it and bringing it back, as the reply itself would: the board's admins
/// do both. The author's own replies are no hindrance.
#[tokio::test]
async fn a_reply_of_somebody_else_keeps_a_comment_from_being_deleted_by_its_author() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let own = &content.comment;
    let path = block_path(board_id, &own.id);
    let back = undelete_path(board_id, &own.id);

    insert_one(
        &boards.commenter,
        &child_of(own, BlockType::Comment, "my own reply"),
    )
    .await;
    let reply = insert_one(
        &boards.editor,
        &child_of(own, BlockType::Comment, "a reply"),
    )
    .await;
    boards.commenter.delete(&path).await.check_error(
        StatusCode::FORBIDDEN,
        "access denied to delete other users' comments",
    );
    assert_eq!(
        blocks_of(&th.client, board_id).await.len(),
        5,
        "nothing was deleted"
    );

    delete(&th.client, own).await;
    boards.commenter.post(&back, &json!({})).await.check_error(
        StatusCode::FORBIDDEN,
        "access denied to undelete other users' comments",
    );
    th.client.post(&back, &json!({})).await.check_ok();

    // Without the reply of somebody else, the author deletes the comment and brings
    // it back with their own reply, also after an admin deleted it.
    delete(&boards.editor, &reply).await;
    delete(&boards.commenter, own).await;
    boards.commenter.post(&back, &json!({})).await.check_ok();
    delete(&th.client, own).await;
    boards.commenter.post(&back, &json!({})).await.check_ok();
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"comment") && titles(&read).contains(&"my own reply"),
        "{:?}",
        titles(&read)
    );
}

/// Not in the original (user decision 2026-11-11 21:00 KST). What comes back is the
/// whole deletion, so the block that was deleted decides: a block below somebody else's
/// comment, deleted with it, does not bring the comment back for an editor, and a
/// comment of somebody else that went with its card comes back with the card, which an
/// editor may delete and bring back.
#[tokio::test]
async fn a_deletion_comes_back_as_its_top_block_allows() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let others = &content.comment;

    // The editor's reply below the commenter's comment, deleted with it by the admin.
    let reply = insert_one(
        &boards.editor,
        &child_of(others, BlockType::Comment, "a reply"),
    )
    .await;
    delete(&th.client, others).await;
    boards
        .editor
        .post(&undelete_path(board_id, &reply.id), &json!({}))
        .await
        .check_error(
            StatusCode::FORBIDDEN,
            "access denied to undelete other users' comments",
        );
    th.client
        .post(&undelete_path(board_id, &reply.id), &json!({}))
        .await
        .check_ok();
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"comment") && titles(&read).contains(&"a reply"),
        "both back by the admin: {:?}",
        titles(&read)
    );

    // The card, deleted by the editor with the commenter's comment on it: the editor
    // brings it back, naming the card or the comment.
    delete(&boards.editor, &content.card).await;
    boards
        .editor
        .post(&undelete_path(board_id, &others.id), &json!({}))
        .await
        .check_ok();
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"card") && titles(&read).contains(&"comment"),
        "{:?}",
        titles(&read)
    );
}

/// Not in the original (user decision 2026-11-11 21:00 KST). A deleted comment is
/// brought back only by who may delete it: its author, and the comment of somebody else
/// the board's admins only. An editor and a commenter are refused another's. A card is
/// brought back by an editor as before.
#[tokio::test]
async fn a_deleted_comment_is_brought_back_only_by_who_may_delete_it() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let others = undelete_path(board_id, &content.comment.id);

    // The commenter's comment, deleted by the admin.
    delete(&th.client, &content.comment).await;
    boards.editor.post(&others, &json!({})).await.check_error(
        StatusCode::FORBIDDEN,
        "access denied to undelete other users' comments",
    );
    let editors = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Comment, "the editor's comment"),
    )
    .await;
    delete(&boards.editor, &editors).await;
    boards
        .commenter
        .post(&undelete_path(board_id, &editors.id), &json!({}))
        .await
        .check_error(
            StatusCode::FORBIDDEN,
            "access denied to undelete other users' comments",
        );
    assert!(
        !titles(&blocks_of(&th.client, board_id).await).contains(&"comment"),
        "still in the trash"
    );
    th.client.post(&others, &json!({})).await.check_ok();
    assert!(titles(&blocks_of(&th.client, board_id).await).contains(&"comment"));

    // The editor's own comment, and a card, by the editor.
    let own = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Comment, "the editor's"),
    )
    .await;
    delete(&boards.editor, &own).await;
    boards
        .editor
        .post(&undelete_path(board_id, &own.id), &json!({}))
        .await
        .check_ok();
    let card = insert_one(
        &th.client,
        &new_block(board_id, BlockType::Card, "card two"),
    )
    .await;
    delete(&th.client, &card).await;
    boards
        .editor
        .post(&undelete_path(board_id, &card.id), &json!({}))
        .await
        .check_ok();
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"the editor's") && titles(&read).contains(&"card two"),
        "{:?}",
        titles(&read)
    );
}

/// A comment is changed by its author with the commenter's role, and the comment of
/// somebody else by the board's admins only, as it is deleted; an editor may not change
/// it. `patchBlock`, `patchBlocks` and `patchBoardsAndBlocks` alike. Who changed it and
/// when is what its `modifiedBy` and `updateAt` say to the members.
#[tokio::test]
async fn a_comment_is_changed_by_its_author_or_an_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let commenter = boards.commenter.me().await;
    let admin = th.client.me().await;
    let comment = &content.comment;
    let path = block_path(board_id, &comment.id);
    let many = format!("/boards/{board_id}/blocks");
    let together = |patch: Value| {
        json!({
            "boardIds": [board_id], "boardPatches": [{}],
            "blockIds": [comment.id], "blockPatches": [patch],
        })
    };
    let others_refused = "access denied to change other users' comments";
    let stored = |blocks: &[Block]| {
        blocks
            .iter()
            .find(|block| block.id == comment.id)
            .cloned()
            .expect("the comment is there")
    };

    // The author, with no more than the commenter's role, on both routes.
    let mut member = Socket::subscribed_to_team(&th, &th.client, TEST_TEAM_ID).await;
    boards
        .commenter
        .patch(&path, &title_patch("by its author"))
        .await
        .check_no_content();
    let told = member.next_block().await;
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(read.title, "by its author");
    assert_eq!(read.modified_by, commenter.id, "who changed it");
    assert!(read.update_at > comment.update_at, "and when: later");
    assert_eq!(told["modifiedBy"], json!(commenter.id), "{told}");
    assert_eq!(told["updateAt"], json!(read.update_at), "{told}");
    boards
        .commenter
        .patch(&many, &batch(&[(comment, "again by its author")]))
        .await
        .check_no_content();
    th.client
        .patch(&many, &batch(&[(comment, "by the admin, many")]))
        .await
        .check_no_content();
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(
        read.modified_by, admin.id,
        "patchBlocks says who changed it"
    );
    boards
        .commenter
        .patch(&many, &batch(&[(comment, "again by its author")]))
        .await
        .check_no_content();

    // An editor is refused, on every route, and nothing changes.
    let response = boards
        .editor
        .patch(&path, &title_patch("by an editor"))
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let response = boards
        .editor
        .patch(&many, &batch(&[(comment, "by an editor")]))
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let response = boards
        .editor
        .patch(
            "/boards-and-blocks",
            &together(json!({"title": "by an editor"})),
        )
        .await;
    response.check_error(StatusCode::FORBIDDEN, others_refused);
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(
        read.title, "again by its author",
        "the editors changed nothing"
    );
    assert_eq!(read.modified_by, commenter.id);

    // The board's admin changes it, and is who changed it last.
    while member.next_json(QUIET).await.is_some() {}
    th.client
        .patch(&path, &title_patch("by the admin"))
        .await
        .check_no_content();
    let told = member.next_block().await;
    assert_eq!(told["modifiedBy"], json!(admin.id), "{told}");
    th.client
        .patch(
            "/boards-and-blocks",
            &together(json!({"title": "by the admin again"})),
        )
        .await
        .check_ok();
    let read = stored(&blocks_of(&th.client, board_id).await);
    assert_eq!(read.title, "by the admin again");
    assert_eq!(read.modified_by, admin.id);
}

/// The commenter's role changes the commenter's own comment, not a card, and makes no
/// card of a comment; a viewer changes nothing.
#[tokio::test]
async fn a_commenter_changes_no_card_and_makes_none() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let path = block_path(board_id, &content.comment.id);
    let many = format!("/boards/{board_id}/blocks");

    boards
        .commenter
        .patch(&path, &json!({"type": "card"}))
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    boards
        .commenter
        .patch(
            &block_path(board_id, &content.card.id),
            &title_patch("a card"),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    let own = insert_one(
        &boards.commenter,
        &child_of(&content.card, BlockType::Comment, "own"),
    )
    .await;
    boards
        .commenter
        .patch(
            &many,
            &batch(&[(&own, "own, changed"), (&content.card, "x")]),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    boards
        .commenter
        .patch(
            &many,
            &json!({"blockIds": [own.id], "blockPatches": [{"type": "card"}]}),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    // A viewer changes nothing.
    boards
        .viewer
        .patch(&block_path(board_id, &own.id), &title_patch("by a viewer"))
        .await
        .check_error(StatusCode::FORBIDDEN, "access denied to make board changes");
    let read = blocks_of(&th.client, board_id).await;
    assert!(
        titles(&read).contains(&"own") && titles(&read).contains(&"card"),
        "nothing changed: {:?}",
        titles(&read)
    );
}

/// Not in the original (coordinator's decision 2026-11-11 21:10 KST). The commenter's
/// role changes the text of the commenter's own comment, its body; moving it to another
/// card (`parentId`) is for editors, refused as any change the role may not make on
/// patchBlock and patchBlocks; patchBoardsAndBlocks refuses a commenter before, by the
/// board's permission. A `parentId` that stays as it was moves nothing. An editor
/// moves their own comment and not somebody else's; an admin moves anybody's.
#[tokio::test]
async fn a_commenter_changes_the_text_of_their_comment_but_does_not_move_it() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let second = insert_one(&th.client, &new_block(board_id, BlockType::Card, "second")).await;
    let own = &content.comment;
    let path = block_path(board_id, &own.id);
    let doc = |text: &str| {
        json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": text}]},
        ]})
    };

    boards
        .commenter
        .patch(
            &path,
            &json!({"body": doc("changed"), "readAt": own.update_at, "parentId": own.parent_id}),
        )
        .await
        .check_no_content();
    let refused = "access denied to make board changes";
    boards
        .commenter
        .patch(&path, &json!({"parentId": second.id}))
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    boards
        .commenter
        .patch(
            &blocks_path(board_id),
            &json!({"blockIds": [own.id], "blockPatches": [{"parentId": second.id}]}),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    let moved_together = json!({
        "boardIds": [board_id], "boardPatches": [{}],
        "blockIds": [own.id], "blockPatches": [{"parentId": second.id}],
    });
    boards
        .commenter
        .patch("/boards-and-blocks", &moved_together)
        .await
        .check_error(
            StatusCode::FORBIDDEN,
            "access denied to modifying board properties",
        );
    let read = blocks_of(&th.client, board_id).await;
    let comment = find(&read, &own.id);
    assert_eq!(comment.parent_id, content.card.id, "not moved");
    assert_eq!(
        comment.body.as_ref().map(|body| body.document().clone()),
        Some(doc("changed")),
        "its text changed"
    );
}

/// Not in the original (coordinator's decision 2026-11-11 21:10 KST). An editor moves
/// their own comment to another card, alone or with the board, but not somebody else's;
/// an admin moves anybody's.
#[tokio::test]
async fn an_editor_moves_their_own_comment_and_an_admin_anybodys() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let second = insert_one(&th.client, &new_block(board_id, BlockType::Card, "second")).await;
    let own = &content.comment;
    let path = block_path(board_id, &own.id);

    // An editor moves their own comment, alone or with the board.
    let editors = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Comment, "the editor's"),
    )
    .await;
    boards
        .editor
        .patch(
            &block_path(board_id, &editors.id),
            &json!({"parentId": second.id}),
        )
        .await
        .check_no_content();
    boards
        .editor
        .patch(
            "/boards-and-blocks",
            &json!({
                "boardIds": [board_id], "boardPatches": [{}],
                "blockIds": [editors.id], "blockPatches": [{"parentId": content.card.id}],
            }),
        )
        .await
        .check_ok();
    // A comment on the board itself: naming the board as its parent moves nothing.
    let on_board = insert_one(
        &boards.commenter,
        &new_block(board_id, BlockType::Comment, "on the board"),
    )
    .await;
    boards
        .commenter
        .patch(
            &block_path(board_id, &on_board.id),
            &json!({"parentId": board_id, "title": "still on the board"}),
        )
        .await
        .check_no_content();

    // Somebody else's comment: not the editor, but the admin.
    boards
        .editor
        .patch(&path, &json!({"parentId": second.id}))
        .await
        .check_error(
            StatusCode::FORBIDDEN,
            "access denied to change other users' comments",
        );
    th.client
        .patch(&path, &json!({"parentId": second.id}))
        .await
        .check_no_content();
    let read = blocks_of(&th.client, board_id).await;
    assert_eq!(find(&read, &editors.id).parent_id, content.card.id);
    assert_eq!(
        find(&read, &own.id).parent_id,
        second.id,
        "moved by the admin"
    );
}

/// A block of somebody else is made a comment by the board's admins only: a comment
/// says who wrote it, and an editor makes nobody else say anything. The author of a
/// block makes it a comment with the editor's role.
#[tokio::test]
async fn a_block_of_somebody_else_is_made_a_comment_by_an_admin() {
    let th = Setup::native().start().await.init_basic().await;
    let boards = boards_with_roles(&th).await;
    let content = card_with_content(&th, &boards).await;
    let board_id = &boards.board.id;
    let into_comment = json!({"type": "comment", "title": "I approve"});
    let refused = "access denied to change other users' comments";

    boards
        .editor
        .patch(&block_path(board_id, &content.text.id), &into_comment)
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    boards
        .editor
        .patch(
            &format!("/boards/{board_id}/blocks"),
            &json!({"blockIds": [content.text.id], "blockPatches": [into_comment]}),
        )
        .await
        .check_error(StatusCode::FORBIDDEN, refused);
    let own = insert_one(
        &boards.editor,
        &child_of(&content.card, BlockType::Attachment, "the editor's"),
    )
    .await;
    boards
        .editor
        .patch(&block_path(board_id, &own.id), &into_comment)
        .await
        .check_no_content();
    th.client
        .patch(&block_path(board_id, &content.text.id), &into_comment)
        .await
        .check_no_content();
}
