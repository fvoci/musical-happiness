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
    Board, BoardType, Category, CategoryBoardMetadata, CategoryBoards, CategoryType, User, is_id,
    new_id,
};
use fvoci_store::StoreError;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::common::{
    ApiResponse, Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer, UNKNOWN_ID,
};

/// What a write answers that was given text where it needs an identifier.
const NOT_AN_IDENTIFIER: &str = "an identifier in the request is not a valid identifier";

/// Stores the user of an application-level test and returns the identifier. Those tests
/// work in [`TEST_TEAM_ID`] for this user, who never signs in; where one needs somebody
/// else, that is the harness's stranger.
async fn stored_user(th: &TestServer) -> String {
    let mut user = User {
        id: new_id(),
        username: "category-owner".to_owned(),
        email: "category-owner@sample.com".to_owned(),
        ..User::default()
    };
    th.app
        .store()
        .create_user(&mut user)
        .await
        .expect("the user is stored");
    user.id
}

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
        id: new_id(),
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
        id: new_id(),
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

/// Puts an open board into the store, of which nobody is a member, and returns its
/// identifier: every user of the team sees it, and it is filed nowhere by itself.
async fn open_board(th: &TestServer, team_id: &str) -> String {
    board_without_member(th, team_id, &th.stranger().await)
        .await
        .id
}

/// Puts `count` open boards into the store, as [`open_board`] does.
async fn open_boards(th: &TestServer, team_id: &str, count: usize) -> Vec<String> {
    let mut boards = Vec::with_capacity(count);
    for _ in 0..count {
        boards.push(open_board(th, team_id).await);
    }
    boards
}

/// Puts a private board into the store that the test's users are no members of, and
/// returns its identifier.
async fn board_of_somebody_else(th: &TestServer, team_id: &str) -> String {
    let mut board = Board {
        id: new_id(),
        team_id: team_id.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    th.app
        .store()
        .insert_board(&mut board, &th.stranger().await)
        .await
        .expect("the board is inserted");
    board.id
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
        .created_json()
}

/// `UpdateCategoryBoard`
async fn file_board(client: &Client, team_id: &str, category_id: &str, board_id: &str) {
    let response = client
        .post_empty(&format!(
            "/teams/{team_id}/categories/{category_id}/boards/{board_id}"
        ))
        .await;
    response.check_no_content();
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

#[track_caller]
fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
    response.check_error(status, message);
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
    let th = Setup::one_user().start().await;
    let user_id = th.client.me().await.id;

    // A new board ends up in the default "Boards" category.
    let board = th.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the new board");

    // A second board is moved into a new category.
    let board2 = th.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let created =
        create_category(&th.client, &category(TEST_TEAM_ID, &user_id, "Category 2")).await;
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
        .check_no_content();
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
    let th = Setup::one_user().start().await;
    let user_id = th.client.me().await.id;

    let board = board_with_admin(&th, TEST_TEAM_ID, &user_id).await;
    let listed = category_boards(&th.client, TEST_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the board");
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::System,
        "the default category is a system category"
    );

    let board2 = board_with_admin(&th, TEST_TEAM_ID, &user_id).await;
    let created =
        create_category(&th.client, &category(TEST_TEAM_ID, &user_id, "Category 2")).await;
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
        .check_no_content();
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
    let th = Setup::one_user().start().await;
    let user_id = th.client.me().await.id;
    board_with_admin(&th, TEST_TEAM_ID, &user_id).await;

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
    response.check_no_content();
    assert!(hidden().await, "hidden now");

    // Hiding a hidden board has no effect.
    put_empty(&th.client, &format!("{place}/hide"))
        .await
        .check_no_content();
    assert!(hidden().await, "still hidden");

    let response = put_empty(&th.client, &format!("{place}/unhide")).await;
    response.check_no_content();
    assert!(!hidden().await, "shown again");

    // Showing a visible board has no effect.
    put_empty(&th.client, &format!("{place}/unhide"))
        .await
        .check_no_content();
    assert!(!hidden().await, "still shown");

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// server/app/category_test.go

/// `TestCreateCategory/"base case"`
#[tokio::test]
async fn create_category_base_case() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;

    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category"))
        .await
        .expect("a valid category is created");

    assert!(is_id(&created.id), "it got an identifier: {}", created.id);
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
    let user_id = stored_user(&th).await;
    let cases = [
        (
            category(TEST_TEAM_ID, &user_id, ""),
            "category name cannot be empty",
        ),
        (
            category(TEST_TEAM_ID, "", "Name"),
            "category user ID cannot be empty",
        ),
        (
            category("", &user_id, "Name"),
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
        .get_user_categories(&user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are read");
    assert!(stored.is_empty(), "nothing was stored: {stored:?}");

    th.teardown().await;
}

/// `TestUpdateCategory/"base case"`
#[tokio::test]
async fn update_category_base_case() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category"))
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
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category"))
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
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Taken over".to_owned(),
        user_id: th.stranger().await,
        ..created.clone()
    };
    let error = th
        .app
        .update_category(change)
        .await
        .expect_err("another user's category cannot be updated");

    // Another user's category, like one of another team, does not exist for this one.
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
    assert_eq!(error.to_string(), "category not found", "the answer");
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// `TestUpdateCategory/"trying to update some other team's category"`
#[tokio::test]
async fn update_category_trying_to_update_some_other_teams_category() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Moved".to_owned(),
        team_id: ROOT_TEAM_ID.to_owned(),
        ..created.clone()
    };
    let error = th
        .app
        .update_category(change)
        .await
        .expect_err("a category of another team cannot be updated");

    // Another user's category, like one of another team, does not exist for this one.
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
    assert_eq!(error.to_string(), "category not found", "the answer");
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// The answer that creates the default category of a user who has other categories is
/// the same as the next one: the categories in their order, with their `sortOrder` as
/// stored. (It was off once: the new category last, the others not yet moved.)
#[tokio::test]
async fn the_answer_that_creates_the_default_category_is_the_next_one() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    for name in ["first", "second"] {
        th.app
            .create_category(Category {
                name: name.to_owned(),
                user_id: user_id.clone(),
                team_id: TEST_TEAM_ID.to_owned(),
                ..Category::default()
            })
            .await
            .expect("a category is made");
    }
    let creating = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
        .await
        .expect("listed");
    let next = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
        .await
        .expect("listed");
    assert_eq!(creating, next);
    assert_eq!(creating.len(), 3, "{creating:?}");
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
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_TEAM_ID).await;

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
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_TEAM_ID).await;
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
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_TEAM_ID).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category 1"))
        .await
        .expect("the category is created");

    let deleted = th
        .app
        .delete_category(&created.id, &user_id, TEST_TEAM_ID)
        .await
        .expect("the owner deletes the category");

    assert_eq!(deleted.id, created.id, "the deleted category is returned");
    assert!(deleted.delete_at > 0, "marked as deleted");
    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
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
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Category 1"))
        .await
        .expect("the category is created");
    let deleted = th
        .app
        .delete_category(&created.id, &user_id, TEST_TEAM_ID)
        .await
        .expect("the owner deletes the category");

    // A deleted category is gone: there is nothing to delete a second time. (The
    // original answers the deleted category again.)
    let error = th
        .app
        .delete_category(&created.id, &user_id, TEST_TEAM_ID)
        .await
        .expect_err("there is no such category any more");
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
    assert_ne!(
        deleted.delete_at, 0,
        "the first answer said when it was deleted"
    );
    let error = th
        .app
        .category(&created.id)
        .await
        .expect_err("it is not stored any more");
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");

    th.teardown().await;
}

