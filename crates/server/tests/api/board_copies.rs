//! Copies of boards and templates (`duplicateBoard`).

use fvoci_model::{
    Block, BlockType, Board, BoardMember, BoardType, BoardsAndBlocks, Category, new_id,
};
use fvoci_store::{BlockFilter, DuplicateAs};
use reqwest::StatusCode;

use crate::common::TEST_WS;
use crate::common::{ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID};

use crate::boards::{
    add_member, another_workspace, board_with_admin, category_name_of, duplicate_board,
    duplicate_board_in, editor, get_board, insert_block, member, members_of, new_board, sorted_ids,
    view_block,
};

// -- TestDuplicateBoard ---------------------------------------------------------------

/// `TestDuplicateBoard/"create and duplicate public board"`
#[tokio::test]
async fn duplicate_board_create_and_duplicate_public_board() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let board: Board = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            BoardType::Open,
            "Public board",
        ))
        .await
        .created_json();
    assert_eq!(board.created_by, me.id, "created by the session's user");
    insert_block(&th, view_block(&board.id), &me.id).await;
    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "two members");

    // Duplicate the board
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client,
        &board.id,
        &format!("?asTemplate=false&toWorkspace={TEST_WORKSPACE_ID}"),
    )
    .await
    .created_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    let duplicate = &copy.boards[0];
    assert_eq!(
        duplicate.board_type,
        BoardType::Private,
        "Duplicated board should be private"
    );
    assert_ne!(duplicate.id, board.id, "under a new identifier");
    assert_eq!(copy.blocks[0].board_id, duplicate.id, "with its block");
    assert_eq!(copy.blocks[0].title, "View 1", "a copy of the view");

    let members = members_of(&th, &duplicate.id).await;
    assert_eq!(
        members.len(),
        1,
        "Duplicated board should only have one member"
    );
    assert_eq!(members[0].user_id, me.id, "the user who duplicated");
    assert_eq!(members[0].board_id, duplicate.id, "of the copy");
    assert!(members[0].scheme_admin, "as an admin");

    assert_eq!(
        category_name_of(&th, &me.id, TEST_WORKSPACE_ID, &duplicate.id).await,
        Some("Boards".to_owned()),
        "the copy is in the default category, like its source"
    );
    th.teardown().await;
}

/// `TestDuplicateBoard/"create and duplicate public board from a custom category"`
#[tokio::test]
async fn duplicate_board_create_and_duplicate_public_board_from_a_custom_category() {
    let th = Setup::native().start().await.init_basic().await;
    let me = th.client.me().await;
    let user2 = th.client2.me().await;
    let category = th
        .app
        .create_category(Category {
            name: "My Category".to_owned(),
            user_id: me.id.clone(),
            workspace_id: TEST_WORKSPACE_ID.to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");
    let board: Board = th
        .client
        .create_board(&new_board(
            TEST_WORKSPACE_ID,
            BoardType::Open,
            "Public board",
        ))
        .await
        .created_json();

    // move board to custom category
    th.app
        .add_update_user_category_board(
            TEST_WORKSPACE_ID,
            &me.id,
            &category.id,
            std::slice::from_ref(&board.id),
        )
        .await
        .expect("the board is moved to the custom category");
    insert_block(&th, view_block(&board.id), &me.id).await;
    th.client
        .add_member(&editor(&board.id, &user2.id))
        .await
        .check_ok();
    assert_eq!(members_of(&th, &board.id).await.len(), 2, "two members");

    // Duplicate the board
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client,
        &board.id,
        &format!("?asTemplate=false&toWorkspace={TEST_WORKSPACE_ID}"),
    )
    .await
    .created_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    let duplicate = &copy.boards[0];
    assert_eq!(
        duplicate.board_type,
        BoardType::Private,
        "Duplicated board should be private"
    );
    assert_eq!(duplicate.title, "Public board copy", "the title says so");

    let members = members_of(&th, &duplicate.id).await;
    assert_eq!(
        members.len(),
        1,
        "Duplicated board should only have one member"
    );
    assert_eq!(members[0].user_id, me.id, "the user who duplicated");
    assert!(members[0].scheme_admin, "as an admin");

    // verify duplicated board is in the same custom category
    assert_eq!(
        category_name_of(&th, &me.id, TEST_WORKSPACE_ID, &duplicate.id).await,
        Some("My Category".to_owned()),
        "the copy is filed where its source is"
    );
    th.teardown().await;
}

/// `TestDuplicateBoard/"base case"` and `"duplicating board as template should not set
/// it's category"` in `server/app/boards_test.go`, through the real store.
#[tokio::test]
async fn duplicate_board_base_case_and_as_template() {
    let th = Setup::native().start().await;
    let user = th.stranger().await;
    let board = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, BoardType::Open, "Source"),
        &user,
    )
    .await;
    insert_block(&th, view_block(&board.id), &user).await;

    let (copy, members) = th
        .app
        .duplicate_board(&board.id, &user, TEST_WORKSPACE_ID, DuplicateAs::Copy)
        .await
        .expect("the board is duplicated");
    assert_eq!(copy.boards.len(), 1, "one board");
    assert_eq!(copy.blocks.len(), 1, "one block");
    assert_eq!(members.len(), 1, "one membership");
    assert_eq!(
        category_name_of(&th, &user, TEST_WORKSPACE_ID, &copy.boards[0].id).await,
        Some("Boards".to_owned()),
        "a copy is filed under a category"
    );

    let (template, members) = th
        .app
        .duplicate_board(&board.id, &user, TEST_WORKSPACE_ID, DuplicateAs::Template)
        .await
        .expect("the board is duplicated as a template");
    assert!(template.boards[0].is_template, "a template");
    assert_eq!(members.len(), 1, "one membership");
    assert_eq!(
        category_name_of(&th, &user, TEST_WORKSPACE_ID, &template.boards[0].id).await,
        None,
        "a template is filed nowhere"
    );
    th.teardown().await;
}

