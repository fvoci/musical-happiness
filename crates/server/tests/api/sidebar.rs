//! Ports of `server/integrationtests/sidebar_test.go`, `server/app/category_test.go` and
//! `server/app/category_boards_test.go`, plus tests of the category routes that the
//! original has none for: validation, the exact answers, and what one user can do to the
//! categories of another.
//!
//! The original's application tests run against a mocked store. Here they run against a
//! real one, so each checks what the mock sequence stood for.

use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{
    Board, BoardType, Category, CategoryBoardMetadata, CategoryBoards, CategoryType, IdKind,
    SINGLE_USER_ID, new_id,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer};

/// The users of the application-level tests. Categories do not need their user to exist.
const USER_ID: &str = "user_id";
const OTHER_USER_ID: &str = "user_id_2";
const TEAM_ID: &str = "team_id";

fn category(team_id: &str, user_id: &str, name: &str) -> Category {
    Category {
        name: name.to_owned(),
        user_id: user_id.to_owned(),
        team_id: team_id.to_owned(),
        ..Category::default()
    }
}

/// Puts a board into the store without going through the board routes, with `user_id` as
/// its administrator.
async fn board_with_admin(th: &TestServer, team_id: &str, user_id: &str) -> Board {
    let mut board = Board {
        id: new_id(IdKind::Board),
        team_id: team_id.to_owned(),
        board_type: BoardType::Open,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is inserted");
    board
}

/// Puts an open board into the store that `creator_id` created but is not a member of.
async fn board_without_member(th: &TestServer, team_id: &str, creator_id: &str) -> Board {
    let mut board = Board {
        id: new_id(IdKind::Board),
        team_id: team_id.to_owned(),
        board_type: BoardType::Open,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board(&mut board, creator_id)
        .await
        .expect("the board is inserted");
    board
}

fn categories_path(team_id: &str) -> String {
    format!("/teams/{team_id}/categories")
}

/// `GetUserCategoryBoards`
async fn category_boards(client: &Client, team_id: &str) -> Vec<CategoryBoards> {
    client.get(&categories_path(team_id)).await.ok_json()
}

/// `CreateCategory`
async fn create_category(client: &Client, category: &Category) -> Category {
    client
        .post(&categories_path(&category.team_id), category)
        .await
        .ok_json()
}

/// `UpdateCategoryBoard`
async fn file_board(client: &Client, team_id: &str, category_id: &str, board_id: &str) {
    let response = client
        .post_empty(&format!(
            "/teams/{team_id}/categories/{category_id}/boards/{board_id}"
        ))
        .await;
    response.check_ok();
    assert_eq!(response.text(), "ok", "the answer is the bare word");
}

async fn put_empty(client: &Client, path: &str) -> ApiResponse {
    Client::send(client.request(Method::PUT, path)).await
}

fn names(category_boards: &[CategoryBoards]) -> Vec<&str> {
    category_boards
        .iter()
        .map(|entry| entry.category.name.as_str())
        .collect()
}

fn ids(category_boards: &[CategoryBoards]) -> Vec<&str> {
    category_boards
        .iter()
        .map(|entry| entry.category.id.as_str())
        .collect()
}

fn board_ids(entry: &CategoryBoards) -> Vec<&str> {
    entry
        .board_metadata
        .iter()
        .map(|metadata| metadata.board_id.as_str())
        .collect()
}

fn sorted(mut values: Vec<&str>) -> Vec<&str> {
    values.sort_unstable();
    values
}

fn visible(board_id: &str) -> CategoryBoardMetadata {
    CategoryBoardMetadata {
        board_id: board_id.to_owned(),
        hidden: false,
    }
}

/// The one entry with the given category identifier.
#[track_caller]
fn entry<'a>(category_boards: &'a [CategoryBoards], category_id: &str) -> &'a CategoryBoards {
    category_boards
        .iter()
        .find(|entry| entry.category.id == category_id)
        .unwrap_or_else(|| panic!("category {category_id} is listed"))
}

/// The answer the original gives for every error it does not classify.
#[track_caller]
fn check_internal_error(response: &ApiResponse) {
    response.check_status(StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.value(),
        json!({"error": "internal server error", "errorCode": 500}),
        "the cause of a server error is not sent"
    );
}

#[track_caller]
fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_status(status);
    assert_eq!(
        response.value(),
        json!({"error": message, "errorCode": status.as_u16()}),
        "the error answer"
    );
}

// ---------------------------------------------------------------------------------------
// server/integrationtests/sidebar_test.go

/// `TestSidebar`
///
/// Creates its boards through the board routes, which file a new board under the
/// creator's default category. [`sidebar_with_boards_from_the_store`] is the same walk
/// without those routes.
#[tokio::test]
async fn sidebar() {
    let th = Setup::single_user().start().await;

    // A new board ends up in the default "Boards" category.
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the new board");

    // A second board is moved into a new category.
    let board2 = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let created = create_category(
        &th.client,
        &category(TEST_TEAM_ID, SINGLE_USER_ID, "Category 2"),
    )
    .await;
    file_board(&th.client, TEST_TEAM_ID, &created.id, &board2.id).await;

    // New categories end up on top.
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Category 2", "Boards"], "two categories");
    assert_eq!(
        board_ids(&listed[0]),
        [board2.id.as_str()],
        "the moved board"
    );

    // Deleting the category moves its boards to the default one.
    th.client
        .delete(&format!("{}/{}", categories_path(TEST_TEAM_ID), created.id))
        .await
        .check_ok();
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the default category is left");
    assert_eq!(listed[0].board_metadata.len(), 2, "both boards are in it");
    assert!(
        listed[0].board_metadata.contains(&visible(&board.id)),
        "the first board is there"
    );
    assert!(
        listed[0].board_metadata.contains(&visible(&board2.id)),
        "the second board is there"
    );

    th.teardown().await;
}

/// The walk of `TestSidebar` with boards that are put into the store directly: the first
/// is filed when the default category is created, the second is filed by hand.
#[tokio::test]
async fn sidebar_with_boards_from_the_store() {
    let th = Setup::single_user().start().await;

    let board = board_with_admin(&th, TEST_TEAM_ID, SINGLE_USER_ID).await;
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the board");
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::System,
        "the default category is a system category"
    );

    let board2 = board_with_admin(&th, TEST_TEAM_ID, SINGLE_USER_ID).await;
    let created = create_category(
        &th.client,
        &category(TEST_TEAM_ID, SINGLE_USER_ID, "Category 2"),
    )
    .await;
    file_board(&th.client, TEST_TEAM_ID, &created.id, &board2.id).await;

    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Category 2", "Boards"], "two categories");
    assert_eq!(
        board_ids(&listed[0]),
        [board2.id.as_str()],
        "the moved board"
    );
    assert_eq!(
        board_ids(&listed[1]),
        [board.id.as_str()],
        "the other stays"
    );

    th.client
        .delete(&format!("{}/{}", categories_path(TEST_TEAM_ID), created.id))
        .await
        .check_ok();
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the default category is left");
    assert_eq!(
        sorted(board_ids(&listed[0])),
        sorted(vec![board.id.as_str(), board2.id.as_str()]),
        "both boards are in it"
    );
    assert!(
        listed[0]
            .board_metadata
            .iter()
            .all(|metadata| !metadata.hidden),
        "and neither is hidden"
    );

    th.teardown().await;
}

