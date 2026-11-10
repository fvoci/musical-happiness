//! Port of `server/services/store/storetests/boards.go`.
//!
//! One test per test function of the original; its subtests are the commented sections,
//! in the original order, on one store.
//!
//! The original names users, teams and boards by whatever text is at hand. Here each of
//! them is a stored row with an identifier of its own, and text that is no identifier
//! names nothing. The original's tests of the board history, the membership history and
//! the board count went with those functions.
//!
//! The tests after [`search_boards_for_user_in_team`] have no counterpart in the
//! original, which does not test templates, the template tracking identifier, the trash
//! or the membership changes that keep a board its administrator.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use fvoci_model::{
    Block, BlockType, Board, BoardMember, BoardPatch, BoardSearchField, BoardType, Body, Category,
    FileInfo, JsonMap, MinimumRole, SubscriberType, Subscription, new_id,
};
use fvoci_store::{
    BlockFilter, Joined, MemberChange, PublicBoards, Purged, SharingChange, Store, StoreError,
};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::Barrier;
use tokio::task::JoinSet;

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
            minimum_role: MinimumRole::Commenter,
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
        assert_eq!(
            MinimumRole::Commenter,
            board.minimum_role,
            "the minimum role"
        );
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

        // The original restores a board that is not deleted once more, without effect.
        // Here only what is in the trash is restored.
        let error = test
            .undelete_board(&board_id, &user_id)
            .await
            .expect_err("the board is not in the trash any more");
        assert!(error.is_not_found(), "got {error:?}");

        test.get_board(&board_id)
            .await
            .expect("the board is still there");
    }

    // "from not existing id": the original succeeds and restores nothing.
    for board_id in [new_id(), NOT_AN_ID.to_owned()] {
        let error = test
            .undelete_board(&board_id, &user_id)
            .await
            .expect_err("no such board is in the trash");
        assert!(error.is_not_found(), "board {board_id}: got {error:?}");

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

/// `server/services/store/sqlstore/board.go:360-399`: what an update leaves alone. The
/// original leaves the caller's board as the caller filled it in; here it comes back as
/// it is stored.
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

    // Whatever the caller says about the team, the creator and the creation time: none
    // of it, another of each, and the same again.
    let claims = [
        (another_team_id.as_str(), "", 0),
        (another_team_id.as_str(), editor_id.as_str(), 12_345),
        (team_id.as_str(), creator_id.as_str(), board.create_at),
    ];
    for (claimed_team, claimed_creator, claimed_create_at) in claims {
        let mut update = Board {
            title: format!("Updated by a caller who claims {claimed_create_at}"),
            created_by: claimed_creator.to_owned(),
            create_at: claimed_create_at,
            ..new_board(&board_id, claimed_team, BoardType::Private)
        };
        test.insert_board(&mut update, &editor_id)
            .await
            .expect("the board is updated");
        assert_eq!(update.team_id, team_id, "the stored team comes back");
        assert_eq!(
            update.created_by, creator_id,
            "the stored creator comes back, not {claimed_creator:?}"
        );
        assert_eq!(
            update.create_at, board.create_at,
            "the stored creation time comes back, not {claimed_create_at}"
        );
        assert_eq!(update.modified_by, editor_id, "the modifier");

        let stored = test.get_board(&board_id).await.expect("the board exists");
        assert_eq!(stored.team_id, team_id, "the team stays");
        assert_eq!(stored.created_by, creator_id, "the creator stays");
        assert_eq!(stored.create_at, board.create_at, "the creation time stays");
        assert_eq!(
            stored.board_type,
            BoardType::Private,
            "the type is the new one"
        );
        assert_eq!(update, stored, "the caller's board is the stored board");
    }

    test.teardown().await;
}