/// `TestDeleteCategory/"trying to delete system category"`
#[tokio::test]
async fn delete_category_trying_to_delete_system_category() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_TEAM_ID).await;

    let error = th
        .app
        .delete_category(&system.id, &user_id, TEST_TEAM_ID)
        .await
        .expect_err("a system category cannot be deleted");

    // (The original does not classify this error and answers 500.)
    assert_eq!(error.status(), StatusCode::FORBIDDEN, "{error}");
    assert_eq!(
        error.to_string(),
        "a system category cannot be deleted",
        "the refusal"
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
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_TEAM_ID).await;
    let custom = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Custom Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed = open_boards(&th, TEST_TEAM_ID, 2).await;
    th.app
        .add_update_user_category_board(TEST_TEAM_ID, &user_id, &custom.id, &board_ids_filed)
        .await
        .expect("the boards are filed");

    th.app
        .delete_category(&custom.id, &user_id, TEST_TEAM_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(
        ids(&listed),
        [system.id.as_str()],
        "only the default is left"
    );
    assert_eq!(
        sorted(board_ids(&listed[0])),
        sorted(board_ids_filed.iter().map(String::as_str).collect()),
        "the boards moved to the default category"
    );

    th.teardown().await;
}

/// `TestMoveBoardsToDefaultCategory/"When default category doesn't already exists"`
#[tokio::test]
async fn move_boards_to_default_category_when_default_category_doesnt_already_exists() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    // Created without listing the categories first, so there is no default one yet. The
    // board is filed through the store for the same reason.
    let custom = th
        .app
        .create_category(category(TEST_TEAM_ID, &user_id, "Custom Category 1"))
        .await
        .expect("the category is created");
    let board_id = open_board(&th, TEST_TEAM_ID).await;
    th.app
        .store()
        .add_update_category_board(&user_id, &custom.id, std::slice::from_ref(&board_id))
        .await
        .expect("the board is filed");
    let stored = th
        .app
        .store()
        .get_user_categories(&user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are read");
    assert_eq!(stored.len(), 1, "only the custom category: {stored:?}");

    th.app
        .delete_category(&custom.id, &user_id, TEST_TEAM_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(names(&listed), ["Boards"], "a default category was created");
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::System,
        "as a system category"
    );
    assert_eq!(
        board_ids(&listed[0]),
        [board_id.as_str()],
        "and got the board"
    );

    th.teardown().await;
}

/// Three categories of the user in [`TEST_TEAM_ID`]: the default one and two custom
/// ones, in the order they are listed.
async fn three_categories(th: &TestServer, user_id: &str) -> Vec<String> {
    default_category(th, user_id, TEST_TEAM_ID).await;
    for name in ["Category 2", "Category 3"] {
        th.app
            .create_category(category(TEST_TEAM_ID, user_id, name))
            .await
            .expect("the category is created");
    }
    let listed = th
        .app
        .user_category_boards(user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are listed");
    assert_eq!(
        names(&listed),
        ["Category 3", "Category 2", "Boards"],
        "new categories go on top"
    );
    listed.into_iter().map(|entry| entry.category.id).collect()
}

async fn listed_ids(th: &TestServer, user_id: &str) -> Vec<String> {
    th.app
        .user_category_boards(user_id, TEST_TEAM_ID)
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
    let user_id = stored_user(&th).await;
    let listed = three_categories(&th, &user_id).await;
    let new_order = vec![listed[2].clone(), listed[0].clone(), listed[1].clone()];

    let answered = th
        .app
        .reorder_categories(&user_id, TEST_TEAM_ID, new_order.clone())
        .await
        .expect("an order that names every category is accepted");

    assert_eq!(answered, new_order, "the new order is returned");
    assert_eq!(
        listed_ids(&th, &user_id).await,
        new_order,
        "and is the listed order"
    );

    th.teardown().await;
}

/// `TestReorderCategories/"not specifying all categories should fail"`
#[tokio::test]
async fn reorder_categories_not_specifying_all_categories_should_fail() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let listed = three_categories(&th, &user_id).await;

    let error = th
        .app
        .reorder_categories(
            &user_id,
            TEST_TEAM_ID,
            vec![listed[1].clone(), listed[0].clone()],
        )
        .await
        .expect_err("an order that leaves a category out is refused");

    // (The original does not classify this error and answers 500.)
    assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(
        listed_ids(&th, &user_id).await,
        listed,
        "the order is unchanged"
    );

    th.teardown().await;
}

/// `TestVerifyNewCategoriesMatchExisting/"base case"`
///
/// The function is private; this goes through the route that calls it.
#[tokio::test]
async fn verify_new_categories_match_existing_base_case() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let second = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 3")).await;
    let new_order = [second.id.as_str(), third.id.as_str(), default_id.as_str()];

    let response = th
        .client
        .put(&format!("/teams/{ROOT_TEAM_ID}/category-order"), &new_order)
        .await;

    assert_eq!(
        response.ok_json::<Vec<String>>(),
        new_order,
        "the new order is answered"
    );
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), new_order, "and is the listed order");

    th.teardown().await;
}