/// An open board of the test workspace that the user administers, with a card titled
/// "Card 1" and a comment on that card.
async fn board_with_a_commented_card(th: &TestServer, user_id: &str) -> Board {
    let board = board_with_admin(
        th,
        new_board(TEST_WORKSPACE_ID, BoardType::Open, "Public board"),
        user_id,
    )
    .await;
    let card = insert_block(
        th,
        Block {
            id: new_id(),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            title: "Card 1".to_owned(),
            ..Block::default()
        },
        user_id,
    )
    .await;
    insert_block(
        th,
        Block {
            id: new_id(),
            board_id: board.id.clone(),
            parent_id: card.id,
            block_type: BlockType::Comment,
            title: "a comment".to_owned(),
            ..Block::default()
        },
        user_id,
    )
    .await;
    board
}

/// Verified on the original: who may duplicate what, and what the query parameters do.
#[tokio::test]
async fn duplicate_board_permissions_and_parameters() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let board = board_with_a_commented_card(&th, &user1.id).await;

    duplicate_board(&th.anonymous(), &board.id, "")
        .await
        .check_unauthorized();
    for board_id in [UNKNOWN_ID, "nonexistent"] {
        duplicate_board(&th.client, board_id, "")
            .await
            .check_error(StatusCode::NOT_FOUND, "board not found");
    }

    // An open board that is not a template can only be duplicated by its members.
    let response = duplicate_board(&th.client2, &board.id, "").await;
    response.check_forbidden();
    assert_eq!(response.error_message(), "access denied to board", "why");

    // Without `toWorkspace` the copy stays in the workspace of the source; comments are not
    // copied.
    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "")
        .await
        .created_json();
    assert_eq!(
        copy.boards[0].workspace_id, TEST_WORKSPACE_ID,
        "the source's workspace"
    );
    assert_eq!(copy.boards[0].title, "Public board copy", "a copy");
    assert_eq!(copy.blocks.len(), 1, "the card without its comment");
    assert_eq!(copy.blocks[0].title, "Card 1", "the card");

    // `asTemplate=true` makes a template, which is filed under no category. Only the
    // exact value counts.
    let template: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "?asTemplate=true")
        .await
        .created_json();
    assert!(template.boards[0].is_template, "a template");
    assert_eq!(template.boards[0].title, "New board template", "its title");
    assert_eq!(
        category_name_of(&th, &user1.id, TEST_WORKSPACE_ID, &template.boards[0].id).await,
        None,
        "not in a category"
    );
    let not_a_template: BoardsAndBlocks =
        duplicate_board(&th.client, &board.id, "?asTemplate=True")
            .await
            .created_json();
    assert!(
        !not_a_template.boards[0].is_template,
        "`True` is not `true`"
    );

    // `toWorkspace` puts the copy, and its category, into another workspace.
    let other_workspace = another_workspace(&th, &user1.id).await;
    let moved: BoardsAndBlocks = duplicate_board(
        &th.client,
        &board.id,
        &format!("?toWorkspace={other_workspace}"),
    )
    .await
    .created_json();
    assert_eq!(
        moved.boards[0].workspace_id, other_workspace,
        "the other workspace"
    );
    assert_eq!(
        category_name_of(&th, &user1.id, &other_workspace, &moved.boards[0].id).await,
        Some("Boards".to_owned()),
        "in the default category of that workspace"
    );

    // The workspace must exist. (The original copies a board into a workspace of any name.)
    for no_workspace in [UNKNOWN_ID, "other-workspace"] {
        duplicate_board(
            &th.client,
            &board.id,
            &format!("?toWorkspace={no_workspace}"),
        )
        .await
        .check_error(StatusCode::NOT_FOUND, "workspace not found");
    }

    // A viewer of a private board may duplicate it, and administers the copy.
    let private = board_with_admin(
        &th,
        new_board(TEST_WORKSPACE_ID, BoardType::Private, "Private one"),
        &user1.id,
    )
    .await;
    insert_block(&th, view_block(&private.id), &user1.id).await;
    add_member(
        &th,
        &BoardMember {
            scheme_viewer: true,
            ..member(&private.id, &user2.id)
        },
    )
    .await;
    let copy: BoardsAndBlocks = duplicate_board(&th.client2, &private.id, "")
        .await
        .created_json();
    assert_eq!(copy.boards[0].created_by, user2.id, "created by the viewer");
    let members = members_of(&th, &copy.boards[0].id).await;
    assert_eq!(members.len(), 1, "one member: {members:?}");
    assert_eq!(members[0].user_id, user2.id, "the viewer");
    assert!(members[0].scheme_admin, "administers the copy");

    th.teardown().await;
}