/// `server/services/store/sqlstore/board.go:414-479` and `:789-874`: a board takes its
/// blocks with it, both ways. The original marks them deleted where they are; here they
/// move to the trash and back.
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
    // The original keeps the memberships of a deleted board. Here they go to the trash
    // with it.
    let members = test
        .get_members_for_board(&board_id)
        .await
        .expect("the members are read");
    assert!(members.is_empty(), "got {members:?}");
    // The original marks the board's last history row as deleted, with the deleter as
    // its modifier. Here the board is in the trash as it was, and the trash says who
    // deleted it.
    let deleted = test
        .get_deleted_board(&board_id)
        .await
        .expect("the board is in the trash");
    assert_eq!(deleted.deleted_by, deleter_id, "who deleted the board");
    assert_eq!(deleted.board.modified_by, user_id, "the board as it was");
    assert!(
        deleted.board.delete_at >= board.update_at,
        "deleted at {} after the last modification at {}",
        deleted.board.delete_at,
        board.update_at
    );
    assert_eq!(
        sorted_user_ids(&deleted.members),
        [user_id.as_str()],
        "the membership is kept with the board"
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
async fn save_member_stores_the_scheme_flags_and_no_roles() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let member_id = common::user(&test, "member").await;
    let team_id = common::team(&test).await;
    let board_id = new_id();

    let mut board = Board {
        minimum_role: MinimumRole::Editor,
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
            scheme_commenter: true,
            ..BoardMember::default()
        },
        "the scheme flags and no roles"
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
/// no user, and `remove_builtin_templates` removes those. The original leaves their
/// history rows; here nothing of them is left, in the trash or elsewhere.
#[tokio::test]
async fn remove_default_templates_removes_system_templates_and_their_blocks_past_the_trash() {
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

    // Nothing is kept: neither the template nor its block is in the trash.
    let error = test
        .get_deleted_board(&builtin_template.id)
        .await
        .expect_err("the built-in template is not in the trash");
    assert!(error.is_not_found(), "got {error:?}");
    let error = test
        .get_deleted_block(&blocks[0].id)
        .await
        .expect_err("the block of the built-in template is not in the trash");
    assert!(error.is_not_found(), "got {error:?}");
    let error = test
        .undelete_board(&builtin_template.id, &user_id)
        .await
        .expect_err("there is nothing to restore");
    assert!(error.is_not_found(), "got {error:?}");

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

// The trash. None of this is in the original, which marks what is deleted where it is.

/// A membership that administers the board.
fn admin_of(board_id: &str, user_id: &str) -> BoardMember {
    BoardMember {
        board_id: board_id.to_owned(),
        user_id: user_id.to_owned(),
        scheme_admin: true,
        scheme_editor: true,
        ..BoardMember::default()
    }
}

/// A membership that may edit the board and does not administer it.
fn editor_of(board_id: &str, user_id: &str) -> BoardMember {
    BoardMember {
        board_id: board_id.to_owned(),
        user_id: user_id.to_owned(),
        scheme_editor: true,
        ..BoardMember::default()
    }
}

/// A block of a board: on the block `parent_id`, or directly on the board if that is
/// empty.
fn new_block(board_id: &str, parent_id: &str, block_type: BlockType) -> Block {
    Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        title: format!("a {}", block_type.as_str()),
        block_type,
        ..Block::default()
    }
}

fn sorted_blocks(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    blocks
}

fn sorted_members(mut members: Vec<BoardMember>) -> Vec<BoardMember> {
    members.sort_by(|a, b| a.user_id.cmp(&b.user_id));
    members
}

/// The blocks of a board that exist, sorted by identifier.
async fn blocks_of(store: &Store, board_id: &str) -> Vec<Block> {
    let blocks = store
        .get_blocks(&BlockFilter::board(board_id))
        .await
        .expect("the blocks are read");
    sorted_blocks(blocks)
}

/// The memberships of a board, sorted by user.
async fn members_of(store: &Store, board_id: &str) -> Vec<BoardMember> {
    let members = store
        .get_members_for_board(board_id)
        .await
        .expect("the members are read");
    sorted_members(members)
}

/// Subscribes a user to a card.
async fn subscribe(store: &Store, card_id: &str, user_id: &str) {
    let subscription = Subscription {
        block_type: BlockType::Card,
        block_id: card_id.to_owned(),
        subscriber_type: SubscriberType::User,
        subscriber_id: user_id.to_owned(),
        notified_at: 0,
        create_at: 0,
        delete_at: 0,
    };
    store
        .create_subscription(subscription)
        .await
        .expect("the subscription is stored");
}

/// The identifiers of the blocks a user subscribes to, sorted.
async fn subscribed_block_ids(store: &Store, user_id: &str) -> Vec<String> {
    let mut block_ids: Vec<String> = store
        .get_subscriptions(user_id)
        .await
        .expect("the subscriptions are read")
        .into_iter()
        .map(|subscription| subscription.block_id)
        .collect();
    block_ids.sort();
    block_ids
}

/// A board with everything that hangs on one.
struct FullBoard {
    team_id: String,
    /// Made the board and administers it.
    admin_id: String,
    /// May edit the board, has it in a sidebar category and subscribes to its card.
    editor_id: String,
    board: Board,
    /// A view and a card on the board, a text on the card and a checkbox on the text,
    /// sorted by identifier.
    blocks: Vec<Block>,
    /// The identifier of the card.
    card_id: String,
    /// The memberships as they read while the board exists, sorted by user.
    members: Vec<BoardMember>,
    file: FileInfo,
}

impl FullBoard {
    async fn store(store: &Store) -> Self {
        let team_id = common::team(store).await;
        let admin_id = common::user(store, "admin").await;
        let editor_id = common::user(store, "editor").await;

        let mut board = Board {
            title: "Roadmap".to_owned(),
            description: "What comes when".to_owned(),
            icon: "🗺".to_owned(),
            show_description: true,
            minimum_role: MinimumRole::Commenter,
            properties: object(json!({ "color": "blue" })),
            card_properties: vec![object(json!({ "id": "status", "name": "Status" }))],
            ..new_board(&new_id(), &team_id, BoardType::Private)
        };
        store
            .insert_board_with_admin(&mut board, &admin_id)
            .await
            .expect("the board and its administrator are stored");
        store
            .save_member(&editor_of(&board.id, &editor_id))
            .await
            .expect("the editor is stored");
        let members = members_of(store, &board.id).await;

        let view = new_block(&board.id, "", BlockType::View);
        let card = new_block(&board.id, "", BlockType::Card);
        let text = new_block(&board.id, &card.id, BlockType::Text);
        let checkbox = new_block(&board.id, &text.id, BlockType::Checkbox);
        let card_id = card.id.clone();
        let mut blocks = vec![view, card, text, checkbox];
        store
            .insert_blocks(&mut blocks, &admin_id)
            .await
            .expect("the blocks are stored");

        let file = FileInfo {
            id: new_id(),
            board_id: board.id.clone(),
            stored_name: "7f3a9c.png".to_owned(),
            name: "diagram.png".to_owned(),
            extension: ".png".to_owned(),
            size: 2048,
            path: format!("boards/{}/7f3a9c.png", board.id),
            create_at: board.create_at,
        };
        store
            .save_file_info(&file)
            .await
            .expect("the file record is stored");

        store
            .set_sharing(&board.id, SharingChange::On, "read-token", &admin_id)
            .await
            .expect("the board is shared");

        let category = Category {
            id: new_id(),
            name: "Plans".to_owned(),
            user_id: editor_id.clone(),
            team_id: team_id.clone(),
            create_at: board.create_at,
            update_at: board.create_at,
            ..Category::default()
        };
        store
            .create_category(&category)
            .await
            .expect("the category is stored");
        store
            .add_update_category_board(&editor_id, &category.id, std::slice::from_ref(&board.id))
            .await
            .expect("the board is filed under the category");

        subscribe(store, &card_id, &editor_id).await;

        Self {
            team_id,
            admin_id,
            editor_id,
            board,
            blocks: sorted_blocks(blocks),
            card_id,
            members,
            file,
        }
    }

    /// The board is not shared, no user has it in a category and nobody subscribes to
    /// its blocks.
    async fn assert_sharing_filing_and_subscriptions_are_gone(&self, store: &Store) {
        let error = store
            .get_sharing(&self.board.id)
            .await
            .expect_err("the board is not shared");
        assert!(error.is_not_found(), "got {error:?}");

        let categories = store
            .get_user_category_boards(&self.editor_id, &self.team_id)
            .await
            .expect("the categories are read");
        assert_eq!(categories.len(), 1, "the category stays: {categories:?}");
        assert!(
            categories[0].board_metadata.is_empty(),
            "the board is filed nowhere: {:?}",
            categories[0].board_metadata
        );

        let subscribed = subscribed_block_ids(store, &self.editor_id).await;
        assert!(subscribed.is_empty(), "got {subscribed:?}");
        let subscribers = store
            .get_subscribers_count_for_block(&self.card_id)
            .await
            .expect("the subscribers are counted");
        assert_eq!(subscribers, 0, "nobody subscribes to the card");
    }
}

/// What a board is made of is not found once the board is deleted, and is in the trash.
#[tokio::test]
async fn delete_board_moves_the_board_and_what_is_its_own_to_the_trash() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;
    let deleter_id = common::user(&test, "deleter").await;
    let board_id = full.board.id.as_str();

    test.delete_board(board_id, &deleter_id)
        .await
        .expect("the board is deleted");

    let error = test
        .get_board(board_id)
        .await
        .expect_err("the board is gone");
    assert!(error.is_not_found(), "got {error:?}");
    let left = blocks_of(&test, board_id).await;
    assert!(left.is_empty(), "blocks are left: {left:?}");
    let members = members_of(&test, board_id).await;
    assert!(members.is_empty(), "memberships are left: {members:?}");
    let of_admin = test
        .get_members_for_user(&full.admin_id)
        .await
        .expect("the memberships are read");
    assert!(
        of_admin.is_empty(),
        "the user is left a member: {of_admin:?}"
    );
    let error = test
        .get_file_info(board_id, &full.file.stored_name)
        .await
        .expect_err("the file record is gone");
    assert!(error.is_not_found(), "got {error:?}");

    let deleted = test
        .get_deleted_board(board_id)
        .await
        .expect("the board is in the trash");
    let deleted_at = deleted.board.delete_at;
    assert!(
        deleted_at >= full.board.update_at,
        "deleted at {deleted_at}, after the board was stored at {}",
        full.board.update_at
    );
    assert_eq!(
        deleted.board,
        Board {
            delete_at: deleted_at,
            ..full.board.clone()
        },
        "the board as it was, marked as deleted"
    );
    assert_eq!(deleted.deleted_by, deleter_id, "who deleted the board");
    assert_eq!(
        sorted_members(deleted.members),
        full.members,
        "the memberships as they were"
    );

    for block in &full.blocks {
        let error = test
            .get_block(&block.id)
            .await
            .expect_err("the block is gone");
        assert!(error.is_not_found(), "block {}: got {error:?}", block.title);
        let in_trash = test
            .get_deleted_block(&block.id)
            .await
            .expect("the block is in the trash");
        assert_eq!(
            in_trash.block,
            Block {
                delete_at: deleted_at,
                ..block.clone()
            },
            "{} as it was, deleted when its board was",
            block.title
        );
        assert_eq!(
            in_trash.deleted_by, deleter_id,
            "who deleted {}",
            block.title
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn delete_board_ends_sharing_category_filing_and_subscriptions() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;

    // Before: the board is shared, filed and subscribed to.
    test.get_sharing(&full.board.id)
        .await
        .expect("the board is shared");
    let categories = test
        .get_user_category_boards(&full.editor_id, &full.team_id)
        .await
        .expect("the categories are read");
    assert_eq!(
        categories[0].board_metadata.len(),
        1,
        "the board is filed under the category: {categories:?}"
    );
    assert_eq!(
        subscribed_block_ids(&test, &full.editor_id).await,
        [full.card_id.as_str()],
        "the editor subscribes to the card"
    );

    test.delete_board(&full.board.id, &full.admin_id)
        .await
        .expect("the board is deleted");

    full.assert_sharing_filing_and_subscriptions_are_gone(&test)
        .await;

    test.teardown().await;
}

#[tokio::test]
async fn undelete_board_brings_back_the_board_its_blocks_files_and_members() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;
    let restorer_id = common::user(&test, "restorer").await;
    let board_id = full.board.id.as_str();

    test.delete_board(board_id, &full.admin_id)
        .await
        .expect("the board is deleted");
    let returned = test
        .undelete_board(board_id, &restorer_id)
        .await
        .expect("the board is restored");

    let restored = test.get_board(board_id).await.expect("the board is back");
    assert_eq!(returned, restored, "what is returned is what is stored");
    assert!(
        restored.update_at >= full.board.update_at,
        "restoring is a modification: {} after {}",
        restored.update_at,
        full.board.update_at
    );
    assert_eq!(
        restored,
        Board {
            modified_by: restorer_id.clone(),
            update_at: restored.update_at,
            ..full.board.clone()
        },
        "the board as it was, modified by whoever restored it"
    );

    // Each block is back where it was: on its board, on its parent.
    let expected: Vec<Block> = full
        .blocks
        .iter()
        .map(|block| Block {
            modified_by: restorer_id.clone(),
            update_at: restored.update_at,
            ..block.clone()
        })
        .collect();
    assert_eq!(
        blocks_of(&test, board_id).await,
        expected,
        "the blocks as they were, modified by whoever restored them"
    );

    let file = test
        .get_file_info(board_id, &full.file.stored_name)
        .await
        .expect("the file record is back");
    assert_eq!(file, full.file, "the file record as it was, path included");
    assert_eq!(
        members_of(&test, board_id).await,
        full.members,
        "the memberships as they were"
    );

    // Nothing of the board stays in the trash.
    let error = test
        .get_deleted_board(board_id)
        .await
        .expect_err("the board is out of the trash");
    assert!(error.is_not_found(), "got {error:?}");
    for block in &full.blocks {
        let error = test
            .get_deleted_block(&block.id)
            .await
            .expect_err("the block is out of the trash");
        assert!(error.is_not_found(), "block {}: got {error:?}", block.title);
    }

    test.teardown().await;
}

#[tokio::test]
async fn undelete_board_does_not_bring_back_sharing_category_filing_or_subscriptions() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;

    test.delete_board(&full.board.id, &full.admin_id)
        .await
        .expect("the board is deleted");
    test.undelete_board(&full.board.id, &full.admin_id)
        .await
        .expect("the board is restored");

    test.get_block(&full.card_id)
        .await
        .expect("the card is back");
    full.assert_sharing_filing_and_subscriptions_are_gone(&test)
        .await;

    test.teardown().await;
}

/// The row of a user may be gone by the time a board comes back: their membership stays
/// behind.
#[tokio::test]
async fn undelete_board_leaves_out_the_membership_of_a_user_who_is_gone() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;
    let board_id = full.board.id.as_str();
    // A member who made and changed nothing: the row of a user whom a board or a block
    // names cannot be deleted.
    let leaver_id = common::user(&test, "leaver").await;
    test.save_member(&editor_of(board_id, &leaver_id))
        .await
        .expect("the member is stored");

    test.delete_board(board_id, &full.admin_id)
        .await
        .expect("the board is deleted");
    let deleted = test
        .get_deleted_board(board_id)
        .await
        .expect("the board is in the trash");
    assert_eq!(
        sorted_user_ids(&deleted.members),
        sorted([&full.admin_id, &full.editor_id, &leaver_id]),
        "all three memberships are in the trash"
    );

    common::execute_raw(
        &test.dbconfig(),
        &format!("DELETE FROM users WHERE id = '{leaver_id}'"),
    )
    .await;
    test.get_user_by_id(&leaver_id)
        .await
        .expect_err("the user is gone");

    test.undelete_board(board_id, &full.admin_id)
        .await
        .expect("the board is restored");
    assert_eq!(
        members_of(&test, board_id).await,
        full.members,
        "the memberships of the users who still exist"
    );

    test.teardown().await;
}