/// `TestVerifyNewCategoriesMatchExisting/"different category counts"`
#[tokio::test]
async fn verify_new_categories_match_existing_different_category_counts() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_TEAM_ID).await;
    let second = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 3")).await;
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;

    let response = th
        .client
        .put(
            &format!("/teams/{ROOT_TEAM_ID}/category-order"),
            &[second.id.as_str(), third.id.as_str()],
        )
        .await;

    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "the new order must list every category once: it has 2, the user has 3",
    );
    let after = category_boards(&th.client, ROOT_TEAM_ID).await;
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
        boards.push(board_with_admin(&th, ROOT_TEAM_ID, &user.id).await.id);
    }

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;

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

    let response = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/categories"))
        .await;

    let listed: Vec<CategoryBoards> = response.ok_json();
    assert_eq!(
        names(&listed),
        ["Boards"],
        "the default category was created"
    );
    assert!(listed[0].board_metadata.is_empty(), "with no boards");
    // The exact wire format of an entry.
    let user = th.client.me().await;
    let entry = &response.value()[0];
    assert_eq!(
        *entry,
        json!({
            "id": listed[0].category.id,
            "name": "Boards",
            "userId": user.id,
            "teamId": ROOT_TEAM_ID,
            "createAt": listed[0].category.create_at,
            "updateAt": listed[0].category.create_at,
            "deleteAt": 0,
            "collapsed": false,
            "type": "system",
            "boardMetadata": [],
            // Preserved quirk: a category created at the top is stored one place down.
            "sortOrder": 10,
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
    board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    let first = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(first[0].board_metadata.len(), 2, "two boards to begin with");

    // A board that appears later is not filed by listing the categories.
    board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    let second = category_boards(&th.client, ROOT_TEAM_ID).await;

    assert_eq!(second, first, "the same category with the same boards");

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user doesn't have any boards - implicit or explicit"`
///
/// The function is private; listing the categories of a user who has none calls it.
#[tokio::test]
async fn create_boards_category_user_doesnt_have_any_boards_implicit_or_explicit() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert!(listed[0].board_metadata.is_empty(), "with no boards");
    assert_eq!(listed[0].category.user_id, user_id, "of the user");
    assert_eq!(listed[0].category.team_id, TEST_TEAM_ID, "in the team");

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has implicit access to some board"`
///
/// Implicit access comes from the host application in the original. What a standalone
/// server has of it are open boards that the user can see without being a member.
#[tokio::test]
async fn create_boards_category_user_has_implicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    for _ in 0..3 {
        board_without_member(&th, TEST_TEAM_ID, &th.stranger().await).await;
    }

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
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
    let user_id = stored_user(&th).await;
    let mut boards = Vec::new();
    for _ in 0..3 {
        boards.push(board_with_admin(&th, TEST_TEAM_ID, &user_id).await.id);
    }
    // A board of another team is not filed in this team's default category.
    board_with_admin(&th, ROOT_TEAM_ID, &user_id).await;

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
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
        .user_category_boards(&user_id, TEST_TEAM_ID)
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
    let user_id = stored_user(&th).await;
    let own = board_with_admin(&th, TEST_TEAM_ID, &user_id).await;
    board_without_member(&th, TEST_TEAM_ID, &th.stranger().await).await;
    board_without_member(&th, TEST_TEAM_ID, &th.stranger().await).await;

    let listed = th
        .app
        .user_category_boards(&user_id, TEST_TEAM_ID)
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

/// A category of the user in [`TEST_TEAM_ID`] with `count` boards filed in it, and those
/// boards in the order they are listed.
async fn category_with_boards(
    th: &TestServer,
    user_id: &str,
    count: usize,
) -> (Category, Vec<String>) {
    let created = th
        .app
        .create_category(category(TEST_TEAM_ID, user_id, "Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed = open_boards(th, TEST_TEAM_ID, count).await;
    th.app
        .add_update_user_category_board(TEST_TEAM_ID, user_id, &created.id, &board_ids_filed)
        .await
        .expect("the boards are filed");
    let order = th
        .app
        .reorder_category_boards(user_id, TEST_TEAM_ID, &created.id, board_ids_filed)
        .await
        .expect("the boards are put in order");
    (created, order)
}

async fn listed_board_ids(th: &TestServer, user_id: &str, category_id: &str) -> Vec<String> {
    let listed = th
        .app
        .user_category_boards(user_id, TEST_TEAM_ID)
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
    let user_id = stored_user(&th).await;
    let (created, boards) = category_with_boards(&th, &user_id, 2).await;
    assert_eq!(
        listed_board_ids(&th, &user_id, &created.id).await,
        boards,
        "the order to begin with"
    );
    let reversed = vec![boards[1].clone(), boards[0].clone()];

    let new_order = th
        .app
        .reorder_category_boards(&user_id, TEST_TEAM_ID, &created.id, reversed.clone())
        .await
        .expect("an order that names every board is accepted");

    assert_eq!(new_order, reversed, "the new order");
    assert_eq!(
        listed_board_ids(&th, &user_id, &created.id).await,
        reversed,
        "and is the listed order"
    );

    th.teardown().await;
}

/// `TestReorderCategoryBoards/"not specifying all boards"`
#[tokio::test]
async fn reorder_category_boards_not_specifying_all_boards() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let (created, boards) = category_with_boards(&th, &user_id, 3).await;

    let error = th
        .app
        .reorder_category_boards(
            &user_id,
            TEST_TEAM_ID,
            &created.id,
            vec![boards[1].clone(), boards[0].clone()],
        )
        .await
        .expect_err("an order that leaves a board out is refused");

    // (The original does not classify this error and answers 500.)
    assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{error}");
    assert_eq!(
        listed_board_ids(&th, &user_id, &created.id).await,
        boards,
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
        (Method::GET, &format!("/teams/{ROOT_TEAM_ID}/categories")),
        (Method::POST, &format!("/teams/{ROOT_TEAM_ID}/categories")),
        (
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/category-order"),
        ),
        (
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any"),
        ),
        (
            Method::DELETE,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any"),
        ),
        (
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any/board-order"),
        ),
        (
            Method::POST,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any/boards/any"),
        ),
        (
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any/boards/any/hide"),
        ),
        (
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/categories/any/boards/any/unhide"),
        ),
    ];

    for (method, path) in requests {
        let response = anonymous.send_raw_body(method.clone(), path, "{}").await;
        check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    }

    th.teardown().await;
}

/// The category routes are about a team that exists. A path that names no team answers
/// that, whatever else it names, and alike for an identifier of nothing and for text
/// that is no identifier. (While the server had one team and took any name for it, these
/// requests were answered for their category.)
#[tokio::test]
async fn category_routes_need_a_team_that_exists() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_TEAM_ID).await;
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let board = board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &board.id).await;
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;

    for team_id in [UNKNOWN_ID, "1"] {
        let categories = categories_path(team_id);
        let one = format!("{categories}/{}", created.id);
        let place = format!("{one}/boards/{}", board.id);
        let new = json!({"name": "New", "userId": user.id, "teamId": team_id});
        let change = json!({
            "id": created.id, "name": "Renamed", "userId": user.id, "teamId": team_id,
        });
        let requests = [
            (Method::GET, categories.clone(), Value::Null),
            (Method::POST, categories.clone(), new),
            (
                Method::PUT,
                format!("/teams/{team_id}/category-order"),
                json!(ids(&before)),
            ),
            (Method::PUT, one.clone(), change),
            (Method::DELETE, one.clone(), Value::Null),
            (Method::PUT, format!("{one}/board-order"), json!([board.id])),
            (Method::POST, place.clone(), Value::Null),
            (Method::PUT, format!("{place}/hide"), Value::Null),
            (Method::PUT, format!("{place}/unhide"), Value::Null),
        ];
        for (method, path, body) in requests {
            let response = th
                .client
                .send_raw_body(method, &path, &body.to_string())
                .await;
            check_error(&response, StatusCode::NOT_FOUND, "team not found");
        }
    }

    let after = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(after, before, "none of these changed anything");

    th.teardown().await;
}

/// The default category is created by the first listing, once per team, and is the same
/// from then on.
#[tokio::test]
async fn the_default_category_is_created_once_per_team() {
    let th = Setup::native().start().await.init_basic().await;

    let first = category_boards(&th.client, ROOT_TEAM_ID).await;
    let second = category_boards(&th.client, ROOT_TEAM_ID).await;
    let other_team = category_boards(&th.client, TEST_TEAM_ID).await;
    let other_user = category_boards(&th.client2, ROOT_TEAM_ID).await;

    assert_eq!(names(&first), ["Boards"], "created by the first listing");
    assert_eq!(second, first, "and not again by the second");
    assert_eq!(names(&other_team), ["Boards"], "each team has its own");
    assert_ne!(ids(&other_team), ids(&first), "with its own identifier");
    assert_eq!(other_team[0].category.team_id, TEST_TEAM_ID, "in that team");
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
    let custom = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Boards")).await;

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;

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
            &format!("/teams/{ROOT_TEAM_ID}/categories"),
            &json!({"name": "Cat A", "userId": user.id, "teamId": ROOT_TEAM_ID}),
        )
        .await;
    let created: Category = response.created_json();
    assert_eq!(
        response.value(),
        json!({
            "id": created.id,
            "name": "Cat A",
            "userId": user.id,
            "teamId": ROOT_TEAM_ID,
            "createAt": created.create_at,
            "updateAt": created.create_at,
            "deleteAt": 0,
            "collapsed": false,
            // Preserved quirk: a category created at the top is stored one place down.
            "sortOrder": 10,
            "type": "custom",
        }),
        "the created category"
    );
    assert!(is_id(&created.id), "a generated identifier: {}", created.id);
    assert!(created.create_at > 0, "a creation time");

    // What the client may choose: the times, the collapsed state and the type. A
    // negative sort order counts as zero and a `sorting` member is no member of a
    // category. The identifier is the server's to make.
    let response = th
        .client
        .post(
            &format!("/teams/{ROOT_TEAM_ID}/categories"),
            &json!({
                "name": "With id",
                "userId": user.id,
                "teamId": ROOT_TEAM_ID,
                "sortOrder": -5,
                "createAt": 5,
                "sorting": "alpha",
                "collapsed": true,
                "type": "system",
            }),
        )
        .await;
    let answered = response.created_json::<Value>();
    let made_id = answered["id"].as_str().expect("an identifier").to_owned();
    assert!(is_id(&made_id), "a generated identifier: {made_id}");
    assert_eq!(
        answered,
        json!({
            "id": made_id,
            "name": "With id",
            "userId": user.id,
            "teamId": ROOT_TEAM_ID,
            "createAt": 5,
            "updateAt": 5,
            "deleteAt": 0,
            "collapsed": true,
            "sortOrder": 10,
            "type": "system",
        }),
        "the created category"
    );

    // A blank type and a `null` type are the default type.
    for blank in [json!("  "), json!(""), Value::Null] {
        let response = th
            .client
            .post(
                &format!("/teams/{ROOT_TEAM_ID}/categories"),
                &json!({"name": "Blank type", "userId": user.id, "teamId": ROOT_TEAM_ID, "type": blank}),
            )
            .await;
        assert_eq!(
            response.created_json::<Category>().category_type,
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
    let path = &format!("/teams/{ROOT_TEAM_ID}/categories");
    let invalid_type = "category type is invalid. Allowed types: system and custom";
    // A category is made for the session's user and nobody else, whatever else the
    // body says.
    for body in [
        json!({"name": "x", "userId": other.id, "teamId": ROOT_TEAM_ID}),
        json!({"name": "x", "teamId": ROOT_TEAM_ID}),
        json!({"name": "x", "userId": other.id, "teamId": ROOT_TEAM_ID, "type": "bogus"}),
        // The user under a name that is no member of a category: no user is named.
        json!({"name": "x", "userID": user.id, "teamID": ROOT_TEAM_ID}),
    ] {
        let response = th.client.post(path, &body).await;
        check_error(
            &response,
            StatusCode::FORBIDDEN,
            "a user makes categories for themselves only",
        );
        assert_eq!(response.value()["field"], json!("userId"), "{body}");
    }

    // Another team than the path's: one that exists, one that does not, and text that
    // is no identifier.
    for team_id in [TEST_TEAM_ID, UNKNOWN_ID, "1"] {
        let body = json!({"name": "x", "userId": user.id, "teamId": team_id});
        let response = th.client.post(path, &body).await;
        check_error(&response, StatusCode::BAD_REQUEST, "teamId mismatch");
    }

    let cases = [
        (
            "a blank name",
            json!({"name": "  ", "userId": user.id, "teamId": ROOT_TEAM_ID}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "a `null` name",
            json!({"name": null, "userId": user.id, "teamId": ROOT_TEAM_ID}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "an unknown type",
            json!({"name": "x", "userId": user.id, "teamId": ROOT_TEAM_ID, "type": "bogus"}),
            invalid_type.to_owned(),
        ),
        (
            "a blank name and an unknown type: the name is checked first",
            json!({"name": "", "userId": user.id, "teamId": ROOT_TEAM_ID, "type": "bogus"}),
            "category name cannot be empty".to_owned(),
        ),
    ];
    for (case, body, message) in cases {
        let response = th.client.post(path, &body).await;
        response.check_bad_request();
        assert_eq!(response.error_message(), message, "{case}");
    }

    // A body that is no JSON object with members of the right types is a bad request.
    // (The original answers 500.)
    for malformed in [
        "{not json",
        "",
        "[]",
        "null",
        r#"{"name": 5}"#,
        r#"{"type": 5}"#,
    ] {
        let response = th.client.send_raw_body(Method::POST, path, malformed).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{malformed}");
    }

    // The server makes the identifier. A request that names one is refused whether or
    // not somebody has a category of that identifier: the answer must not tell. (The
    // original stores the client's identifier, and answers 500 for one that is taken.)
    let others = create_category(&th.client2, &category(ROOT_TEAM_ID, &other.id, "Theirs")).await;
    for id in ["fresh", UNKNOWN_ID, others.id.as_str()] {
        let named = json!({"id": id, "name": "x", "userId": user.id, "teamId": ROOT_TEAM_ID});
        let response = th.client.post(path, &named).await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "a new category cannot have an ID",
        );
        assert_eq!(
            response.value()["field"],
            json!("id"),
            "the member at fault"
        );
    }
    th.client
        .post(
            path,
            &json!({"name": "x", "userId": user.id, "teamId": ROOT_TEAM_ID}),
        )
        .await
        .check_created();

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
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
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;

    let response = th
        .client
        .put(
            &format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id),
            &json!({
                "id": created.id,
                "name": "Cat A2",
                "userId": user.id,
                "teamId": ROOT_TEAM_ID,
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
            "userId": user.id,
            "teamId": ROOT_TEAM_ID,
            "createAt": created.create_at,
            "updateAt": updated.update_at,
            "deleteAt": 0,
            "collapsed": true,
            "sortOrder": created.sort_order,
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
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);
    let body = |change: Value| {
        let mut body =
            json!({"id": created.id, "name": "n", "userId": user.id, "teamId": ROOT_TEAM_ID});
        for (name, value) in change.as_object().expect("an object") {
            body[name] = value.clone();
        }
        body
    };

    let bad_requests = [
        (
            body(json!({"id": "other"})),
            "the category of the body is not the one of the path",
        ),
        (
            body(json!({"id": UNKNOWN_ID})),
            "the category of the body is not the one of the path",
        ),
        (
            body(json!({"userId": other.id})),
            "the user of the body is not the session's",
        ),
        (body(json!({"teamId": TEST_TEAM_ID})), "teamId mismatch"),
        (body(json!({"teamId": UNKNOWN_ID})), "teamId mismatch"),
        (body(json!({"teamId": "1"})), "teamId mismatch"),
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

    // A category of another team than the path's, one that does not exist, and
    // another user's all answer alike, whatever the body.
    let others = create_category(&th.client2, &category(ROOT_TEAM_ID, &other.id, "Theirs")).await;
    for (route, request) in [
        (
            format!("/teams/{TEST_TEAM_ID}/categories/{}", created.id),
            body(json!({"teamId": TEST_TEAM_ID})),
        ),
        (
            format!("/teams/{ROOT_TEAM_ID}/categories/{UNKNOWN_ID}"),
            json!({"id": UNKNOWN_ID, "name": "n", "userId": user.id, "teamId": ROOT_TEAM_ID}),
        ),
        (
            format!("/teams/{ROOT_TEAM_ID}/categories/nope"),
            json!({"id": "nope", "name": "n", "userId": user.id, "teamId": ROOT_TEAM_ID}),
        ),
        (
            format!("/teams/{ROOT_TEAM_ID}/categories/{}", others.id),
            json!({"id": others.id, "name": "n", "userId": user.id, "teamId": ROOT_TEAM_ID}),
        ),
    ] {
        let response = th.client.put(&route, &request).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
        let response = th.client.send_raw_body(Method::PUT, &route, "{nope").await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // A body that cannot be decoded is a bad request. (The original answers 500.)
    th.client
        .send_raw_body(Method::PUT, &path, "{nope")
        .await
        .check_bad_request();

    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "none of these changed the category");

    // A deleted category cannot be updated.
    th.client.delete(&path).await.check_no_content();
    let response = th.client.put(&path, &body(json!({}))).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    th.teardown().await;
}

/// What deleting a category answers, and its refusals.
#[tokio::test]
async fn delete_category_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);

    for missing in [UNKNOWN_ID, "nope"] {
        let response = th
            .client
            .delete(&format!("/teams/{ROOT_TEAM_ID}/categories/{missing}"))
            .await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // In another team than its own the category does not exist.
    let response = th
        .client
        .delete(&format!("/teams/{TEST_TEAM_ID}/categories/{}", created.id))
        .await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // (The original answers 500.)
    let response = th
        .client
        .delete(&format!("/teams/{ROOT_TEAM_ID}/categories/{default_id}"))
        .await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "a system category cannot be deleted",
    );

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(names(&listed), ["Cat A", "Boards"], "nothing was deleted");

    // A deletion has no result to answer with.
    let response = th.client.delete(&path).await;
    response.check_no_content();
    let error = th
        .app
        .category(&created.id)
        .await
        .expect_err("the category is gone from the store");
    assert!(error.is_not_found(), "not found: {error:?}");

    // It is gone: there is nothing to delete a second time.
    let response = th.client.delete(&path).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the category is gone");

    th.teardown().await;
}

/// A reorder request must name the user's categories and nothing else.
#[tokio::test]
async fn reorder_categories_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let path = &format!("/teams/{ROOT_TEAM_ID}/category-order");

    // With no categories, the empty order is accepted and answered with the empty
    // list. The user has none before the first listing creates the default one. (The
    // original answers `null`, also to a `null` body.)
    let response = th.client.send_raw_body(Method::PUT, path, "[]").await;
    response.check_ok();
    assert_eq!(response.value(), json!([]), "the answer to the empty order");
    th.client
        .send_raw_body(Method::PUT, path, "null")
        .await
        .check_bad_request();

    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let second = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 2")).await;
    let third = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Category 3")).await;
    let before = [third.id.as_str(), second.id.as_str(), default_id.as_str()];
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), before, "the order to begin with");

    let refused = [
        json!([default_id, second.id]),
        json!([default_id, second.id, "zzz"]),
        json!([default_id, second.id, UNKNOWN_ID]),
        json!([default_id, second.id, third.id, third.id]),
        json!([]),
        Value::Null,
        json!({}),
        json!("reorder"),
    ];
    // (The original answers all of these with 500.)
    for body in refused {
        let response = th.client.put(path, &body).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{body}");
    }
    th.client
        .send_raw_body(Method::PUT, path, "[")
        .await
        .check_bad_request();
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), before, "a refused order changes nothing");

    // Another user's order of these categories is refused: they are not theirs.
    category_boards(&th.client2, ROOT_TEAM_ID).await;
    let response = th.client2.put(path, &json!([default_id])).await;
    response.check_bad_request();
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), before, "and changes nothing");

    let new_order = [default_id.as_str(), third.id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), new_order, "the new order");
    let places: Vec<i64> = listed
        .iter()
        .map(|entry| entry.category.sort_order)
        .collect();
    assert_eq!(
        places,
        [10, 20, 30],
        "the listing tells each category's place, from the first gap"
    );

    // Preserved quirk: only the count and the membership are checked, so naming one
    // category twice passes. The one left out keeps its place value.
    let twice = [default_id.as_str(), default_id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &twice).await;
    assert_eq!(response.ok_json::<Vec<String>>(), twice, "the answer");
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(
        sorted(ids(&listed)),
        sorted(new_order.to_vec()),
        "every category is still listed"
    );

    // A new category comes first, also after a reorder.
    let new = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "New")).await;
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed)[0], new.id, "the new category is first");

    th.teardown().await;
}