/// `TestHideUnhideBoard`
///
/// The board comes from the store rather than from the board routes; it is filed under
/// the default category when that is created, which is all this test needs of it.
#[tokio::test]
async fn hide_unhide_board() {
    let th = Setup::single_user().start().await;
    board_with_admin(&th, TEST_TEAM_ID, SINGLE_USER_ID).await;

    // The board is not hidden to begin with.
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(listed[0].board_metadata.len(), 1, "one board");
    assert!(!listed[0].board_metadata[0].hidden, "not hidden");

    let place = format!(
        "{}/{}/boards/{}",
        categories_path(TEST_TEAM_ID),
        listed[0].category.id,
        listed[0].board_metadata[0].board_id
    );
    let hidden =
        async || category_boards(&th.client, TEST_TEAM_ID).await[0].board_metadata[0].hidden;

    let response = put_empty(&th.client, &format!("{place}/hide")).await;
    response.check_ok();
    assert_eq!(response.value(), json!({}), "the answer is an empty object");
    assert!(hidden().await, "hidden now");

    // Hiding a hidden board has no effect.
    put_empty(&th.client, &format!("{place}/hide"))
        .await
        .check_ok();
    assert!(hidden().await, "still hidden");

    let response = put_empty(&th.client, &format!("{place}/unhide")).await;
    response.check_ok();
    assert_eq!(response.value(), json!({}), "the answer is an empty object");
    assert!(!hidden().await, "shown again");

    // Showing a visible board has no effect.
    put_empty(&th.client, &format!("{place}/unhide"))
        .await
        .check_ok();
    assert!(!hidden().await, "still shown");

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// server/app/category_test.go

/// `TestCreateCategory/"base case"`
#[tokio::test]
async fn create_category_base_case() {
    let th = Setup::native().start().await;

    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category"))
        .await
        .expect("a valid category is created");

    assert_eq!(created.id.len(), 27, "it got an identifier");
    assert_eq!(created.name, "Category", "the name is kept");
    assert_eq!(
        created.category_type,
        CategoryType::Custom,
        "custom by default"
    );
    assert!(created.create_at > 0, "it got a creation time");
    assert_eq!(
        created.update_at, created.create_at,
        "and the same update time"
    );
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "what is returned is what is stored");

    th.teardown().await;
}

/// `TestCreateCategory/"creating invalid category"`
///
/// The original's last case, an unknown type, cannot be built here: the type is an enum.
/// It is checked over the API in [`create_category_refuses_invalid_requests`].
#[tokio::test]
async fn create_category_creating_invalid_category() {
    let th = Setup::native().start().await;
    let cases = [
        (
            category(TEAM_ID, USER_ID, ""),
            "category name cannot be empty",
        ),
        (
            category(TEAM_ID, "", "Name"),
            "category user ID cannot be empty",
        ),
        (
            category("", USER_ID, "Name"),
            "category team id ID cannot be empty",
        ),
    ];

    for (invalid, message) in cases {
        let error = th
            .app
            .create_category(invalid)
            .await
            .expect_err("an invalid category is refused");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error.to_string(), message, "the reason");
    }
    let stored = th
        .app
        .store()
        .get_user_categories(USER_ID, TEAM_ID)
        .await
        .expect("the categories are read");
    assert!(stored.is_empty(), "nothing was stored: {stored:?}");

    th.teardown().await;
}

/// `TestUpdateCategory/"base case"`
#[tokio::test]
async fn update_category_base_case() {
    let th = Setup::native().start().await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Renamed".to_owned(),
        collapsed: true,
        // Members that an update does not touch.
        sort_order: 990,
        create_at: 1,
        category_type: CategoryType::System,
        ..created.clone()
    };
    let updated = th
        .app
        .update_category(change)
        .await
        .expect("the owner updates the category");

    assert_eq!(updated.name, "Renamed", "the name changed");
    assert!(updated.collapsed, "the collapsed state changed");
    assert!(
        updated.update_at >= created.update_at,
        "the update time moved"
    );
    assert_eq!(updated.sort_order, created.sort_order, "the place is kept");
    assert_eq!(
        updated.create_at, created.create_at,
        "the creation time is kept"
    );
    assert_eq!(
        updated.category_type,
        CategoryType::Custom,
        "the type is kept"
    );
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, updated, "what is returned is what is stored");

    th.teardown().await;
}

/// `TestUpdateCategory/"updating invalid category"`
///
/// The unknown-type case is checked over the API in
/// [`update_category_refuses_invalid_requests`].
#[tokio::test]
async fn update_category_updating_invalid_category() {
    let th = Setup::native().start().await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category"))
        .await
        .expect("the category is created");

    // Without an identifier the update is given a fresh one, which names no category.
    let no_id = Category {
        id: String::new(),
        ..created.clone()
    };
    let error = th
        .app
        .update_category(no_id)
        .await
        .expect_err("an update needs the identifier");
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");

    let cases = [
        (
            Category {
                name: String::new(),
                ..created.clone()
            },
            "category name cannot be empty",
        ),
        (
            Category {
                user_id: String::new(),
                ..created.clone()
            },
            "category user ID cannot be empty",
        ),
        (
            Category {
                team_id: String::new(),
                ..created.clone()
            },
            "category team id ID cannot be empty",
        ),
    ];
    for (invalid, message) in cases {
        let error = th
            .app
            .update_category(invalid)
            .await
            .expect_err("an invalid category is refused");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error.to_string(), message, "the reason");
    }
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// `TestUpdateCategory/"trying to update someone else's category"`
#[tokio::test]
async fn update_category_trying_to_update_someone_elses_category() {
    let th = Setup::native().start().await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Taken over".to_owned(),
        user_id: OTHER_USER_ID.to_owned(),
        ..created.clone()
    };
    let error = th
        .app
        .update_category(change)
        .await
        .expect_err("another user's category cannot be updated");

    assert_eq!(error.status(), StatusCode::FORBIDDEN, "{error}");
    assert_eq!(
        error.to_string(),
        "category doesn't belong to user",
        "the reason"
    );
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// `TestUpdateCategory/"trying to update some other team's category"`
#[tokio::test]
async fn update_category_trying_to_update_some_other_teams_category() {
    let th = Setup::native().start().await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Moved".to_owned(),
        team_id: "team_id_2".to_owned(),
        ..created.clone()
    };
    let error = th
        .app
        .update_category(change)
        .await
        .expect_err("a category of another team cannot be updated");

    assert_eq!(error.status(), StatusCode::FORBIDDEN, "{error}");
    assert_eq!(
        error.to_string(),
        "category doesn't belong to user",
        "the reason"
    );
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// The default category of a user, created by listing the user's categories.
async fn default_category(th: &TestServer, user_id: &str, team_id: &str) -> Category {
    let listed = th
        .app
        .user_category_boards(user_id, team_id)
        .await
        .expect("the categories are listed");
    listed
        .into_iter()
        .map(|entry| entry.category)
        .find(|category| category.category_type == CategoryType::System)
        .expect("the default category exists")
}

/// `TestUpdateCategory/"should not be allowed to rename system category"`
#[tokio::test]
async fn update_category_should_not_be_allowed_to_rename_system_category() {
    let th = Setup::native().start().await;
    let system = default_category(&th, USER_ID, TEAM_ID).await;

    let change = Category {
        name: "Updated Name".to_owned(),
        ..system.clone()
    };
    let updated = th
        .app
        .update_category(change)
        .await
        .expect("the update is accepted");

    assert_eq!(updated.name, "Boards", "the name is not changed");
    assert_eq!(
        updated.category_type,
        CategoryType::System,
        "still a system one"
    );

    th.teardown().await;
}

/// `TestUpdateCategory/"should be allowed to collapse and expand any category type"`
#[tokio::test]
async fn update_category_should_be_allowed_to_collapse_and_expand_any_category_type() {
    let th = Setup::native().start().await;
    let system = default_category(&th, USER_ID, TEAM_ID).await;
    assert!(!system.collapsed, "expanded to begin with");

    let change = Category {
        name: "Updated Name".to_owned(),
        collapsed: true,
        // The type of the request is ignored: the category stays a system one.
        category_type: CategoryType::Custom,
        ..system.clone()
    };
    let updated = th
        .app
        .update_category(change)
        .await
        .expect("the update is accepted");

    assert_eq!(
        updated.name, "Boards",
        "the name should not have been updated"
    );
    assert!(updated.collapsed, "collapsed now");
    assert_eq!(
        updated.category_type,
        CategoryType::System,
        "still a system one"
    );

    let expanded = th
        .app
        .update_category(Category {
            collapsed: false,
            ..updated
        })
        .await
        .expect("the update is accepted");
    assert!(!expanded.collapsed, "expanded again");

    th.teardown().await;
}

/// `TestDeleteCategory/"base case"`
#[tokio::test]
async fn delete_category_base_case() {
    let th = Setup::native().start().await;
    let system = default_category(&th, USER_ID, TEAM_ID).await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category 1"))
        .await
        .expect("the category is created");

    let deleted = th
        .app
        .delete_category(&created.id, USER_ID, TEAM_ID)
        .await
        .expect("the owner deletes the category");

    assert_eq!(deleted.id, created.id, "the deleted category is returned");
    assert!(deleted.delete_at > 0, "marked as deleted");
    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(
        ids(&listed),
        [system.id.as_str()],
        "only the default is left"
    );

    th.teardown().await;
}