/// A user who is only deactivated still has a row, and a membership can refer to it:
/// it comes back with the board.
#[tokio::test]
async fn undelete_board_brings_back_the_membership_of_a_user_who_was_deactivated() {
    let test = common::setup().await;
    let full = FullBoard::store(&test).await;
    let board_id = full.board.id.as_str();
    let deactivated_id = common::user(&test, "deactivated").await;
    let membership = editor_of(board_id, &deactivated_id);
    test.save_member(&membership)
        .await
        .expect("the member is stored");
    let members_before = members_of(&test, board_id).await;
    assert_eq!(members_before.len(), 3, "got {members_before:?}");

    test.delete_board(board_id, &full.admin_id)
        .await
        .expect("the board is deleted");
    common::execute_raw(
        &test.dbconfig(),
        &format!("UPDATE users SET delete_at = 1700000000000 WHERE id = '{deactivated_id}'"),
    )
    .await;
    test.get_user_by_id(&deactivated_id)
        .await
        .expect_err("a deactivated user is not found");

    test.undelete_board(board_id, &full.admin_id)
        .await
        .expect("the board is restored");
    assert_eq!(
        members_of(&test, board_id).await,
        members_before,
        "all three memberships are back, the deactivated user's among them"
    );

    test.teardown().await;
}

/// What is in the trash refers to nothing, so the row of the user who made a board can
/// go while the board is there. The board comes back all the same, as made by nobody.
#[tokio::test]
async fn undelete_board_restores_a_board_whose_creator_is_gone_as_made_by_nobody() {
    let test = common::setup().await;
    let creator_id = common::user(&test, "creator").await;
    let editor_id = common::user(&test, "editor").await;
    let team_id = common::team(&test).await;

    let mut board = new_board(&new_id(), &team_id, BoardType::Open);
    test.insert_board_with_admin(&mut board, &creator_id)
        .await
        .expect("the board and its administrator are stored");
    test.save_member(&editor_of(&board.id, &editor_id))
        .await
        .expect("the editor is stored");
    let mut card = new_block(&board.id, "", BlockType::Card);
    test.insert_block(&mut card, &creator_id)
        .await
        .expect("the creator's card is stored");
    let mut paragraph = new_block(&board.id, &card.id, BlockType::Text);
    test.insert_block(&mut paragraph, &editor_id)
        .await
        .expect("the editor's text is stored");

    test.delete_board(&board.id, &editor_id)
        .await
        .expect("the board is deleted");
    // Nothing that exists names the creator any more.
    common::execute_raw(
        &test.dbconfig(),
        &format!("DELETE FROM users WHERE id = '{creator_id}'"),
    )
    .await;

    let returned = test
        .undelete_board(&board.id, &editor_id)
        .await
        .expect("the board is restored though its creator is gone");
    assert_eq!(returned.created_by, "", "the board was made by nobody");
    let restored = test.get_board(&board.id).await.expect("the board is back");
    assert_eq!(returned, restored, "what is returned is what is stored");
    assert_eq!(
        restored,
        Board {
            created_by: String::new(),
            modified_by: editor_id.clone(),
            update_at: restored.update_at,
            ..board.clone()
        },
        "the board as it was, but for its creator"
    );

    let restored_card = test.get_block(&card.id).await.expect("the card is back");
    assert_eq!(
        restored_card.created_by, "",
        "the card of the user who is gone was made by nobody"
    );
    let restored_paragraph = test
        .get_block(&paragraph.id)
        .await
        .expect("the text is back");
    assert_eq!(
        restored_paragraph.created_by, editor_id,
        "the text keeps its creator, who still exists"
    );
    assert_eq!(
        restored_paragraph.parent_id, card.id,
        "the text is on the card"
    );

    assert_eq!(
        members_of(&test, &board.id).await,
        [editor_of(&board.id, &editor_id)],
        "the editor is a member again; the creator, who is gone, is not"
    );

    test.teardown().await;
}

/// As for a board: a block whose creator's row went while the block was in the trash
/// comes back as made by nobody.
#[tokio::test]
async fn undelete_block_restores_a_block_whose_creator_is_gone_as_made_by_nobody() {
    let test = common::setup().await;
    let owner_id = common::user(&test, "owner").await;
    let author_id = common::user(&test, "author").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &owner_id).await;

    let mut card = new_block(&board_id, "", BlockType::Card);
    test.insert_block(&mut card, &author_id)
        .await
        .expect("the author's card is stored");
    let mut paragraph = new_block(&board_id, &card.id, BlockType::Text);
    test.insert_block(&mut paragraph, &owner_id)
        .await
        .expect("the owner's text is stored");

    // The card goes with the text on it. Nothing that exists names the author then.
    test.delete_block(&card.id, &owner_id)
        .await
        .expect("the card is deleted");
    common::execute_raw(
        &test.dbconfig(),
        &format!("DELETE FROM users WHERE id = '{author_id}'"),
    )
    .await;
    let in_trash = test
        .get_deleted_block(&card.id)
        .await
        .expect("the card is in the trash");
    assert_eq!(
        in_trash.block.created_by, author_id,
        "the trash describes what was"
    );

    let returned = test
        .undelete_block(&card.id, &owner_id)
        .await
        .expect("the card is restored though its creator is gone");
    assert_eq!(returned.created_by, "", "the card was made by nobody");
    let restored = test.get_block(&card.id).await.expect("the card is back");
    assert_eq!(returned, restored, "what is returned is what is stored");
    assert_eq!(
        restored,
        Block {
            created_by: String::new(),
            modified_by: owner_id.clone(),
            update_at: restored.update_at,
            ..card.clone()
        },
        "the card as it was, but for its creator"
    );
    let restored_paragraph = test
        .get_block(&paragraph.id)
        .await
        .expect("the text is back with its card");
    assert_eq!(
        restored_paragraph.created_by, owner_id,
        "the text keeps its creator, who still exists"
    );

    test.teardown().await;
}

/// A block that was deleted on its own is a deletion of its own: it does not come back
/// with the board, and it can be restored once the board is back.
#[tokio::test]
async fn a_block_deleted_before_its_board_stays_in_the_trash_when_the_board_is_restored() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    let card = new_block(&board_id, "", BlockType::Card);
    let paragraph = new_block(&board_id, &card.id, BlockType::Text);
    let checkbox = new_block(&board_id, &paragraph.id, BlockType::Checkbox);
    let (card_id, text_id, checkbox_id) =
        (card.id.clone(), paragraph.id.clone(), checkbox.id.clone());
    let mut blocks = [card, paragraph, checkbox];
    test.insert_blocks(&mut blocks, &user_id)
        .await
        .expect("the blocks are stored");

    // The text goes, with the checkbox on it; then the board, with the card.
    test.delete_block(&text_id, &user_id)
        .await
        .expect("the text is deleted");
    test.delete_board(&board_id, &user_id)
        .await
        .expect("the board is deleted");

    // While its board is in the trash a block has nothing to come back to.
    let error = test
        .undelete_block(&text_id, &user_id)
        .await
        .expect_err("the board of the text does not exist");
    assert!(error.is_not_found(), "got {error:?}");

    test.undelete_board(&board_id, &user_id)
        .await
        .expect("the board is restored");
    assert_eq!(
        common::sorted_ids(&blocks_of(&test, &board_id).await),
        [card_id.as_str()],
        "only the card went with the board, and only the card is back"
    );
    for block_id in [&text_id, &checkbox_id] {
        test.get_deleted_block(block_id)
            .await
            .expect("what was deleted on its own is still in the trash");
    }

    let restored = test
        .undelete_block(&text_id, &user_id)
        .await
        .expect("the text is restored");
    assert_eq!(restored.parent_id, card_id, "the text is back on its card");
    let back = blocks_of(&test, &board_id).await;
    assert_eq!(
        common::sorted_ids(&back),
        sorted([&card_id, &text_id, &checkbox_id]),
        "the text and the checkbox on it are back"
    );
    let checkbox = back
        .iter()
        .find(|block| block.id == checkbox_id)
        .expect("the checkbox is among the blocks");
    assert_eq!(checkbox.parent_id, text_id, "the checkbox is on the text");

    test.teardown().await;
}

/// Only what is in the trash is restored: a board that exists, an identifier no board
/// has had and text that is no identifier are not found, and nothing is written.
#[tokio::test]
async fn undelete_board_of_what_is_not_in_the_trash_is_not_found_and_writes_nothing() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let other_user_id = common::user(&test, "other-user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;
    let mut card = new_block(&board_id, "", BlockType::Card);
    test.insert_block(&mut card, &user_id)
        .await
        .expect("the card is stored");
    let board_before = test.get_board(&board_id).await.expect("the board exists");
    let blocks_before = blocks_of(&test, &board_id).await;

    // A later modification time would show.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let error = test
        .undelete_board(&board_id, &other_user_id)
        .await
        .expect_err("a board that exists is not in the trash");
    assert!(error.is_not_found(), "got {error:?}");
    let board_after = test.get_board(&board_id).await.expect("the board exists");
    assert_eq!(board_after, board_before, "the board is as it was");
    assert_eq!(
        blocks_of(&test, &board_id).await,
        blocks_before,
        "its blocks are as they were"
    );

    for unknown_id in [new_id(), NOT_AN_ID.to_owned()] {
        let error = test
            .undelete_board(&unknown_id, &user_id)
            .await
            .expect_err("no such board is in the trash");
        assert!(error.is_not_found(), "board {unknown_id}: got {error:?}");
        let error = test
            .get_board(&unknown_id)
            .await
            .expect_err("no board was made");
        assert!(error.is_not_found(), "board {unknown_id}: got {error:?}");
        let error = test
            .get_deleted_board(&unknown_id)
            .await
            .expect_err("nothing was put in the trash");
        assert!(error.is_not_found(), "board {unknown_id}: got {error:?}");
    }
    let boards = test
        .get_boards_for_user_and_team(&user_id, &team_id, PublicBoards::Include)
        .await
        .expect("the boards are read");
    assert_eq!(
        sorted_board_ids(&boards),
        [board_id.as_str()],
        "the one board of the team"
    );

    test.teardown().await;
}