/// A board reorder request must name the boards of the category and nothing else.
#[tokio::test]
async fn reorder_category_boards_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let boards = open_boards(&th, ROOT_TEAM_ID, 3).await;
    for board_id in &boards {
        file_board(&th.client, ROOT_TEAM_ID, &created.id, board_id).await;
    }
    let path = format!(
        "/teams/{ROOT_TEAM_ID}/categories/{}/board-order",
        created.id
    );
    let order = async || {
        let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
        board_ids(entry(&listed, &created.id))
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    let new_order = [boards[1].as_str(), boards[2].as_str(), boards[0].as_str()];
    let response = th.client.put(&path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    assert_eq!(order().await, new_order, "the new order");

    let refused = [
        json!([boards[0], boards[1]]),
        json!([boards[0], boards[1], "other"]),
        json!([boards[0], boards[1], UNKNOWN_ID]),
        json!([]),
        Value::Null,
        json!({}),
    ];
    // (The original answers all of these with 500.)
    for body in refused {
        let response = th.client.put(&path, &body).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "{body}");
    }
    th.client
        .send_raw_body(Method::PUT, &path, "{")
        .await
        .check_bad_request();
    assert_eq!(order().await, new_order, "a refused order changes nothing");

    // A category of another team than the path's, and one that does not exist.
    for route in [
        format!(
            "/teams/{TEST_TEAM_ID}/categories/{}/board-order",
            created.id
        ),
        format!("/teams/{ROOT_TEAM_ID}/categories/{UNKNOWN_ID}/board-order"),
        format!("/teams/{ROOT_TEAM_ID}/categories/nope/board-order"),
    ] {
        let response = th.client.put(&route, &new_order).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }
    assert_eq!(order().await, new_order, "a refused order changes nothing");

    // The empty order of an empty category is answered with the empty list. (The
    // original answers `null`.)
    let response = th
        .client
        .send_raw_body(
            Method::PUT,
            &format!("/teams/{ROOT_TEAM_ID}/categories/{default_id}/board-order"),
            "[]",
        )
        .await;
    response.check_ok();
    assert_eq!(response.value(), json!([]), "the answer to the empty order");

    // A deleted category has no boards to order.
    th.client
        .delete(&format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id))
        .await
        .check_no_content();
    let response = th.client.put(&path, &new_order).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    th.teardown().await;
}

