//! Port of `server/services/store/storetests/boards.go`.
//!
//! One test per test function of the original; its subtests are the commented sections,
//! in the original order, on one store. The original sleeps between writes of one board
//! so that two history rows do not collide; the port does not need that and sleeps only
//! where an assertion compares timestamps.
//!
//! The original names users, teams and boards by whatever text is at hand. Here each of
//! them is a stored row with an identifier of its own, and text that is no identifier
//! names nothing. The original's tests of the membership history and of the board count
//! went with those functions.
//!
//! The tests after [`get_board_history`] have no counterpart in the original, which does
//! not test templates, the template tracking identifier or `CanSeeUser`.

use std::time::Duration;

use fvoci_model::{
    Block, Board, BoardMember, BoardPatch, BoardRole, BoardSearchField, BoardType, JsonMap, new_id,
};
use fvoci_store::{BlockFilter, HistoryOrder, HistoryQuery, PublicBoards, Store, StoreError};
use serde_json::{Value, json};

use crate::common;

/// Text that is not an identifier, and so names no row.
const NOT_AN_ID: &str = "nonexistent-id";

/// A board with the three members nearly every test sets.
fn new_board(id: &str, team_id: &str, board_type: BoardType) -> Board {
    Board {
        id: id.to_owned(),
        team_id: team_id.to_owned(),
        board_type,
        ..Board::default()
    }
}

/// Stores an open board of a team, without members, and returns its identifier.
async fn stored_board(store: &Store, team_id: &str, user_id: &str) -> String {
    let mut board = new_board(&new_id(), team_id, BoardType::Open);
    store
        .insert_board(&mut board, user_id)
        .await
        .expect("the board is inserted");
    board.id
}

fn object(value: Value) -> JsonMap {
    serde_json::from_value(value).expect("the value is a JSON object")
}

fn sorted<const N: usize>(ids: [&str; N]) -> Vec<String> {
    let mut ids: Vec<String> = ids.into_iter().map(str::to_owned).collect();
    ids.sort();
    ids
}