/// Stores a board with a card and a text on the card, and returns the identifiers of
/// the board, the card and the text.
async fn board_with_card_and_text(
    store: &Store,
    team_id: &str,
    user_id: &str,
) -> (String, String, String) {
    let board_id = stored_board(store, team_id, user_id).await;
    let card = new_block(&board_id, "", BlockType::Card);
    let text = new_block(&board_id, &card.id, BlockType::Text);
    let (card_id, text_id) = (card.id.clone(), text.id.clone());
    store
        .insert_blocks(&mut [card, text], user_id)
        .await
        .expect("the blocks are stored");
    (board_id, card_id, text_id)
}

/// When a board in the trash was deleted, in milliseconds since the Unix epoch.
async fn deleted_at(store: &Store, board_id: &str) -> i64 {
    store
        .get_deleted_board(board_id)
        .await
        .expect("the board is in the trash")
        .board
        .delete_at
}

#[tokio::test]
async fn purge_trash_removes_an_old_deleted_board_with_its_blocks_and_keeps_newer_trash() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let (old_id, old_card_id, old_text_id) =
        board_with_card_and_text(&test, &team_id, &user_id).await;
    let (newer_id, newer_card_id, newer_text_id) =
        board_with_card_and_text(&test, &team_id, &user_id).await;

    test.delete_board(&old_id, &user_id)
        .await
        .expect("the old board is deleted");
    let cutoff = deleted_at(&test, &old_id).await + 1;
    // What follows is deleted at the cutoff or later.
    tokio::time::sleep(Duration::from_millis(5)).await;
    test.delete_board(&newer_id, &user_id)
        .await
        .expect("the newer board is deleted");

    let purged = test.purge_trash(cutoff).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(3),
        "the old board and its two blocks"
    );

    let error = test
        .get_deleted_board(&old_id)
        .await
        .expect_err("the old board is gone for good");
    assert!(error.is_not_found(), "got {error:?}");
    for block_id in [&old_card_id, &old_text_id] {
        let error = test
            .get_deleted_block(block_id)
            .await
            .expect_err("the blocks of the old board are gone for good");
        assert!(error.is_not_found(), "block {block_id}: got {error:?}");
    }
    let error = test
        .undelete_board(&old_id, &user_id)
        .await
        .expect_err("there is nothing to restore");
    assert!(error.is_not_found(), "got {error:?}");

    test.undelete_board(&newer_id, &user_id)
        .await
        .expect("what is newer than the cutoff is restored");
    assert_eq!(
        common::sorted_ids(&blocks_of(&test, &newer_id).await),
        sorted([&newer_card_id, &newer_text_id]),
        "the blocks of the newer board are back"
    );

    test.teardown().await;
}

#[tokio::test]
async fn purge_trash_removes_a_block_deleted_on_its_own_with_its_purged_board() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let (board_id, card_id, text_id) = board_with_card_and_text(&test, &team_id, &user_id).await;

    test.delete_block(&text_id, &user_id)
        .await
        .expect("the text is deleted on its own");
    test.delete_board(&board_id, &user_id)
        .await
        .expect("the board is deleted");
    let cutoff = deleted_at(&test, &board_id).await + 1;

    let purged = test.purge_trash(cutoff).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(3),
        "the board, its card and the text deleted before"
    );
    for block_id in [&card_id, &text_id] {
        let error = test
            .get_deleted_block(block_id)
            .await
            .expect_err("the block is gone for good");
        assert!(error.is_not_found(), "block {block_id}: got {error:?}");
    }
    let again = test
        .purge_trash(cutoff)
        .await
        .expect("the trash is purged again");
    assert_eq!(
        again,
        common::purged_without_files(0),
        "nothing is left to purge"
    );

    test.teardown().await;
}

/// The cutoff itself is not old: what was deleted at that very millisecond stays.
#[tokio::test]
async fn purge_trash_keeps_what_was_deleted_at_the_cutoff_or_later() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let (kept_board_id, kept_card_id, lone_text_id) =
        board_with_card_and_text(&test, &team_id, &user_id).await;
    let (board_id, card_id, text_id) = board_with_card_and_text(&test, &team_id, &user_id).await;

    // A block deleted on its own on a board that stays, and a board deleted whole.
    test.delete_block(&lone_text_id, &user_id)
        .await
        .expect("the text is deleted");
    let lone_deleted_at = test
        .get_deleted_block(&lone_text_id)
        .await
        .expect("the text is in the trash")
        .block
        .delete_at;
    test.delete_board(&board_id, &user_id)
        .await
        .expect("the board is deleted");
    let cutoff = lone_deleted_at.min(deleted_at(&test, &board_id).await);

    let purged = test.purge_trash(cutoff).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(0),
        "nothing was deleted before the cutoff {cutoff}"
    );

    let restored = test
        .undelete_block(&lone_text_id, &user_id)
        .await
        .expect("the text is restored");
    assert_eq!(
        restored.parent_id, kept_card_id,
        "the text is back on its card"
    );
    assert_eq!(
        common::sorted_ids(&blocks_of(&test, &kept_board_id).await),
        sorted([&kept_card_id, &lone_text_id]),
        "the board that stayed has both blocks again"
    );
    test.undelete_board(&board_id, &user_id)
        .await
        .expect("the board is restored");
    assert_eq!(
        common::sorted_ids(&blocks_of(&test, &board_id).await),
        sorted([&card_id, &text_id]),
        "the blocks of the deleted board are back"
    );

    test.teardown().await;
}

/// A built-in template is removed past the trash. A block of it that was deleted before
/// has nothing to come back to, and goes with the next purge however new it is.
#[tokio::test]
async fn purge_trash_removes_deleted_blocks_of_a_board_that_is_gone_for_good() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    let mut template = Board {
        is_template: true,
        ..new_board(&new_id(), &team_id, BoardType::Open)
    };
    test.insert_board(&mut template, "")
        .await
        .expect("the built-in template is stored");
    let mut orphan = new_block(&template.id, "", BlockType::Card);
    test.insert_block(&mut orphan, "")
        .await
        .expect("the card of the template is stored");
    let (_, _, kept_text_id) = board_with_card_and_text(&test, &team_id, &user_id).await;

    test.delete_block(&orphan.id, &user_id)
        .await
        .expect("the card of the template is deleted");
    test.delete_block(&kept_text_id, &user_id)
        .await
        .expect("the text of the other board is deleted");
    test.remove_builtin_templates()
        .await
        .expect("the built-in templates are removed");
    test.get_deleted_block(&orphan.id)
        .await
        .expect("the card is still in the trash");

    // Nothing is older than the beginning of time.
    let purged = test.purge_trash(0).await.expect("the trash is purged");
    assert_eq!(
        purged,
        common::purged_without_files(1),
        "the card whose board is gone for good"
    );
    let error = test
        .get_deleted_block(&orphan.id)
        .await
        .expect_err("the card is gone for good");
    assert!(error.is_not_found(), "got {error:?}");
    test.undelete_block(&kept_text_id, &user_id)
        .await
        .expect("a deleted block of a board that exists stays, and is restored");

    test.teardown().await;
}

/// A file record of the board, not stored yet.
fn file_of(board_id: &str, stored_name: &str) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        name: format!("uploaded as {stored_name}"),
        extension: ".png".to_owned(),
        size: 2048,
        path: format!("boards/{board_id}/{stored_name}"),
        create_at: 1,
    }
}

/// A purge hands back the file records of the boards it removed for good, whole, and of
/// no other board: nothing else says where those files are.
#[tokio::test]
async fn purge_trash_returns_the_file_records_of_the_boards_it_removed() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let (old_id, ..) = board_with_card_and_text(&test, &team_id, &user_id).await;
    let (newer_id, ..) = board_with_card_and_text(&test, &team_id, &user_id).await;
    let (kept_id, ..) = board_with_card_and_text(&test, &team_id, &user_id).await;

    let mut of_old = vec![file_of(&old_id, "7a.png"), file_of(&old_id, "7b.png")];
    of_old.sort_by(|a, b| a.id.cmp(&b.id));
    let of_newer = file_of(&newer_id, "7a.png");
    let of_kept = file_of(&kept_id, "7a.png");
    for file in of_old.iter().chain([&of_newer, &of_kept]) {
        test.save_file_info(file)
            .await
            .expect("the file record is stored");
    }

    test.delete_board(&old_id, &user_id)
        .await
        .expect("the old board is deleted");
    let cutoff = deleted_at(&test, &old_id).await + 1;
    // What follows is deleted at the cutoff or later.
    tokio::time::sleep(Duration::from_millis(5)).await;
    test.delete_board(&newer_id, &user_id)
        .await
        .expect("the newer board is deleted");

    let Purged { records, mut files } =
        test.purge_trash(cutoff).await.expect("the trash is purged");
    files.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(records, 3, "the old board and its two blocks");
    assert_eq!(
        files, of_old,
        "the file records of the board that went, as they were stored"
    );

    // The files of the board that is still in the trash come back with it, and those
    // of the board that exists were never touched.
    test.undelete_board(&newer_id, &user_id)
        .await
        .expect("what is newer than the cutoff is restored");
    for file in [&of_newer, &of_kept] {
        let stored = test
            .get_file_info(&file.board_id, &file.stored_name)
            .await
            .expect("the file record is there");
        assert_eq!(&stored, file, "as it was stored");
    }
    let again = test
        .purge_trash(cutoff)
        .await
        .expect("the trash is purged again");
    assert_eq!(
        again,
        common::purged_without_files(0),
        "nothing is left to purge, and no file record is handed back twice"
    );

    test.teardown().await;
}

