//! Port of `server/services/store/storetests/boards.go`.
//!
//! One test per test function of the original; its subtests are the commented sections,
//! in the original order, on one store. The original sleeps between writes of one board
//! so that two history rows do not collide; the port does not need that and sleeps only
//! where an assertion compares timestamps.
//!
//! The tests after [`get_board_count`] have no counterpart in the original, which does
//! not test templates, the template tracking identifier or `CanSeeUser`.

use std::time::Duration;

use fvoci_model::{
    Block, Board, BoardMember, BoardPatch, BoardRole, BoardSearchField, BoardType, GLOBAL_TEAM_ID,
    IdKind, JsonMap, SYSTEM_USER_ID, new_id,
};
use fvoci_store::{BlockFilter, HistoryOrder, HistoryQuery, PublicBoards, StoreError};
use serde_json::{Value, json};

use crate::common;

const TEST_USER_ID: &str = "user-id";
const TEST_TEAM_ID: &str = "team-id";
const TEST_BOARD_ID: &str = "board-id";

/// A board with the three members nearly every test sets.
fn new_board(id: &str, team_id: &str, board_type: BoardType) -> Board {
    Board {
        id: id.to_owned(),
        team_id: team_id.to_owned(),
        board_type,
        ..Board::default()
    }
}

fn object(value: Value) -> JsonMap {
    serde_json::from_value(value).expect("the value is a JSON object")
}

fn sorted_board_ids(boards: &[Board]) -> Vec<String> {
    let mut ids: Vec<String> = boards.iter().map(|board| board.id.clone()).collect();
    ids.sort();
    ids
}

fn sorted_by_id(mut boards: Vec<Board>) -> Vec<Board> {
    boards.sort_by(|a, b| a.id.cmp(&b.id));
    boards
}