/// `TestDeleteCategory/"trying to delete already deleted category"`
#[tokio::test]
async fn delete_category_trying_to_delete_already_deleted_category() {
    let th = Setup::native().start().await;
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category 1"))
        .await
        .expect("the category is created");
    let deleted = th
        .app
        .delete_category(&created.id, USER_ID, TEAM_ID)
        .await
        .expect("the owner deletes the category");

    tokio::time::sleep(Duration::from_millis(5)).await;
    let again = th
        .app
        .delete_category(&created.id, USER_ID, TEAM_ID)
        .await
        .expect("deleting it again is not an error");

    assert_eq!(
        again, deleted,
        "the time of the deletion is not overwritten"
    );

    th.teardown().await;
}

/// `TestDeleteCategory/"trying to delete system category"`
#[tokio::test]
async fn delete_category_trying_to_delete_system_category() {
    let th = Setup::native().start().await;
    let system = default_category(&th, USER_ID, TEAM_ID).await;

    let error = th
        .app
        .delete_category(&system.id, USER_ID, TEAM_ID)
        .await
        .expect_err("a system category cannot be deleted");

    // Preserved quirk: the original does not classify this error.
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert_eq!(
        error.to_string(),
        "cannot delete a system category",
        "the cause"
    );
    let stored = th.app.category(&system.id).await.expect("it is stored");
    assert_eq!(stored.delete_at, 0, "the category is not deleted");

    th.teardown().await;
}

/// `TestMoveBoardsToDefaultCategory/"When default category already exists"`
///
/// The function is private; deleting a category is what calls it.
#[tokio::test]
async fn move_boards_to_default_category_when_default_category_already_exists() {
    let th = Setup::native().start().await;
    let system = default_category(&th, USER_ID, TEAM_ID).await;
    let custom = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Custom Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed = ["board_id_1".to_owned(), "board_id_2".to_owned()];
    th.app
        .add_update_user_category_board(TEAM_ID, USER_ID, &custom.id, &board_ids_filed)
        .await
        .expect("the boards are filed");

    th.app
        .delete_category(&custom.id, USER_ID, TEAM_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(
        ids(&listed),
        [system.id.as_str()],
        "only the default is left"
    );
    assert_eq!(
        sorted(board_ids(&listed[0])),
        ["board_id_1", "board_id_2"],
        "the boards moved to the default category"
    );

    th.teardown().await;
}

/// `TestMoveBoardsToDefaultCategory/"When default category doesn't already exists"`
#[tokio::test]
async fn move_boards_to_default_category_when_default_category_doesnt_already_exists() {
    let th = Setup::native().start().await;
    // Created without listing the categories first, so there is no default one yet. The
    // board is filed through the store for the same reason.
    let custom = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Custom Category 1"))
        .await
        .expect("the category is created");
    th.app
        .store()
        .add_update_category_board(USER_ID, &custom.id, &["board_id_1".to_owned()])
        .await
        .expect("the board is filed");
    let stored = th
        .app
        .store()
        .get_user_categories(USER_ID, TEAM_ID)
        .await
        .expect("the categories are read");
    assert_eq!(stored.len(), 1, "only the custom category: {stored:?}");

    th.app
        .delete_category(&custom.id, USER_ID, TEAM_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(names(&listed), ["Boards"], "a default category was created");
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::System,
        "as a system category"
    );
    assert_eq!(board_ids(&listed[0]), ["board_id_1"], "and got the board");

    th.teardown().await;
}

/// Three categories of [`USER_ID`]: the default one and two custom ones, in the order
/// they are listed.
async fn three_categories(th: &TestServer) -> Vec<String> {
    default_category(th, USER_ID, TEAM_ID).await;
    for name in ["Category 2", "Category 3"] {
        th.app
            .create_category(category(TEAM_ID, USER_ID, name))
            .await
            .expect("the category is created");
    }
    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(
        names(&listed),
        ["Category 3", "Category 2", "Boards"],
        "new categories go on top"
    );
    listed.into_iter().map(|entry| entry.category.id).collect()
}

async fn listed_ids(th: &TestServer) -> Vec<String> {
    th.app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed")
        .into_iter()
        .map(|entry| entry.category.id)
        .collect()
}

/// `TestReorderCategories/"base case"`
#[tokio::test]
async fn reorder_categories_base_case() {
    let th = Setup::native().start().await;
    let listed = three_categories(&th).await;
    let new_order = vec![listed[2].clone(), listed[0].clone(), listed[1].clone()];

    let answered = th
        .app
        .reorder_categories(USER_ID, TEAM_ID, new_order.clone())
        .await
        .expect("an order that names every category is accepted");

    assert_eq!(answered, new_order, "the new order is returned");
    assert_eq!(listed_ids(&th).await, new_order, "and is the listed order");

    th.teardown().await;
}

/// `TestReorderCategories/"not specifying all categories should fail"`
#[tokio::test]
async fn reorder_categories_not_specifying_all_categories_should_fail() {
    let th = Setup::native().start().await;
    let listed = three_categories(&th).await;

    let error = th
        .app
        .reorder_categories(USER_ID, TEAM_ID, vec![listed[1].clone(), listed[0].clone()])
        .await
        .expect_err("an order that leaves a category out is refused");

    // Preserved quirk: the original does not classify this error.
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert_eq!(listed_ids(&th).await, listed, "the order is unchanged");

    th.teardown().await;
}

/// `TestVerifyNewCategoriesMatchExisting/"base case"`
///
/// The function is private; this goes through the route that calls it.
#[tokio::test]
async fn verify_new_categories_match_existing_base_case() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let second = create_category(&th.client, &category("0", &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category("0", &user.id, "Category 3")).await;
    let new_order = [second.id.as_str(), third.id.as_str(), default_id.as_str()];

    let response = th
        .client
        .put("/teams/0/categories/reorder", &new_order)
        .await;

    assert_eq!(
        response.ok_json::<Vec<String>>(),
        new_order,
        "the new order is answered"
    );
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), new_order, "and is the listed order");

    th.teardown().await;
}

/// `TestVerifyNewCategoriesMatchExisting/"different category counts"`
#[tokio::test]
async fn verify_new_categories_match_existing_different_category_counts() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, "0").await;
    let second = create_category(&th.client, &category("0", &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category("0", &user.id, "Category 3")).await;
    let before = category_boards(&th.client, "0").await;

    let response = th
        .client
        .put(
            "/teams/0/categories/reorder",
            &[second.id.as_str(), third.id.as_str()],
        )
        .await;

    check_internal_error(&response);
    let after = category_boards(&th.client, "0").await;
    assert_eq!(ids(&after), ids(&before), "the order is unchanged");

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// server/app/category_boards_test.go

/// `TestGetUserCategoryBoards/"user had no default category and had boards"`
#[tokio::test]
async fn get_user_category_boards_user_had_no_default_category_and_had_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let mut boards = Vec::new();
    for _ in 0..3 {
        boards.push(board_with_admin(&th, "0", &user.id).await.id);
    }

    let listed = category_boards(&th.client, "0").await;

    assert_eq!(
        names(&listed),
        ["Boards"],
        "the default category was created"
    );
    assert_eq!(listed[0].board_metadata.len(), 3, "with the three boards");
    for board_id in &boards {
        assert!(
            listed[0].board_metadata.contains(&visible(board_id)),
            "board {board_id} is in it, not hidden"
        );
    }

    th.teardown().await;
}

/// `TestGetUserCategoryBoards/"user had no default category BUT had no boards"`
#[tokio::test]
async fn get_user_category_boards_user_had_no_default_category_but_had_no_boards() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.client.get("/teams/0/categories").await;

    let listed: Vec<CategoryBoards> = response.ok_json();
    assert_eq!(
        names(&listed),
        ["Boards"],
        "the default category was created"
    );
    assert!(listed[0].board_metadata.is_empty(), "with no boards");
    // The exact wire format of an entry, as the original answers it.
    let user = th.client.me().await;
    let entry = &response.value()[0];
    assert_eq!(
        *entry,
        json!({
            "id": listed[0].category.id,
            "name": "Boards",
            "userID": user.id,
            "teamID": "0",
            "createAt": listed[0].category.create_at,
            "updateAt": listed[0].category.create_at,
            "deleteAt": 0,
            "collapsed": false,
            "sorting": "",
            "type": "system",
            "boardMetadata": [],
            // Preserved quirk: always zero here, whatever the category's place.
            "sortOrder": 0,
        }),
        "the entry"
    );

    th.teardown().await;
}