// Membership changes that say what they did, and that keep a board its administrator.
// The original checks for the last administrator outside the store, without holding
// anything, and does not test the store's part.

#[tokio::test]
async fn add_member_stores_a_new_membership_and_returns_none() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    let member = BoardMember {
        scheme_commenter: true,
        scheme_viewer: true,
        ..editor_of(&board_id, &user_id)
    };
    let existing = test.add_member(&member).await.expect("the member is added");
    assert_eq!(existing, None, "there was no membership before");

    assert_eq!(
        members_of(&test, &board_id).await,
        [member],
        "the membership as it was given"
    );

    test.teardown().await;
}

#[tokio::test]
async fn add_member_returns_the_existing_membership_and_changes_nothing() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;
    let stored = editor_of(&board_id, &user_id);
    test.save_member(&stored)
        .await
        .expect("the member is stored");

    // The same flags again, and others: more rights and fewer.
    let offers = [
        editor_of(&board_id, &user_id),
        admin_of(&board_id, &user_id),
        BoardMember {
            scheme_editor: false,
            scheme_viewer: true,
            ..editor_of(&board_id, &user_id)
        },
    ];
    for offer in offers {
        let existing = test
            .add_member(&offer)
            .await
            .expect("adding a member again succeeds");
        assert_eq!(
            existing.as_ref(),
            Some(&stored),
            "the membership as it stands, whatever was offered: {offer:?}"
        );
        assert_eq!(
            members_of(&test, &board_id).await,
            std::slice::from_ref(&stored),
            "the stored membership is unchanged by the offer {offer:?}"
        );
    }

    test.teardown().await;
}

#[tokio::test]
async fn add_member_to_a_board_or_of_a_user_that_does_not_exist_is_an_error() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    // An identifier nothing has is not found, and the refusal says what is missing.
    for (member, missing) in [
        (editor_of(&new_id(), &user_id), "board"),
        (editor_of(&board_id, &new_id()), "user"),
    ] {
        let error = test
            .add_member(&member)
            .await
            .expect_err("a membership needs its board and its user");
        assert!(
            matches!(&error, StoreError::NotFound(what) if what.contains(missing)),
            "{member:?}: no such {missing}: got {error:?}"
        );
        assert!(error.is_not_found(), "{member:?}: got {error:?}");
    }
    // Text that is no identifier names no board, as an identifier nothing has. For
    // the user of a membership it is refused as what it is.
    let error = test
        .add_member(&editor_of(NOT_AN_ID, &user_id))
        .await
        .expect_err("text that is no identifier names no board");
    assert!(
        matches!(&error, StoreError::NotFound(what) if what.contains("board")),
        "got {error:?}"
    );
    let error = test
        .add_member(&editor_of(&board_id, NOT_AN_ID))
        .await
        .expect_err("text that is no identifier is no user");
    assert!(
        matches!(&error, StoreError::InvalidId(invalid) if invalid == NOT_AN_ID),
        "got {error:?}"
    );

    let members = members_of(&test, &board_id).await;
    assert!(members.is_empty(), "the board got a member: {members:?}");
    let memberships = test
        .get_members_for_user(&user_id)
        .await
        .expect("the memberships are read");
    assert!(
        memberships.is_empty(),
        "the user got a membership: {memberships:?}"
    );

    test.teardown().await;
}

/// How many boards the tests of races go through.
const RACE_ROUNDS: usize = 25;

/// How many requests add the same user to the same board at once.
const RACING_ADDERS: u8 = 8;

/// Two requests that add the same user at once end in one membership, the one that was
/// stored first; every other request is told of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_add_member_calls_end_in_one_membership_and_one_of_them_added_it() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    for round in 0..RACE_ROUNDS {
        let board_id = stored_board(&test, &team_id, &user_id).await;
        let mut adders = JoinSet::new();
        for number in 1..=RACING_ADDERS {
            // No two requests offer the same flags.
            let offer = BoardMember {
                board_id: board_id.clone(),
                user_id: user_id.clone(),
                scheme_admin: number & 1 != 0,
                scheme_editor: number & 2 != 0,
                scheme_commenter: number & 4 != 0,
                scheme_viewer: number & 8 != 0,
                ..BoardMember::default()
            };
            let store = test.store.clone();
            adders.spawn(async move {
                let existing = store
                    .add_member(&offer)
                    .await
                    .expect("adding a member succeeds");
                (offer, existing)
            });
        }
        let outcomes = adders.join_all().await;

        let members = members_of(&test, &board_id).await;
        assert_eq!(
            members.len(),
            1,
            "round {round}: one membership: {members:?}"
        );
        let stored = &members[0];
        let added: Vec<&BoardMember> = outcomes
            .iter()
            .filter(|(_, existing)| existing.is_none())
            .map(|(offer, _)| offer)
            .collect();
        assert_eq!(
            added,
            [stored],
            "round {round}: exactly one request added the membership, and it is the stored one"
        );
        for (offer, existing) in &outcomes {
            if let Some(existing) = existing {
                assert_eq!(
                    existing, stored,
                    "round {round}: the request offering {offer:?} was told of the stored one"
                );
            }
        }
    }

    test.teardown().await;
}

#[tokio::test]
async fn change_member_replaces_the_flags_of_a_membership_and_of_no_one_else() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let member_id = common::user(&test, "member").await;
    let stranger_id = common::user(&test, "stranger").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &admin_id).await;
    for member in [
        admin_of(&board_id, &admin_id),
        editor_of(&board_id, &member_id),
    ] {
        test.save_member(&member)
            .await
            .expect("the member is stored");
    }

    // Up to administrator and down again: there is another one.
    let viewer = BoardMember {
        scheme_editor: false,
        scheme_viewer: true,
        ..editor_of(&board_id, &member_id)
    };
    for change in [admin_of(&board_id, &member_id), viewer] {
        let outcome = test
            .change_member(&change)
            .await
            .expect("the membership is changed");
        assert_eq!(outcome, MemberChange::Done, "the change to {change:?}");
        let stored = test
            .get_member_for_board(&board_id, &member_id)
            .await
            .expect("the membership exists");
        assert_eq!(stored, change, "the flags are the given ones");
    }

    // Who is no member is not made one: on this board, on a board that does not exist
    // and on text that is no identifier.
    for change in [
        admin_of(&board_id, &stranger_id),
        admin_of(&new_id(), &member_id),
        admin_of(NOT_AN_ID, &member_id),
    ] {
        let outcome = test
            .change_member(&change)
            .await
            .expect("changing no membership succeeds");
        assert_eq!(
            outcome,
            MemberChange::NotAMember,
            "the change to {change:?}"
        );
    }
    assert_eq!(
        sorted_user_ids(&members_of(&test, &board_id).await),
        sorted([&admin_id, &member_id]),
        "the board has the members it had"
    );
    let of_stranger = test
        .get_members_for_user(&stranger_id)
        .await
        .expect("the memberships are read");
    assert!(of_stranger.is_empty(), "got {of_stranger:?}");

    test.teardown().await;
}

#[tokio::test]
async fn change_member_refuses_to_take_the_role_from_the_only_administrator() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let editor_id = common::user(&test, "editor").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &admin_id).await;
    // Other members do not count, only other administrators.
    for member in [
        admin_of(&board_id, &admin_id),
        editor_of(&board_id, &editor_id),
    ] {
        test.save_member(&member)
            .await
            .expect("the member is stored");
    }

    let outcome = test
        .change_member(&editor_of(&board_id, &admin_id))
        .await
        .expect("the change is answered");
    assert_eq!(outcome, MemberChange::LastAdmin, "stepping down is refused");
    let stored = test
        .get_member_for_board(&board_id, &admin_id)
        .await
        .expect("the membership exists");
    assert_eq!(stored, admin_of(&board_id, &admin_id), "nothing changed");

    // A change that leaves the role alone is made.
    let also_viewer = BoardMember {
        scheme_viewer: true,
        ..admin_of(&board_id, &admin_id)
    };
    let outcome = test
        .change_member(&also_viewer)
        .await
        .expect("the membership is changed");
    assert_eq!(outcome, MemberChange::Done, "the administrator stays one");
    let stored = test
        .get_member_for_board(&board_id, &admin_id)
        .await
        .expect("the membership exists");
    assert_eq!(stored, also_viewer, "the flags are the given ones");

    test.teardown().await;
}