/// Filing a board under a category: the answer, the move, and the refusals.
#[tokio::test]
async fn update_category_board_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let board_path = |team_id: &str, category_id: &str| {
        format!(
            "/teams/{team_id}/categories/{category_id}/boards/{}",
            board.id
        )
    };
    let places = async || {
        let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
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

    // (The original answers the bare word `ok`, labelled as JSON.)
    let response = th
        .client
        .post_empty(&board_path(ROOT_TEAM_ID, &created.id))
        .await;
    response.check_no_content();
    assert_eq!(
        places().await,
        (vec![board.id.clone()], vec![]),
        "the board moved"
    );

    // A category that does not exist, is in another team, or is deleted: the original
    // answers these with a server error after moving the board out of sight. Here the
    // board stays where it is.
    let deleted = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Deleted")).await;
    th.client
        .delete(&format!("/teams/{ROOT_TEAM_ID}/categories/{}", deleted.id))
        .await
        .check_no_content();
    for path in [
        board_path(ROOT_TEAM_ID, UNKNOWN_ID),
        board_path(ROOT_TEAM_ID, "nope"),
        board_path(TEST_TEAM_ID, &default_id),
        board_path(ROOT_TEAM_ID, &deleted.id),
    ] {
        let response = th.client.post_empty(&path).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
        assert_eq!(
            places().await,
            (vec![board.id.clone()], vec![]),
            "the board stays after {path}"
        );
    }

    // Only a board the user can see is filed. (The original does not look at the
    // board, so any identifier can be filed.)
    let private = board_of_somebody_else(&th, ROOT_TEAM_ID).await;
    for board_id in [UNKNOWN_ID, "no-such-board", private.as_str()] {
        let response = th
            .client
            .post_empty(&format!(
                "/teams/{ROOT_TEAM_ID}/categories/{default_id}/boards/{board_id}"
            ))
            .await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
    }
    assert_eq!(
        places().await,
        (vec![board.id.clone()], vec![]),
        "nothing was filed"
    );

    th.teardown().await;
}

/// Hiding and showing need a category of the user's and a board the user can see. (The
/// original checks nothing and answers every such request as a success.) A board that
/// is not filed in the category is answered as a success and changes nothing.
#[tokio::test]
async fn hiding_a_board_that_is_not_in_the_category_changes_nothing() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let board = board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(before[0].board_metadata, [visible(&board.id)], "one board");
    let default_id = &before[0].category.id;

    // What does not exist is asked for by an identifier of nothing and by text that is
    // no identifier: the answers are the same.
    for nothing in [UNKNOWN_ID, "nope"] {
        for (path, missing) in [
            (
                format!("/teams/{ROOT_TEAM_ID}/categories/{nothing}/boards/{nothing}/hide"),
                "category not found",
            ),
            (
                format!(
                    "/teams/{ROOT_TEAM_ID}/categories/{nothing}/boards/{}/hide",
                    board.id
                ),
                "category not found",
            ),
            (
                format!("/teams/{ROOT_TEAM_ID}/categories/{nothing}/boards/{nothing}/unhide"),
                "category not found",
            ),
            (
                format!("/teams/{ROOT_TEAM_ID}/categories/{default_id}/boards/{nothing}/hide"),
                "board not found",
            ),
            (
                format!("/teams/{ROOT_TEAM_ID}/categories/{default_id}/boards/{nothing}/unhide"),
                "board not found",
            ),
        ] {
            let response = put_empty(&th.client, &path).await;
            check_error(&response, StatusCode::NOT_FOUND, missing);
        }
    }

    // A board the user sees, in a category of the user's it is not filed in.
    let other = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Other")).await;
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;
    let response = put_empty(
        &th.client,
        &format!(
            "/teams/{ROOT_TEAM_ID}/categories/{}/boards/{}/hide",
            other.id, board.id
        ),
    )
    .await;
    response.check_no_content();

    let after = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(after, before, "nothing changed");

    th.teardown().await;
}