fn sorted_board_ids<'a>(boards: impl IntoIterator<Item = &'a Board>) -> Vec<String> {
    let mut ids: Vec<String> = boards.into_iter().map(|board| board.id.clone()).collect();
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
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "existing board"
    {
        let mut board = new_board(&new_id(), &team_id, BoardType::Open);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        let r_board = test.get_board(&board.id).await.expect("the board exists");
        assert_eq!(board.id, r_board.id, "the identifier");
        assert_eq!(board.team_id, r_board.team_id, "the team");
        assert_eq!(user_id, r_board.created_by, "the creator");
        assert_eq!(user_id, r_board.modified_by, "the modifier");
        assert_eq!(board.board_type, r_board.board_type, "the type");
        assert_ne!(r_board.create_at, 0, "the creation time is set");
        assert_ne!(r_board.update_at, 0, "the modification time is set");
    }

    // "nonexisting board": an identifier no board has, and text that is no identifier.
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        let error = test
            .get_board(&board_id)
            .await
            .expect_err("there is no such board");
        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
        assert_eq!(error.to_string(), "{boards} not found", "board {board_id}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_boards_for_user_and_team() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let other_user_id = common::user(&test, "other-user").await;
    let team_id_1 = common::team(&test).await;
    let team_id_2 = common::team(&test).await;

    // "should return empty list if no results are found"
    {
        let boards = test
            .get_boards_for_user_and_team(&user_id, &team_id_1, PublicBoards::Include)
            .await
            .expect("the query succeeds");
        assert!(boards.is_empty(), "got {boards:?}");
    }

    // "should return only the boards of the team that the user is a member of"
    {
        // team 1 boards
        let mut board1 = new_board(&new_id(), &team_id_1, BoardType::Open);
        test.insert_board_with_admin(&mut board1, &user_id)
            .await
            .expect("board 1 is inserted");

        let mut board2 = new_board(&new_id(), &team_id_1, BoardType::Private);
        test.insert_board_with_admin(&mut board2, &user_id)
            .await
            .expect("board 2 is inserted");

        let mut board3 = new_board(&new_id(), &team_id_1, BoardType::Open);
        test.insert_board(&mut board3, &other_user_id)
            .await
            .expect("board 3 is inserted");

        let mut board4 = new_board(&new_id(), &team_id_1, BoardType::Private);
        test.insert_board(&mut board4, &other_user_id)
            .await
            .expect("board 4 is inserted");

        // team 2 boards
        let mut board5 = new_board(&new_id(), &team_id_2, BoardType::Open);
        test.insert_board_with_admin(&mut board5, &user_id)
            .await
            .expect("board 5 is inserted");

        let mut board6 = new_board(&new_id(), &team_id_1, BoardType::Private);
        test.insert_board(&mut board6, &other_user_id)
            .await
            .expect("board 6 is inserted");

        // "should only find the two boards that the user is a member of for team 1 plus
        // the one open board"
        {
            let found = test
                .get_boards_for_user_and_team(&user_id, &team_id_1, PublicBoards::Include)
                .await
                .expect("the query succeeds");
            assert_eq!(
                sorted_by_id(found),
                sorted_by_id(vec![board1.clone(), board2.clone(), board3]),
                "the user's boards of team 1 and its open board"
            );
        }

        // "should only find the two boards that the user is a member of for team 1"
        {
            let found = test
                .get_boards_for_user_and_team(&user_id, &team_id_1, PublicBoards::Exclude)
                .await
                .expect("the query succeeds");
            assert_eq!(
                sorted_by_id(found),
                sorted_by_id(vec![board1, board2]),
                "the user's boards of team 1"
            );
        }

        // "should only find the board that the user is a member of for team 2"
        {
            let found = test
                .get_boards_for_user_and_team(&user_id, &team_id_2, PublicBoards::Include)
                .await
                .expect("the query succeeds");
            assert_eq!(found.len(), 1, "got {found:?}");
            assert_eq!(board5.id, found[0].id, "the user's board of team 2");
        }
    }

    test.teardown().await;
}

/// A case of the original's `testGetBoardsInTeamByIds` table; each is a subtest there.
struct BoardsByIdsCase {
    name: &'static str,
    board_ids: [String; 2],
    expected_error: bool,
    expected_len: usize,
}

#[tokio::test]
async fn get_boards_in_team_by_ids() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "should return err not all found if one or more of the ids are not found"
    {
        let board_id_1 = new_id();
        let board_id_2 = new_id();
        for board_id in [&board_id_1, &board_id_2] {
            let mut board = new_board(board_id, &team_id, BoardType::Open);
            test.insert_board_with_admin(&mut board, &user_id)
                .await
                .expect("the board is inserted");
        }

        let cases = [
            BoardsByIdsCase {
                name: "if none of the IDs are found",
                // An identifier no board has, and text that is no identifier.
                board_ids: [new_id(), NOT_AN_ID.to_owned()],
                expected_error: true,
                expected_len: 0,
            },
            BoardsByIdsCase {
                name: "if not all of the IDs are found",
                board_ids: [new_id(), board_id_1.clone()],
                expected_error: true,
                expected_len: 1,
            },
            BoardsByIdsCase {
                name: "if all of the IDs are found",
                board_ids: [board_id_1, board_id_2],
                expected_error: false,
                expected_len: 2,
            },
        ];

        for case in cases {
            let board_ids = case.board_ids;
            let result = test.get_boards_in_team_by_ids(&board_ids, &team_id).await;
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
                    .get_boards_for_user_and_team(&user_id, &team_id, PublicBoards::Include)
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
    let user_id = common::user(&test, "user").await;
    let user_id_2 = common::user(&test, "user2").await;
    let team_id = common::team(&test).await;
    let public_board_id = new_id();

    // "valid public board"
    {
        let mut board = new_board(&public_board_id, &team_id, BoardType::Open);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.id, public_board_id, "the identifier is kept");
        assert_eq!(board.board_type, BoardType::Open, "the type is kept");
        assert_ne!(board.create_at, 0, "the creation time is set");
        assert_ne!(board.update_at, 0, "the modification time is set");
        assert_eq!(board.delete_at, 0, "the board is not deleted");
        assert_eq!(user_id, board.created_by, "the creator");
        assert_eq!(board.created_by, board.modified_by, "the modifier");
    }

    // "valid private board"
    {
        let private_board_id = new_id();
        let mut board = new_board(&private_board_id, &team_id, BoardType::Private);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.id, private_board_id, "the identifier is kept");
        assert_eq!(board.board_type, BoardType::Private, "the type is kept");
        assert_ne!(board.create_at, 0, "the creation time is set");
        assert_ne!(board.update_at, 0, "the modification time is set");
        assert_eq!(board.delete_at, 0, "the board is not deleted");
        assert_eq!(user_id, board.created_by, "the creator");
        assert_eq!(board.created_by, board.modified_by, "the modifier");
    }

    // "invalid properties field board"
    //
    // Not expressible: the original stores a function value in the board's properties to
    // make their serialization fail. The properties are a `JsonMap` here, which can only
    // hold values that serialize.

    // "update board"
    {
        let mut board = Board {
            id: public_board_id,
            team_id: team_id.clone(),
            title: "New title".to_owned(),
            ..Board::default()
        };
        test.insert_board(&mut board, &user_id_2)
            .await
            .expect("the board is updated");
        assert_eq!("New title", board.title, "the title is the new one");
        assert_eq!(user_id_2, board.modified_by, "the modifier");
    }

    // "test update board type"
    {
        let type_board_id = new_id();

        // The original stores this board without a team. Here a board is some team's:
        // one without a team is refused, and nothing is stored.
        let mut board = Board {
            id: type_board_id.clone(),
            title: "Public board".to_owned(),
            board_type: BoardType::Open,
            ..Board::default()
        };
        let error = test
            .insert_board(&mut board, &user_id)
            .await
            .expect_err("a board needs a team");
        assert!(
            matches!(&error, StoreError::InvalidId(invalid) if invalid.is_empty()),
            "got {error:?}"
        );
        let error = test
            .get_board(&type_board_id)
            .await
            .expect_err("nothing was stored");
        assert!(error.is_not_found(), "got {error:?}");

        board.team_id.clone_from(&team_id);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(BoardType::Open, board.board_type, "the board is open");

        let mut board_update = Board {
            id: type_board_id,
            team_id,
            board_type: BoardType::Private,
            ..Board::default()
        };
        test.insert_board(&mut board_update, &user_id)
            .await
            .expect("the board is updated");
        assert_eq!(
            BoardType::Private,
            board_update.board_type,
            "the board is private now"
        );
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
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "should return error if the board doesn't exist"
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        let patch = BoardPatch {
            title: Some("A new title".to_owned()),
            ..BoardPatch::default()
        };
        let error = test
            .patch_board(&board_id, &patch, &user_id)
            .await
            .expect_err("there is no such board");
        assert!(error.is_not_found(), "board {board_id}: got {error:?}");
    }

    // "should correctly apply a simple patch"
    {
        let board_id = new_id();
        let user_id_2 = common::user(&test, "user2").await;

        let mut board = Board {
            title: "A simple title".to_owned(),
            description: "A simple description".to_owned(),
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(user_id, board.created_by, "the creator");

        let new_title = "A new title";
        let new_description = "A new description";
        let patch = BoardPatch {
            title: Some(new_title.to_owned()),
            description: Some(new_description.to_owned()),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id_2)
            .await
            .expect("the patch is applied");
        assert_eq!(new_title, patched_board.title, "the title is patched");
        assert_eq!(
            new_description, patched_board.description,
            "the description is patched"
        );
        assert_eq!(user_id, patched_board.created_by, "the creator stays");
        assert_eq!(user_id_2, patched_board.modified_by, "the modifier");
    }

    // "should correctly update the board properties"
    {
        let board_id = new_id();

        let mut board = Board {
            properties: object(json!({ "one": "1", "two": "2" })),
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.properties["one"], json!("1"), "property one");
        assert_eq!(board.properties["two"], json!("2"), "property two");

        let patch = BoardPatch {
            updated_properties: object(json!({ "three": "3" })),
            deleted_properties: vec!["one".to_owned()],
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id)
            .await
            .expect("the patch is applied");
        assert!(
            !patched_board.properties.contains_key("one"),
            "got {:?}",
            patched_board.properties
        );
        assert_eq!(patched_board.properties["two"], json!("2"), "property two");
        assert_eq!(
            patched_board.properties["three"],
            json!("3"),
            "property three"
        );
    }

    // "should correctly modify the board's type"
    {
        let board_id = new_id();

        let mut board = new_board(&board_id, &team_id, BoardType::Open);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open, "the board is open");

        let patch = BoardPatch {
            board_type: Some(BoardType::Private),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id)
            .await
            .expect("the patch is applied");
        assert_eq!(
            BoardType::Private,
            patched_board.board_type,
            "the board is private now"
        );
    }

    // "a patch that doesn't include any of the properties should not modify them"
    {
        let board_id = new_id();
        let properties = object(json!({ "prop1": "val1" }));
        // The original's test data is `{"prop2": "val2"}`, a definition without an
        // identifier, which is no longer stored.
        let card_properties = vec![object(json!({ "id": "prop2", "name": "val2" }))];

        let mut board = Board {
            properties: properties.clone(),
            card_properties: card_properties.clone(),
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open, "the board is open");
        assert_eq!(properties, board.properties, "the properties");
        assert_eq!(
            card_properties, board.card_properties,
            "the card properties"
        );

        let patch = BoardPatch {
            board_type: Some(BoardType::Private),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id)
            .await
            .expect("the patch is applied");
        assert_eq!(
            BoardType::Private,
            patched_board.board_type,
            "the board is private now"
        );
        assert_eq!(
            properties, patched_board.properties,
            "the properties are unchanged"
        );
        assert_eq!(
            card_properties, patched_board.card_properties,
            "the card properties are unchanged"
        );
    }

    // "a patch that removes a card property and updates another should work correctly"
    {
        let board_id = new_id();
        let prop1 = object(json!({ "id": "prop1", "value": "val1" }));
        let prop2 = object(json!({ "id": "prop2", "value": "val2" }));
        let prop3 = object(json!({ "id": "prop3", "value": "val3" }));
        let card_properties = vec![prop1, prop2, prop3.clone()];

        let mut board = Board {
            card_properties: card_properties.clone(),
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        assert_eq!(board.board_type, BoardType::Open, "the board is open");
        assert_eq!(
            card_properties, board.card_properties,
            "the card properties"
        );

        let new_prop1 = object(json!({ "id": "prop1", "value": "newval1" }));
        let patch = BoardPatch {
            updated_card_properties: vec![new_prop1.clone()],
            deleted_card_properties: vec!["prop2".to_owned()],
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id)
            .await
            .expect("the patch is applied");
        // The original compares without regard to order; so does this.
        let by_property_id = |a: &JsonMap, b: &JsonMap| a["id"].as_str().cmp(&b["id"].as_str());
        let mut expected = vec![new_prop1, prop3];
        expected.sort_by(by_property_id);
        let mut got = patched_board.card_properties;
        got.sort_by(by_property_id);
        assert_eq!(expected, got, "one definition updated, one removed");
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "should return an error if the board doesn't exist"
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        let error = test
            .delete_board(&board_id, &user_id)
            .await
            .expect_err("there is no such board");
        assert!(error.is_not_found(), "board {board_id}: got {error:?}");
    }

    // "should correctly delete the board"
    {
        let board_id = new_id();

        let mut board = new_board(&board_id, &team_id, BoardType::Open);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        test.get_board(&board_id).await.expect("the board exists");

        test.delete_board(&board_id, &user_id)
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
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "existing id"
    {
        let board_id = new_id();

        let mut board = Board {
            title: "Dunder Mifflin Scranton".to_owned(),
            minimum_role: BoardRole::Commenter,
            description: "Bears, beets, Battlestar Gallectica".to_owned(),
            icon: "🐻".to_owned(),
            show_description: true,
            is_template: false,
            properties: object(json!({ "prop_1": "value_1" })),
            // The original's test data has no `id`; see `patch_board`.
            card_properties: vec![object(json!({ "id": "prop_1", "name": "value_1" }))],
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        test.delete_board(&board_id, &user_id)
            .await
            .expect("the board is deleted");

        test.get_board(&board_id)
            .await
            .expect_err("the board is gone");

        test.undelete_board(&board_id, &user_id)
            .await
            .expect("the board is restored");

        let board = test.get_board(&board_id).await.expect("the board is back");

        // verifying the data after un-delete
        assert_eq!("Dunder Mifflin Scranton", board.title, "the title");
        assert_eq!(team_id, board.team_id, "the team");
        assert_eq!(user_id, board.created_by, "the creator");
        assert_eq!(user_id, board.modified_by, "the modifier");
        assert_eq!(BoardRole::Commenter, board.minimum_role, "the minimum role");
        assert_eq!(
            "Bears, beets, Battlestar Gallectica", board.description,
            "the description"
        );
        assert_eq!("🐻", board.icon, "the icon");
        assert!(board.show_description, "the description is shown");
        assert!(!board.is_template, "the board is no template");
        assert_eq!(board.properties["prop_1"], json!("value_1"), "a property");
        assert_eq!(1, board.card_properties.len(), "the card properties");
        assert_eq!(
            board.card_properties[0]["name"],
            json!("value_1"),
            "a card property"
        );
    }

    // "existing id multiple times"
    {
        let board_id = new_id();

        let mut board = new_board(&board_id, &team_id, BoardType::Open);
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");

        test.delete_board(&board_id, &user_id)
            .await
            .expect("the board is deleted");

        test.get_board(&board_id)
            .await
            .expect_err("the board is gone");

        test.undelete_board(&board_id, &user_id)
            .await
            .expect("the board is restored");

        test.get_board(&board_id).await.expect("the board is back");

        test.undelete_board(&board_id, &user_id)
            .await
            .expect("undeleting a board that is not deleted succeeds");

        test.get_board(&board_id)
            .await
            .expect("the board is still there");
    }

    // "from not existing id"
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        test.undelete_board(&board_id, &user_id)
            .await
            .expect("undeleting a board that never existed succeeds");

        test.get_board(&board_id)
            .await
            .expect_err("there is still no such board");
    }

    test.teardown().await;
}

#[tokio::test]
async fn insert_board_with_admin() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "should correctly create a board and the admin membership with the creator"
    {
        let board_id = new_id();

        let mut board = new_board(&board_id, &team_id, BoardType::Open);
        let new_member = test
            .insert_board_with_admin(&mut board, &user_id)
            .await
            .expect("the board and its admin are inserted");
        assert_eq!(user_id, board.created_by, "the creator");
        assert_eq!(user_id, board.modified_by, "the modifier");
        assert_eq!(user_id, new_member.user_id, "the member is the creator");
        assert_eq!(board_id, new_member.board_id, "the member's board");
        assert!(new_member.scheme_admin, "the creator administers the board");
        assert!(new_member.scheme_editor, "the creator may edit the board");

        // The membership that is returned is the one that is stored.
        let stored = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect("the membership exists");
        assert_eq!(stored, new_member, "the stored membership");
    }

    test.teardown().await;
}

#[tokio::test]
async fn save_member() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    // "should correctly create a member"
    {
        let member = BoardMember {
            user_id: user_id.clone(),
            board_id: board_id.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        };

        test.save_member(&member)
            .await
            .expect("the member is saved");
        // The original checks the member it gets back; the port returns nothing, so the
        // same members are checked on the stored membership.
        let stored = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id, "the member");
        assert_eq!(board_id, stored.board_id, "the member's board");

        assert!(stored.scheme_admin, "the member administers the board");
    }

    // "should correctly update a member"
    {
        let member = BoardMember {
            user_id: user_id.clone(),
            board_id: board_id.clone(),
            scheme_editor: true,
            scheme_viewer: true,
            ..BoardMember::default()
        };

        test.save_member(&member)
            .await
            .expect("the member is saved");
        let stored = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id, "the member");
        assert_eq!(board_id, stored.board_id, "the member's board");

        assert!(!stored.scheme_admin, "no longer an administrator");
        assert!(stored.scheme_editor, "an editor now");
        assert!(stored.scheme_viewer, "a viewer now");

        // The original checks here that the membership history did not grow. There is
        // no such history any more; that the membership was updated and not added to
        // shows in there still being one.
        let members = test
            .get_members_for_board(&board_id)
            .await
            .expect("the members are read");
        assert_eq!(members, [stored], "one membership, the updated one");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_member_for_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    // "should return an error not found for nonexisting membership"
    {
        let error = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect_err("there is no such membership");
        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
        assert_eq!(
            error.to_string(),
            format!("{{board member BoardID={board_id} UserID={user_id}}} not found"),
            "the error names the board and the user"
        );
    }

    // "should return the membership if exists"
    {
        let member = BoardMember {
            user_id: user_id.clone(),
            board_id: board_id.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member)
            .await
            .expect("the member is saved");

        let stored = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect("the member exists");
        assert_eq!(user_id, stored.user_id, "the member");
        assert_eq!(board_id, stored.board_id, "the member's board");
        assert!(stored.scheme_admin, "the member administers the board");
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_members_for_board() {
    let test = common::setup().await;
    let user_id_1 = common::user(&test, "user-11").await;
    let user_id_2 = common::user(&test, "user-12").await;
    let user_id_3 = common::user(&test, "user-13").await;
    let team_id = common::team(&test).await;
    let board_id_1 = stored_board(&test, &team_id, &user_id_1).await;
    let board_id_2 = stored_board(&test, &team_id, &user_id_1).await;

    // "should return empty list if there are no members on a board"
    {
        let members = test
            .get_members_for_board(&board_id_1)
            .await
            .expect("the query succeeds");
        assert!(members.is_empty(), "got {members:?}");
    }

    // "should return the members of the board"
    {
        let member1 = BoardMember {
            board_id: board_id_1.clone(),
            user_id: user_id_1.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member1).await.expect("member 1 is saved");

        let member2 = BoardMember {
            board_id: board_id_1.clone(),
            user_id: user_id_2.clone(),
            scheme_editor: true,
            ..BoardMember::default()
        };
        test.save_member(&member2).await.expect("member 2 is saved");

        let member3 = BoardMember {
            board_id: board_id_2.clone(),
            user_id: user_id_3.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member3).await.expect("member 3 is saved");

        let board1_members = test
            .get_members_for_board(&board_id_1)
            .await
            .expect("the query succeeds");
        assert_eq!(board1_members.len(), 2, "got {board1_members:?}");
        assert_eq!(
            sorted_user_ids(&board1_members),
            sorted([&user_id_1, &user_id_2]),
            "the members of board 1"
        );

        let board2_members = test
            .get_members_for_board(&board_id_2)
            .await
            .expect("the query succeeds");
        assert_eq!(board2_members.len(), 1, "got {board2_members:?}");
        assert_eq!(
            sorted_user_ids(&board2_members),
            [user_id_3],
            "the member of board 2"
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn get_members_for_user() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;

    // "should return empty list if there are no memberships for a user"
    {
        let members = test
            .get_members_for_user(&user_id)
            .await
            .expect("the query succeeds");
        assert!(members.is_empty(), "got {members:?}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_member() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    // "should return nil if deleting a nonexistent member"
    {
        test.delete_member(&board_id, &user_id)
            .await
            .expect("deleting a member that does not exist succeeds");
    }

    // "should correctly delete a member"
    {
        let member = BoardMember {
            user_id: user_id.clone(),
            board_id: board_id.clone(),
            scheme_admin: true,
            ..BoardMember::default()
        };
        test.save_member(&member)
            .await
            .expect("the member is saved");

        test.delete_member(&board_id, &user_id)
            .await
            .expect("the member is deleted");

        let error = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect_err("the membership is gone");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error"
        );
    }

    test.teardown().await;
}

/// A case of the original's `testSearchBoardsForUser` table; each is a subtest there.
/// The original's cases also name a team, which the search does not take.
struct SearchCase<'a> {
    name: &'static str,
    term: &'static str,
    search_field: BoardSearchField,
    public_boards: PublicBoards,
    expected_boards: Vec<&'a Board>,
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one test per test function of the original, whose subtests share a store"
)]
async fn search_boards_for_user() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id_1 = common::team(&test).await;
    let team_id_2 = common::team(&test).await;

    // "should return empty if user is not a member of any board and there are no public
    // boards on the team"
    {
        let boards = test
            .search_boards_for_user("", BoardSearchField::Title, &user_id, PublicBoards::Include)
            .await
            .expect("the search succeeds");
        assert!(boards.is_empty(), "got {boards:?}");
    }

    let mut board1 = Board {
        title: "Public Board with admin".to_owned(),
        properties: object(json!({ "foo": "bar1" })),
        ..new_board(&new_id(), &team_id_1, BoardType::Open)
    };
    test.insert_board_with_admin(&mut board1, &user_id)
        .await
        .expect("board 1 is inserted");

    let mut board2 = Board {
        title: "Public Board".to_owned(),
        properties: object(json!({ "foo": "bar2" })),
        ..new_board(&new_id(), &team_id_1, BoardType::Open)
    };
    test.insert_board(&mut board2, &user_id)
        .await
        .expect("board 2 is inserted");

    let mut board3 = Board {
        title: "Private Board with admin".to_owned(),
        ..new_board(&new_id(), &team_id_1, BoardType::Private)
    };
    test.insert_board_with_admin(&mut board3, &user_id)
        .await
        .expect("board 3 is inserted");

    let mut board4 = Board {
        title: "Private Board".to_owned(),
        ..new_board(&new_id(), &team_id_1, BoardType::Private)
    };
    test.insert_board(&mut board4, &user_id)
        .await
        .expect("board 4 is inserted");

    let mut board5 = Board {
        title: "Public Board with admin in team 2".to_owned(),
        ..new_board(&new_id(), &team_id_2, BoardType::Open)
    };
    test.insert_board_with_admin(&mut board5, &user_id)
        .await
        .expect("board 5 is inserted");

    let cases = [
        SearchCase {
            name: "should find all private boards that the user is a member of and public \
                   boards with an empty term",
            term: "",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_boards: vec![&board1, &board2, &board3, &board5],
        },
        SearchCase {
            name: "should find all with term board",
            term: "board",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_boards: vec![&board1, &board2, &board3, &board5],
        },
        SearchCase {
            name: "should find all with term board where the user is member of",
            term: "board",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Exclude,
            expected_boards: vec![&board1, &board3, &board5],
        },
        SearchCase {
            name: "should find only public as per the term, wether user is a member or not",
            term: "public",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_boards: vec![&board1, &board2, &board5],
        },
        SearchCase {
            name: "should find only private as per the term, wether user is a member or not",
            term: "priv",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_boards: vec![&board3],
        },
        SearchCase {
            name: "should find no board in team 2 with a non matching term",
            term: "non-matching-term",
            search_field: BoardSearchField::Title,
            public_boards: PublicBoards::Include,
            expected_boards: vec![],
        },
        SearchCase {
            name: "should find all boards with a named property",
            term: "foo",
            search_field: BoardSearchField::PropertyName,
            public_boards: PublicBoards::Include,
            expected_boards: vec![&board1, &board2],
        },
        SearchCase {
            name: "should find no boards with a non-existing named property",
            term: "bogus",
            search_field: BoardSearchField::PropertyName,
            public_boards: PublicBoards::Include,
            expected_boards: vec![],
        },
    ];

    for case in cases {
        let found = test
            .search_boards_for_user(case.term, case.search_field, &user_id, case.public_boards)
            .await
            .expect(case.name);
        assert_eq!(
            sorted_board_ids(&found),
            sorted_board_ids(case.expected_boards),
            "{}",
            case.name
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn search_boards_for_user_in_team() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;

    // "should return empty list if there are no resutls": for a team that does not
    // exist, and for text that is no identifier.
    for team_id in [new_id(), NOT_AN_ID.to_owned()] {
        let boards = test
            .search_boards_for_user_in_team(&team_id, "", &user_id)
            .await
            .expect("the search succeeds");
        assert!(boards.is_empty(), "team {team_id}: got {boards:?}");
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
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    // "testGetBoardHistory: create board"
    {
        let original_title = "Board: original title";
        let board_id = new_id();
        let mut board = Board {
            title: original_title.to_owned(),
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board(&mut board, &user_id)
            .await
            .expect("the board is inserted");
        let r_board1 = board;

        let ascending = HistoryQuery::default();

        let boards = test
            .get_board_history(&board_id, &ascending)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 1, "got {boards:?}");

        let user_id_2 = common::user(&test, "user2").await;
        let new_title = "Board: A new title";
        let new_description = "A new description";
        let patch = BoardPatch {
            title: Some(new_title.to_owned()),
            description: Some(new_description.to_owned()),
            ..BoardPatch::default()
        };
        let patched_board = test
            .patch_board(&board_id, &patch, &user_id_2)
            .await
            .expect("the patch is applied");

        // Updated history
        let boards = test
            .get_board_history(&board_id, &ascending)
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 2, "got {boards:?}");
        assert_eq!(boards[0].title, original_title, "the oldest state");
        assert_eq!(boards[1].title, new_title, "the patched title");
        assert_eq!(
            boards[1].description, new_description,
            "the patched description"
        );

        // Check history against latest board
        let r_board2 = test.get_board(&board_id).await.expect("the board exists");
        assert_eq!(r_board2.title, new_title, "the stored title");
        assert_eq!(r_board2.title, boards[1].title, "the newest state's title");
        assert_ne!(r_board2.update_at, 0, "the modification time is set");
        assert_eq!(
            r_board1.update_at, boards[0].update_at,
            "the oldest state's time"
        );
        assert_eq!(
            r_board2.update_at, patched_board.update_at,
            "the patched board's time"
        );
        assert_eq!(
            r_board2.update_at, boards[1].update_at,
            "the newest state's time"
        );
        assert_eq!(
            r_board1, boards[0],
            "the oldest state is the inserted board"
        );
        assert_eq!(r_board2, boards[1], "the newest state is the stored board");

        let new_title_2 = "Board: A new title 2";
        let patch2 = BoardPatch {
            title: Some(new_title_2.to_owned()),
            ..BoardPatch::default()
        };
        let patch_board2 = test
            .patch_board(&board_id, &patch2, &user_id_2)
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
        assert_eq!(boards[0].title, new_title_2, "the newest title");
        assert_eq!(boards[0], patch_board2, "the newest state");

        // Delete board. The assertions below need the deletion to be stamped later than
        // the last patch.
        tokio::time::sleep(Duration::from_millis(10)).await;
        test.delete_board(&board_id, &user_id)
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
        assert_ne!(boards[0].update_at, 0, "the deletion is stamped");
        assert!(
            boards[0].update_at > patch_board2.update_at,
            "{} > {}",
            boards[0].update_at,
            patch_board2.update_at
        );
        assert_ne!(boards[0].delete_at, 0, "the newest state is a deleted one");
        assert!(
            boards[0].delete_at > patch_board2.update_at,
            "{} > {}",
            boards[0].delete_at,
            patch_board2.update_at
        );
    }

    // "testGetBoardHistory: nonexisting board"
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        let boards = test
            .get_board_history(&board_id, &HistoryQuery::default())
            .await
            .expect("the history is read");
        assert_eq!(boards.len(), 0, "board {board_id}: got {boards:?}");
    }

    test.teardown().await;
}

// Characterization tests: behavior of the original that its test suite does not cover,
// or what this server does in its place.

/// The original stamps the templates of its global team
/// (`server/services/store/sqlstore/board.go:299-303`). There is no global team here:
/// the templates the server ships with are the ones stored on behalf of no user, and
/// those are stamped.
#[tokio::test]
async fn insert_board_stamps_templates_of_the_global_team_with_a_tracking_id() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    let mut builtin = Board {
        title: "hello".to_owned(),
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut builtin, "")
        .await
        .expect("the template is inserted");
    assert!(
        builtin.is_builtin,
        "a board stored on behalf of no user is built in"
    );
    assert_eq!(builtin.created_by, "", "nobody made it");
    // The MD5 of the title, in hexadecimal.
    let tracking_id = json!("5d41402abc4b2a76b9719d911017c592");
    assert_eq!(
        builtin.properties["trackingTemplateId"], tracking_id,
        "the caller's board is stamped"
    );
    let stored = test
        .get_board(&builtin.id)
        .await
        .expect("the template exists");
    assert_eq!(stored, builtin, "what was stored is the caller's board");

    // What a caller says about `is_builtin` does not count.
    let mut of_user = Board {
        title: "hello".to_owned(),
        is_template: true,
        is_builtin: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut of_user, &user_id)
        .await
        .expect("the template is inserted");
    assert!(!of_user.is_builtin, "a user's template is not built in");
    assert!(
        !of_user.properties.contains_key("trackingTemplateId"),
        "only built-in templates are stamped"
    );

    let mut board = Board {
        title: "hello".to_owned(),
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut board, "")
        .await
        .expect("the board is inserted");
    assert!(
        !board.properties.contains_key("trackingTemplateId"),
        "only templates are stamped"
    );

    // A built-in template stays one whoever writes it later, and is stamped again.
    let mut renamed = Board {
        title: "world".to_owned(),
        is_template: true,
        ..new_board(&builtin.id, &team_id, BoardType::Open)
    };
    test.insert_board(&mut renamed, &user_id)
        .await
        .expect("the template is updated");
    assert!(renamed.is_builtin, "the template is still built in");
    assert_eq!(
        renamed.properties["trackingTemplateId"],
        json!("7d793037a0760186574b0282f2f435e7"),
        "the stamp follows the title"
    );

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:360-399`: what an update leaves alone, and
/// what it nevertheless writes to the history.
#[tokio::test]
async fn insert_board_update_keeps_team_creator_and_creation_time() {
    let test = common::setup().await;
    let creator_id = common::user(&test, "creator").await;
    let editor_id = common::user(&test, "editor").await;
    let team_id = common::team(&test).await;
    let another_team_id = common::team(&test).await;
    let board_id = new_id();

    let mut board = new_board(&board_id, &team_id, BoardType::Open);
    test.insert_board(&mut board, &creator_id)
        .await
        .expect("the board is inserted");

    let mut update = Board {
        title: "Updated".to_owned(),
        ..new_board(&board_id, &another_team_id, BoardType::Private)
    };
    test.insert_board(&mut update, &editor_id)
        .await
        .expect("the board is updated");
    // The caller's board is only stamped with the modifier and the modification time.
    assert_eq!(update.created_by, "", "the creator is not copied");
    assert_eq!(update.create_at, 0, "the creation time is not copied");
    assert_eq!(update.modified_by, editor_id, "the modifier");

    let stored = test.get_board(&board_id).await.expect("the board exists");
    assert_eq!(stored.team_id, team_id, "the team stays");
    assert_eq!(stored.created_by, creator_id, "the creator stays");
    assert_eq!(stored.create_at, board.create_at, "the creation time stays");
    assert_eq!(stored.modified_by, editor_id, "the modifier");
    assert_eq!(stored.title, "Updated", "the title is the new one");
    assert_eq!(stored.board_type, BoardType::Private, "so is the type");

    // The history row is the caller's board, not the stored one.
    let history = test
        .get_board_history(&board_id, &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 2, "got {history:?}");
    assert_eq!(history[1], update, "the newest state");

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:414-479` and `:789-874`: a board takes its
/// blocks with it, both ways.
#[tokio::test]
async fn delete_and_undelete_board_take_the_blocks_along() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let deleter_id = common::user(&test, "deleter").await;
    let restorer_id = common::user(&test, "restorer").await;
    let team_id = common::team(&test).await;
    let board_id = new_id();
    let card_id = new_id();
    let text_id = new_id();

    let mut board = new_board(&board_id, &team_id, BoardType::Open);
    test.insert_board(&mut board, &user_id)
        .await
        .expect("the board is inserted");
    let mut blocks = vec![
        Block {
            id: card_id.clone(),
            board_id: board_id.clone(),
            parent_id: board_id.clone(),
            ..Block::default()
        },
        Block {
            id: text_id.clone(),
            board_id: board_id.clone(),
            parent_id: card_id.clone(),
            ..Block::default()
        },
    ];
    common::insert_blocks(&test, &mut blocks, &user_id).await;
    let member = BoardMember {
        board_id: board_id.clone(),
        user_id: user_id.clone(),
        scheme_admin: true,
        ..BoardMember::default()
    };
    test.save_member(&member)
        .await
        .expect("the member is saved");

    test.delete_board(&board_id, &deleter_id)
        .await
        .expect("the board is deleted");
    let left = test
        .get_blocks(&BlockFilter::board(&board_id))
        .await
        .expect("the blocks are read");
    assert!(left.is_empty(), "got {left:?}");
    let members = test
        .get_members_for_board(&board_id)
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
        .get_board_history(&board_id, &newest)
        .await
        .expect("the history is read");
    assert_eq!(history[0].modified_by, deleter_id, "who deleted the board");
    assert_eq!(
        history[0].delete_at, history[0].update_at,
        "the deletion is the last modification"
    );

    test.undelete_board(&board_id, &restorer_id)
        .await
        .expect("the board is restored");
    let restored = test.get_board(&board_id).await.expect("the board is back");
    assert_eq!(restored.modified_by, restorer_id, "who restored the board");
    assert_eq!(restored.created_by, user_id, "the creator stays");
    assert_eq!(
        restored.create_at, board.create_at,
        "the creation time stays"
    );
    assert_eq!(restored.delete_at, 0, "the board is not deleted");
    let back = test
        .get_blocks(&BlockFilter::board(&board_id))
        .await
        .expect("the blocks are read");
    assert_eq!(
        common::sorted_ids(&back),
        sorted([&card_id, &text_id]),
        "both blocks are back"
    );

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:502-550`. There is no column for the roles
/// any more; the member reads as empty.
#[tokio::test]
async fn save_member_stores_no_roles_and_reads_the_minimum_role_of_the_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let member_id = common::user(&test, "member").await;
    let team_id = common::team(&test).await;
    let board_id = new_id();

    let mut board = Board {
        minimum_role: BoardRole::Editor,
        ..new_board(&board_id, &team_id, BoardType::Open)
    };
    test.insert_board(&mut board, &user_id)
        .await
        .expect("the board is inserted");

    let member = BoardMember {
        board_id: board_id.clone(),
        user_id: member_id.clone(),
        roles: "some-role".to_owned(),
        scheme_commenter: true,
        ..BoardMember::default()
    };
    test.save_member(&member)
        .await
        .expect("the member is saved");

    let stored = test
        .get_member_for_board(&board_id, &member_id)
        .await
        .expect("the member exists");
    assert_eq!(
        stored,
        BoardMember {
            board_id,
            user_id: member_id.clone(),
            roles: String::new(),
            minimum_role: "editor".to_owned(),
            scheme_commenter: true,
            ..BoardMember::default()
        },
        "no roles, and the minimum role of the board"
    );
    let of_user = test
        .get_members_for_user(&member_id)
        .await
        .expect("the memberships are read");
    assert_eq!(of_user, vec![stored], "the user's memberships");

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:713-753`.
#[tokio::test]
async fn search_boards_for_user_in_team_finds_open_boards_and_own_private_boards() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let other_user_id = common::user(&test, "other-user").await;
    let team_id_1 = common::team(&test).await;
    let team_id_2 = common::team(&test).await;

    let mut open = Board {
        title: "Open Roadmap".to_owned(),
        ..new_board(&new_id(), &team_id_1, BoardType::Open)
    };
    test.insert_board(&mut open, &other_user_id)
        .await
        .expect("the board is inserted");
    let mut own_private = Board {
        title: "Private Roadmap".to_owned(),
        ..new_board(&new_id(), &team_id_1, BoardType::Private)
    };
    test.insert_board_with_admin(&mut own_private, &user_id)
        .await
        .expect("the board is inserted");
    let mut foreign_private = Board {
        title: "Secret Roadmap".to_owned(),
        ..new_board(&new_id(), &team_id_1, BoardType::Private)
    };
    test.insert_board_with_admin(&mut foreign_private, &other_user_id)
        .await
        .expect("the board is inserted");
    let mut other_team = Board {
        title: "Open Roadmap elsewhere".to_owned(),
        ..new_board(&new_id(), &team_id_2, BoardType::Open)
    };
    test.insert_board(&mut other_team, &user_id)
        .await
        .expect("the board is inserted");
    let mut template = Board {
        title: "Roadmap template".to_owned(),
        is_template: true,
        ..new_board(&new_id(), &team_id_1, BoardType::Open)
    };
    test.insert_board(&mut template, &user_id)
        .await
        .expect("the board is inserted");

    let search = async |term: &str| {
        let boards = test
            .search_boards_for_user_in_team(&team_id_1, term, &user_id)
            .await
            .expect("the search succeeds");
        sorted_board_ids(&boards)
    };
    let open_and_own_private = sorted_board_ids([&open, &own_private]);
    assert_eq!(
        search("").await,
        open_and_own_private,
        "an empty term matches every board the user can reach"
    );
    assert_eq!(
        search("ROADMAP").await,
        open_and_own_private,
        "case does not matter"
    );
    assert_eq!(
        search("roadmap open").await,
        [open.id.as_str()],
        "every word must match"
    );
    assert_eq!(
        search("  private  ").await,
        [own_private.id.as_str()],
        "spaces around the term do not count"
    );
    assert_eq!(
        search("secret").await,
        Vec::<String>::new(),
        "a private board of somebody else is not found"
    );

    test.teardown().await;
}

/// `server/services/store/sqlstore/templates.go:57-89`. The original lists its built-in
/// templates through this query too; here they are read on their own.
#[tokio::test]
async fn get_template_boards_returns_open_templates_and_templates_the_user_is_member_of() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let other_user_id = common::user(&test, "other-user").await;
    let team_id = common::team(&test).await;
    let another_team_id = common::team(&test).await;

    let mut open_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut open_template, &other_user_id)
        .await
        .expect("the template is inserted");
    let mut own_private_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Private)
    };
    test.insert_board_with_admin(&mut own_private_template, &user_id)
        .await
        .expect("the template is inserted");
    let mut foreign_private_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Private)
    };
    test.insert_board_with_admin(&mut foreign_private_template, &other_user_id)
        .await
        .expect("the template is inserted");
    let mut other_team_template = Board {
        is_template: true,
        ..new_board(&new_id(), &another_team_id, BoardType::Open)
    };
    test.insert_board(&mut other_team_template, &user_id)
        .await
        .expect("the template is inserted");
    let mut builtin_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut builtin_template, "")
        .await
        .expect("the template is inserted");
    let mut board = new_board(&new_id(), &team_id, BoardType::Open);
    test.insert_board_with_admin(&mut board, &user_id)
        .await
        .expect("the board is inserted");

    let templates = test
        .get_template_boards(&team_id, &user_id)
        .await
        .expect("the templates are read");
    assert_eq!(
        sorted_by_id(templates),
        sorted_by_id(vec![open_template.clone(), own_private_template]),
        "the open template of the team and the user's private one"
    );

    // No user is a member of nothing: the open templates are left. The built-in one,
    // open and of this team, is not among them either way.
    let templates = test
        .get_template_boards(&team_id, "")
        .await
        .expect("the templates are read");
    assert_eq!(templates, vec![open_template], "the open template");

    let builtin = test
        .get_builtin_templates()
        .await
        .expect("the built-in templates are read");
    assert_eq!(builtin, vec![builtin_template], "the built-in template");

    test.teardown().await;
}