#[tokio::test]
async fn remove_member_removes_a_member_and_refuses_the_only_administrator() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let editor_id = common::user(&test, "editor").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &admin_id).await;
    for member in [
        admin_of(&board_id, &admin_id),
        editor_of(&board_id, &editor_id),
    ] {
        test.save_member(&member)
            .await
            .expect("the member is stored");
    }

    let outcome = test
        .remove_member(&board_id, &admin_id)
        .await
        .expect("the removal is answered");
    assert_eq!(outcome, MemberChange::LastAdmin, "leaving is refused");
    assert_eq!(
        sorted_user_ids(&members_of(&test, &board_id).await),
        sorted([&admin_id, &editor_id]),
        "nobody left"
    );

    let outcome = test
        .remove_member(&board_id, &editor_id)
        .await
        .expect("the member is removed");
    assert_eq!(outcome, MemberChange::Done, "an editor may leave");
    assert_eq!(
        members_of(&test, &board_id).await,
        [admin_of(&board_id, &admin_id)],
        "the administrator is left"
    );

    // Who is no member (any more) is not removed: from this board, from a board that
    // does not exist and from text that is no identifier.
    for board in [board_id.clone(), new_id(), NOT_AN_ID.to_owned()] {
        let outcome = test
            .remove_member(&board, &editor_id)
            .await
            .expect("removing no member succeeds");
        assert_eq!(outcome, MemberChange::NotAMember, "board {board}");
    }

    test.teardown().await;
}

#[tokio::test]
async fn an_administrator_may_step_down_or_leave_while_another_one_exists() {
    let test = common::setup().await;
    let first_id = common::user(&test, "first").await;
    let second_id = common::user(&test, "second").await;
    let third_id = common::user(&test, "third").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &first_id).await;
    for user_id in [&first_id, &second_id, &third_id] {
        test.save_member(&admin_of(&board_id, user_id))
            .await
            .expect("the administrator is stored");
    }

    let outcome = test
        .change_member(&editor_of(&board_id, &first_id))
        .await
        .expect("the membership is changed");
    assert_eq!(outcome, MemberChange::Done, "two administrators are left");
    let outcome = test
        .remove_member(&board_id, &second_id)
        .await
        .expect("the member is removed");
    assert_eq!(outcome, MemberChange::Done, "one administrator is left");
    assert_eq!(
        members_of(&test, &board_id).await,
        sorted_members(vec![
            editor_of(&board_id, &first_id),
            admin_of(&board_id, &third_id)
        ]),
        "an editor and the administrator who stayed"
    );

    // The one who stayed is the last, and is held to it either way.
    let outcome = test
        .change_member(&editor_of(&board_id, &third_id))
        .await
        .expect("the change is answered");
    assert_eq!(outcome, MemberChange::LastAdmin, "stepping down is refused");
    let outcome = test
        .remove_member(&board_id, &third_id)
        .await
        .expect("the removal is answered");
    assert_eq!(outcome, MemberChange::LastAdmin, "leaving is refused");

    test.teardown().await;
}

#[tokio::test]
async fn remove_member_ends_the_subscriptions_of_the_user_to_the_blocks_of_the_board() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let member_id = common::user(&test, "member").await;
    let team_id = common::team(&test).await;
    let (left_board_id, left_card_id, _) =
        board_with_card_and_text(&test, &team_id, &admin_id).await;
    let (other_board_id, other_card_id, _) =
        board_with_card_and_text(&test, &team_id, &admin_id).await;
    for board_id in [&left_board_id, &other_board_id] {
        test.save_member(&admin_of(board_id, &admin_id))
            .await
            .expect("the administrator is stored");
        test.save_member(&editor_of(board_id, &member_id))
            .await
            .expect("the member is stored");
    }
    for card_id in [&left_card_id, &other_card_id] {
        subscribe(&test, card_id, &member_id).await;
        subscribe(&test, card_id, &admin_id).await;
    }

    let outcome = test
        .remove_member(&left_board_id, &member_id)
        .await
        .expect("the member is removed");
    assert_eq!(outcome, MemberChange::Done, "the member left the board");

    assert_eq!(
        subscribed_block_ids(&test, &member_id).await,
        [other_card_id.as_str()],
        "the subscription to the card of the board the user is still on"
    );
    assert_eq!(
        subscribed_block_ids(&test, &admin_id).await,
        sorted([&left_card_id, &other_card_id]),
        "the subscriptions of who stayed are untouched"
    );

    test.teardown().await;
}

/// How an administrator gives up the role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GiveUp {
    /// Stays a member, as an editor.
    StepDown,
    /// Leaves the board.
    Leave,
}

impl GiveUp {
    async fn run(self, store: &Store, board_id: &str, user_id: &str) -> MemberChange {
        let outcome = match self {
            Self::StepDown => store.change_member(&editor_of(board_id, user_id)).await,
            Self::Leave => store.remove_member(board_id, user_id).await,
        };
        outcome.expect("giving up the role is answered")
    }
}

/// The check for another administrator and the write are one step: of the two
/// administrators of a board who give up the role at once, however each does it, one is
/// refused and stays the administrator.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn of_two_administrators_who_give_up_the_role_at_once_one_is_refused() {
    let test = common::setup().await;
    let first_id = common::user(&test, "first").await;
    let second_id = common::user(&test, "second").await;
    let editor_id = common::user(&test, "editor").await;
    let team_id = common::team(&test).await;

    let pairs = [
        (GiveUp::StepDown, GiveUp::StepDown),
        (GiveUp::Leave, GiveUp::Leave),
        (GiveUp::StepDown, GiveUp::Leave),
    ];
    for (round, (first_way, second_way)) in
        pairs.into_iter().cycle().take(3 * RACE_ROUNDS).enumerate()
    {
        let board_id = stored_board(&test, &team_id, &first_id).await;
        for member in [
            admin_of(&board_id, &first_id),
            admin_of(&board_id, &second_id),
            editor_of(&board_id, &editor_id),
        ] {
            test.save_member(&member)
                .await
                .expect("the member is stored");
        }

        let mut racers = JoinSet::new();
        for (way, user_id) in [(first_way, &first_id), (second_way, &second_id)] {
            let store = test.store.clone();
            let board_id = board_id.clone();
            let user_id = user_id.clone();
            racers.spawn(async move {
                let outcome = way.run(&store, &board_id, &user_id).await;
                (user_id, outcome)
            });
        }
        let outcomes = racers.join_all().await;

        let what = format!("round {round}, {first_way:?} against {second_way:?}");
        let refused: Vec<&str> = outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == MemberChange::LastAdmin)
            .map(|(user_id, _)| user_id.as_str())
            .collect();
        let done = outcomes
            .iter()
            .filter(|(_, outcome)| *outcome == MemberChange::Done)
            .count();
        assert_eq!(
            (done, refused.len()),
            (1, 1),
            "{what}: one gave up the role and one was refused: {outcomes:?}"
        );
        let members = members_of(&test, &board_id).await;
        let admins: Vec<&str> = members
            .iter()
            .filter(|member| member.scheme_admin)
            .map(|member| member.user_id.as_str())
            .collect();
        assert_eq!(
            admins, refused,
            "{what}: the one who was refused is the administrator: {members:?}"
        );
    }

    test.teardown().await;
}

// -- joining with the minimum role ------------------------------------------------------

/// The scheme flags of a membership: admin, editor, commenter, viewer.
fn flags(member: &BoardMember) -> [bool; 4] {
    [
        member.scheme_admin,
        member.scheme_editor,
        member.scheme_commenter,
        member.scheme_viewer,
    ]
}

const VIEWER: [bool; 4] = [false, false, false, true];
const COMMENTER: [bool; 4] = [false, false, true, false];
const EDITOR: [bool; 4] = [false, true, false, false];

/// Joining gives the board's minimum role to a user who is no member, raises a member
/// who is below it, and leaves alone a member who has it or more. It says which.
#[tokio::test]
async fn join_board_gives_the_minimum_role_raises_to_it_and_never_lowers() {
    let test = common::setup().await;
    let owner_id = common::user(&test, "owner").await;
    let user_id = common::user(&test, "joiner").await;
    let team_id = common::team(&test).await;

    for (minimum_role, expected) in [
        (MinimumRole::Viewer, VIEWER),
        (MinimumRole::Commenter, COMMENTER),
        (MinimumRole::Editor, EDITOR),
    ] {
        let board_id = new_id();
        let mut board = Board {
            minimum_role,
            ..new_board(&board_id, &team_id, BoardType::Open)
        };
        test.insert_board_with_admin(&mut board, &owner_id)
            .await
            .expect("the board is stored");

        let joined = test.join_board(&board_id, &user_id).await.expect("joined");
        assert!(
            matches!(joined, Joined::New(_)),
            "{minimum_role:?}: {joined:?}"
        );
        assert_eq!(flags(&joined.into_member()), expected, "{minimum_role:?}");
        let again = test.join_board(&board_id, &user_id).await.expect("joined");
        assert!(
            matches!(again, Joined::Already(_)),
            "{minimum_role:?}: {again:?}"
        );
        let stored = test
            .get_member_for_board(&board_id, &user_id)
            .await
            .expect("the membership is stored");
        assert_eq!(flags(&stored), expected, "{minimum_role:?}: as stored");

        // A member without any role is below every minimum role.
        test.save_member(&BoardMember {
            board_id: board_id.clone(),
            user_id: user_id.clone(),
            ..BoardMember::default()
        })
        .await
        .expect("the roles are taken away");
        let raised = test.join_board(&board_id, &user_id).await.expect("joined");
        assert!(
            matches!(raised, Joined::Raised(_)),
            "{minimum_role:?}: {raised:?}"
        );
        assert_eq!(flags(&raised.into_member()), expected, "{minimum_role:?}");

        // The owner is the administrator and stays it.
        let owner = test.join_board(&board_id, &owner_id).await.expect("joined");
        assert!(matches!(owner, Joined::Already(_)), "{owner:?}");
        assert!(owner.into_member().scheme_admin, "not lowered");
    }

    // A board whose minimum role is commenter: a viewer is raised, an editor is not.
    let board_id = new_id();
    let mut board = Board {
        minimum_role: MinimumRole::Commenter,
        ..new_board(&board_id, &team_id, BoardType::Open)
    };
    test.insert_board_with_admin(&mut board, &owner_id)
        .await
        .expect("the board is stored");
    for (before, outcome_is_raise, after) in [
        (VIEWER, true, COMMENTER),
        (EDITOR, false, EDITOR),
        // Several flags at once count as the highest of them.
        (
            [false, true, false, true],
            false,
            [false, true, false, true],
        ),
    ] {
        test.save_member(&BoardMember {
            board_id: board_id.clone(),
            user_id: user_id.clone(),
            scheme_editor: before[1],
            scheme_commenter: before[2],
            scheme_viewer: before[3],
            ..BoardMember::default()
        })
        .await
        .expect("the member is stored");
        let joined = test.join_board(&board_id, &user_id).await.expect("joined");
        assert_eq!(
            matches!(joined, Joined::Raised(_)),
            outcome_is_raise,
            "{before:?}: {joined:?}"
        );
        assert_eq!(flags(&joined.into_member()), after, "{before:?}");
    }

    // What does not exist is not joined.
    let error = test
        .join_board(&new_id(), &user_id)
        .await
        .expect_err("no such board");
    assert!(error.is_not_found(), "{error:?}");
    let error = test
        .join_board(&board_id, &new_id())
        .await
        .expect_err("no such user");
    assert!(
        matches!(&error, StoreError::NotFound(what) if what == "user"),
        "{error:?}"
    );

    test.teardown().await;
}