/// `TestGetUserCategoryBoards/"user already had a default Boards category with boards in
/// it"`
#[tokio::test]
async fn get_user_category_boards_user_already_had_a_default_boards_category_with_boards_in_it() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    board_with_admin(&th, "0", &user.id).await;
    board_with_admin(&th, "0", &user.id).await;
    let first = category_boards(&th.client, "0").await;
    assert_eq!(first[0].board_metadata.len(), 2, "two boards to begin with");

    // A board that appears later is not filed by listing the categories.
    board_with_admin(&th, "0", &user.id).await;
    let second = category_boards(&th.client, "0").await;

    assert_eq!(second, first, "the same category with the same boards");

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user doesn't have any boards - implicit or explicit"`
///
/// The function is private; listing the categories of a user who has none calls it.
#[tokio::test]
async fn create_boards_category_user_doesnt_have_any_boards_implicit_or_explicit() {
    let th = Setup::native().start().await;

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert!(listed[0].board_metadata.is_empty(), "with no boards");
    assert_eq!(listed[0].category.user_id, USER_ID, "of the user");
    assert_eq!(listed[0].category.team_id, TEAM_ID, "in the team");

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has implicit access to some board"`
///
/// Implicit access comes from the host application in the original. What a standalone
/// server has of it are open boards that the user can see without being a member.
#[tokio::test]
async fn create_boards_category_user_has_implicit_access_to_some_board() {
    let th = Setup::native().start().await;
    for _ in 0..3 {
        board_without_member(&th, TEAM_ID, OTHER_USER_ID).await;
    }

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert!(
        listed[0].board_metadata.is_empty(),
        "boards the user is not a member of are not filed"
    );

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has explicit access to some board"`
#[tokio::test]
async fn create_boards_category_user_has_explicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let mut boards = Vec::new();
    for _ in 0..3 {
        boards.push(board_with_admin(&th, TEAM_ID, USER_ID).await.id);
    }
    // A board of another team is not filed in this team's default category.
    board_with_admin(&th, "team_id_2", USER_ID).await;

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert_eq!(
        sorted(board_ids(&listed[0])),
        sorted(boards.iter().map(String::as_str).collect()),
        "the three boards the user is a member of"
    );
    // What was returned at creation is what is stored.
    let again = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(ids(&again), ids(&listed), "the same category");
    assert_eq!(
        sorted(board_ids(&again[0])),
        sorted(board_ids(&listed[0])),
        "the same boards"
    );

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has both implicit and explicit access to some board"`
#[tokio::test]
async fn create_boards_category_user_has_both_implicit_and_explicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let own = board_with_admin(&th, TEAM_ID, USER_ID).await;
    board_without_member(&th, TEAM_ID, OTHER_USER_ID).await;
    board_without_member(&th, TEAM_ID, OTHER_USER_ID).await;

    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert_eq!(
        board_ids(&listed[0]),
        [own.id.as_str()],
        "only the board the user is a member of"
    );

    th.teardown().await;
}

/// A category of [`USER_ID`] with the given boards filed in it, in the listed order.
async fn category_with_boards(th: &TestServer, board_ids_filed: &[&str]) -> Category {
    let created = th
        .app
        .create_category(category(TEAM_ID, USER_ID, "Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed: Vec<String> = board_ids_filed.iter().map(|id| (*id).to_owned()).collect();
    th.app
        .add_update_user_category_board(TEAM_ID, USER_ID, &created.id, &board_ids_filed)
        .await
        .expect("the boards are filed");
    th.app
        .reorder_category_boards(USER_ID, TEAM_ID, &created.id, board_ids_filed)
        .await
        .expect("the boards are put in order");
    created
}

async fn listed_board_ids(th: &TestServer, category_id: &str) -> Vec<String> {
    let listed = th
        .app
        .user_category_boards(USER_ID, TEAM_ID)
        .await
        .expect("the categories are listed");
    board_ids(entry(&listed, category_id))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// `TestReorderCategoryBoards/"base case"`
#[tokio::test]
async fn reorder_category_boards_base_case() {
    let th = Setup::native().start().await;
    let created = category_with_boards(&th, &["board_id_1", "board_id_2"]).await;
    assert_eq!(
        listed_board_ids(&th, &created.id).await,
        ["board_id_1", "board_id_2"],
        "the order to begin with"
    );

    let new_order = th
        .app
        .reorder_category_boards(
            USER_ID,
            TEAM_ID,
            &created.id,
            vec!["board_id_2".to_owned(), "board_id_1".to_owned()],
        )
        .await
        .expect("an order that names every board is accepted");

    assert_eq!(new_order, ["board_id_2", "board_id_1"], "the new order");
    assert_eq!(
        listed_board_ids(&th, &created.id).await,
        ["board_id_2", "board_id_1"],
        "and is the listed order"
    );

    th.teardown().await;
}

/// `TestReorderCategoryBoards/"not specifying all boards"`
#[tokio::test]
async fn reorder_category_boards_not_specifying_all_boards() {
    let th = Setup::native().start().await;
    let created = category_with_boards(&th, &["board_id_1", "board_id_2", "board_id_3"]).await;

    let error = th
        .app
        .reorder_category_boards(
            USER_ID,
            TEAM_ID,
            &created.id,
            vec!["board_id_2".to_owned(), "board_id_1".to_owned()],
        )
        .await
        .expect_err("an order that leaves a board out is refused");

    // Preserved quirk: the original does not classify this error.
    assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert_eq!(
        listed_board_ids(&th, &created.id).await,
        ["board_id_1", "board_id_2", "board_id_3"],
        "the order is unchanged"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: the routes' answers, checked against the original server.

/// The category routes need a session.
#[tokio::test]
async fn category_routes_need_a_session() {
    let th = Setup::native().start().await.init_basic().await;
    let anonymous = th.anonymous();
    let requests = [
        (Method::GET, "/teams/0/categories"),
        (Method::POST, "/teams/0/categories"),
        (Method::PUT, "/teams/0/categories/reorder"),
        (Method::PUT, "/teams/0/categories/any"),
        (Method::DELETE, "/teams/0/categories/any"),
        (Method::PUT, "/teams/0/categories/any/boards/reorder"),
        (Method::POST, "/teams/0/categories/any/boards/any"),
        (Method::PUT, "/teams/0/categories/any/boards/any/hide"),
        (Method::PUT, "/teams/0/categories/any/boards/any/unhide"),
    ];

    for (method, path) in requests {
        let response = anonymous.send_raw_body(method.clone(), path, "{}").await;
        check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    }

    th.teardown().await;
}

/// The default category is created by the first listing, once per team, and is the same
/// from then on.
#[tokio::test]
async fn the_default_category_is_created_once_per_team() {
    let th = Setup::native().start().await.init_basic().await;

    let first = category_boards(&th.client, "0").await;
    let second = category_boards(&th.client, "0").await;
    let other_team = category_boards(&th.client, "other-team").await;
    let other_user = category_boards(&th.client2, "0").await;

    assert_eq!(names(&first), ["Boards"], "created by the first listing");
    assert_eq!(second, first, "and not again by the second");
    assert_eq!(names(&other_team), ["Boards"], "each team has its own");
    assert_ne!(ids(&other_team), ids(&first), "with its own identifier");
    assert_eq!(other_team[0].category.team_id, "other-team", "in that team");
    assert_eq!(names(&other_user), ["Boards"], "each user has their own");
    assert_ne!(ids(&other_user), ids(&first), "with its own identifier");

    th.teardown().await;
}

/// Preserved quirk: the default category is recognised by its name alone, so a custom
/// category that the user named "Boards" stands in for it.
#[tokio::test]
async fn a_custom_category_named_boards_stands_in_for_the_default_one() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let custom = create_category(&th.client, &category("0", &user.id, "Boards")).await;

    let listed = category_boards(&th.client, "0").await;

    assert_eq!(
        ids(&listed),
        [custom.id.as_str()],
        "no system category is added"
    );
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::Custom,
        "the stand-in is a custom category"
    );

    th.teardown().await;
}

/// What creating a category answers, member by member.
#[tokio::test]
async fn create_category_answers_the_stored_category() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;

    let response = th
        .client
        .post(
            "/teams/0/categories",
            &json!({"name": "Cat A", "userID": user.id, "teamID": "0"}),
        )
        .await;
    let created: Category = response.ok_json();
    assert_eq!(
        response.value(),
        json!({
            "id": created.id,
            "name": "Cat A",
            "userID": user.id,
            "teamID": "0",
            "createAt": created.create_at,
            "updateAt": created.create_at,
            "deleteAt": 0,
            "collapsed": false,
            // Preserved quirk: a category created at the top is stored one place down.
            "sortOrder": 10,
            "sorting": "",
            "type": "custom",
        }),
        "the created category"
    );
    assert_eq!(created.id.len(), 27, "a generated identifier");
    assert!(created.create_at > 0, "a creation time");

    // What the client may choose: the identifier, the times, the collapsed state and the
    // type. A negative sort order counts as zero and the sorting method is not stored.
    let response = th
        .client
        .post(
            "/teams/0/categories",
            &json!({
                "id": "myid1",
                "name": "With id",
                "userID": user.id,
                "teamID": "0",
                "sortOrder": -5,
                "createAt": 5,
                "sorting": "alpha",
                "collapsed": true,
                "type": "system",
            }),
        )
        .await;
    assert_eq!(
        response.ok_json::<Value>(),
        json!({
            "id": "myid1",
            "name": "With id",
            "userID": user.id,
            "teamID": "0",
            "createAt": 5,
            "updateAt": 5,
            "deleteAt": 0,
            "collapsed": true,
            "sortOrder": 10,
            "sorting": "",
            "type": "system",
        }),
        "the created category"
    );

    // A blank type and a `null` type are the default type.
    for blank in [json!("  "), json!(""), Value::Null] {
        let response = th
            .client
            .post(
                "/teams/0/categories",
                &json!({"name": "Blank type", "userID": user.id, "teamID": "0", "type": blank}),
            )
            .await;
        assert_eq!(
            response.ok_json::<Category>().category_type,
            CategoryType::Custom,
            "type {blank} is read as custom"
        );
    }

    th.teardown().await;
}

/// The refusals of creating a category.
#[tokio::test]
async fn create_category_refuses_invalid_requests() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    let path = "/teams/0/categories";
    let invalid_type = "category type is invalid. Allowed types: system and custom";
    let user_mismatch = |category_user: &str| {
        format!(
            "userID {} and category userID {category_user} mismatch",
            user.id
        )
    };

    let cases = [
        (
            "another user's category",
            json!({"name": "x", "userID": other.id, "teamID": "0"}),
            user_mismatch(&other.id),
        ),
        (
            "no user",
            json!({"name": "x", "teamID": "0"}),
            user_mismatch(""),
        ),
        ("a `null` body", Value::Null, user_mismatch("")),
        (
            "another team than the path's",
            json!({"name": "x", "userID": user.id, "teamID": "1"}),
            "teamID mismatch".to_owned(),
        ),
        (
            "a blank name",
            json!({"name": "  ", "userID": user.id, "teamID": "0"}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "a `null` name",
            json!({"name": null, "userID": user.id, "teamID": "0"}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "an unknown type",
            json!({"name": "x", "userID": user.id, "teamID": "0", "type": "bogus"}),
            invalid_type.to_owned(),
        ),
        (
            "a blank name and an unknown type: the name is checked first",
            json!({"name": "", "userID": user.id, "teamID": "0", "type": "bogus"}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "another user and an unknown type: the user is checked first",
            json!({"name": "x", "userID": other.id, "teamID": "0", "type": "bogus"}),
            user_mismatch(&other.id),
        ),
    ];
    for (case, body, message) in cases {
        let response = th.client.post(path, &body).await;
        response.check_bad_request();
        assert_eq!(response.error_message(), message, "{case}");
    }

    // Preserved quirk: a body that cannot be decoded is a server error.
    for malformed in ["{not json", "", "[]", r#"{"name": 5}"#, r#"{"type": 5}"#] {
        let response = th.client.send_raw_body(Method::POST, path, malformed).await;
        check_internal_error(&response);
    }

    // An identifier that is taken is a server error too.
    let taken = json!({"id": "taken", "name": "x", "userID": user.id, "teamID": "0"});
    th.client.post(path, &taken).await.check_ok();
    check_internal_error(&th.client.post(path, &taken).await);

    let listed = category_boards(&th.client, "0").await;
    assert_eq!(
        names(&listed),
        ["x", "Boards"],
        "only the valid one was stored"
    );

    th.teardown().await;
}

/// What updating a category answers and changes.
#[tokio::test]
async fn update_category_changes_the_name_and_the_collapsed_state_only() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;

    let response = th
        .client
        .put(
            &format!("/teams/0/categories/{}", created.id),
            &json!({
                "id": created.id,
                "name": "Cat A2",
                "userID": user.id,
                "teamID": "0",
                "collapsed": true,
                "sortOrder": 99,
                "type": "system",
                "deleteAt": 77,
                "createAt": 3,
            }),
        )
        .await;

    let updated: Category = response.ok_json();
    assert_eq!(
        response.value(),
        json!({
            "id": created.id,
            "name": "Cat A2",
            "userID": user.id,
            "teamID": "0",
            "createAt": created.create_at,
            "updateAt": updated.update_at,
            "deleteAt": 0,
            "collapsed": true,
            "sortOrder": created.sort_order,
            "sorting": "",
            "type": "custom",
        }),
        "the updated category"
    );
    assert!(
        updated.update_at >= created.update_at,
        "the update time moved"
    );

    th.teardown().await;
}

/// The refusals of updating a category.
#[tokio::test]
async fn update_category_refuses_invalid_requests() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    let path = format!("/teams/0/categories/{}", created.id);
    let body = |change: Value| {
        let mut body = json!({"id": created.id, "name": "n", "userID": user.id, "teamID": "0"});
        for (name, value) in change.as_object().expect("an object") {
            body[name] = value.clone();
        }
        body
    };

    let bad_requests = [
        (
            body(json!({"id": "other"})),
            "categoryID mismatch in patch and body",
        ),
        (
            body(json!({"userID": other.id})),
            "user ID mismatch in session and category",
        ),
        (body(json!({"teamID": "1"})), "teamID mismatch"),
        (body(json!({"name": ""})), "category name cannot be empty"),
        (
            body(json!({"type": "bogus"})),
            "category type is invalid. Allowed types: system and custom",
        ),
    ];
    for (request, message) in bad_requests {
        let response = th.client.put(&path, &request).await;
        check_error(&response, StatusCode::BAD_REQUEST, message);
    }

    // The category is in another team than the path and the body agree on.
    let response = th
        .client
        .put(
            &format!("/teams/1/categories/{}", created.id),
            &body(json!({"teamID": "1"})),
        )
        .await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "category doesn't belong to user",
    );

    let response = th
        .client
        .put(
            "/teams/0/categories/nope",
            &json!({"id": "nope", "name": "n", "userID": user.id, "teamID": "0"}),
        )
        .await;
    check_error(
        &response,
        StatusCode::NOT_FOUND,
        "{category ID=nope} not found",
    );

    check_internal_error(&th.client.send_raw_body(Method::PUT, &path, "{nope").await);

    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "none of these changed the category");

    // A deleted category cannot be updated.
    th.client.delete(&path).await.check_ok();
    let response = th.client.put(&path, &body(json!({}))).await;
    check_error(&response, StatusCode::NOT_FOUND, "category is deleted");

    th.teardown().await;
}

/// What deleting a category answers, and its refusals.
#[tokio::test]
async fn delete_category_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    let path = format!("/teams/0/categories/{}", created.id);

    let response = th.client.delete("/teams/0/categories/nope").await;
    check_error(
        &response,
        StatusCode::NOT_FOUND,
        "{category ID=nope} not found",
    );

    let response = th
        .client
        .delete(&format!("/teams/1/categories/{}", created.id))
        .await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "category doesn't belong to the team",
    );

    // Preserved quirk: refusing to delete a system category is a server error.
    let response = th
        .client
        .delete(&format!("/teams/0/categories/{default_id}"))
        .await;
    check_internal_error(&response);

    let listed = category_boards(&th.client, "0").await;
    assert_eq!(names(&listed), ["Cat A", "Boards"], "nothing was deleted");

    let response = th.client.delete(&path).await;
    let deleted: Category = response.ok_json();
    assert!(deleted.delete_at > 0, "marked as deleted");
    assert_eq!(
        response.value(),
        json!({
            "id": created.id,
            "name": "Cat A",
            "userID": user.id,
            "teamID": "0",
            "createAt": created.create_at,
            "updateAt": created.update_at,
            "deleteAt": deleted.delete_at,
            "collapsed": false,
            "sortOrder": created.sort_order,
            "sorting": "",
            "type": "custom",
        }),
        "the deleted category"
    );

    // Deleting it again answers the same and keeps the time of the deletion.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let again: Category = th.client.delete(&path).await.ok_json();
    assert_eq!(again, deleted, "the same answer");

    let listed = category_boards(&th.client, "0").await;
    assert_eq!(names(&listed), ["Boards"], "the category is gone");

    th.teardown().await;
}