/// The orders have paths of their own, so no word is reserved where the category
/// routes have an identifier: `reorder` there names a category or a board like any
/// other text, and no category or board has it for an identifier.
#[tokio::test]
async fn the_word_reorder_is_an_identifier_like_any_other() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_TEAM_ID).await;
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let board = board_with_admin(&th, ROOT_TEAM_ID, &user.id).await;
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &board.id).await;
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;

    // Among the categories: an update and a deletion of the category `reorder`.
    let as_category = format!("/teams/{ROOT_TEAM_ID}/categories/reorder");
    let mut reversed = ids(&before);
    reversed.reverse();
    let response = th.client.put(&as_category, &reversed).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");
    let response = th.client.delete(&as_category).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // Among the boards of a category: the place of the board `reorder`, which takes
    // `POST` only.
    let as_board = format!(
        "/teams/{ROOT_TEAM_ID}/categories/{}/boards/reorder",
        created.id
    );
    let response = th.client.put(&as_board, &[&board.id]).await;
    check_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method not allowed",
    );
    assert_eq!(response.header("allow"), "POST", "what the path takes");
    let response = th.client.post_empty(&as_board).await;
    check_error(&response, StatusCode::NOT_FOUND, "board not found");

    let after = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(after, before, "none of these changed anything");

    // The paths of the orders take `PUT` and nothing else.
    let response = th
        .client
        .delete(&format!("/teams/{ROOT_TEAM_ID}/category-order"))
        .await;
    check_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method not allowed",
    );
    assert_eq!(response.header("allow"), "PUT", "what the path takes");

    let response = th
        .client
        .post_empty(&format!(
            "/teams/{ROOT_TEAM_ID}/categories/{}/board-order",
            created.id
        ))
        .await;
    check_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method not allowed",
    );
    assert_eq!(response.header("allow"), "PUT", "what the path takes");

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
        category_boards(&th.client, ROOT_TEAM_ID).await;
        category_boards(&th.client2, ROOT_TEAM_ID).await;
        let board = board_with_admin(&th, ROOT_TEAM_ID, &owner.id).await;
        let category =
            create_category(&th.client, &category(ROOT_TEAM_ID, &owner.id, "Private")).await;
        file_board(&th.client, ROOT_TEAM_ID, &category.id, &board.id).await;
        Self {
            th,
            category,
            board_id: board.id,
            intruder_id: intruder.id,
        }
    }

    /// What the owner sees of their categories.
    async fn owner_view(&self) -> Vec<CategoryBoards> {
        category_boards(&self.th.client, ROOT_TEAM_ID).await
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
    let intruder_before = category_boards(&two.th.client2, ROOT_TEAM_ID).await;

    // Another user's category answers as one that does not exist.
    for category_id in [two.category.id.as_str(), UNKNOWN_ID, "nope"] {
        for board_id in [UNKNOWN_ID, "planted", two.board_id.as_str()] {
            let response = two
                .th
                .client2
                .post_empty(&format!(
                    "/teams/{ROOT_TEAM_ID}/categories/{category_id}/boards/{board_id}"
                ))
                .await;
            check_error(&response, StatusCode::NOT_FOUND, "category not found");
        }
    }

    assert_eq!(
        two.owner_view().await,
        before,
        "the owner's categories are untouched"
    );
    assert_eq!(
        category_boards(&two.th.client2, ROOT_TEAM_ID).await,
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
    let own = create_category(
        &two.th.client2,
        &category(ROOT_TEAM_ID, &two.intruder_id, "Mine"),
    )
    .await;

    // The same board identifier, filed by each user under a category of their own.
    file_board(&two.th.client2, ROOT_TEAM_ID, &own.id, &two.board_id).await;

    let listed = category_boards(&two.th.client2, ROOT_TEAM_ID).await;
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
/// with the category, whoever asks (`server/app/category.go:104-112`). Here a deleted
/// category is gone for everyone.
#[tokio::test]
async fn another_users_deleted_category_is_not_handed_out() {
    let two = TwoUsers::start().await;
    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", two.category.id);
    two.th.client.delete(&path).await.check_no_content();

    for client in [&two.th.client2, &two.th.client] {
        let response = client.delete(&path).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    two.th.teardown().await;
}

/// For a user who does not own it, a category does not exist: every route that names it
/// answers as for an identifier nobody has.
#[tokio::test]
async fn another_users_category_cannot_be_changed() {
    let two = TwoUsers::start().await;
    let before = two.owner_view().await;
    let intruder = &two.th.client2;
    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", two.category.id);

    // Update, whoever the body says the category belongs to.
    let as_owner = Category {
        name: "Taken over".to_owned(),
        ..two.category.clone()
    };
    let as_intruder = Category {
        user_id: two.intruder_id.clone(),
        ..as_owner.clone()
    };
    for body in [&as_owner, &as_intruder] {
        let response = intruder.put(&path, body).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // Delete.
    let response = intruder.delete(&path).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // Reorder the boards in it.
    let response = intruder
        .put(&format!("{path}/board-order"), &[two.board_id.as_str()])
        .await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // Reorder the categories: they are not among the intruder's.
    let order: Vec<&str> = ids(&before);
    let response = intruder
        .put(&format!("/teams/{ROOT_TEAM_ID}/category-order"), &order)
        .await;
    response.check_bad_request();

    // The same answers as for a category nobody has.
    for nobodys in [UNKNOWN_ID, "nope"] {
        let nobody = &format!("/teams/{ROOT_TEAM_ID}/categories/{nobodys}");
        let response = intruder.put(nobody, &as_intruder).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
        let response = intruder.delete(nobody).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // Hide and show.
    let place = format!("{path}/boards/{}", two.board_id);
    let response = put_empty(intruder, &format!("{place}/hide")).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");
    assert_eq!(
        two.owner_view().await,
        before,
        "hiding by another user changes nothing"
    );
    put_empty(&two.th.client, &format!("{place}/hide"))
        .await
        .check_no_content();
    let hidden_by_owner = two.owner_view().await;
    assert_ne!(hidden_by_owner, before, "the owner hides the board");
    let response = put_empty(intruder, &format!("{place}/unhide")).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");
    assert_eq!(
        two.owner_view().await,
        hidden_by_owner,
        "showing by another user changes nothing"
    );
    put_empty(&two.th.client, &format!("{place}/unhide"))
        .await
        .check_no_content();

    assert_eq!(
        two.owner_view().await,
        before,
        "the owner's view is as before"
    );

    two.th.teardown().await;
}

/// Text as one segment of a path, with every byte that is no letter or digit
/// percent-encoded, so that the server is handed exactly this text.
fn path_segment(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Checks that the store refused to write because `text` is no identifier.
#[track_caller]
fn check_not_an_identifier<T: std::fmt::Debug>(written: Result<T, StoreError>, text: &str) {
    match written {
        Err(StoreError::InvalidId(refused)) => {
            assert_eq!(refused, text, "the text that was refused");
        }
        other => panic!("the store refuses to write with {text:?}, but answered {other:?}"),
    }
}

/// The stored categories of a user in the root team, places included.
async fn stored_categories(th: &TestServer, user_id: &str) -> Vec<Category> {
    th.app
        .store()
        .get_user_categories(user_id, ROOT_TEAM_ID)
        .await
        .expect("the categories are read")
}

/// Security fix of the store, checked through the route. The original builds the reorder
/// statement by pasting the identifiers into it, and a category's identifier is the
/// client's to choose (`server/services/store/sqlstore/category.go:221`): with the first
/// identifier below, the original answers 200 and lists the categories in another order
/// than the one asked for.
///
/// Here the server makes the identifiers, and text that is not one reaches a statement
/// as a bound value only: no category can be stored under it, an order that names it is
/// refused, the store does not run a statement that would write with it, and in a path
/// it names nothing. None of that changes a row, the user's or anybody else's.
#[tokio::test]
async fn reorder_categories_treats_a_hostile_identifier_as_data() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let plain = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "plain")).await;
    let second = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "second")).await;
    category_boards(&th.client2, ROOT_TEAM_ID).await;
    create_category(&th.client2, &category(ROOT_TEAM_ID, &other.id, "theirs")).await;
    let before = stored_categories(&th, &user.id).await;
    let others_before = stored_categories(&th, &other.id).await;
    let path = &format!("/teams/{ROOT_TEAM_ID}/category-order");

    for hostile_id in [
        "a' THEN 777 WHEN 'zz",
        "q'q",
        "x'; DROP TABLE categories; --",
        "zz' OR '1'='1",
    ] {
        // The API makes its own identifiers. Past it, where the original took any text
        // (as it could from an import or an older database), the category is refused.
        let hostile = Category {
            id: hostile_id.to_owned(),
            ..category(ROOT_TEAM_ID, &user.id, "hostile")
        };
        let error = th
            .app
            .create_category(hostile)
            .await
            .expect_err("no category is stored under text that is no identifier");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST, "{error}");
        assert_eq!(error.to_string(), NOT_AN_IDENTIFIER, "for {hostile_id:?}");

        // An order that names it where a category belongs is refused like any other
        // that names something the user does not have.
        let order = [second.id.as_str(), hostile_id, default_id.as_str()];
        let response = th.client.put(path, &order).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "for {hostile_id:?}");

        // Asked past that check, the store does not run the statement.
        let written = th
            .app
            .store()
            .reorder_categories(&user.id, ROOT_TEAM_ID, order.map(str::to_owned).into())
            .await;
        check_not_an_identifier(written, hostile_id);

        // In a path it names no category.
        let named = format!(
            "/teams/{ROOT_TEAM_ID}/categories/{}",
            path_segment(hostile_id)
        );
        let change = category(ROOT_TEAM_ID, &user.id, "renamed");
        let response = th.client.put(&named, &change).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
        let response = th.client.delete(&named).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    assert_eq!(
        stored_categories(&th, &user.id).await,
        before,
        "the user's categories are as they were, each in its place"
    );
    assert_eq!(
        stored_categories(&th, &other.id).await,
        others_before,
        "and so are another user's"
    );

    // An order of identifiers is still taken, and is exactly the order asked for.
    let new_order = [plain.id.as_str(), default_id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(ids(&listed), new_order, "exactly the order asked for");
    assert_eq!(
        stored_categories(&th, &other.id).await,
        others_before,
        "which moves nobody else's categories"
    );

    th.teardown().await;
}

/// Security fix of the store, checked through the route. The original pastes the board
/// identifiers into the statement too (`server/services/store/sqlstore/category_boards.go:144`),
/// and the identifier comes from the path of the request that files the board: with
/// `q'q` filed, the original answers the reorder request with 500 (a syntax error).
///
/// Here only a board that exists is filed, and no board has text that is no identifier
/// for its identifier: the request that would file it finds no such board, an order
/// that names it is refused, the store does not run a statement that would write with
/// it, and as the category of a path it names nothing. None of that changes what is
/// filed where, nor in which order.
#[tokio::test]
async fn reorder_category_boards_treats_a_hostile_identifier_as_data() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_TEAM_ID).await;
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let other = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat B")).await;
    let bystander = open_board(&th, ROOT_TEAM_ID).await;
    file_board(&th.client, ROOT_TEAM_ID, &other.id, &bystander).await;
    let boards = open_boards(&th, ROOT_TEAM_ID, 3).await;
    for board_id in &boards {
        file_board(&th.client, ROOT_TEAM_ID, &created.id, board_id).await;
    }
    let category_path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);
    let path = format!("{category_path}/board-order");
    let new_order = [boards[2].as_str(), boards[0].as_str(), boards[1].as_str()];
    let response = th.client.put(&path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let before = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(
        board_ids(entry(&before, &created.id)),
        new_order,
        "exactly the order asked for"
    );

    for hostile_id in [
        "q'q",
        "b' THEN 5 WHEN 'c",
        "x' OR category_id <> '",
        "x'; DROP TABLE category_boards; --",
    ] {
        let segment = path_segment(hostile_id);

        // The request that files a board finds no board of that name, and the store,
        // asked past the route, does not file one.
        let response = th
            .client
            .post_empty(&format!("{category_path}/boards/{segment}"))
            .await;
        check_error(&response, StatusCode::NOT_FOUND, "board not found");
        let filed = th
            .app
            .store()
            .add_update_category_board(&user.id, &created.id, &[hostile_id.to_owned()])
            .await;
        check_not_an_identifier(filed, hostile_id);

        // An order that names it where a board belongs is refused like any other that
        // names a board the category does not have; the store does not run the statement.
        let order = [boards[0].as_str(), hostile_id, boards[2].as_str()];
        let response = th.client.put(&path, &order).await;
        response.check_bad_request();
        assert_eq!(response.error_code(), "bad_request", "for {hostile_id:?}");
        let written = th
            .app
            .store()
            .reorder_category_boards(&created.id, order.map(str::to_owned).into())
            .await;
        check_not_an_identifier(written, hostile_id);

        // As the category of the path it names nothing.
        let response = th
            .client
            .put(
                &format!("/teams/{ROOT_TEAM_ID}/categories/{segment}/board-order"),
                &new_order,
            )
            .await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    let listed = category_boards(&th.client, ROOT_TEAM_ID).await;
    assert_eq!(
        board_ids(entry(&listed, &created.id)),
        new_order,
        "the boards of the category are in the order they were"
    );
    assert_eq!(
        board_ids(entry(&listed, &other.id)),
        [bystander.as_str()],
        "the other category is untouched"
    );
    assert_eq!(listed, before, "and nothing else changed either");

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
    subscribed_socket_of(th, &th.client, team_id, user_id).await
}

/// A socket of the client's user, `user_id`, subscribed to the team.
async fn subscribed_socket_of(
    th: &TestServer,
    client: &Client,
    team_id: &str,
    user_id: &str,
) -> Stream {
    let mut stream = client.connect_websocket().await;
    let command = json!({"action": "SUBSCRIBE_TEAM", "teamId": team_id});
    stream
        .send(Message::Text(command.to_string().into()))
        .await
        .expect("the command is sent");
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
    let default_id = category_boards(&th.client, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let mut socket = subscribed_socket(&th, ROOT_TEAM_ID, &user.id).await;

    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    assert_eq!(
        expect_message(&mut socket, "creating a category").await,
        json!({"action": "UPDATE_CATEGORY", "teamId": ROOT_TEAM_ID, "category": created}),
        "after creating a category"
    );

    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);
    let change = Category {
        name: "Cat B".to_owned(),
        ..created.clone()
    };
    let renamed: Category = th.client.put(&path, &change).await.ok_json();
    assert_eq!(
        expect_message(&mut socket, "updating a category").await,
        json!({"action": "UPDATE_CATEGORY", "teamId": ROOT_TEAM_ID, "category": renamed}),
        "after updating a category"
    );

    let category_order = [default_id.as_str(), created.id.as_str()];
    th.client
        .put(
            &format!("/teams/{ROOT_TEAM_ID}/category-order"),
            &category_order,
        )
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "reordering the categories").await,
        json!({
            "action": "REORDER_CATEGORIES",
            "categoryOrder": category_order,
            "teamId": ROOT_TEAM_ID,
        }),
        "after reordering the categories"
    );

    // Deleting moves the boards to the default category first, which is its own message.
    let board_id = open_board(&th, ROOT_TEAM_ID).await;
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &board_id).await;
    expect_message(&mut socket, "filing a board").await;
    let stored = th
        .app
        .category(&created.id)
        .await
        .expect("the category is stored");
    th.client.delete(&path).await.check_no_content();
    let moved = json!({"boardId": board_id, "categoryId": default_id, "hidden": false});
    assert_eq!(
        expect_message(&mut socket, "deleting a category").await,
        json!({"action": "UPDATE_BOARD_CATEGORY", "teamId": ROOT_TEAM_ID, "boardCategories": [moved]}),
        "after deleting a category: its board is in the default category"
    );
    // The answer has no body: the message is where the deleted category is told.
    let message = expect_message(&mut socket, "deleting a category").await;
    let delete_at = message["category"]["deleteAt"].as_i64().unwrap_or_default();
    assert!(delete_at > 0, "marked as deleted: {message}");
    let deleted = Category {
        delete_at,
        ..stored
    };
    assert_eq!(
        message,
        json!({"action": "UPDATE_CATEGORY", "teamId": ROOT_TEAM_ID, "category": deleted}),
        "after deleting a category: the category as it was"
    );

    th.teardown().await;
}