/// The original's `RemoveDefaultTemplates`
/// (`server/services/store/sqlstore/templates.go:17-55`) removes the templates its system
/// user made. Here the templates the server ships with are the boards stored on behalf of
/// no user, and `remove_builtin_templates` removes those.
#[tokio::test]
async fn remove_default_templates_removes_system_templates_and_their_blocks_without_history() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    let mut builtin_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut builtin_template, "")
        .await
        .expect("the template is inserted");
    let mut user_template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut user_template, &user_id)
        .await
        .expect("the template is inserted");
    let mut plain_board = new_board(&new_id(), &team_id, BoardType::Open);
    test.insert_board(&mut plain_board, "")
        .await
        .expect("the board is inserted");

    let new_block_of = |board: &Board| Block {
        id: new_id(),
        board_id: board.id.clone(),
        parent_id: board.id.clone(),
        ..Block::default()
    };
    let mut blocks = vec![
        new_block_of(&builtin_template),
        new_block_of(&user_template),
        new_block_of(&plain_board),
    ];
    common::insert_blocks(&test, &mut blocks, "").await;

    // Only a template stored on behalf of no user is built in. A board that is no
    // template is a board without a creator, and nothing else.
    let builtin = test
        .get_builtin_templates()
        .await
        .expect("the built-in templates are read");
    assert_eq!(
        sorted_board_ids(&builtin),
        sorted_board_ids([&builtin_template]),
        "the templates stored on behalf of no user"
    );

    test.remove_builtin_templates()
        .await
        .expect("the built-in templates are removed");

    let blocks_of = async |board: &Board| {
        test.get_blocks(&BlockFilter::board(&board.id))
            .await
            .expect("the blocks are read")
            .len()
    };
    let error = test
        .get_board(&builtin_template.id)
        .await
        .expect_err("the built-in template is gone");
    assert!(error.is_not_found(), "got {error:?}");
    assert_eq!(blocks_of(&builtin_template).await, 0, "and so is its block");

    test.get_board(&user_template.id)
        .await
        .expect("a template created by a user stays");
    assert_eq!(blocks_of(&user_template).await, 1, "and so does its block");

    // The original keeps a board of its system user that is not a template and removes
    // its blocks all the same. Here such a board is not built in, and stays whole.
    let kept = test
        .get_board(&plain_board.id)
        .await
        .expect("a board without a creator that is no template stays");
    assert!(!kept.is_builtin, "it is not built in: {kept:?}");
    assert_eq!(blocks_of(&plain_board).await, 1, "and so does its block");
    let builtin = test
        .get_builtin_templates()
        .await
        .expect("the built-in templates are read");
    assert!(builtin.is_empty(), "got {builtin:?}");

    // Nothing is recorded: the history still ends with the insertion.
    let history = test
        .get_board_history(&builtin_template.id, &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(history.len(), 1, "got {history:?}");
    assert_eq!(history[0].delete_at, 0, "the insertion, not a deletion");
    let block_history = test
        .get_block_history(&blocks[0].id, &HistoryQuery::default())
        .await
        .expect("the history is read");
    assert_eq!(block_history.len(), 1, "got {block_history:?}");

    test.teardown().await;
}