/// A reorder request must name the user's categories and nothing else.
#[tokio::test]
async fn reorder_categories_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let path = "/teams/0/categories/reorder";

    // Preserved quirk: with no categories, the empty order is accepted and answered with
    // `null`. The user has none before the first listing creates the default one.
    for body in ["[]", "null"] {
        let response = th.client.send_raw_body(Method::PUT, path, body).await;
        response.check_ok();
        assert_eq!(response.value(), Value::Null, "the answer to {body}");
    }

    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let second = create_category(&th.client, &category("0", &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category("0", &user.id, "Category 3")).await;
    let before = [third.id.as_str(), second.id.as_str(), default_id.as_str()];
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), before, "the order to begin with");

    let refused = [
        json!([default_id, second.id]),
        json!([default_id, second.id, "zzz"]),
        json!([default_id, second.id, third.id, third.id]),
        json!([]),
        Value::Null,
        json!({}),
        json!("reorder"),
    ];
    for body in refused {
        let response = th.client.put(path, &body).await;
        check_internal_error(&response);
    }
    check_internal_error(&th.client.send_raw_body(Method::PUT, path, "[").await);
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), before, "a refused order changes nothing");

    // Another user's order of these categories is refused: they are not theirs.
    category_boards(&th.client2, "0").await;
    let response = th.client2.put(path, &json!([default_id])).await;
    check_internal_error(&response);
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), before, "and changes nothing");

    let new_order = [default_id.as_str(), third.id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), new_order, "the new order");

    // Preserved quirk: only the count and the membership are checked, so naming one
    // category twice passes. The one left out keeps its place value.
    let twice = [default_id.as_str(), default_id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &twice).await;
    assert_eq!(response.ok_json::<Vec<String>>(), twice, "the answer");
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(
        sorted(ids(&listed)),
        sorted(new_order.to_vec()),
        "every category is still listed"
    );

    th.teardown().await;
}