/// Filing, hiding, showing and reordering the boards of a category is sent to the user's
/// connection.
#[tokio::test]
async fn category_board_changes_are_sent_to_the_users_connection() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_TEAM_ID).await;
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);
    let mut socket = subscribed_socket(&th, ROOT_TEAM_ID, &user.id).await;
    let board_change = |boards: Value| json!({"action": "UPDATE_BOARD_CATEGORY", "teamId": ROOT_TEAM_ID, "boardCategories": boards});
    let first = open_board(&th, ROOT_TEAM_ID).await;
    let second = open_board(&th, ROOT_TEAM_ID).await;
    let shown = json!({"boardId": first, "categoryId": created.id, "hidden": false});
    let hidden = json!({"boardId": first, "categoryId": created.id, "hidden": true});

    // Filing a board sends all the boards of the category.
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &first).await;
    assert_eq!(
        expect_message(&mut socket, "filing a board").await,
        board_change(json!([shown])),
        "after filing a board"
    );
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &second).await;
    let message = expect_message(&mut socket, "filing a second board").await;
    assert_eq!(
        message["boardCategories"].as_array().map(Vec::len),
        Some(2),
        "after filing a second board: {message}"
    );

    // Hiding and showing send the one board.
    put_empty(&th.client, &format!("{path}/boards/{first}/hide"))
        .await
        .check_no_content();
    assert_eq!(
        expect_message(&mut socket, "hiding a board").await,
        board_change(json!([hidden])),
        "after hiding a board"
    );
    put_empty(&th.client, &format!("{path}/boards/{first}/unhide"))
        .await
        .check_no_content();
    assert_eq!(
        expect_message(&mut socket, "showing a board").await,
        board_change(json!([shown])),
        "after showing a board"
    );

    let board_order = [second.as_str(), first.as_str()];
    th.client
        .put(&format!("{path}/board-order"), &board_order)
        .await
        .check_ok();
    assert_eq!(
        expect_message(&mut socket, "reordering the boards").await,
        json!({
            "action": "REORDER_CATEGORY_BOARDS",
            "categoryId": created.id,
            "boardOrder": board_order,
            "teamId": ROOT_TEAM_ID,
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
            let body = json!({"teamId": ROOT_TEAM_ID, "type": "P", "title": title});
            client.post("/boards", &body).await
        }
    };

    let responses =
        futures_util::future::join_all((0..12).map(|n| board(format!("board {n}")))).await;
    for response in &responses {
        response.check_created();
    }

    let categories = th
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/categories"))
        .await
        .value();
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

/// The first message of the action that arrives, passing over the others.
async fn message_of(stream: &mut Stream, action: &str) -> Value {
    loop {
        let message = expect_message(stream, action).await;
        if message["action"] == json!(action) {
            return message;
        }
    }
}

/// Not in the original (user decision 2026-11-11 14:25 KST). A deleted board that comes
/// back is where each user had filed it, hidden as it was, or in the user's default
/// category if that one went meanwhile; each such user's connection is told the board's
/// entry and the category's order, and a user who had not filed it is told nothing.
#[tokio::test]
async fn a_restored_board_is_filed_where_it_was_and_the_users_are_told() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let other = th.client2.me().await;
    let third = th.add_user("third").await;
    let third_user = third.me().await;
    let other_default = category_boards(&th.client2, ROOT_TEAM_ID).await[0]
        .category
        .id
        .clone();
    let created = create_category(&th.client, &category(ROOT_TEAM_ID, &user.id, "Cat A")).await;
    let gone = create_category(&th.client2, &category(ROOT_TEAM_ID, &other.id, "Cat B")).await;
    let board: Board = th
        .client
        .create_board(&Board {
            team_id: ROOT_TEAM_ID.to_owned(),
            board_type: BoardType::Open,
            ..Board::default()
        })
        .await
        .created_json();
    file_board(&th.client, ROOT_TEAM_ID, &created.id, &board.id).await;
    file_board(&th.client2, ROOT_TEAM_ID, &gone.id, &board.id).await;
    let category_path = format!("/teams/{ROOT_TEAM_ID}/categories/{}", created.id);
    put_empty(
        &th.client,
        &format!("{category_path}/boards/{}/hide", board.id),
    )
    .await
    .check_no_content();
    let mut socket = subscribed_socket(&th, ROOT_TEAM_ID, &user.id).await;
    let mut other_socket = subscribed_socket_of(&th, &th.client2, ROOT_TEAM_ID, &other.id).await;
    let mut third_socket = subscribed_socket_of(&th, &third, ROOT_TEAM_ID, &third_user.id).await;

    th.client
        .delete(&format!("/boards/{}", board.id))
        .await
        .check_no_content();
    th.client2
        .delete(&format!("/teams/{ROOT_TEAM_ID}/categories/{}", gone.id))
        .await
        .check_no_content();
    th.client
        .post_empty(&format!("/boards/{}/undelete", board.id))
        .await
        .check_ok();

    let entry = |category_id: &str, hidden: bool| {
        json!({
            "action": "UPDATE_BOARD_CATEGORY",
            "teamId": ROOT_TEAM_ID,
            "boardCategories": [{"boardId": board.id, "categoryId": category_id, "hidden": hidden}],
        })
    };
    assert_eq!(
        message_of(&mut socket, "UPDATE_BOARD_CATEGORY").await,
        entry(&created.id, true)
    );
    let order = message_of(&mut socket, "REORDER_CATEGORY_BOARDS").await;
    assert_eq!(order["categoryId"], json!(created.id), "{order}");
    assert_eq!(order["boardOrder"], json!([board.id]), "{order}");
    assert_eq!(
        message_of(&mut other_socket, "UPDATE_BOARD_CATEGORY").await,
        entry(&other_default, false),
        "the other user's category went: the default one"
    );
    while let Some(message) = next_message(&mut third_socket, Duration::from_millis(500)).await {
        assert_ne!(
            message["action"],
            json!("UPDATE_BOARD_CATEGORY"),
            "{message}"
        );
    }
    let filed = category_boards(&th.client, ROOT_TEAM_ID)
        .await
        .into_iter()
        .find(|entry| entry.category.id == created.id)
        .expect("the category");
    assert_eq!(
        filed.board_metadata,
        [CategoryBoardMetadata {
            board_id: board.id.clone(),
            hidden: true,
        }],
        "the sidebar has it where it was"
    );

    th.teardown().await;
}