fn sorted_user_ids(members: &[BoardMember]) -> Vec<String> {
    let mut ids: Vec<String> = members
        .iter()
        .map(|member| member.user_id.clone())
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn get_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "existing board"
    {
        let mut board = new_board("id-1", TEST_TEAM_ID, BoardType::Open);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");

        let r_board = test.get_board(&board.id).await.expect("the board exists");
        assert_eq!(board.id, r_board.id);
        assert_eq!(board.team_id, r_board.team_id);
        assert_eq!(user_id, r_board.created_by);
        assert_eq!(user_id, r_board.modified_by);
        assert_eq!(board.board_type, r_board.board_type);
        assert_ne!(r_board.create_at, 0);
        assert_ne!(r_board.update_at, 0);
    }

    // "nonexisting board"
    {
        let error = test
            .get_board("nonexistent-id")
            .await
            .expect_err("there is no such board");
        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
        assert_eq!(error.to_string(), "{boards} not found");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_boards_for_user_and_team() {
    let test = common::setup().await;
    let user_id = "user-id-1";

    // "should return empty list if no results are found"
    {
        let boards = test
            .get_boards_for_user_and_team(TEST_USER_ID, TEST_TEAM_ID, PublicBoards::Include)
            .await
            .expect("the query succeeds");
        assert!(boards.is_empty(), "got {boards:?}");
    }

    // "should return only the boards of the team that the user is a member of"
    {
        let team_id_1 = "team-id-1";
        let team_id_2 = "team-id-2";

        // team 1 boards
        let mut board1 = new_board("board-id-1", team_id_1, BoardType::Open);
        test.insert_board_with_admin(&mut board1, user_id)
            .await
            .expect("board 1 is inserted");

        let mut board2 = new_board("board-id-2", team_id_1, BoardType::Private);
        test.insert_board_with_admin(&mut board2, user_id)
            .await
            .expect("board 2 is inserted");

        let mut board3 = new_board("board-id-3", team_id_1, BoardType::Open);
        test.insert_board(&mut board3, "other-user")
            .await
            .expect("board 3 is inserted");

        let mut board4 = new_board("board-id-4", team_id_1, BoardType::Private);
        test.insert_board(&mut board4, "other-user")
            .await
            .expect("board 4 is inserted");

        // team 2 boards
        let mut board5 = new_board("board-id-5", team_id_2, BoardType::Open);
        test.insert_board_with_admin(&mut board5, user_id)
            .await
            .expect("board 5 is inserted");

        let mut board6 = new_board("board-id-6", team_id_1, BoardType::Private);
        test.insert_board(&mut board6, "other-user")
            .await
            .expect("board 6 is inserted");

        // "should only find the two boards that the user is a member of for team 1 plus
        // the one open board"
        {
            let found = test
                .get_boards_for_user_and_team(user_id, team_id_1, PublicBoards::Include)
                .await
                .expect("the query succeeds");
            assert_eq!(
                sorted_by_id(found),
                [board1.clone(), board2.clone(), board3]
            );
        }

        // "should only find the two boards that the user is a member of for team 1"
        {
            let found = test
                .get_boards_for_user_and_team(user_id, team_id_1, PublicBoards::Exclude)
                .await
                .expect("the query succeeds");
            assert_eq!(sorted_by_id(found), [board1, board2]);
        }

        // "should only find the board that the user is a member of for team 2"
        {
            let found = test
                .get_boards_for_user_and_team(user_id, team_id_2, PublicBoards::Include)
                .await
                .expect("the query succeeds");
            assert_eq!(found.len(), 1, "got {found:?}");
            assert_eq!(board5.id, found[0].id);
        }
    }

    test.teardown().await;
}

/// A case of the original's `testGetBoardsInTeamByIds` table; each is a subtest there.
struct BoardsByIdsCase {
    name: &'static str,
    board_ids: [&'static str; 2],
    expected_error: bool,
    expected_len: usize,
}

#[tokio::test]
async fn get_boards_in_team_by_ids() {
    let test = common::setup().await;

    // "should return err not all found if one or more of the ids are not found"
    {
        for board_id in ["board-id-1", "board-id-2"] {
            let mut board = new_board(board_id, TEST_TEAM_ID, BoardType::Open);
            test.insert_board_with_admin(&mut board, TEST_USER_ID)
                .await
                .expect("the board is inserted");
        }

        let cases = [
            BoardsByIdsCase {
                name: "if none of the IDs are found",
                board_ids: ["nonexistent-1", "nonexistent-2"],
                expected_error: true,
                expected_len: 0,
            },
            BoardsByIdsCase {
                name: "if not all of the IDs are found",
                board_ids: ["nonexistent-1", "board-id-1"],
                expected_error: true,
                expected_len: 1,
            },
            BoardsByIdsCase {
                name: "if all of the IDs are found",
                board_ids: ["board-id-1", "board-id-2"],
                expected_error: false,
                expected_len: 2,
            },
        ];

        for case in cases {
            let board_ids = case.board_ids.map(str::to_owned);
            let result = test
                .get_boards_in_team_by_ids(&board_ids, TEST_TEAM_ID)
                .await;
            if case.expected_error {
                let error = result.expect_err(case.name);
                // The original returns the boards it did find next to the error and the
                // test checks their number. A `Result` carries one or the other, so how
                // many of the boards exist is checked through another query below.
                match &error {
                    StoreError::NotAllFound { entity, ids } => {
                        assert_eq!(*entity, "board", "{}", case.name);
                        assert_eq!(ids, &board_ids, "{}", case.name);
                    }
                    other => panic!("{}: got {other:?}", case.name),
                }
                assert!(
                    error.is_not_found(),
                    "Should be ErrNotFound compatible error"
                );
                let found = test
                    .get_boards_for_user_and_team(TEST_USER_ID, TEST_TEAM_ID, PublicBoards::Include)
                    .await
                    .expect("the query succeeds")
                    .iter()
                    .filter(|board| board_ids.contains(&board.id))
                    .count();
                assert_eq!(found, case.expected_len, "{}", case.name);
            } else {
                let boards = result.expect(case.name);
                assert_eq!(boards.len(), case.expected_len, "{}", case.name);
            }
        }
    }

    test.teardown().await;
}

#[tokio::test]
async fn insert_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "valid public board"
    {
        let mut board = new_board("id-test-public", TEST_TEAM_ID, BoardType::Open);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.id, "id-test-public");
        assert_eq!(board.board_type, BoardType::Open);
        assert_ne!(board.create_at, 0);
        assert_ne!(board.update_at, 0);
        assert_eq!(board.delete_at, 0);
        assert_eq!(user_id, board.created_by);
        assert_eq!(board.created_by, board.modified_by);
    }

    // "valid private board"
    {
        let mut board = new_board("id-test-private", TEST_TEAM_ID, BoardType::Private);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.id, "id-test-private");
        assert_eq!(board.board_type, BoardType::Private);
        assert_ne!(board.create_at, 0);
        assert_ne!(board.update_at, 0);
        assert_eq!(board.delete_at, 0);
        assert_eq!(user_id, board.created_by);
        assert_eq!(board.created_by, board.modified_by);
    }

    // "invalid properties field board"
    //
    // Not expressible: the original stores a function value in the board's properties to
    // make their serialization fail. The properties are a `JsonMap` here, which can only
    // hold values that serialize.

    // "update board"
    {
        let mut board = Board {
            id: "id-test-public".to_owned(),
            team_id: TEST_TEAM_ID.to_owned(),
            title: "New title".to_owned(),
            ..Board::default()
        };
        test.insert_board(&mut board, "user2")
            .await
            .expect("the board is updated");
        assert_eq!("New title", board.title);
        assert_eq!("user2", board.modified_by);
    }

    // "test update board type"
    {
        let mut board = Board {
            id: "id-test-type-board".to_owned(),
            title: "Public board".to_owned(),
            board_type: BoardType::Open,
            ..Board::default()
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(BoardType::Open, board.board_type);

        let mut board_update = Board {
            id: "id-test-type-board".to_owned(),
            board_type: BoardType::Private,
            ..Board::default()
        };
        test.insert_board(&mut board_update, user_id)
            .await
            .expect("the board is updated");
        assert_eq!(BoardType::Private, board_update.board_type);
    }

    test.teardown().await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn patch_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "should return error if the board doesn't exist"
    {
        let patch = BoardPatch {
            title: Some("A new title".to_owned()),
            ..BoardPatch::default()
        };
        let error = test
            .patch_board("nonexistent-board-id", &patch, user_id)
            .await
            .expect_err("there is no such board");
        assert!(error.is_not_found(), "got {error:?}");
    }

    // "should correctly apply a simple patch"
    {
        let board_id = new_id(IdKind::Board);
        let user_id_2 = "user-id-2";

        let mut board = Board {
            title: "A simple title".to_owned(),
            description: "A simple description".to_owned(),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(user_id, board.created_by);

        let new_title = "A new title";
        let new_description = "A new description";
        let patch = BoardPatch {
            title: Some(new_title.to_owned()),
            description: Some(new_description.to_owned()),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id_2)
            .await
            .expect("the patch is applied");
        assert_eq!(new_title, patched_board.title);
        assert_eq!(new_description, patched_board.description);
        assert_eq!(user_id, patched_board.created_by);
        assert_eq!(user_id_2, patched_board.modified_by);
    }

    // "should correctly update the board properties"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = Board {
            properties: object(json!({ "one": "1", "two": "2" })),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.properties["one"], json!("1"));
        assert_eq!(board.properties["two"], json!("2"));

        let patch = BoardPatch {
            updated_properties: object(json!({ "three": "3" })),
            deleted_properties: vec!["one".to_owned()],
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id)
            .await
            .expect("the patch is applied");
        assert!(
            !patched_board.properties.contains_key("one"),
            "got {:?}",
            patched_board.properties
        );
        assert_eq!(patched_board.properties["two"], json!("2"));
        assert_eq!(patched_board.properties["three"], json!("3"));
    }

    // "should correctly modify the board's type"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = new_board(&board_id, TEST_TEAM_ID, BoardType::Open);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open);

        let patch = BoardPatch {
            board_type: Some(BoardType::Private),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id)
            .await
            .expect("the patch is applied");
        assert_eq!(BoardType::Private, patched_board.board_type);
    }

    // "a patch that doesn't include any of the properties should not modify them"
    {
        let board_id = new_id(IdKind::Board);
        let properties = object(json!({ "prop1": "val1" }));
        let card_properties = vec![object(json!({ "prop2": "val2" }))];

        let mut board = Board {
            properties: properties.clone(),
            card_properties: card_properties.clone(),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open);
        assert_eq!(properties, board.properties);
        assert_eq!(card_properties, board.card_properties);

        let patch = BoardPatch {
            board_type: Some(BoardType::Private),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id)
            .await
            .expect("the patch is applied");
        assert_eq!(BoardType::Private, patched_board.board_type);
        assert_eq!(properties, patched_board.properties);
        assert_eq!(card_properties, patched_board.card_properties);
    }

    // "a patch that removes a card property and updates another should work correctly"
    {
        let board_id = new_id(IdKind::Board);
        let prop1 = object(json!({ "id": "prop1", "value": "val1" }));
        let prop2 = object(json!({ "id": "prop2", "value": "val2" }));
        let prop3 = object(json!({ "id": "prop3", "value": "val3" }));
        let card_properties = vec![prop1, prop2, prop3.clone()];

        let mut board = Board {
            card_properties: card_properties.clone(),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open);
        assert_eq!(card_properties, board.card_properties);

        let new_prop1 = object(json!({ "id": "prop1", "value": "newval1" }));
        let patch = BoardPatch {
            updated_card_properties: vec![new_prop1.clone()],
            deleted_card_properties: vec!["prop2".to_owned()],
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id)
            .await
            .expect("the patch is applied");
        // The original compares without regard to order; so does this.
        let by_property_id = |a: &JsonMap, b: &JsonMap| a["id"].as_str().cmp(&b["id"].as_str());
        let mut expected = vec![new_prop1, prop3];
        expected.sort_by(by_property_id);
        let mut got = patched_board.card_properties;
        got.sort_by(by_property_id);
        assert_eq!(expected, got);
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "should return an error if the board doesn't exist"
    {
        let error = test
            .delete_board("nonexistent-board-id", user_id)
            .await
            .expect_err("there is no such board");
        assert!(error.is_not_found(), "got {error:?}");
    }

    // "should correctly delete the board"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = new_board(&board_id, TEST_TEAM_ID, BoardType::Open);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");

        test.get_board(&board_id).await.expect("the board exists");

        test.delete_board(&board_id, user_id)
            .await
            .expect("the board is deleted");

        let error = test
            .get_board(&board_id)
            .await
            .expect_err("the board is gone");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn undelete_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "existing id"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = Board {
            title: "Dunder Mifflin Scranton".to_owned(),
            minimum_role: BoardRole::Commenter,
            description: "Bears, beets, Battlestar Gallectica".to_owned(),
            icon: "🐻".to_owned(),
            show_description: true,
            is_template: false,
            properties: object(json!({ "prop_1": "value_1" })),
            card_properties: vec![object(json!({ "prop_1": "value_1" }))],
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");

        test.delete_board(&board_id, user_id)
            .await
            .expect("the board is deleted");

        test.get_board(&board_id)
            .await
            .expect_err("the board is gone");

        test.undelete_board(&board_id, user_id)
            .await
            .expect("the board is restored");

        let board = test.get_board(&board_id).await.expect("the board is back");

        // verifying the data after un-delete
        assert_eq!("Dunder Mifflin Scranton", board.title);
        assert_eq!("user-id", board.created_by);
        assert_eq!("user-id", board.modified_by);
        assert_eq!(BoardRole::Commenter, board.minimum_role);
        assert_eq!("Bears, beets, Battlestar Gallectica", board.description);
        assert_eq!("🐻", board.icon);
        assert!(board.show_description);
        assert!(!board.is_template);
        assert_eq!(board.properties["prop_1"], json!("value_1"));
        assert_eq!(1, board.card_properties.len());
        assert_eq!(board.card_properties[0]["prop_1"], json!("value_1"));
    }

    // "existing id multiple times"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = new_board(&board_id, TEST_TEAM_ID, BoardType::Open);
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");

        test.delete_board(&board_id, user_id)
            .await
            .expect("the board is deleted");

        test.get_board(&board_id)
            .await
            .expect_err("the board is gone");

        test.undelete_board(&board_id, user_id)
            .await
            .expect("the board is restored");

        test.get_board(&board_id).await.expect("the board is back");

        test.undelete_board(&board_id, user_id)
            .await
            .expect("undeleting a board that is not deleted succeeds");

        test.get_board(&board_id)
            .await
            .expect("the board is still there");
    }

    // "from not existing id"
    {
        test.undelete_board("not-exists", user_id)
            .await
            .expect("undeleting a board that never existed succeeds");

        test.get_board("not-exists")
            .await
            .expect_err("there is still no such board");
    }

    test.teardown().await;
}