/// A board reorder request must name the boards of the category and nothing else.
#[tokio::test]
async fn reorder_category_boards_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    for board_id in ["board-1", "board-2", "board-3"] {
        file_board(&th.client, "0", &created.id, board_id).await;
    }
    let path = format!("/teams/0/categories/{}/boards/reorder", created.id);
    let order = async || {
        let listed = category_boards(&th.client, "0").await;
        board_ids(entry(&listed, &created.id))
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    let new_order = ["board-2", "board-3", "board-1"];
    let response = th.client.put(&path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    assert_eq!(order().await, new_order, "the new order");

    let refused = [
        json!(["board-1", "board-2"]),
        json!(["board-1", "board-2", "other"]),
        json!([]),
        Value::Null,
        json!({}),
    ];
    for body in refused {
        check_internal_error(&th.client.put(&path, &body).await);
    }
    check_internal_error(&th.client.send_raw_body(Method::PUT, &path, "{").await);
    // The category is not one of the path's team.
    let response = th
        .client
        .put(
            &format!("/teams/1/categories/{}/boards/reorder", created.id),
            &new_order,
        )
        .await;
    check_internal_error(&response);
    assert_eq!(order().await, new_order, "a refused order changes nothing");

    let response = th
        .client
        .put("/teams/0/categories/nope/boards/reorder", &new_order)
        .await;
    check_error(
        &response,
        StatusCode::NOT_FOUND,
        "{category ID=nope} not found",
    );

    // Preserved quirk: the empty order of an empty category is answered with `null`.
    for body in ["[]", "null"] {
        let response = th
            .client
            .send_raw_body(
                Method::PUT,
                &format!("/teams/0/categories/{default_id}/boards/reorder"),
                body,
            )
            .await;
        response.check_ok();
        assert_eq!(response.value(), Value::Null, "the answer to {body}");
    }

    // A deleted category has no boards to order.
    th.client
        .delete(&format!("/teams/0/categories/{}", created.id))
        .await
        .check_ok();
    check_internal_error(&th.client.put(&path, &new_order).await);

    th.teardown().await;
}

/// Filing a board under a category: the answer, the move, and the refusals.
#[tokio::test]
async fn update_category_board_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, "0", &user.id).await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    let board_path = |team_id: &str, category_id: &str| {
        format!(
            "/teams/{team_id}/categories/{category_id}/boards/{}",
            board.id
        )
    };
    let places = async || {
        let listed = category_boards(&th.client, "0").await;
        (
            board_ids(entry(&listed, &created.id))
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
            board_ids(entry(&listed, &default_id))
                .into_iter()
                .map(str::to_owned)
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        places().await,
        (vec![], vec![board.id.clone()]),
        "the board starts in the default category"
    );

    // Preserved quirk: the answer is the bare word `ok`, labelled as JSON.
    let response = th.client.post_empty(&board_path("0", &created.id)).await;
    response.check_ok();
    assert_eq!(response.text(), "ok", "the body");
    assert_eq!(
        response.header("content-type"),
        "application/json",
        "the label"
    );
    assert_eq!(
        places().await,
        (vec![board.id.clone()], vec![]),
        "the board moved"
    );

    // A category that does not exist, is in another team, or is deleted: the original
    // answers these with a server error after moving the board out of sight. Here the
    // board stays where it is.
    let deleted = create_category(&th.client, &category("0", &user.id, "Deleted")).await;
    th.client
        .delete(&format!("/teams/0/categories/{}", deleted.id))
        .await
        .check_ok();
    for path in [
        board_path("0", "nope"),
        board_path("1", &default_id),
        board_path("0", &deleted.id),
    ] {
        check_internal_error(&th.client.post_empty(&path).await);
        assert_eq!(
            places().await,
            (vec![board.id.clone()], vec![]),
            "the board stays after {path}"
        );
    }

    // Preserved quirk: the board is not looked at, so any identifier can be filed.
    file_board(&th.client, "0", &default_id, "no-such-board").await;
    assert_eq!(
        places().await,
        (vec![board.id.clone()], vec!["no-such-board".to_owned()]),
        "an identifier that names no board is filed all the same"
    );

    th.teardown().await;
}

/// Preserved quirk: hiding a board that is not in the category, or in a category that
/// does not exist, is answered as a success and changes nothing.
#[tokio::test]
async fn hiding_a_board_that_is_not_in_the_category_changes_nothing() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, "0", &user.id).await;
    let before = category_boards(&th.client, "0").await;
    assert_eq!(before[0].board_metadata, [visible(&board.id)], "one board");

    for path in [
        "/teams/0/categories/nope/boards/nope/hide".to_owned(),
        format!("/teams/0/categories/nope/boards/{}/hide", board.id),
        format!(
            "/teams/0/categories/{}/boards/nope/hide",
            before[0].category.id
        ),
        "/teams/0/categories/nope/boards/nope/unhide".to_owned(),
    ] {
        let response = put_empty(&th.client, &path).await;
        response.check_ok();
        assert_eq!(response.value(), json!({}), "the answer to {path}");
    }

    let after = category_boards(&th.client, "0").await;
    assert_eq!(after, before, "nothing changed");

    th.teardown().await;
}