/// Not in the original: a board is not stored with a malformed card property
/// definition, whether it is inserted or patched, and the stored board stays as it was.
#[tokio::test]
async fn boards_with_malformed_card_property_definitions_are_not_stored() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = new_id();
    let well_formed = vec![object(
        json!({ "id": "p1", "name": "Status", "options": [] }),
    )];

    let mut malformed = Board {
        card_properties: vec![object(json!({ "name": "no id" }))],
        ..new_board(&board_id, &team_id, BoardType::Open)
    };
    let error = test
        .insert_board(&mut malformed, &user_id)
        .await
        .expect_err("a definition without an identifier");
    assert_eq!(
        error.to_string(),
        "card property definition 1 needs a non-empty string as id",
        "what is wrong with the definition"
    );
    assert!(
        test.get_board(&board_id).await.unwrap_err().is_not_found(),
        "nothing was stored"
    );

    let mut board = Board {
        card_properties: well_formed.clone(),
        ..new_board(&board_id, &team_id, BoardType::Open)
    };
    test.insert_board(&mut board, &user_id)
        .await
        .expect("a well-formed board is inserted");

    let patch = BoardPatch {
        title: Some("renamed".to_owned()),
        updated_card_properties: vec![object(json!({ "id": "p2", "options": "none" }))],
        ..BoardPatch::default()
    };
    let error = test
        .patch_board(&board_id, &patch, &user_id)
        .await
        .expect_err("options that are no list");
    assert!(
        error
            .to_string()
            .contains("options of card property \"p2\""),
        "got {error}"
    );
    let stored = test.get_board(&board_id).await.unwrap();
    assert_eq!(
        stored.card_properties, well_formed,
        "the definitions are unchanged"
    );
    assert_ne!(stored.title, "renamed", "and so is the rest of the board");

    test.teardown().await;
}