/// The schema keeps what the model promises: a board's minimum role is one of the three,
/// never empty and never admin, whoever writes the row.
#[tokio::test]
async fn the_schema_refuses_a_minimum_role_that_is_none() {
    let test = common::setup().await;
    let user_id = common::user(&test, "owner").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    for role in ["", "admin", "owner"] {
        let refused = common::try_execute_raw(
            &test.dbconfig(),
            &format!("UPDATE boards SET minimum_role = '{role}' WHERE id = '{board_id}'"),
        )
        .await;
        assert!(refused.is_err(), "{role:?} is stored: {refused:?}");
    }
    for role in ["viewer", "commenter", "editor"] {
        common::execute_raw(
            &test.dbconfig(),
            &format!("UPDATE boards SET minimum_role = '{role}' WHERE id = '{board_id}'"),
        )
        .await;
        let board = test.get_board(board_id).await.expect("the board is read");
        assert_eq!(board.minimum_role.as_str(), role, "as written");
    }

    test.teardown().await;
}

/// Reading the membership and writing it are one step: of users who join a board at
/// once, each ends with one membership with the minimum role, and one call made it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn joining_at_once_makes_one_membership() {
    let test = common::setup().await;
    let owner_id = common::user(&test, "owner").await;
    let user_id = common::user(&test, "joiner").await;
    let team_id = common::team(&test).await;

    for round in 0..RACE_ROUNDS {
        let board_id = stored_board(&test, &team_id, &owner_id).await;
        let mut racers = JoinSet::new();
        for _ in 0..6 {
            let store = test.store.clone();
            let (board_id, user_id) = (board_id.clone(), user_id.clone());
            racers.spawn(async move { store.join_board(&board_id, &user_id).await });
        }
        let outcomes: Vec<Joined> = racers
            .join_all()
            .await
            .into_iter()
            .map(|outcome| outcome.expect("joining succeeds"))
            .collect();

        let new = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Joined::New(_)))
            .count();
        assert_eq!(new, 1, "round {round}: one call made the membership");
        assert!(
            outcomes
                .iter()
                .all(|outcome| matches!(outcome, Joined::New(_) | Joined::Already(_))),
            "round {round}: {outcomes:?}"
        );
        let members = test
            .get_members_for_board(&board_id)
            .await
            .expect("the members are read");
        assert_eq!(
            members
                .iter()
                .filter(|member| member.user_id == user_id)
                .count(),
            1,
            "round {round}: one membership"
        );
    }

    test.teardown().await;
}

// -- writes of one board at once ----------------------------------------------------------

/// A definition of a card property with the given identifier.
fn card_property(id: &str) -> JsonMap {
    object(json!({ "id": id, "name": format!("Property {id}"), "type": "text" }))
}

/// The identifiers of the card property definitions of a board, in their order.
fn card_property_ids(board: &Board) -> Vec<&str> {
    board
        .card_properties
        .iter()
        .filter_map(|definition| definition.get("id").and_then(Value::as_str))
        .collect()
}

/// A patch is applied to the board as the patch before it left it: of two patches at
/// once that each add a card property, both definitions are there afterwards, next to
/// those of every round before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_patches_of_a_board_at_once_both_take_effect() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &user_id).await;

    let mut expected = BTreeSet::new();
    for round in 0..2 * RACE_ROUNDS {
        let start = Arc::new(Barrier::new(2));
        let mut patchers = JoinSet::new();
        for side in ["left", "right"] {
            let property_id = format!("{side}-{round}");
            expected.insert(property_id.clone());
            let (store, start) = (test.store.clone(), Arc::clone(&start));
            let (board_id, user_id) = (board_id.clone(), user_id.clone());
            patchers.spawn(async move {
                let patch = BoardPatch {
                    updated_card_properties: vec![card_property(&property_id)],
                    ..BoardPatch::default()
                };
                start.wait().await;
                store.patch_board(&board_id, &patch, &user_id).await
            });
        }
        for outcome in patchers.join_all().await {
            let patched =
                outcome.unwrap_or_else(|error| panic!("round {round}: a patch failed: {error:?}"));
            assert!(
                card_property_ids(&patched).len() > 2 * round,
                "round {round}: a patch answered a board without the definitions of the \
                 rounds before: {:?}",
                card_property_ids(&patched)
            );
        }

        let board = test.get_board(&board_id).await.expect("the board is read");
        let stored: BTreeSet<String> = card_property_ids(&board)
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            stored, expected,
            "round {round}: the definitions of both patches, and of every round before"
        );
        assert_eq!(
            board.card_properties.len(),
            expected.len(),
            "round {round}: each definition once"
        );
    }

    test.teardown().await;
}

/// A member is added again while being removed. What adding answers is true when both
/// are done: "new" only of a membership that exists (the one that was offered), "there
/// was one" only of the membership that the removal then took. Never "new" with no
/// membership.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn add_member_racing_remove_member_answers_what_is_true() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;
    let board_id = stored_board(&test, &team_id, &admin_id).await;
    test.save_member(&admin_of(&board_id, &admin_id))
        .await
        .expect("the administrator is stored");
    let there_before = editor_of(&board_id, &user_id);
    // Not the flags of the membership that is there: which one is stored shows.
    let offer = BoardMember {
        scheme_editor: false,
        scheme_viewer: true,
        ..editor_of(&board_id, &user_id)
    };

    let (mut new, mut existing) = (0, 0);
    for round in 0..4 * RACE_ROUNDS {
        test.save_member(&there_before)
            .await
            .expect("the member is there before the round");

        let start = Arc::new(Barrier::new(2));
        let adding = tokio::spawn({
            let (store, start, offer) = (test.store.clone(), Arc::clone(&start), offer.clone());
            async move {
                start.wait().await;
                store.add_member(&offer).await
            }
        });
        let removing = tokio::spawn({
            let (store, start) = (test.store.clone(), Arc::clone(&start));
            let (board_id, user_id) = (board_id.clone(), user_id.clone());
            async move {
                start.wait().await;
                store.remove_member(&board_id, &user_id).await
            }
        });
        let added = adding
            .await
            .expect("the adding task runs to its end")
            .unwrap_or_else(|error| panic!("round {round}: adding failed: {error:?}"));
        let removed = removing
            .await
            .expect("the removing task runs to its end")
            .unwrap_or_else(|error| panic!("round {round}: removing failed: {error:?}"));

        assert_eq!(
            removed,
            MemberChange::Done,
            "round {round}: there was a member to remove, before the addition or after it"
        );
        let stored = test.get_member_for_board(&board_id, &user_id).await;
        match added {
            None => {
                new += 1;
                let stored = stored.unwrap_or_else(|error| {
                    panic!("round {round}: \"new\" of a membership that does not exist: {error:?}")
                });
                assert_eq!(
                    stored, offer,
                    "round {round}: the membership that was offered"
                );
            }
            Some(told) => {
                existing += 1;
                assert_eq!(
                    told, there_before,
                    "round {round}: told of the membership that was there"
                );
                let error = stored.expect_err("the removal came after and took the membership");
                assert!(error.is_not_found(), "round {round}: got {error:?}");
            }
        }
    }
    assert_eq!(new + existing, 4 * RACE_ROUNDS, "every round was answered");

    test.teardown().await;
}