/// Preserved quirk: the word `reorder` is not reserved. With another method than the
/// reorder routes', a path that ends in it addresses the category or the board that has
/// the word as its identifier.
#[tokio::test]
async fn the_word_reorder_can_be_an_identifier() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, "0").await;
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;

    // As a category: there is none with that identifier yet.
    let response = th.client.delete("/teams/0/categories/reorder").await;
    check_error(
        &response,
        StatusCode::NOT_FOUND,
        "{category ID=reorder} not found",
    );
    let named = Category {
        id: "reorder".to_owned(),
        ..category("0", &user.id, "Named reorder")
    };
    create_category(&th.client, &named).await;
    let deleted: Category = th
        .client
        .delete("/teams/0/categories/reorder")
        .await
        .ok_json();
    assert_eq!(deleted.id, "reorder", "the category with that identifier");
    assert!(deleted.delete_at > 0, "is deleted");

    // As a board: filed, and hidden, like any other identifier.
    file_board(&th.client, "0", &created.id, "reorder").await;
    put_empty(
        &th.client,
        &format!("/teams/0/categories/{}/boards/reorder/hide", created.id),
    )
    .await
    .check_ok();
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(
        entry(&listed, &created.id).board_metadata,
        [CategoryBoardMetadata {
            board_id: "reorder".to_owned(),
            hidden: true,
        }],
        "the board named reorder is filed and hidden"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: what one user can do to the categories of another.

/// The state two users start from: each has listed their categories, and the first has a
/// custom category with one board in it.
struct TwoUsers {
    th: TestServer,
    /// The first user's custom category.
    category: Category,
    /// The board in it.
    board_id: String,
    /// The second user's identifier.
    intruder_id: String,
}

impl TwoUsers {
    async fn start() -> Self {
        let th = Setup::native().start().await.init_basic().await;
        let owner = th.client.me().await;
        let intruder = th.client2.me().await;
        category_boards(&th.client, "0").await;
        category_boards(&th.client2, "0").await;
        let board = board_with_admin(&th, "0", &owner.id).await;
        let category = create_category(&th.client, &category("0", &owner.id, "Private")).await;
        file_board(&th.client, "0", &category.id, &board.id).await;
        Self {
            th,
            category,
            board_id: board.id,
            intruder_id: intruder.id,
        }
    }

    /// What the owner sees of their categories.
    async fn owner_view(&self) -> Vec<CategoryBoards> {
        category_boards(&self.th.client, "0").await
    }
}

/// Security fix. In the original, `POST /teams/{team}/categories/{category}/boards/{id}`
/// by a user who does not own the category answers 500, but the board has been filed and
/// shows in the owner's category (`server/app/category_boards.go:150-170`). Here the
/// request is refused before anything is written.
#[tokio::test]
async fn filing_a_board_into_another_users_category_is_refused() {
    let two = TwoUsers::start().await;
    let before = two.owner_view().await;
    let intruder_before = category_boards(&two.th.client2, "0").await;

    for board_id in ["planted", two.board_id.as_str()] {
        let response = two
            .th
            .client2
            .post_empty(&format!(
                "/teams/0/categories/{}/boards/{board_id}",
                two.category.id
            ))
            .await;
        check_error(
            &response,
            StatusCode::FORBIDDEN,
            "category doesn't belong to user",
        );
    }

    assert_eq!(
        two.owner_view().await,
        before,
        "the owner's categories are untouched"
    );
    assert_eq!(
        category_boards(&two.th.client2, "0").await,
        intruder_before,
        "and nothing was filed for the intruder either"
    );

    two.th.teardown().await;
}

/// The legitimate counterpart of [`filing_a_board_into_another_users_category_is_refused`]:
/// each user files boards under their own categories, and neither sees the other's.
#[tokio::test]
async fn filing_a_board_into_an_own_category_works() {
    let two = TwoUsers::start().await;
    let own = create_category(&two.th.client2, &category("0", &two.intruder_id, "Mine")).await;

    // The same board identifier, filed by each user under a category of their own.
    file_board(&two.th.client2, "0", &own.id, &two.board_id).await;

    let listed = category_boards(&two.th.client2, "0").await;
    assert_eq!(
        board_ids(entry(&listed, &own.id)),
        [two.board_id.as_str()],
        "filed for the second user"
    );
    let owner_view = two.owner_view().await;
    assert_eq!(
        board_ids(entry(&owner_view, &two.category.id)),
        [two.board_id.as_str()],
        "and still filed for the first, once"
    );

    two.th.teardown().await;
}

/// Security fix. The original answers a `DELETE` of a category that is deleted already
/// with the category, whoever asks (`server/app/category.go:104-112`). Here only the
/// owner gets it; [`delete_category_trying_to_delete_already_deleted_category`] is the
/// legitimate case.
#[tokio::test]
async fn another_users_deleted_category_is_not_handed_out() {
    let two = TwoUsers::start().await;
    let path = format!("/teams/0/categories/{}", two.category.id);
    two.th.client.delete(&path).await.check_ok();

    let response = two.th.client2.delete(&path).await;

    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "category doesn't belong to user",
    );
    // The owner still gets it.
    let again: Category = two.th.client.delete(&path).await.ok_json();
    assert_eq!(again.id, two.category.id, "the owner's answer");

    two.th.teardown().await;
}

/// The routes that change a category refuse a user who does not own it, as in the
/// original, and the two that hide and show change nothing.
#[tokio::test]
async fn another_users_category_cannot_be_changed() {
    let two = TwoUsers::start().await;
    let before = two.owner_view().await;
    let intruder = &two.th.client2;
    let path = format!("/teams/0/categories/{}", two.category.id);

    // Update: saying whose category it is gives it away either way.
    let as_owner = Category {
        name: "Taken over".to_owned(),
        ..two.category.clone()
    };
    let response = intruder.put(&path, &as_owner).await;
    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "user ID mismatch in session and category",
    );
    let as_intruder = Category {
        user_id: two.intruder_id.clone(),
        ..as_owner
    };
    let response = intruder.put(&path, &as_intruder).await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "category doesn't belong to user",
    );

    // Delete.
    let response = intruder.delete(&path).await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "category doesn't belong to user",
    );

    // Reorder the boards in it.
    let response = intruder
        .put(&format!("{path}/boards/reorder"), &[two.board_id.as_str()])
        .await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "access denied to category",
    );

    // Reorder the categories: they are not among the intruder's.
    let order: Vec<&str> = ids(&before);
    let response = intruder.put("/teams/0/categories/reorder", &order).await;
    check_internal_error(&response);

    // Hide and show: answered as a success, as for any board that is not there.
    let place = format!("{path}/boards/{}", two.board_id);
    put_empty(intruder, &format!("{place}/hide"))
        .await
        .check_ok();
    assert_eq!(
        two.owner_view().await,
        before,
        "hiding by another user changes nothing"
    );
    put_empty(&two.th.client, &format!("{place}/hide"))
        .await
        .check_ok();
    let hidden_by_owner = two.owner_view().await;
    assert_ne!(hidden_by_owner, before, "the owner hides the board");
    put_empty(intruder, &format!("{place}/unhide"))
        .await
        .check_ok();
    assert_eq!(
        two.owner_view().await,
        hidden_by_owner,
        "showing by another user changes nothing"
    );
    put_empty(&two.th.client, &format!("{place}/unhide"))
        .await
        .check_ok();

    assert_eq!(
        two.owner_view().await,
        before,
        "the owner's view is as before"
    );

    two.th.teardown().await;
}

/// Security fix of the store, checked through the route. The original builds the reorder
/// statement by pasting the identifiers into it, and a category's identifier is the
/// client's to choose (`server/services/store/sqlstore/category.go:221`): with the
/// identifier below, the original answers 200 and lists the categories in another order
/// than the one asked for.
#[tokio::test]
async fn reorder_categories_treats_a_hostile_identifier_as_data() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let hostile_ids = [
        "a' THEN 777 WHEN 'zz",
        "q'q",
        "x'; DROP TABLE test_categories; --",
    ];
    for hostile_id in hostile_ids {
        let hostile = Category {
            id: hostile_id.to_owned(),
            ..category("0", &user.id, "hostile")
        };
        assert_eq!(
            create_category(&th.client, &hostile).await.id,
            hostile_id,
            "the identifier is the client's to choose"
        );
    }
    let plain = create_category(&th.client, &category("0", &user.id, "plain")).await;
    let path = "/teams/0/categories/reorder";

    let new_order = [
        plain.id.as_str(),
        hostile_ids[1],
        default_id.as_str(),
        hostile_ids[2],
        hostile_ids[0],
    ];
    let response = th.client.put(path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), new_order, "exactly the order asked for");

    // A hostile identifier that names no category is refused like any other.
    let unknown = [
        plain.id.as_str(),
        hostile_ids[1],
        default_id.as_str(),
        hostile_ids[2],
        "zz' OR '1'='1",
    ];
    check_internal_error(&th.client.put(path, &unknown).await);
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(ids(&listed), new_order, "and changes nothing");

    th.teardown().await;
}