/// The part of `TestGetTemplates` that belongs to this area: any user of the workspace
/// duplicates an open template into a board of their own. Verified on the original, also
/// for a template of the global workspace and for a private template.
#[tokio::test]
async fn duplicate_board_from_a_template() {
    let th = Setup::native().start().await.init_basic().await;
    let user1 = th.client.me().await;
    let user2 = th.client2.me().await;
    let mut templates = Vec::new();
    for (workspace_id, board_type, title) in [
        (TEST_WORKSPACE_ID, BoardType::Open, "Open template"),
        (ROOT_WORKSPACE_ID, BoardType::Open, "Global template"),
        (ROOT_WORKSPACE_ID, BoardType::Private, "Private template"),
    ] {
        let template = board_with_admin(
            &th,
            Board {
                is_template: true,
                ..new_board(workspace_id, board_type, title)
            },
            &user1.id,
        )
        .await;
        insert_block(&th, view_block(&template.id), &user1.id).await;
        templates.push(template);
    }

    // user2 is not a member of any of them.
    let copy: BoardsAndBlocks = duplicate_board(
        &th.client2,
        &templates[0].id,
        &format!("?asTemplate=false&toWorkspace={TEST_WORKSPACE_ID}"),
    )
    .await
    .created_json();
    let board = &copy.boards[0];
    assert_eq!(board.title, "Open template", "the template's title");
    assert!(!board.is_template, "a board, not a template");
    assert_eq!(board.board_type, BoardType::Private, "private to its user");
    assert_eq!(board.created_by, user2.id, "created by the user");
    assert_eq!(copy.blocks.len(), 1, "with the template's blocks");
    let read: Board = get_board(&th.client2, &board.id).await.ok_json();
    assert_eq!(&read, board, "the user reads their new board");
    let stored_blocks = th
        .app
        .store()
        .get_blocks(&BlockFilter::board(&board.id))
        .await
        .expect("the blocks are read");
    assert_eq!(
        stored_blocks.len(),
        copy.blocks.len(),
        "the blocks are stored"
    );
    assert_eq!(
        category_name_of(&th, &user2.id, TEST_WORKSPACE_ID, &board.id).await,
        Some("Boards".to_owned()),
        "filed under the user's default category"
    );

    // A template of a template keeps being one, and says "copy".
    let copy: BoardsAndBlocks = duplicate_board(&th.client2, &templates[0].id, "?asTemplate=true")
        .await
        .created_json();
    assert!(copy.boards[0].is_template, "a template");
    assert_eq!(copy.boards[0].title, "Open template copy", "a copy");

    // A template of the root workspace (the original's global workspace), into the user's workspace.
    let copy: BoardsAndBlocks = duplicate_board_in(
        &th.client2,
        ROOT_WORKSPACE_ID,
        &templates[1].id,
        &format!("?toWorkspace={TEST_WORKSPACE_ID}"),
    )
    .await
    .created_json();
    assert_eq!(
        copy.boards[0].workspace_id, TEST_WORKSPACE_ID,
        "in the user's workspace"
    );

    // A private template is for its members only; for others it does not exist.
    duplicate_board(
        &th.client2,
        &templates[2].id,
        &format!("?toWorkspace={TEST_WORKSPACE_ID}"),
    )
    .await
    .check_error(StatusCode::NOT_FOUND, "board not found");

    th.teardown().await;
}

/// A board without blocks is duplicated as a board without blocks. (The original
/// cannot duplicate it and answers 500.)
#[tokio::test]
async fn duplicate_board_without_blocks() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;

    let copy: BoardsAndBlocks = duplicate_board(&th.client, &board.id, "")
        .await
        .created_json();

    assert_eq!(copy.boards.len(), 1, "one board");
    assert!(copy.blocks.is_empty(), "no blocks");
    let boards: Vec<Board> = th.client.get(&format!("{TEST_WS}/boards")).await.ok_json();
    let mut expected = vec![board.id.clone(), copy.boards[0].id.clone()];
    expected.sort_unstable();
    assert_eq!(sorted_ids(&boards), expected, "the board and its copy");
    th.teardown().await;
}