#[tokio::test]
async fn insert_board_with_admin() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "should correctly create a board and the admin membership with the creator"
    {
        let board_id = new_id(IdKind::Board);

        let mut board = new_board(&board_id, TEST_TEAM_ID, BoardType::Open);
        let new_member = test
            .insert_board_with_admin(&mut board, user_id)
            .await
            .expect("the board and its admin are inserted");
        assert_eq!(user_id, board.created_by);
        assert_eq!(user_id, board.modified_by);
        assert_eq!(user_id, new_member.user_id);
        assert_eq!(board_id, new_member.board_id);
        assert!(new_member.scheme_admin);
        assert!(new_member.scheme_editor);
    }

    test.teardown().await;
}

#[tokio::test]
async fn save_member() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;
    let board_id = TEST_BOARD_ID;

    // "should correctly create a member"
    {
        let member = BoardMember {
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            scheme_admin: true,
            ..BoardMember::default()
        };

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        let initial_member_history = member_history.len();

        test.save_member(&member)
            .await
            .expect("the member is saved");
        // The original checks the member it gets back; the port returns nothing, so the
        // same members are checked on the stored membership.
        let stored = test
            .get_member_for_board(board_id, user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id);
        assert_eq!(board_id, stored.board_id);

        assert!(stored.scheme_admin);

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        assert_eq!(member_history.len(), initial_member_history + 1);
    }

    // "should correctly update a member"
    {
        let member = BoardMember {
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            scheme_editor: true,
            scheme_viewer: true,
            ..BoardMember::default()
        };

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        let initial_member_history = member_history.len();

        test.save_member(&member)
            .await
            .expect("the member is saved");
        let stored = test
            .get_member_for_board(board_id, user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id);
        assert_eq!(board_id, stored.board_id);

        assert!(!stored.scheme_admin);
        assert!(stored.scheme_editor);
        assert!(stored.scheme_viewer);

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        assert_eq!(member_history.len(), initial_member_history);
    }

    // "should return empty list if no results are found"
    {
        let member_history = test
            .get_board_member_history(board_id, "nonexistent-user", None)
            .await
            .expect("the history is read");
        assert!(member_history.is_empty(), "got {member_history:?}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_member_for_board() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;
    let board_id = TEST_BOARD_ID;

    // "should return an error not found for nonexisting membership"
    {
        let error = test
            .get_member_for_board(board_id, user_id)
            .await
            .expect_err("there is no such membership");
        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
        assert_eq!(
            error.to_string(),
            "{board member BoardID=board-id UserID=user-id} not found"
        );
    }

    // "should return the membership if exists"
    {
        let member = BoardMember {
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member)
            .await
            .expect("the member is saved");

        let stored = test
            .get_member_for_board(board_id, user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id);
        assert_eq!(board_id, stored.board_id);
        assert!(stored.scheme_admin);
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_members_for_board() {
    let test = common::setup().await;

    // "should return empty list if there are no members on a board"
    {
        let members = test
            .get_members_for_board(TEST_BOARD_ID)
            .await
            .expect("the query succeeds");
        assert!(members.is_empty(), "got {members:?}");
    }

    // "should return the members of the board"
    {
        let board_id_1 = "board-id-1";
        let board_id_2 = "board-id-2";

        let user_id_1 = "user-id-11";
        let user_id_2 = "user-id-12";
        let user_id_3 = "user-id-13";

        let member1 = BoardMember {
            board_id: board_id_1.to_owned(),
            user_id: user_id_1.to_owned(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member1).await.expect("member 1 is saved");

        let member2 = BoardMember {
            board_id: board_id_1.to_owned(),
            user_id: user_id_2.to_owned(),
            scheme_editor: true,
            ..BoardMember::default()
        };
        test.save_member(&member2).await.expect("member 2 is saved");

        let member3 = BoardMember {
            board_id: board_id_2.to_owned(),
            user_id: user_id_3.to_owned(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member3).await.expect("member 3 is saved");

        let board1_members = test
            .get_members_for_board(board_id_1)
            .await
            .expect("the query succeeds");
        assert_eq!(board1_members.len(), 2, "got {board1_members:?}");
        assert_eq!(sorted_user_ids(&board1_members), [user_id_1, user_id_2]);

        let board2_members = test
            .get_members_for_board(board_id_2)
            .await
            .expect("the query succeeds");
        assert_eq!(board2_members.len(), 1, "got {board2_members:?}");
        assert_eq!(sorted_user_ids(&board2_members), [user_id_3]);
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_members_for_user() {
    let test = common::setup().await;

    // "should return empty list if there are no memberships for a user"
    {
        let members = test
            .get_members_for_user(TEST_USER_ID)
            .await
            .expect("the query succeeds");
        assert!(members.is_empty(), "got {members:?}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_member() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;
    let board_id = TEST_BOARD_ID;

    // "should return nil if deleting a nonexistent member"
    {
        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        let initial_member_history = member_history.len();

        test.delete_member(board_id, user_id)
            .await
            .expect("deleting a member that does not exist succeeds");

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        assert_eq!(member_history.len(), initial_member_history);
    }

    // "should correctly delete a member"
    {
        let member = BoardMember {
            user_id: user_id.to_owned(),
            board_id: board_id.to_owned(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member)
            .await
            .expect("the member is saved");

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        let initial_member_history = member_history.len();

        test.delete_member(board_id, user_id)
            .await
            .expect("the member is deleted");

        let error = test
            .get_member_for_board(board_id, user_id)
            .await
            .expect_err("the membership is gone");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );

        let member_history = test
            .get_board_member_history(board_id, user_id, None)
            .await
            .expect("the history is read");
        assert_eq!(member_history.len(), initial_member_history + 1);
    }

    test.teardown().await;
}

/// A case of the original's `testSearchBoardsForUser` table; each is a subtest there.
/// The original's cases also name a team, which the search does not take.
struct SearchCase {
    name: &'static str,
    term: &'static str,
    search_field: BoardSearchField,
    public_boards: PublicBoards,
    expected_board_ids: &'static [&'static str],
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn search_boards_for_user() {
    let test = common::setup().await;
    let team_id_1 = "team-id-1";
    let team_id_2 = "team-id-2";
    let user_id = "user-id-1";

    // "should return empty if user is not a member of any board and there are no public
    // boards on the team"
    {
        let boards = test
            .search_boards_for_user("", BoardSearchField::Title, user_id, PublicBoards::Include)
            .await
            .expect("the search succeeds");
        assert!(boards.is_empty(), "got {boards:?}");
    }

    let mut board1 = Board {
        title: "Public Board with admin".to_owned(),
        properties: object(json!({ "foo": "bar1" })),
        ..new_board("board-id-1", team_id_1, BoardType::Open)
    };
    test.insert_board_with_admin(&mut board1, user_id)
        .await
        .expect("board 1 is inserted");

    let mut board2 = Board {
        title: "Public Board".to_owned(),
        properties: object(json!({ "foo": "bar2" })),
        ..new_board("board-id-2", team_id_1, BoardType::Open)
    };
    test.insert_board(&mut board2, user_id)
        .await
        .expect("board 2 is inserted");

    let mut board3 = Board {
        title: "Private Board with admin".to_owned(),
        ..new_board("board-id-3", team_id_1, BoardType::Private)
    };
    test.insert_board_with_admin(&mut board3, user_id)
        .await
        .expect("board 3 is inserted");

    let mut board4 = Board {
        title: "Private Board".to_owned(),
        ..new_board("board-id-4", team_id_1, BoardType::Private)
    };
    test.insert_board(&mut board4, user_id)
        .await
        .expect("board 4 is inserted");

    let mut board5 = Board {
        title: "Public Board with admin in team 2".to_owned(),
        ..new_board("board-id-5", team_id_2, BoardType::Open)
    };
    test.insert_board_with_admin(&mut board5, user_id)
        .await
        .expect("board 5 is inserted");

    let cases = [
        SearchCase {
            name: "should find all private boards that the user is a member of and public \
                   boards with an empty term",
            term: "",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_board_ids: &["board-id-1", "board-id-2", "board-id-3", "board-id-5"],
        },
        SearchCase {
            name: "should find all with term board",
            term: "board",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_board_ids: &["board-id-1", "board-id-2", "board-id-3", "board-id-5"],
        },
        SearchCase {
            name: "should find all with term board where the user is member of",
            term: "board",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Exclude,
            expected_board_ids: &["board-id-1", "board-id-3", "board-id-5"],
        },
        SearchCase {
            name: "should find only public as per the term, wether user is a member or not",
            term: "public",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_board_ids: &["board-id-1", "board-id-2", "board-id-5"],
        },
        SearchCase {
            name: "should find only private as per the term, wether user is a member or not",
            term: "priv",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_board_ids: &["board-id-3"],
        },
        SearchCase {
            name: "should find no board in team 2 with a non matching term",
            term: "non-matching-term",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_board_ids: &[],
        },
        SearchCase {
            name: "should find all boards with a named property",
            term: "foo",
            search_field: BoardSearchField::PropertyName,
            public_boards: PublicBoards::Include,
            expected_board_ids: &["board-id-1", "board-id-2"],
        },
        SearchCase {
            name: "should find no boards with a non-existing named property",
            term: "bogus",
            search_field: BoardSearchField::PropertyName,
            public_boards: PublicBoards::Include,
            expected_board_ids: &[],
        },
    ];

    for case in cases {
        let found = test
            .search_boards_for_user(case.term, case.search_field, user_id, case.public_boards)
            .await
            .expect(case.name);
        assert_eq!(
            sorted_board_ids(&found),
            case.expected_board_ids,
            "{}",
            case.name
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn search_boards_for_user_in_team() {
    let test = common::setup().await;

    // "should return empty list if there are no resutls"
    {
        let boards = test
            .search_boards_for_user_in_team("nonexistent-team-id", "", TEST_USER_ID)
            .await
            .expect("the search succeeds");
        assert!(boards.is_empty(), "got {boards:?}");
    }

    test.teardown().await;
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn get_board_history() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "testGetBoardHistory: create board"
    {
        let original_title = "Board: original title";
        let board_id = new_id(IdKind::Board);
        let mut board = Board {
            title: original_title.to_owned(),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");
        let r_board1 = board;

        let ascending = HistoryQuery::default();

        let boards = test
            .get_board_history(&board_id, &ascending)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 1, "got {boards:?}");

        let user_id_2 = "user-id-2";
        let new_title = "Board: A new title";
        let new_description = "A new description";
        let patch = BoardPatch {
            title: Some(new_title.to_owned()),
            description: Some(new_description.to_owned()),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, user_id_2)
            .await
            .expect("the patch is applied");

        // Updated history
        let boards = test
            .get_board_history(&board_id, &ascending)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 2, "got {boards:?}");
        assert_eq!(boards[0].title, original_title);
        assert_eq!(boards[1].title, new_title);
        assert_eq!(boards[1].description, new_description);

        // Check history against latest board
        let r_board2 = test.get_board(&board_id).await.expect("the board exists");
        assert_eq!(r_board2.title, new_title);
        assert_eq!(r_board2.title, boards[1].title);
        assert_ne!(r_board2.update_at, 0);
        assert_eq!(r_board1.update_at, boards[0].update_at);
        assert_eq!(r_board2.update_at, patched_board.update_at);
        assert_eq!(r_board2.update_at, boards[1].update_at);
        assert_eq!(r_board1, boards[0]);
        assert_eq!(r_board2, boards[1]);

        let new_title_2 = "Board: A new title 2";
        let patch2 = BoardPatch {
            title: Some(new_title_2.to_owned()),
            ..BoardPatch::default()
        };
        let patch_board2 = test
            .patch_board(&board_id, &patch2, user_id_2)
            .await
            .expect("the patch is applied");

        // Updated history
        let newest = HistoryQuery {
            limit: Some(1),
            order: HistoryOrder::Descending,
            ..HistoryQuery::default()
        };
        let boards = test
            .get_board_history(&board_id, &newest)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 1, "got {boards:?}");
        assert_eq!(boards[0].title, new_title_2);
        assert_eq!(boards[0], patch_board2);

        // Delete board. The assertions below need the deletion to be stamped later than
        // the last patch.
        tokio::time::sleep(Duration::from_millis(10)).await;
        test.delete_board(&board_id, user_id)
            .await
            .expect("the board is deleted");

        // Updated history after delete
        let descending = HistoryQuery {
            order: HistoryOrder::Descending,
            ..HistoryQuery::default()
        };
        let boards = test
            .get_board_history(&board_id, &descending)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 4, "got {boards:?}");
        assert_ne!(boards[0].update_at, 0);
        assert!(
            boards[0].update_at > patch_board2.update_at,
            "{} > {}",
            boards[0].update_at,
            patch_board2.update_at
        );
        assert_ne!(boards[0].delete_at, 0);
        assert!(
            boards[0].delete_at > patch_board2.update_at,
            "{} > {}",
            boards[0].delete_at,
            patch_board2.update_at
        );
    }

    // "testGetBoardHistory: nonexisting board"
    {
        let boards = test
            .get_board_history("nonexistent-id", &HistoryQuery::default())
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 0, "got {boards:?}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_board_count() {
    let test = common::setup().await;
    let user_id = TEST_USER_ID;

    // "test GetBoardCount"
    {
        let original_count = test
            .get_board_count()
            .await
            .expect("the boards are counted");

        let board_id = new_id(IdKind::Board);
        let mut board = Board {
            title: "Board: original title".to_owned(),
            ..new_board(&board_id, TEST_TEAM_ID, BoardType::Open)
        };
        test.insert_board(&mut board, user_id)
            .await
            .expect("the board is inserted");

        let new_count = test
            .get_board_count()
            .await
            .expect("the boards are counted");
        assert_eq!(original_count + 1, new_count);
    }

    test.teardown().await;
}

// Characterization tests: behavior of the original that its test suite does not cover.

/// `server/services/store/sqlstore/board.go:299-303`.
#[tokio::test]
async fn insert_board_stamps_templates_of_the_global_team_with_a_tracking_id() {
    let test = common::setup().await;

    let mut template = Board {
        title: "hello".to_owned(),
        is_template: true,
        ..new_board("template-global", GLOBAL_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut template, TEST_USER_ID)
        .await
        .expect("the template is inserted");
    // The MD5 of the title, in hexadecimal.
    let tracking_id = json!("5d41402abc4b2a76b9719d911017c592");
    assert_eq!(template.properties["trackingTemplateId"], tracking_id);
    let stored = test
        .get_board("template-global")
        .await
        .expect("the template exists");
    assert_eq!(stored.properties["trackingTemplateId"], tracking_id);

    let mut other_team = Board {
        title: "hello".to_owned(),
        is_template: true,
        ..new_board("template-team", TEST_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut other_team, TEST_USER_ID)
        .await
        .expect("the template is inserted");
    assert!(
        !other_team.properties.contains_key("trackingTemplateId"),
        "only templates of the global team are stamped"
    );

    let mut board = Board {
        title: "hello".to_owned(),
        ..new_board("board-global", GLOBAL_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut board, TEST_USER_ID)
        .await
        .expect("the board is inserted");
    assert!(
        !board.properties.contains_key("trackingTemplateId"),
        "only templates are stamped"
    );

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:360-399`: what an update leaves alone, and
/// what it nevertheless writes to the history.
#[tokio::test]
async fn insert_board_update_keeps_team_creator_and_creation_time() {
    let test = common::setup().await;

    let mut board = new_board("board-1", TEST_TEAM_ID, BoardType::Open);
    test.insert_board(&mut board, "creator")
        .await
        .expect("the board is inserted");

    let mut update = Board {
        title: "Updated".to_owned(),
        ..new_board("board-1", "another-team", BoardType::Private)
    };
    test.insert_board(&mut update, "editor")
        .await
        .expect("the board is updated");
    // The caller's board is only stamped with the modifier and the modification time.
    assert_eq!(update.created_by, "");
    assert_eq!(update.create_at, 0);
    assert_eq!(update.modified_by, "editor");

    let stored = test.get_board("board-1").await.expect("the board exists");
    assert_eq!(stored.team_id, TEST_TEAM_ID);
    assert_eq!(stored.created_by, "creator");
    assert_eq!(stored.create_at, board.create_at);
    assert_eq!(stored.modified_by, "editor");
    assert_eq!(stored.title, "Updated");
    assert_eq!(stored.board_type, BoardType::Private);

    // The history row is the caller's board, not the stored one.
    let history = test
        .get_board_history("board-1", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 2, "got {history:?}");
    assert_eq!(history[1], update);

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:414-479` and `:789-874`: a board takes its
/// blocks with it, both ways, and comes back without its channel.
#[tokio::test]
async fn delete_and_undelete_board_take_the_blocks_along() {
    let test = common::setup().await;

    let mut board = Board {
        channel_id: "channel".to_owned(),
        ..new_board("board-1", TEST_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut board, TEST_USER_ID)
        .await
        .expect("the board is inserted");
    let mut blocks = vec![
        Block {
            id: "card-1".to_owned(),
            board_id: "board-1".to_owned(),
            parent_id: "board-1".to_owned(),
            ..Block::default()
        },
        Block {
            id: "text-1".to_owned(),
            board_id: "board-1".to_owned(),
            parent_id: "card-1".to_owned(),
            ..Block::default()
        },
    ];
    common::insert_blocks(&test, &mut blocks, TEST_USER_ID).await;
    let member = BoardMember {
        board_id: "board-1".to_owned(),
        user_id: TEST_USER_ID.to_owned(),
        scheme_admin: true,
        ..BoardMember::default()
    };
    test.save_member(&member)
        .await
        .expect("the member is saved");

    test.delete_board("board-1", "deleter")
        .await
        .expect("the board is deleted");
    let left = test
        .get_blocks(&BlockFilter::board("board-1"))
        .await
        .expect("the blocks are read");
    assert!(left.is_empty(), "got {left:?}");
    let members = test
        .get_members_for_board("board-1")
        .await
        .expect("the members are read");
    assert_eq!(members.len(), 1, "memberships outlive the board");
    assert_eq!(
        members[0].minimum_role, "",
        "a membership of a deleted board reads with an empty minimum role"
    );
    let newest = HistoryQuery {
        limit: Some(1),
        order: HistoryOrder::Descending,
        ..HistoryQuery::default()
    };
    let history = test
        .get_board_history("board-1", &newest)
        .await
        .expect("the history is read");
    assert_eq!(history[0].modified_by, "deleter");
    assert_eq!(history[0].delete_at, history[0].update_at);
    assert_eq!(history[0].channel_id, "channel");

    test.undelete_board("board-1", "restorer")
        .await
        .expect("the board is restored");
    let restored = test.get_board("board-1").await.expect("the board is back");
    assert_eq!(restored.modified_by, "restorer");
    assert_eq!(restored.created_by, TEST_USER_ID);
    assert_eq!(restored.create_at, board.create_at);
    assert_eq!(restored.delete_at, 0);
    assert_eq!(restored.channel_id, "", "the channel is not restored");
    let back = test
        .get_blocks(&BlockFilter::board("board-1"))
        .await
        .expect("the blocks are read");
    assert_eq!(common::sorted_ids(&back), ["card-1", "text-1"]);

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:502-550`.
#[tokio::test]
async fn save_member_stores_no_roles_and_reads_the_minimum_role_of_the_board() {
    let test = common::setup().await;

    let mut board = Board {
        minimum_role: BoardRole::Editor,
        ..new_board("board-1", TEST_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut board, TEST_USER_ID)
        .await
        .expect("the board is inserted");

    let member = BoardMember {
        board_id: "board-1".to_owned(),
        user_id: "member".to_owned(),
        roles: "some-role".to_owned(),
        scheme_commenter: true,
        ..BoardMember::default()
    };
    test.save_member(&member)
        .await
        .expect("the member is saved");

    let stored = test
        .get_member_for_board("board-1", "member")
        .await
        .expect("the member exists");
    assert_eq!(
        stored,
        BoardMember {
            board_id: "board-1".to_owned(),
            user_id: "member".to_owned(),
            roles: String::new(),
            minimum_role: "editor".to_owned(),
            scheme_commenter: true,
            ..BoardMember::default()
        }
    );
    let of_user = test
        .get_members_for_user("member")
        .await
        .expect("the memberships are read");
    assert_eq!(of_user, vec![stored]);

    let history = test
        .get_board_member_history("board-1", "member", None)
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 1, "got {history:?}");
    assert_eq!(history[0].action, "created");
    assert_eq!(history[0].board_id, "board-1");
    assert_eq!(history[0].user_id, "member");

    test.delete_member("board-1", "member")
        .await
        .expect("the member is deleted");
    let history = test
        .get_board_member_history("board-1", "member", None)
        .await
        .expect("the history is read");
    let actions: Vec<&str> = history.iter().map(|entry| entry.action.as_str()).collect();
    assert_eq!(actions, ["deleted", "created"], "newest first");
    assert!(
        history[0].insert_at >= history[1].insert_at && history[1].insert_at > 0,
        "got {history:?}"
    );
    let limited = test
        .get_board_member_history("board-1", "member", Some(1))
        .await
        .expect("the history is read");
    assert_eq!(limited.len(), 1, "got {limited:?}");
    assert_eq!(limited[0].action, "deleted");

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:713-753`.
#[tokio::test]
async fn search_boards_for_user_in_team_finds_open_boards_and_own_private_boards() {
    let test = common::setup().await;
    let user_id = "user-id-1";

    let mut open = Board {
        title: "Open Roadmap".to_owned(),
        ..new_board("board-open", "team-1", BoardType::Open)
    };
    test.insert_board(&mut open, "other-user")
        .await
        .expect("the board is inserted");
    let mut own_private = Board {
        title: "Private Roadmap".to_owned(),
        ..new_board("board-private-own", "team-1", BoardType::Private)
    };
    test.insert_board_with_admin(&mut own_private, user_id)
        .await
        .expect("the board is inserted");
    let mut foreign_private = Board {
        title: "Secret Roadmap".to_owned(),
        ..new_board("board-private-foreign", "team-1", BoardType::Private)
    };
    test.insert_board_with_admin(&mut foreign_private, "other-user")
        .await
        .expect("the board is inserted");
    let mut other_team = Board {
        title: "Open Roadmap elsewhere".to_owned(),
        ..new_board("board-other-team", "team-2", BoardType::Open)
    };
    test.insert_board(&mut other_team, user_id)
        .await
        .expect("the board is inserted");
    let mut template = Board {
        title: "Roadmap template".to_owned(),
        is_template: true,
        ..new_board("board-template", "team-1", BoardType::Open)
    };
    test.insert_board(&mut template, user_id)
        .await
        .expect("the board is inserted");

    let search = async |term: &str| {
        let boards = test
            .search_boards_for_user_in_team("team-1", term, user_id)
            .await
            .expect("the search succeeds");
        sorted_board_ids(&boards)
    };
    assert_eq!(search("").await, ["board-open", "board-private-own"]);
    assert_eq!(search("ROADMAP").await, ["board-open", "board-private-own"]);
    assert_eq!(
        search("roadmap open").await,
        ["board-open"],
        "every word must match"
    );
    assert_eq!(search("  private  ").await, ["board-private-own"]);
    assert_eq!(search("secret").await, Vec::<String>::new());

    test.teardown().await;
}

/// `server/services/store/sqlstore/templates.go:57-89`.
#[tokio::test]
async fn get_template_boards_returns_open_templates_and_templates_the_user_is_member_of() {
    let test = common::setup().await;
    let user_id = "user-id-1";

    let mut open_template = Board {
        is_template: true,
        ..new_board("template-open", TEST_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut open_template, SYSTEM_USER_ID)
        .await
        .expect("the template is inserted");
    let mut own_private_template = Board {
        is_template: true,
        ..new_board("template-private-own", TEST_TEAM_ID, BoardType::Private)
    };
    test.insert_board_with_admin(&mut own_private_template, user_id)
        .await
        .expect("the template is inserted");
    let mut foreign_private_template = Board {
        is_template: true,
        ..new_board("template-private-foreign", TEST_TEAM_ID, BoardType::Private)
    };
    test.insert_board_with_admin(&mut foreign_private_template, "other-user")
        .await
        .expect("the template is inserted");
    let mut other_team_template = Board {
        is_template: true,
        ..new_board("template-other-team", "another-team", BoardType::Open)
    };
    test.insert_board(&mut other_team_template, user_id)
        .await
        .expect("the template is inserted");
    let mut board = new_board("not-a-template", TEST_TEAM_ID, BoardType::Open);
    test.insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is inserted");

    let templates = test
        .get_template_boards(TEST_TEAM_ID, user_id)
        .await
        .expect("the templates are read");
    assert_eq!(
        sorted_by_id(templates),
        vec![open_template.clone(), own_private_template]
    );

    // The server lists the built-in templates with an empty user.
    let templates = test
        .get_template_boards(TEST_TEAM_ID, "")
        .await
        .expect("the templates are read");
    assert_eq!(templates, vec![open_template]);

    test.teardown().await;
}

/// `server/services/store/sqlstore/templates.go:17-55`.
#[tokio::test]
async fn remove_default_templates_removes_system_templates_and_their_blocks_without_history() {
    let test = common::setup().await;

    let mut system_template = Board {
        is_template: true,
        ..new_board("template-system", GLOBAL_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut system_template, SYSTEM_USER_ID)
        .await
        .expect("the template is inserted");
    let mut user_template = Board {
        is_template: true,
        ..new_board("template-user", GLOBAL_TEAM_ID, BoardType::Open)
    };
    test.insert_board(&mut user_template, TEST_USER_ID)
        .await
        .expect("the template is inserted");
    let mut system_board = new_board("board-system", GLOBAL_TEAM_ID, BoardType::Open);
    test.insert_board(&mut system_board, SYSTEM_USER_ID)
        .await
        .expect("the board is inserted");

    let new_block_of = |board_id: &str| Block {
        id: format!("block-of-{board_id}"),
        board_id: board_id.to_owned(),
        parent_id: board_id.to_owned(),
        ..Block::default()
    };
    let mut blocks = vec![
        new_block_of("template-system"),
        new_block_of("template-user"),
        new_block_of("board-system"),
    ];
    common::insert_blocks(&test, &mut blocks, SYSTEM_USER_ID).await;

    test.remove_default_templates(&[system_template, user_template, system_board])
        .await
        .expect("the default templates are removed");

    test.get_board("template-system")
        .await
        .expect_err("the system template is gone");
    let blocks_of = async |board_id: &str| {
        test.get_blocks(&BlockFilter::board(board_id))
            .await
            .expect("the blocks are read")
            .len()
    };
    assert_eq!(blocks_of("template-system").await, 0);

    test.get_board("template-user")
        .await
        .expect("a template created by a user stays");
    assert_eq!(blocks_of("template-user").await, 1);

    // A board created by the system user that is not a template stays, but its blocks
    // go: only the board deletion checks the template flag.
    test.get_board("board-system")
        .await
        .expect("a board that is not a template stays");
    assert_eq!(blocks_of("board-system").await, 0);

    // Nothing is recorded: the history still ends with the insertion.
    let history = test
        .get_board_history("template-system", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 1, "got {history:?}");
    assert_eq!(history[0].delete_at, 0);
    let block_history = test
        .get_block_history("block-of-template-system", &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(block_history.len(), 1, "got {block_history:?}");

    test.teardown().await;
}