/// A user is added to a board while the board is deleted. Either the membership came
/// first, and then it is in the trash with the board and comes back with it, or the
/// board was gone, and then that is a not-found error and there is no membership. It is
/// never stored and lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_member_added_while_the_board_is_deleted_goes_with_the_board_or_is_not_found() {
    let test = common::setup().await;
    let admin_id = common::user(&test, "admin").await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    for round in 0..2 * RACE_ROUNDS {
        let board_id = stored_board(&test, &team_id, &admin_id).await;
        test.save_member(&admin_of(&board_id, &admin_id))
            .await
            .expect("the administrator is stored");
        let late = editor_of(&board_id, &user_id);

        let start = Arc::new(Barrier::new(2));
        let deleting = tokio::spawn({
            let (store, start) = (test.store.clone(), Arc::clone(&start));
            let (board_id, admin_id) = (board_id.clone(), admin_id.clone());
            async move {
                start.wait().await;
                store.delete_board(&board_id, &admin_id).await
            }
        });
        let adding = tokio::spawn({
            let (store, start, late) = (test.store.clone(), Arc::clone(&start), late.clone());
            async move {
                start.wait().await;
                store.add_member(&late).await
            }
        });
        deleting
            .await
            .expect("the deleting task runs to its end")
            .unwrap_or_else(|error| panic!("round {round}: the board is deleted: {error:?}"));
        let added = adding.await.expect("the adding task runs to its end");

        let left = members_of(&test, &board_id).await;
        assert!(
            left.is_empty(),
            "round {round}: memberships of a deleted board are left: {left:?}"
        );
        let in_trash = test
            .get_deleted_board(&board_id)
            .await
            .expect("the board is in the trash")
            .members;
        let expected = match &added {
            Ok(None) => vec![admin_of(&board_id, &admin_id), late.clone()],
            Err(error) => {
                assert!(
                    matches!(error, StoreError::NotFound(_)),
                    "round {round}: a member who comes after the board is gone is a \
                     not-found error, got {error:?}"
                );
                vec![admin_of(&board_id, &admin_id)]
            }
            Ok(Some(member)) => {
                panic!("round {round}: told of a membership nobody stored: {member:?}")
            }
        };
        let expected = sorted_members(expected);
        assert_eq!(
            sorted_members(in_trash),
            expected,
            "round {round}: the memberships in the trash, after adding answered {added:?}"
        );

        test.undelete_board(&board_id, &admin_id)
            .await
            .unwrap_or_else(|error| panic!("round {round}: the board is restored: {error:?}"));
        assert_eq!(
            members_of(&test, &board_id).await,
            expected,
            "round {round}: the memberships that came back, after adding answered {added:?}"
        );
    }

    test.teardown().await;
}

/// A file record is stored for a board while the board is deleted. Either the record
/// came first, and then it is in the trash with the board and comes back with it, or
/// the board was gone, and then that is a not-found error and there is no record. It is
/// never stored and lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_saved_while_the_board_is_deleted_goes_with_the_board_or_is_not_found() {
    let test = common::setup().await;
    let user_id = common::user(&test, "user").await;
    let team_id = common::team(&test).await;

    for round in 0..2 * RACE_ROUNDS {
        let board_id = stored_board(&test, &team_id, &user_id).await;
        let late = file_of(&board_id, "7late.png");

        let start = Arc::new(Barrier::new(2));
        let deleting = tokio::spawn({
            let (store, start) = (test.store.clone(), Arc::clone(&start));
            let (board_id, user_id) = (board_id.clone(), user_id.clone());
            async move {
                start.wait().await;
                store.delete_board(&board_id, &user_id).await
            }
        });
        let saving = tokio::spawn({
            let (store, start, late) = (test.store.clone(), Arc::clone(&start), late.clone());
            async move {
                start.wait().await;
                store.save_file_info(&late).await
            }
        });
        deleting
            .await
            .expect("the deleting task runs to its end")
            .unwrap_or_else(|error| panic!("round {round}: the board is deleted: {error:?}"));
        let saved = saving.await.expect("the saving task runs to its end");

        let error = test
            .get_file_info(&board_id, &late.stored_name)
            .await
            .expect_err("a deleted board has no file records");
        assert!(error.is_not_found(), "round {round}: got {error:?}");
        if let Err(error) = &saved {
            assert!(
                matches!(error, StoreError::NotFound(_)),
                "round {round}: a file record that comes after the board is gone is a \
                 not-found error, got {error:?}"
            );
        }

        test.undelete_board(&board_id, &user_id)
            .await
            .unwrap_or_else(|error| panic!("round {round}: the board is restored: {error:?}"));
        let back = test.get_file_info(&board_id, &late.stored_name).await;
        match (&saved, back) {
            (Ok(()), Ok(back)) => {
                assert_eq!(back, late, "round {round}: the record came back as it was");
            }
            (Err(_), Err(error)) => {
                assert!(error.is_not_found(), "round {round}: got {error:?}");
            }
            (Ok(()), Err(error)) => {
                panic!("round {round}: the record was stored and is lost: {error:?}")
            }
            (Err(refusal), Ok(back)) => {
                panic!("round {round}: the record was refused ({refusal:?}) and is there: {back:?}")
            }
        }
    }

    test.teardown().await;
}

// -- the trash keeps everything ---------------------------------------------------------

/// The members of `value`, by their names in its JSON form, that are what they are in
/// `default`.
fn members_at_their_default<T: Serialize>(value: &T, default: &T) -> Vec<String> {
    let members = |of: &T| match serde_json::to_value(of).expect("the value has a JSON form") {
        Value::Object(members) => members,
        other => panic!("the JSON form is an object, not {other}"),
    };
    let default = members(default);
    members(value)
        .into_iter()
        .filter(|(name, member)| default.get(name) == Some(member))
        .map(|(name, _)| name)
        .collect()
}

/// What goes to the trash comes back whole. The trash keeps a board, its memberships
/// and its blocks as JSON, so a member that the JSON form of one of them left out would
/// come back empty, and only here would anybody see it.
///
/// Every member of the board, of a block on it and of a membership is something other
/// than its default before the board is deleted, except what cannot be for a row that
/// exists: `delete_at`, and of a membership `roles` and `synthetic`, which have no
/// column. The literals below name every member on purpose, without `..`: a member
/// that is added later does not compile until it is given a value here.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "one board with every member of every record set, compared member for member"
)]
async fn a_board_comes_back_from_the_trash_with_every_member_it_had() {
    let test = common::setup().await;
    let author_id = common::user(&test, "author").await;
    let restorer_id = common::user(&test, "restorer").await;
    let team_id = common::team(&test).await;

    let mut board = Board {
        id: new_id(),
        team_id: team_id.clone(),
        // Stamped by the store when the board is inserted.
        created_by: String::new(),
        modified_by: String::new(),
        board_type: BoardType::Private,
        minimum_role: MinimumRole::Commenter,
        title: "Roadmap".to_owned(),
        description: "What comes when".to_owned(),
        icon: "🗺".to_owned(),
        show_description: true,
        is_template: true,
        // The store decides this, and only a template that no user made is built-in.
        // Set past the store below, so that this board has a creator as well.
        is_builtin: false,
        template_version: 3,
        properties: object(json!({ "color": "blue" })),
        card_properties: vec![object(json!({
            "id": "status",
            "name": "Status",
            "type": "select",
            "options": [{ "id": "open", "value": "Open", "color": "propColorGreen" }],
        }))],
        create_at: 0,
        update_at: 0,
        delete_at: 0,
    };
    test.insert_board(&mut board, &author_id)
        .await
        .expect("the board is stored");
    common::execute_raw(
        &test.dbconfig(),
        &format!(
            "UPDATE boards SET is_builtin = TRUE WHERE id = '{}'",
            board.id
        ),
    )
    .await;
    for member in [
        BoardMember {
            board_id: board.id.clone(),
            user_id: author_id.clone(),
            roles: String::new(),
            scheme_admin: true,
            scheme_editor: true,
            scheme_commenter: true,
            scheme_viewer: true,
            synthetic: false,
        },
        editor_of(&board.id, &restorer_id),
    ] {
        test.save_member(&member)
            .await
            .expect("the member is stored");
    }
    let card = new_block(&board.id, "", BlockType::Card);
    let mut note = Block {
        id: new_id(),
        parent_id: card.id.clone(),
        // Stamped by the store when the block is inserted.
        created_by: String::new(),
        modified_by: String::new(),
        schema: 7,
        block_type: BlockType::Text,
        title: "a text".to_owned(),
        fields: object(json!({ "contentOrder": ["a", "b"], "isTemplate": true })),
        body: Body::parse(json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "a body"}]},
        ]}))
        .expect("a body that may be stored"),
        create_at: 0,
        update_at: 0,
        delete_at: 0,
        board_id: board.id.clone(),
    };
    test.insert_blocks(&mut [card], &author_id)
        .await
        .expect("the card is stored");
    test.insert_block(&mut note, &author_id)
        .await
        .expect("the text on the card is stored");

    // As stored, nothing that can be set is at its default.
    let board = test.get_board(&board.id).await.expect("the board is read");
    let members = members_of(&test, &board.id).await;
    let blocks = blocks_of(&test, &board.id).await;
    let note = test.get_block(&note.id).await.expect("the text is read");
    assert_eq!(
        members_at_their_default(&board, &Board::default()),
        ["deleteAt"],
        "of the board: {board:?}"
    );
    assert_eq!(
        members_at_their_default(&note, &Block::default()),
        ["deleteAt"],
        "of the block: {note:?}"
    );
    let all_flags = members
        .iter()
        .find(|member| member.user_id == author_id)
        .expect("the author is a member");
    assert_eq!(
        members_at_their_default(all_flags, &BoardMember::default()),
        Vec::<String>::new(),
        "of the membership: {all_flags:?}"
    );

    test.delete_board(&board.id, &author_id)
        .await
        .expect("the board is deleted");
    let returned = test
        .undelete_board(&board.id, &restorer_id)
        .await
        .expect("the board is restored");

    // Restoring changes who modified the board and its blocks last, and when. Nothing
    // else.
    let restored = test.get_board(&board.id).await.expect("the board is back");
    assert_eq!(returned, restored, "what is returned is what is stored");
    assert_eq!(
        restored,
        Board {
            modified_by: restorer_id.clone(),
            update_at: restored.update_at,
            ..board.clone()
        },
        "the board as it was"
    );
    assert_ne!(
        restored.modified_by, board.modified_by,
        "restored by another user than the one who wrote the board"
    );
    assert_eq!(
        members_of(&test, &board.id).await,
        members,
        "the memberships as they were"
    );
    let expected: Vec<Block> = blocks
        .iter()
        .map(|block| Block {
            modified_by: restorer_id.clone(),
            update_at: restored.update_at,
            ..block.clone()
        })
        .collect();
    assert_eq!(
        blocks_of(&test, &board.id).await,
        expected,
        "the blocks as they were"
    );

    test.teardown().await;
}