/// Security fix of the store, checked through the route. The original pastes the board
/// identifiers into the statement too (`server/services/store/sqlstore/category_boards.go:144`),
/// and the identifier comes from the path of the request that files the board: with
/// `q'q` filed, the original answers the reorder request with 500 (a syntax error).
#[tokio::test]
async fn reorder_category_boards_treats_a_hostile_identifier_as_data() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, "0").await;
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    let other = create_category(&th.client, &category("0", &user.id, "Cat B")).await;
    file_board(&th.client, "0", &other.id, "bystander").await;
    // `%27` is the apostrophe and `%20` the space.
    file_board(&th.client, "0", &created.id, "q%27q").await;
    file_board(
        &th.client,
        "0",
        &created.id,
        "b%27%20THEN%205%20WHEN%20%27c",
    )
    .await;
    file_board(&th.client, "0", &created.id, "plain").await;
    let path = format!("/teams/0/categories/{}/boards/reorder", created.id);

    let new_order = ["plain", "b' THEN 5 WHEN 'c", "q'q"];
    let response = th.client.put(&path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(
        board_ids(entry(&listed, &created.id)),
        new_order,
        "exactly the order asked for"
    );

    // A hostile identifier that is not in the category is refused like any other.
    let unknown = ["plain", "q'q", "x' OR category_id <> '"];
    check_internal_error(&th.client.put(&path, &unknown).await);
    let listed = category_boards(&th.client, "0").await;
    assert_eq!(
        board_ids(entry(&listed, &created.id)),
        new_order,
        "and changes nothing"
    );
    assert_eq!(
        board_ids(entry(&listed, &other.id)),
        ["bystander"],
        "the other category is untouched"
    );

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// Not in the original: the websocket messages of the category routes.

/// How long to wait for a message that should arrive.
const PATIENCE: Duration = Duration::from_secs(5);

type Stream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The next JSON message on the socket, if one arrives in time.
async fn next_message(stream: &mut Stream, wait: Duration) -> Option<Value> {
    loop {
        match tokio::time::timeout(wait, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                return Some(serde_json::from_str(&text).expect("messages are JSON"));
            }
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
            _ => return None,
        }
    }
}

/// A socket of the first client's user, subscribed to the team.
async fn subscribed_socket(th: &TestServer, team_id: &str, user_id: &str) -> Stream {
    let (mut stream, _) = tokio_tungstenite::connect_async(th.client.websocket_url())
        .await
        .expect("the websocket endpoint accepts the connection");
    for command in [
        json!({"action": "AUTH", "token": th.client.token}),
        json!({"action": "SUBSCRIBE_TEAM", "teamId": team_id}),
    ] {
        stream
            .send(Message::Text(command.to_string().into()))
            .await
            .expect("the command is sent");
    }
    // Commands are not acknowledged: wait until a broadcast comes through.
    let probe = category(team_id, user_id, "subscription probe");
    for _ in 0..50 {
        th.app.ws().broadcast_category_change(&probe);
        if next_message(&mut stream, Duration::from_millis(100))
            .await
            .is_some()
        {
            while next_message(&mut stream, Duration::from_millis(150))
                .await
                .is_some()
            {}
            return stream;
        }
    }
    panic!("the socket was never subscribed to team {team_id}");
}

/// The next message, which must arrive.
async fn expect_message(stream: &mut Stream, after: &str) -> Value {
    next_message(stream, PATIENCE)
        .await
        .unwrap_or_else(|| panic!("a message arrives after {after}"))
}

/// Creating, updating, reordering and deleting categories is sent to the user's
/// connection.
#[tokio::test]
async fn category_changes_are_sent_to_the_users_connection() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, "0").await[0]
        .category
        .id
        .clone();
    let mut socket = subscribed_socket(&th, "0", &user.id).await;

    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    assert_eq!(
        expect_message(&mut socket, "creating a category").await,
        json!({"action": "UPDATE_CATEGORY", "teamId": "0", "category": created}),
        "after creating a category"
    );

    let path = format!("/teams/0/categories/{}", created.id);
    let change = Category {
        name: "Cat B".to_owned(),
        ..created.clone()
    };
    let renamed: Category = th.client.put(&path, &change).await.ok_json();
    assert_eq!(
        expect_message(&mut socket, "updating a category").await,
        json!({"action": "UPDATE_CATEGORY", "teamId": "0", "category": renamed}),
        "after updating a category"
    );

    let category_order = [default_id.as_str(), created.id.as_str()];
    th.client
        .put("/teams/0/categories/reorder", &category_order)
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "reordering the categories").await,
        json!({
            "action": "REORDER_CATEGORIES",
            "categoryOrder": category_order,
            "teamId": "0",
        }),
        "after reordering the categories"
    );

    // Deleting moves the boards to the default category first, which is its own message.
    file_board(&th.client, "0", &created.id, "board-1").await;
    expect_message(&mut socket, "filing a board").await;
    let deleted: Category = th.client.delete(&path).await.ok_json();
    let moved = json!({"boardID": "board-1", "categoryID": default_id, "hidden": false});
    assert_eq!(
        expect_message(&mut socket, "deleting a category").await,
        json!({"action": "UPDATE_BOARD_CATEGORY", "teamId": "0", "blockCategories": [moved]}),
        "after deleting a category: its board is in the default category"
    );
    assert_eq!(
        expect_message(&mut socket, "deleting a category").await,
        json!({"action": "UPDATE_CATEGORY", "teamId": "0", "category": deleted}),
        "after deleting a category: the category"
    );

    th.teardown().await;
}

/// Filing, hiding, showing and reordering the boards of a category is sent to the user's
/// connection.
#[tokio::test]
async fn category_board_changes_are_sent_to_the_users_connection() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, "0").await;
    let created = create_category(&th.client, &category("0", &user.id, "Cat A")).await;
    let path = format!("/teams/0/categories/{}", created.id);
    let mut socket = subscribed_socket(&th, "0", &user.id).await;
    let board_change = |boards: Value| json!({"action": "UPDATE_BOARD_CATEGORY", "teamId": "0", "blockCategories": boards});
    let shown = json!({"boardID": "board-1", "categoryID": created.id, "hidden": false});
    let hidden = json!({"boardID": "board-1", "categoryID": created.id, "hidden": true});

    // Filing a board sends all the boards of the category.
    file_board(&th.client, "0", &created.id, "board-1").await;
    assert_eq!(
        expect_message(&mut socket, "filing a board").await,
        board_change(json!([shown])),
        "after filing a board"
    );
    file_board(&th.client, "0", &created.id, "board-2").await;
    let message = expect_message(&mut socket, "filing a second board").await;
    assert_eq!(
        message["blockCategories"].as_array().map(Vec::len),
        Some(2),
        "after filing a second board: {message}"
    );

    // Hiding and showing send the one board.
    put_empty(&th.client, &format!("{path}/boards/board-1/hide"))
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "hiding a board").await,
        board_change(json!([hidden])),
        "after hiding a board"
    );
    put_empty(&th.client, &format!("{path}/boards/board-1/unhide"))
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "showing a board").await,
        board_change(json!([shown])),
        "after showing a board"
    );

    let board_order = ["board-2", "board-1"];
    th.client
        .put(&format!("{path}/boards/reorder"), &board_order)
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "reordering the boards").await,
        json!({
            "action": "REORDER_CATEGORY_BOARDS",
            "CategoryId": created.id,
            "BoardOrder": board_order,
            "teamId": "0",
        }),
        "after reordering the boards"
    );

    th.teardown().await;
}

/// Requests of a new user that arrive together create one default category between
/// them. The original looks for the category and creates it without holding anything in
/// between (`server/app/category_boards.go:16-51`); with it, each of the requests would
/// create a category of its own, and boards would be filed under none.
#[tokio::test]
async fn the_default_category_is_created_once_by_concurrent_requests() {
    let th = Setup::native().start().await.init_basic().await;
    let board = |title: String| {
        let client = th.client.clone();
        async move {
            let body = json!({"teamId": "0", "type": "P", "title": title});
            client.post("/boards", &body).await
        }
    };

    let responses =
        futures_util::future::join_all((0..12).map(|n| board(format!("board {n}")))).await;
    for response in &responses {
        response.check_ok();
    }

    let categories = th.client.get("/teams/0/categories").await.value();
    let categories = categories.as_array().expect("a list of categories");
    assert_eq!(categories.len(), 1, "one default category: {categories:?}");
    assert_eq!(
        categories[0]["name"],
        json!("Boards"),
        "named as the original names it"
    );
    assert_eq!(
        categories[0]["boardMetadata"].as_array().map(Vec::len),
        Some(12),
        "with all twelve boards filed"
    );
    th.teardown().await;
}
