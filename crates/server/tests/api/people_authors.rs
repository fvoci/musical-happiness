//! Departed authors remain named only through work the reader can read.

use fvoci_model::{
    Block, BlockPatch, BlockType, Board, BoardMember, BoardType, Body, BodyChange, User, new_id,
};
use serde_json::json;

use crate::boards::{member, new_board};
use crate::common::{
    Client, ROOT_WORKSPACE_ID, ROOT_WS, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer,
};

#[derive(Clone, Copy)]
enum Authorship {
    Board,
    CommentVersion,
    TrashedCommentVersion,
}

fn body(text: &str) -> Body {
    Body::parse(json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": text}]}
    ]}))
    .expect("valid body")
    .expect("nonempty body")
}

async fn authored_board(th: &TestServer, authorship: Authorship, leaver: &str) -> Board {
    let owner = th.client.me().await.id;
    let creator = match authorship {
        Authorship::Board => &th.client2,
        Authorship::CommentVersion | Authorship::TrashedCommentVersion => &th.client,
    };
    let board: Board = creator
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            BoardType::Private,
            "Authored",
        ))
        .await
        .created_json();
    if matches!(authorship, Authorship::Board) {
        creator
            .add_member(&BoardMember {
                scheme_admin: true,
                ..member(&board.id, &owner)
            })
            .await
            .check_ok();
    } else {
        let mut card = Block {
            id: new_id(),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            ..Block::default()
        };
        th.app
            .store()
            .insert_block(&mut card, &owner)
            .await
            .expect("a card");
        let mut comment = Block {
            id: new_id(),
            board_id: board.id.clone(),
            parent_id: card.id,
            block_type: BlockType::Comment,
            body: Some(body("Original")),
            ..Block::default()
        };
        th.app
            .store()
            .insert_block(&mut comment, &owner)
            .await
            .expect("a comment");
        for (writer, text) in [(leaver, "An earlier revision"), (owner.as_str(), "Current")] {
            th.app
                .store()
                .patch_block(
                    &comment.id,
                    &BlockPatch {
                        body: BodyChange::Set(body(text)),
                        ..BlockPatch::default()
                    },
                    writer,
                )
                .await
                .expect("a revised comment");
        }
        let current = th
            .app
            .store()
            .get_block(&comment.id)
            .await
            .expect("the comment");
        assert_eq!(
            current.created_by, owner,
            "the leaver is not the original writer"
        );
        assert_eq!(
            current.modified_by, owner,
            "the leaver is not the last writer"
        );
        if matches!(authorship, Authorship::TrashedCommentVersion) {
            th.app
                .store()
                .delete_block(&comment.id, &owner)
                .await
                .expect("the comment is trashed");
            th.client
                .get(&format!(
                    "{TEST_WS}/boards/{}/blocks/{}/versions",
                    board.id, comment.id
                ))
                .await
                .check_ok();
        }
    }
    board
}

async fn named(client: &Client, workspace: &str, user: &str) -> Vec<User> {
    client
        .post(&format!("{workspace}/users"), &[user])
        .await
        .ok_json()
}

async fn check_authorship(authorship: Authorship) {
    let th = Setup::native().start().await.init_basic().await;
    let leaver = th.client2.me().await.id;
    let board = authored_board(&th, authorship, &leaver).await;
    for workspace in [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID] {
        th.app.store().execute_unchecked(&format!(
            "DELETE FROM workspace_members WHERE user_id = '{leaver}' AND workspace_id = '{workspace}'"
        )).await.expect("the author leaves");
    }
    let by_id = named(&th.client, TEST_WS, &leaver).await;
    assert_eq!(
        by_id
            .iter()
            .map(|user| user.id.as_str())
            .collect::<Vec<_>>(),
        [leaver.as_str()],
        "a readable contribution still names its departed author"
    );
    th.client
        .get(&format!("{TEST_WS}/users/{leaver}"))
        .await
        .check_ok();
    assert!(
        named(&th.client, ROOT_WS, &leaver).await.is_empty(),
        "no contribution in another workspace"
    );
    let found: Vec<User> = th
        .client
        .get(&format!("{TEST_WS}/users?search=user2"))
        .await
        .ok_json();
    assert!(
        found.is_empty(),
        "an author is not offered among current people"
    );
    let members: Vec<BoardMember> = th
        .client
        .get(&format!("{TEST_WS}/boards/{}/members", board.id))
        .await
        .ok_json();
    assert!(
        members.iter().all(|member| member.user_id != leaver),
        "an author is no member"
    );

    let guest = th.add_user("reader").await;
    let guest_id = guest.me().await.id;
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = 'guest' WHERE user_id = '{guest_id}'"
        ))
        .await
        .expect("a guest");
    assert!(
        named(&guest, TEST_WS, &leaver).await.is_empty(),
        "a guest cannot name an unread author"
    );
    th.client
        .add_member(&BoardMember {
            scheme_viewer: true,
            ..member(&board.id, &guest_id)
        })
        .await
        .check_ok();
    assert_eq!(
        named(&guest, TEST_WS, &leaver).await.len(),
        1,
        "a guest reading the contribution names its author"
    );
    th.app
        .store()
        .execute_unchecked(&format!(
            "DELETE FROM board_members WHERE board_id = '{}'",
            board.id,
        ))
        .await
        .expect("neither reader has a board role");
    for client in [&th.client, &guest] {
        assert!(
            named(client, TEST_WS, &leaver).await.is_empty(),
            "an unread private board reveals no author"
        );
        client
            .get(&format!("{TEST_WS}/users/{leaver}"))
            .await
            .check_not_found();
    }
    th.teardown().await;
}

#[tokio::test]
async fn a_departed_board_creator_is_named_only_to_readers() {
    check_authorship(Authorship::Board).await;
}

#[tokio::test]
async fn a_departed_comment_revision_writer_is_named_only_to_readers() {
    check_authorship(Authorship::CommentVersion).await;
}

#[tokio::test]
async fn a_departed_writer_of_a_readable_trashed_comment_revision_is_named() {
    check_authorship(Authorship::TrashedCommentVersion).await;
}
