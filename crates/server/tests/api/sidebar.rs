//! Ports of `server/integrationtests/sidebar_test.go`, `server/app/category_test.go` and
//! `server/app/category_boards_test.go`, plus tests of the category routes that the
//! original has none for: validation, the exact answers, and what one user can do to the
//! categories of another.
//!
//! The original's application tests run against a mocked store. Here they run against a
//! real one, so each checks what the mock sequence stood for.

use fvoci_model::{
    Category, CategoryContainerMetadata, CategoryContainers, CategoryType, Container,
    ContainerType, User, is_id, new_id,
};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::ROOT_WS;
use crate::common::TEST_WS;
use crate::common::{
    ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TestServer, UNKNOWN_ID,
};

/// What a write answers that was given text where it needs an identifier.
pub(crate) const NOT_AN_IDENTIFIER: &str = "an identifier in the request is not a valid identifier";

/// Stores the user of an application-level test, a member of [`TEST_WORKSPACE_ID`] who never
/// signs in, and returns the identifier; somebody else is the harness's stranger.
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
    th.let_into_the_second_workspace(&user.id).await;
    user.id
}

pub(crate) fn category(workspace_id: &str, user_id: &str, name: &str) -> Category {
    Category {
        name: name.to_owned(),
        user_id: user_id.to_owned(),
        workspace_id: workspace_id.to_owned(),
        ..Category::default()
    }
}

/// Puts a board into the store without going through the board routes, with `user_id` as
/// its administrator.
async fn board_with_admin(th: &TestServer, workspace_id: &str, user_id: &str) -> Container {
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Open,
        ..Container::default()
    };
    th.app
        .store()
        .insert_container_with_admin(&mut board, user_id)
        .await
        .expect("the board is inserted");
    board
}

/// Puts an open board into the store that `creator_id` created but is not a member of.
async fn board_without_member(th: &TestServer, workspace_id: &str, creator_id: &str) -> Container {
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Open,
        ..Container::default()
    };
    th.app
        .store()
        .insert_container(&mut board, creator_id)
        .await
        .expect("the board is inserted");
    board
}

/// Puts an open board into the store, of which nobody is a member, and returns its
/// identifier: every user of the workspace sees it, and it is filed nowhere by itself.
pub(crate) async fn open_board(th: &TestServer, workspace_id: &str) -> String {
    board_without_member(th, workspace_id, &th.stranger().await)
        .await
        .id
}

/// Puts `count` open boards into the store, as [`open_board`] does.
pub(crate) async fn open_boards(th: &TestServer, workspace_id: &str, count: usize) -> Vec<String> {
    let mut boards = Vec::with_capacity(count);
    for _ in 0..count {
        boards.push(open_board(th, workspace_id).await);
    }
    boards
}

/// Puts a private board into the store that the test's users are no members of, and
/// returns its identifier.
async fn board_of_somebody_else(th: &TestServer, workspace_id: &str) -> String {
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Private,
        ..Container::default()
    };
    th.app
        .store()
        .insert_container(&mut board, &th.stranger().await)
        .await
        .expect("the board is inserted");
    board.id
}

fn categories_path(workspace_id: &str) -> String {
    format!("/w/{workspace_id}/categories")
}

/// `GetUserCategoryBoards`
pub(crate) async fn category_boards(
    client: &Client,
    workspace_id: &str,
) -> Vec<CategoryContainers> {
    client.get(&categories_path(workspace_id)).await.ok_json()
}

/// `CreateCategory`
pub(crate) async fn create_category(client: &Client, category: &Category) -> Category {
    client
        .post(&categories_path(&category.workspace_id), category)
        .await
        .created_json()
}

/// `UpdateCategoryBoard`
pub(crate) async fn file_board(
    client: &Client,
    workspace_id: &str,
    category_id: &str,
    board_id: &str,
) {
    let response = client
        .post_empty(&format!(
            "/w/{workspace_id}/categories/{category_id}/containers/{board_id}"
        ))
        .await;
    response.check_no_content();
}

pub(crate) async fn put_empty(client: &Client, path: &str) -> ApiResponse {
    Client::send(client.request(Method::PUT, path)).await
}

fn names(category_boards: &[CategoryContainers]) -> Vec<&str> {
    category_boards
        .iter()
        .map(|entry| entry.category.name.as_str())
        .collect()
}

pub(crate) fn ids(category_boards: &[CategoryContainers]) -> Vec<&str> {
    category_boards
        .iter()
        .map(|entry| entry.category.id.as_str())
        .collect()
}

pub(crate) fn board_ids(entry: &CategoryContainers) -> Vec<&str> {
    entry
        .container_metadata
        .iter()
        .map(|metadata| metadata.container_id.as_str())
        .collect()
}

fn sorted(mut values: Vec<&str>) -> Vec<&str> {
    values.sort_unstable();
    values
}

fn visible(board_id: &str) -> CategoryContainerMetadata {
    CategoryContainerMetadata {
        container_id: board_id.to_owned(),
        hidden: false,
    }
}

/// The one entry with the given category identifier.
#[track_caller]
pub(crate) fn entry<'a>(
    category_boards: &'a [CategoryContainers],
    category_id: &str,
) -> &'a CategoryContainers {
    category_boards
        .iter()
        .find(|entry| entry.category.id == category_id)
        .unwrap_or_else(|| panic!("category {category_id} is listed"))
}

#[track_caller]
pub(crate) fn check_error(response: &ApiResponse, status: StatusCode, message: &str) {
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
    let board = th
        .create_board(TEST_WORKSPACE_ID, ContainerType::Open)
        .await;

    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the new board");

    // A second board is moved into a new category.
    let board2 = th
        .create_board(TEST_WORKSPACE_ID, ContainerType::Open)
        .await;
    let created = create_category(
        &th.client,
        &category(TEST_WORKSPACE_ID, &user_id, "Category 2"),
    )
    .await;
    file_board(&th.client, TEST_WORKSPACE_ID, &created.id, &board2.id).await;

    // New categories end up on top.
    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Category 2", "Boards"], "two categories");
    assert_eq!(
        board_ids(&listed[0]),
        [board2.id.as_str()],
        "the moved board"
    );

    // Deleting the category moves its boards to the default one.
    th.client
        .delete(&format!(
            "{}/{}",
            categories_path(TEST_WORKSPACE_ID),
            created.id
        ))
        .await
        .check_no_content();
    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the default category is left");
    assert_eq!(
        listed[0].container_metadata.len(),
        2,
        "both boards are in it"
    );
    assert!(
        listed[0].container_metadata.contains(&visible(&board.id)),
        "the first board is there"
    );
    assert!(
        listed[0].container_metadata.contains(&visible(&board2.id)),
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

    let board = board_with_admin(&th, TEST_WORKSPACE_ID, &user_id).await;
    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(board_ids(&listed[0]), [board.id.as_str()], "the board");
    assert_eq!(
        listed[0].category.category_type,
        CategoryType::System,
        "the default category is a system category"
    );

    let board2 = board_with_admin(&th, TEST_WORKSPACE_ID, &user_id).await;
    let created = create_category(
        &th.client,
        &category(TEST_WORKSPACE_ID, &user_id, "Category 2"),
    )
    .await;
    file_board(&th.client, TEST_WORKSPACE_ID, &created.id, &board2.id).await;

    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
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
        .delete(&format!(
            "{}/{}",
            categories_path(TEST_WORKSPACE_ID),
            created.id
        ))
        .await
        .check_no_content();
    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the default category is left");
    assert_eq!(
        sorted(board_ids(&listed[0])),
        sorted(vec![board.id.as_str(), board2.id.as_str()]),
        "both boards are in it"
    );
    assert!(
        listed[0]
            .container_metadata
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
    board_with_admin(&th, TEST_WORKSPACE_ID, &user_id).await;

    // The board is not hidden to begin with.
    let listed = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "only the default category");
    assert_eq!(listed[0].container_metadata.len(), 1, "one board");
    assert!(!listed[0].container_metadata[0].hidden, "not hidden");

    let place = format!(
        "{}/{}/containers/{}",
        categories_path(TEST_WORKSPACE_ID),
        listed[0].category.id,
        listed[0].container_metadata[0].container_id
    );
    let hidden = async || {
        category_boards(&th.client, TEST_WORKSPACE_ID).await[0].container_metadata[0].hidden
    };

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
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category"))
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
            category(TEST_WORKSPACE_ID, &user_id, ""),
            "category name cannot be empty",
        ),
        (
            category(TEST_WORKSPACE_ID, "", "Name"),
            "category user ID cannot be empty",
        ),
        (
            category("", &user_id, "Name"),
            "category workspace id ID cannot be empty",
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
        .get_user_categories(&user_id, TEST_WORKSPACE_ID)
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
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category"))
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
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category"))
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
                workspace_id: String::new(),
                ..created.clone()
            },
            "category workspace id ID cannot be empty",
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
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category"))
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

    // Another user's category, like one of another workspace, does not exist for this one.
    assert_eq!(error.status(), StatusCode::NOT_FOUND, "{error}");
    assert_eq!(error.to_string(), "category not found", "the answer");
    let stored = th.app.category(&created.id).await.expect("it is stored");
    assert_eq!(stored, created, "nothing changed");

    th.teardown().await;
}

/// `TestUpdateCategory/"trying to update some other team's category"`
#[tokio::test]
async fn update_category_trying_to_update_some_other_workspaces_category() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let created = th
        .app
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category"))
        .await
        .expect("the category is created");

    let change = Category {
        name: "Moved".to_owned(),
        workspace_id: ROOT_WORKSPACE_ID.to_owned(),
        ..created.clone()
    };
    let error = th
        .app
        .update_category(change)
        .await
        .expect_err("a category of another workspace cannot be updated");

    // Another user's category, like one of another workspace, does not exist for this one.
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
                workspace_id: TEST_WORKSPACE_ID.to_owned(),
                ..Category::default()
            })
            .await
            .expect("a category is made");
    }
    let creating = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
        .await
        .expect("listed");
    let next = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
        .await
        .expect("listed");
    assert_eq!(creating, next);
    assert_eq!(creating.len(), 3, "{creating:?}");
    th.teardown().await;
}

/// The default category of a user, created by listing the user's categories.
async fn default_category(th: &TestServer, user_id: &str, workspace_id: &str) -> Category {
    let listed = th
        .app
        .user_category_containers(user_id, workspace_id)
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
    let system = default_category(&th, &user_id, TEST_WORKSPACE_ID).await;

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
    let system = default_category(&th, &user_id, TEST_WORKSPACE_ID).await;
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
    let system = default_category(&th, &user_id, TEST_WORKSPACE_ID).await;
    let created = th
        .app
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category 1"))
        .await
        .expect("the category is created");

    let deleted = th
        .app
        .delete_category(&created.id, &user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the owner deletes the category");

    assert_eq!(deleted.id, created.id, "the deleted category is returned");
    assert!(deleted.delete_at > 0, "marked as deleted");
    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Category 1"))
        .await
        .expect("the category is created");
    let deleted = th
        .app
        .delete_category(&created.id, &user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the owner deletes the category");

    // A deleted category is gone: there is nothing to delete a second time. (The
    // original answers the deleted category again.)
    let error = th
        .app
        .delete_category(&created.id, &user_id, TEST_WORKSPACE_ID)
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
    let system = default_category(&th, &user_id, TEST_WORKSPACE_ID).await;

    let error = th
        .app
        .delete_category(&system.id, &user_id, TEST_WORKSPACE_ID)
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
async fn move_containers_to_default_category_when_default_category_already_exists() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let system = default_category(&th, &user_id, TEST_WORKSPACE_ID).await;
    let custom = th
        .app
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Custom Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed = open_boards(&th, TEST_WORKSPACE_ID, 2).await;
    th.app
        .add_update_user_category_container(
            TEST_WORKSPACE_ID,
            &user_id,
            &custom.id,
            &board_ids_filed,
        )
        .await
        .expect("the boards are filed");

    th.app
        .delete_category(&custom.id, &user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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
async fn move_containers_to_default_category_when_default_category_doesnt_already_exists() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    // Created without listing the categories first, so there is no default one yet. The
    // board is filed through the store for the same reason.
    let custom = th
        .app
        .create_category(category(TEST_WORKSPACE_ID, &user_id, "Custom Category 1"))
        .await
        .expect("the category is created");
    let board_id = open_board(&th, TEST_WORKSPACE_ID).await;
    th.app
        .store()
        .add_update_category_container(&user_id, &custom.id, std::slice::from_ref(&board_id))
        .await
        .expect("the board is filed");
    let stored = th
        .app
        .store()
        .get_user_categories(&user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the categories are read");
    assert_eq!(stored.len(), 1, "only the custom category: {stored:?}");

    th.app
        .delete_category(&custom.id, &user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the category is deleted");

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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

/// Three categories of the user in [`TEST_WORKSPACE_ID`]: the default one and two custom
/// ones, in the order they are listed.
async fn three_categories(th: &TestServer, user_id: &str) -> Vec<String> {
    default_category(th, user_id, TEST_WORKSPACE_ID).await;
    for name in ["Category 2", "Category 3"] {
        th.app
            .create_category(category(TEST_WORKSPACE_ID, user_id, name))
            .await
            .expect("the category is created");
    }
    let listed = th
        .app
        .user_category_containers(user_id, TEST_WORKSPACE_ID)
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
        .user_category_containers(user_id, TEST_WORKSPACE_ID)
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
        .reorder_categories(&user_id, TEST_WORKSPACE_ID, new_order.clone())
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
            TEST_WORKSPACE_ID,
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
    let default_id = category_boards(&th.client, ROOT_WORKSPACE_ID).await[0]
        .category
        .id
        .clone();
    let second = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 2"),
    )
    .await;
    let third = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 3"),
    )
    .await;
    let new_order = [second.id.as_str(), third.id.as_str(), default_id.as_str()];

    let response = th
        .client
        .put(&format!("{ROOT_WS}/category-order"), &new_order)
        .await;

    assert_eq!(
        response.ok_json::<Vec<String>>(),
        new_order,
        "the new order is answered"
    );
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(ids(&listed), new_order, "and is the listed order");

    th.teardown().await;
}

/// `TestVerifyNewCategoriesMatchExisting/"different category counts"`
#[tokio::test]
async fn verify_new_categories_match_existing_different_category_counts() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let second = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 2"),
    )
    .await;
    let third = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 3"),
    )
    .await;
    let before = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

    let response = th
        .client
        .put(
            &format!("{ROOT_WS}/category-order"),
            &[second.id.as_str(), third.id.as_str()],
        )
        .await;

    check_error(
        &response,
        StatusCode::BAD_REQUEST,
        "the new order must list every category once: it has 2, the user has 3",
    );
    let after = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(ids(&after), ids(&before), "the order is unchanged");

    th.teardown().await;
}

// ---------------------------------------------------------------------------------------
// server/app/category_boards_test.go

/// `TestGetUserCategoryBoards/"user had no default category and had boards"`
#[tokio::test]
async fn get_user_category_containers_user_had_no_default_category_and_had_boards() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let mut boards = Vec::new();
    for _ in 0..3 {
        boards.push(board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await.id);
    }

    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

    assert_eq!(
        names(&listed),
        ["Boards"],
        "the default category was created"
    );
    assert_eq!(
        listed[0].container_metadata.len(),
        3,
        "with the three boards"
    );
    for board_id in &boards {
        assert!(
            listed[0].container_metadata.contains(&visible(board_id)),
            "board {board_id} is in it, not hidden"
        );
    }

    th.teardown().await;
}

/// `TestGetUserCategoryBoards/"user had no default category BUT had no boards"`
#[tokio::test]
async fn get_user_category_containers_user_had_no_default_category_but_had_no_boards() {
    let th = Setup::native().start().await.init_basic().await;

    let response = th.client.get(&format!("{ROOT_WS}/categories")).await;

    let listed: Vec<CategoryContainers> = response.ok_json();
    assert_eq!(
        names(&listed),
        ["Boards"],
        "the default category was created"
    );
    assert!(listed[0].container_metadata.is_empty(), "with no boards");
    // The exact wire format of an entry.
    let user = th.client.me().await;
    let entry = &response.value()[0];
    assert_eq!(
        *entry,
        json!({
            "id": listed[0].category.id,
            "name": "Boards",
            "userId": user.id,
            "workspaceId": ROOT_WORKSPACE_ID,
            "createAt": listed[0].category.create_at,
            "updateAt": listed[0].category.create_at,
            "deleteAt": 0,
            "collapsed": false,
            "type": "system",
            "containerMetadata": [],
            // Preserved quirk: a category created at the top is stored one place down.
            "sortOrder": 10,
        }),
        "the entry"
    );

    th.teardown().await;
}

/// `TestGetUserCategoryBoards/"user already had a default Boards category with boards in it"`
#[tokio::test]
async fn get_user_category_containers_user_already_had_a_default_boards_category_with_boards_in_it()
{
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    let first = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(
        first[0].container_metadata.len(),
        2,
        "two boards to begin with"
    );

    // A board that appears later is not filed by listing the categories.
    board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    let second = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

    assert_eq!(second, first, "the same category with the same boards");

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user doesn't have any boards - implicit or explicit"`
///
/// The function is private; listing the categories of a user who has none calls it.
#[tokio::test]
async fn create_containers_category_user_doesnt_have_any_boards_implicit_or_explicit() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert!(listed[0].container_metadata.is_empty(), "with no boards");
    assert_eq!(listed[0].category.user_id, user_id, "of the user");
    assert_eq!(
        listed[0].category.workspace_id, TEST_WORKSPACE_ID,
        "in the workspace"
    );

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has implicit access to some board"`
///
/// Implicit access comes from the host application in the original. What a standalone
/// server has of it are open boards that the user can see without being a member.
#[tokio::test]
async fn create_containers_category_user_has_implicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    for _ in 0..3 {
        board_without_member(&th, TEST_WORKSPACE_ID, &th.stranger().await).await;
    }

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the categories are listed");

    assert_eq!(names(&listed), ["Boards"], "the default category");
    assert!(
        listed[0].container_metadata.is_empty(),
        "boards the user is not a member of are not filed"
    );

    th.teardown().await;
}

/// `TestCreateBoardsCategory/"user has explicit access to some board"`
#[tokio::test]
async fn create_containers_category_user_has_explicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let mut boards = Vec::new();
    for _ in 0..3 {
        boards.push(board_with_admin(&th, TEST_WORKSPACE_ID, &user_id).await.id);
    }
    // A board of another workspace is not filed in this workspace's default category.
    board_with_admin(&th, ROOT_WORKSPACE_ID, &user_id).await;

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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
async fn create_containers_category_user_has_both_implicit_and_explicit_access_to_some_board() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let own = board_with_admin(&th, TEST_WORKSPACE_ID, &user_id).await;
    board_without_member(&th, TEST_WORKSPACE_ID, &th.stranger().await).await;
    board_without_member(&th, TEST_WORKSPACE_ID, &th.stranger().await).await;

    let listed = th
        .app
        .user_category_containers(&user_id, TEST_WORKSPACE_ID)
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

/// A category of the user in [`TEST_WORKSPACE_ID`] with `count` boards filed in it, and those
/// boards in the order they are listed.
async fn category_with_boards(
    th: &TestServer,
    user_id: &str,
    count: usize,
) -> (Category, Vec<String>) {
    let created = th
        .app
        .create_category(category(TEST_WORKSPACE_ID, user_id, "Category 1"))
        .await
        .expect("the category is created");
    let board_ids_filed = open_boards(th, TEST_WORKSPACE_ID, count).await;
    th.app
        .add_update_user_category_container(
            TEST_WORKSPACE_ID,
            user_id,
            &created.id,
            &board_ids_filed,
        )
        .await
        .expect("the boards are filed");
    let order = th
        .app
        .reorder_category_containers(user_id, TEST_WORKSPACE_ID, &created.id, board_ids_filed)
        .await
        .expect("the boards are put in order");
    (created, order)
}

async fn listed_board_ids(th: &TestServer, user_id: &str, category_id: &str) -> Vec<String> {
    let listed = th
        .app
        .user_category_containers(user_id, TEST_WORKSPACE_ID)
        .await
        .expect("the categories are listed");
    board_ids(entry(&listed, category_id))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// `TestReorderCategoryBoards/"base case"`
#[tokio::test]
async fn reorder_category_containers_base_case() {
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
        .reorder_category_containers(&user_id, TEST_WORKSPACE_ID, &created.id, reversed.clone())
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
async fn reorder_category_containers_not_specifying_all_boards() {
    let th = Setup::native().start().await;
    let user_id = stored_user(&th).await;
    let (created, boards) = category_with_boards(&th, &user_id, 3).await;

    let error = th
        .app
        .reorder_category_containers(
            &user_id,
            TEST_WORKSPACE_ID,
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
        (Method::GET, &format!("{ROOT_WS}/categories")),
        (Method::POST, &format!("{ROOT_WS}/categories")),
        (Method::PUT, &format!("{ROOT_WS}/category-order")),
        (Method::PUT, &format!("{ROOT_WS}/categories/any")),
        (Method::DELETE, &format!("{ROOT_WS}/categories/any")),
        (
            Method::PUT,
            &format!("{ROOT_WS}/categories/any/container-order"),
        ),
        (
            Method::POST,
            &format!("{ROOT_WS}/categories/any/containers/any"),
        ),
        (
            Method::PUT,
            &format!("{ROOT_WS}/categories/any/containers/any/hide"),
        ),
        (
            Method::PUT,
            &format!("{ROOT_WS}/categories/any/containers/any/unhide"),
        ),
    ];

    for (method, path) in requests {
        let response = anonymous.send_raw_body(method.clone(), path, "{}").await;
        check_error(&response, StatusCode::UNAUTHORIZED, "no session token");
    }

    th.teardown().await;
}

/// The category routes are about a workspace that exists. A path that names no workspace answers
/// that, whatever else it names, and alike for an identifier of nothing and for text
/// that is no identifier. (While the server had one workspace and took any name for it, these
/// requests were answered for their category.)
#[tokio::test]
async fn category_routes_need_a_workspace_that_exists() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let board = board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    file_board(&th.client, ROOT_WORKSPACE_ID, &created.id, &board.id).await;
    let before = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

    for workspace_id in [UNKNOWN_ID, "1"] {
        let categories = categories_path(workspace_id);
        let one = format!("{categories}/{}", created.id);
        let place = format!("{one}/containers/{}", board.id);
        let order = format!("{one}/container-order");
        let new = json!({"name": "New", "userId": user.id, "workspaceId": workspace_id});
        let change = json!({
            "id": created.id, "name": "Renamed", "userId": user.id, "workspaceId": workspace_id,
        });
        let requests = [
            (Method::GET, categories.clone(), Value::Null),
            (Method::POST, categories.clone(), new),
            (
                Method::PUT,
                format!("/w/{workspace_id}/category-order"),
                json!(ids(&before)),
            ),
            (Method::PUT, one.clone(), change),
            (Method::DELETE, one.clone(), Value::Null),
            (Method::PUT, order, json!([board.id])),
            (Method::POST, place.clone(), Value::Null),
            (Method::PUT, format!("{place}/hide"), Value::Null),
            (Method::PUT, format!("{place}/unhide"), Value::Null),
        ];
        for (method, path, body) in requests {
            let response = th
                .client
                .send_raw_body(method, &path, &body.to_string())
                .await;
            check_error(&response, StatusCode::NOT_FOUND, "workspace not found");
        }
    }

    let after = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(after, before, "none of these changed anything");

    th.teardown().await;
}

/// The default category is created by the first listing, once per workspace, and is the same
/// from then on.
#[tokio::test]
async fn the_default_category_is_created_once_per_workspace() {
    let th = Setup::native().start().await.init_basic().await;

    let first = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let second = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let other_workspace = category_boards(&th.client, TEST_WORKSPACE_ID).await;
    let other_user = category_boards(&th.client2, ROOT_WORKSPACE_ID).await;

    assert_eq!(names(&first), ["Boards"], "created by the first listing");
    assert_eq!(second, first, "and not again by the second");
    assert_eq!(
        names(&other_workspace),
        ["Boards"],
        "each workspace has its own"
    );
    assert_ne!(
        ids(&other_workspace),
        ids(&first),
        "with its own identifier"
    );
    assert_eq!(
        other_workspace[0].category.workspace_id, TEST_WORKSPACE_ID,
        "in that workspace"
    );
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
    let custom =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Boards")).await;

    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

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
            &format!("{ROOT_WS}/categories"),
            &json!({"name": "Cat A", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
        )
        .await;
    let created: Category = response.created_json();
    assert_eq!(
        response.value(),
        json!({
            "id": created.id,
            "name": "Cat A",
            "userId": user.id,
            "workspaceId": ROOT_WORKSPACE_ID,
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
            &format!("{ROOT_WS}/categories"),
            &json!({
                "name": "With id",
                "userId": user.id,
                "workspaceId": ROOT_WORKSPACE_ID,
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
            "workspaceId": ROOT_WORKSPACE_ID,
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
                &format!("{ROOT_WS}/categories"),
                &json!({"name": "Blank type", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID, "type": blank}),
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
    let path = &format!("{ROOT_WS}/categories");
    let invalid_type = "category type is invalid. Allowed types: system and custom";
    // A category is made for the session's user and nobody else, whatever else the
    // body says.
    for body in [
        json!({"name": "x", "userId": other.id, "workspaceId": ROOT_WORKSPACE_ID}),
        json!({"name": "x", "workspaceId": ROOT_WORKSPACE_ID}),
        json!({"name": "x", "userId": other.id, "workspaceId": ROOT_WORKSPACE_ID, "type": "bogus"}),
        // The user under a name that is no member of a category: no user is named.
        json!({"name": "x", "userID": user.id, "teamID": ROOT_WORKSPACE_ID}),
    ] {
        let response = th.client.post(path, &body).await;
        check_error(
            &response,
            StatusCode::FORBIDDEN,
            "a user makes categories for themselves only",
        );
        assert_eq!(response.refused_at(), "/userId", "{body}");
    }

    // Another workspace than the path's: one that exists, one that does not, and text that
    // is no identifier.
    for workspace_id in [TEST_WORKSPACE_ID, UNKNOWN_ID, "1"] {
        let body = json!({"name": "x", "userId": user.id, "workspaceId": workspace_id});
        let response = th.client.post(path, &body).await;
        check_error(&response, StatusCode::BAD_REQUEST, "workspaceId mismatch");
    }

    let cases = [
        (
            "a blank name",
            json!({"name": "  ", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "a `null` name",
            json!({"name": null, "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
            "category name cannot be empty".to_owned(),
        ),
        (
            "an unknown type",
            json!({"name": "x", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID, "type": "bogus"}),
            invalid_type.to_owned(),
        ),
        (
            "a blank name and an unknown type: the name is checked first",
            json!({"name": "", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID, "type": "bogus"}),
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
        assert_eq!(
            response.problem_type(),
            "/problems/bad-request",
            "{malformed}"
        );
    }

    // The server makes the identifier. A request that names one is refused whether or
    // not somebody has a category of that identifier: the answer must not tell. (The
    // original stores the client's identifier, and answers 500 for one that is taken.)
    let others = create_category(
        &th.client2,
        &category(ROOT_WORKSPACE_ID, &other.id, "Theirs"),
    )
    .await;
    for id in ["fresh", UNKNOWN_ID, others.id.as_str()] {
        let named =
            json!({"id": id, "name": "x", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID});
        let response = th.client.post(path, &named).await;
        check_error(
            &response,
            StatusCode::BAD_REQUEST,
            "a new category cannot have an ID",
        );
        assert_eq!(response.refused_at(), "/id", "the member at fault");
    }
    th.client
        .post(
            path,
            &json!({"name": "x", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
        )
        .await
        .check_created();

    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;

    let response = th
        .client
        .put(
            &format!("{ROOT_WS}/categories/{}", created.id),
            &json!({
                "id": created.id,
                "name": "Cat A2",
                "userId": user.id,
                "workspaceId": ROOT_WORKSPACE_ID,
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
            "workspaceId": ROOT_WORKSPACE_ID,
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
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let path = format!("{ROOT_WS}/categories/{}", created.id);
    let body = |change: Value| {
        let mut body = json!({"id": created.id, "name": "n", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID});
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
        (
            body(json!({"workspaceId": TEST_WORKSPACE_ID})),
            "workspaceId mismatch",
        ),
        (
            body(json!({"workspaceId": UNKNOWN_ID})),
            "workspaceId mismatch",
        ),
        (body(json!({"workspaceId": "1"})), "workspaceId mismatch"),
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

    // A category of another workspace than the path's, one that does not exist, and
    // another user's all answer alike, whatever the body.
    let others = create_category(
        &th.client2,
        &category(ROOT_WORKSPACE_ID, &other.id, "Theirs"),
    )
    .await;
    for (route, request) in [
        (
            format!("{TEST_WS}/categories/{}", created.id),
            body(json!({"workspaceId": TEST_WORKSPACE_ID})),
        ),
        (
            format!("{ROOT_WS}/categories/{UNKNOWN_ID}"),
            json!({"id": UNKNOWN_ID, "name": "n", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
        ),
        (
            format!("{ROOT_WS}/categories/nope"),
            json!({"id": "nope", "name": "n", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
        ),
        (
            format!("{ROOT_WS}/categories/{}", others.id),
            json!({"id": others.id, "name": "n", "userId": user.id, "workspaceId": ROOT_WORKSPACE_ID}),
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
    let default_id = category_boards(&th.client, ROOT_WORKSPACE_ID).await[0]
        .category
        .id
        .clone();
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let path = format!("{ROOT_WS}/categories/{}", created.id);

    for missing in [UNKNOWN_ID, "nope"] {
        let response = th
            .client
            .delete(&format!("{ROOT_WS}/categories/{missing}"))
            .await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // In another workspace than its own the category does not exist.
    let response = th
        .client
        .delete(&format!("{TEST_WS}/categories/{}", created.id))
        .await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // (The original answers 500.)
    let response = th
        .client
        .delete(&format!("{ROOT_WS}/categories/{default_id}"))
        .await;
    check_error(
        &response,
        StatusCode::FORBIDDEN,
        "a system category cannot be deleted",
    );

    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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

    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(names(&listed), ["Boards"], "the category is gone");

    th.teardown().await;
}

/// A reorder request must name the user's categories and nothing else.
#[tokio::test]
async fn reorder_categories_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let path = &format!("{ROOT_WS}/category-order");

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

    let default_id = category_boards(&th.client, ROOT_WORKSPACE_ID).await[0]
        .category
        .id
        .clone();
    let second = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 2"),
    )
    .await;
    let third = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Category 3"),
    )
    .await;
    let before = [third.id.as_str(), second.id.as_str(), default_id.as_str()];
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }
    th.client
        .send_raw_body(Method::PUT, path, "[")
        .await
        .check_bad_request();
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(ids(&listed), before, "a refused order changes nothing");

    // Another user's order of these categories is refused: they are not theirs.
    category_boards(&th.client2, ROOT_WORKSPACE_ID).await;
    let response = th.client2.put(path, &json!([default_id])).await;
    response.check_bad_request();
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(ids(&listed), before, "and changes nothing");

    let new_order = [default_id.as_str(), third.id.as_str(), second.id.as_str()];
    let response = th.client.put(path, &new_order).await;
    assert_eq!(response.ok_json::<Vec<String>>(), new_order, "the answer");
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(
        sorted(ids(&listed)),
        sorted(new_order.to_vec()),
        "every category is still listed"
    );

    // A new category comes first, also after a reorder.
    let new = create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "New")).await;
    let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(ids(&listed)[0], new.id, "the new category is first");

    th.teardown().await;
}

/// A board reorder request must name the boards of the category and nothing else.
#[tokio::test]
async fn reorder_category_containers_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.client.me().await;
    let default_id = category_boards(&th.client, ROOT_WORKSPACE_ID).await[0]
        .category
        .id
        .clone();
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let boards = open_boards(&th, ROOT_WORKSPACE_ID, 3).await;
    for board_id in &boards {
        file_board(&th.client, ROOT_WORKSPACE_ID, &created.id, board_id).await;
    }
    let path = format!("{ROOT_WS}/categories/{}/container-order", created.id);
    let order = async || {
        let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
        assert_eq!(response.problem_type(), "/problems/bad-request", "{body}");
    }
    th.client
        .send_raw_body(Method::PUT, &path, "{")
        .await
        .check_bad_request();
    assert_eq!(order().await, new_order, "a refused order changes nothing");

    // A category of another workspace than the path's, and one that does not exist.
    for route in [
        format!("{TEST_WS}/categories/{}/container-order", created.id),
        format!("{ROOT_WS}/categories/{UNKNOWN_ID}/container-order"),
        format!("{ROOT_WS}/categories/nope/container-order"),
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
            &format!("{ROOT_WS}/categories/{default_id}/container-order"),
            "[]",
        )
        .await;
    response.check_ok();
    assert_eq!(response.value(), json!([]), "the answer to the empty order");

    // A deleted category has no boards to order.
    th.client
        .delete(&format!("{ROOT_WS}/categories/{}", created.id))
        .await
        .check_no_content();
    let response = th.client.put(&path, &new_order).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    th.teardown().await;
}

/// Filing a board under a category: the answer, the move, and the refusals.
#[tokio::test]
async fn update_category_container_over_the_api() {
    let th = Setup::native().start().await.init_basic().await;
    let user = th.me_as_no_admin(&th.client).await;
    let board = board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    let default_id = category_boards(&th.client, ROOT_WORKSPACE_ID).await[0]
        .category
        .id
        .clone();
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let board_path = |workspace_id: &str, category_id: &str| {
        format!(
            "/w/{workspace_id}/categories/{category_id}/containers/{}",
            board.id
        )
    };
    let places = async || {
        let listed = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
        .post_empty(&board_path(ROOT_WORKSPACE_ID, &created.id))
        .await;
    response.check_no_content();
    assert_eq!(
        places().await,
        (vec![board.id.clone()], vec![]),
        "the board moved"
    );

    // A category that does not exist, is in another workspace, or is deleted: the original
    // answers these with a server error after moving the board out of sight. Here the
    // board stays where it is.
    let deleted = create_category(
        &th.client,
        &category(ROOT_WORKSPACE_ID, &user.id, "Deleted"),
    )
    .await;
    th.client
        .delete(&format!("{ROOT_WS}/categories/{}", deleted.id))
        .await
        .check_no_content();
    for path in [
        board_path(ROOT_WORKSPACE_ID, UNKNOWN_ID),
        board_path(ROOT_WORKSPACE_ID, "nope"),
        board_path(TEST_WORKSPACE_ID, &default_id),
        board_path(ROOT_WORKSPACE_ID, &deleted.id),
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
    let private = board_of_somebody_else(&th, ROOT_WORKSPACE_ID).await;
    for board_id in [UNKNOWN_ID, "no-such-board", private.as_str()] {
        let response = th
            .client
            .post_empty(&format!(
                "{ROOT_WS}/categories/{default_id}/containers/{board_id}"
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
    let board = board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    let before = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(
        before[0].container_metadata,
        [visible(&board.id)],
        "one board"
    );
    let default_id = &before[0].category.id;
    let board_id = &board.id;

    // What does not exist is asked for by an identifier of nothing and by text that is
    // no identifier: the answers are the same.
    for nothing in [UNKNOWN_ID, "nope"] {
        for (path, missing) in [
            (
                format!("{ROOT_WS}/categories/{nothing}/containers/{nothing}/hide"),
                "category not found",
            ),
            (
                format!("{ROOT_WS}/categories/{nothing}/containers/{board_id}/hide"),
                "category not found",
            ),
            (
                format!("{ROOT_WS}/categories/{nothing}/containers/{nothing}/unhide"),
                "category not found",
            ),
            (
                format!("{ROOT_WS}/categories/{default_id}/containers/{nothing}/hide"),
                "board not found",
            ),
            (
                format!("{ROOT_WS}/categories/{default_id}/containers/{nothing}/unhide"),
                "board not found",
            ),
        ] {
            let response = put_empty(&th.client, &path).await;
            check_error(&response, StatusCode::NOT_FOUND, missing);
        }
    }

    // A board the user sees, in a category of the user's it is not filed in.
    let other = create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Other")).await;
    let before = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let hide = format!(
        "{ROOT_WS}/categories/{}/containers/{board_id}/hide",
        other.id
    );
    let response = put_empty(&th.client, &hide).await;
    response.check_no_content();

    let after = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
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
    category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    let created =
        create_category(&th.client, &category(ROOT_WORKSPACE_ID, &user.id, "Cat A")).await;
    let board = board_with_admin(&th, ROOT_WORKSPACE_ID, &user.id).await;
    file_board(&th.client, ROOT_WORKSPACE_ID, &created.id, &board.id).await;
    let before = category_boards(&th.client, ROOT_WORKSPACE_ID).await;

    // Among the categories: an update and a deletion of the category `reorder`.
    let as_category = format!("{ROOT_WS}/categories/reorder");
    let mut reversed = ids(&before);
    reversed.reverse();
    let response = th.client.put(&as_category, &reversed).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");
    let response = th.client.delete(&as_category).await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // Among the boards of a category: the place of the board `reorder`, which takes
    // `POST` only.
    let as_board = format!("{ROOT_WS}/categories/{}/containers/reorder", created.id);
    let response = th.client.put(&as_board, &[&board.id]).await;
    check_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method not allowed",
    );
    assert_eq!(response.header("allow"), "POST", "what the path takes");
    let response = th.client.post_empty(&as_board).await;
    check_error(&response, StatusCode::NOT_FOUND, "board not found");

    let after = category_boards(&th.client, ROOT_WORKSPACE_ID).await;
    assert_eq!(after, before, "none of these changed anything");

    // The paths of the orders take `PUT` and nothing else.
    let response = th.client.delete(&format!("{ROOT_WS}/category-order")).await;
    check_error(
        &response,
        StatusCode::METHOD_NOT_ALLOWED,
        "method not allowed",
    );
    assert_eq!(response.header("allow"), "PUT", "what the path takes");

    let order = format!("{ROOT_WS}/categories/{}/container-order", created.id);
    let response = th.client.post_empty(&order).await;
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
        category_boards(&th.client, ROOT_WORKSPACE_ID).await;
        category_boards(&th.client2, ROOT_WORKSPACE_ID).await;
        let board = board_with_admin(&th, ROOT_WORKSPACE_ID, &owner.id).await;
        let category = create_category(
            &th.client,
            &category(ROOT_WORKSPACE_ID, &owner.id, "Private"),
        )
        .await;
        file_board(&th.client, ROOT_WORKSPACE_ID, &category.id, &board.id).await;
        Self {
            th,
            category,
            board_id: board.id,
            intruder_id: intruder.id,
        }
    }

    /// What the owner sees of their categories.
    async fn owner_view(&self) -> Vec<CategoryContainers> {
        category_boards(&self.th.client, ROOT_WORKSPACE_ID).await
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
    let intruder_before = category_boards(&two.th.client2, ROOT_WORKSPACE_ID).await;

    // Another user's category answers as one that does not exist.
    for category_id in [two.category.id.as_str(), UNKNOWN_ID, "nope"] {
        for board_id in [UNKNOWN_ID, "planted", two.board_id.as_str()] {
            let response = two
                .th
                .client2
                .post_empty(&format!(
                    "{ROOT_WS}/categories/{category_id}/containers/{board_id}"
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
        category_boards(&two.th.client2, ROOT_WORKSPACE_ID).await,
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
        &category(ROOT_WORKSPACE_ID, &two.intruder_id, "Mine"),
    )
    .await;

    // The same board identifier, filed by each user under a category of their own.
    file_board(&two.th.client2, ROOT_WORKSPACE_ID, &own.id, &two.board_id).await;

    let listed = category_boards(&two.th.client2, ROOT_WORKSPACE_ID).await;
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
    let path = format!("{ROOT_WS}/categories/{}", two.category.id);
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
    let path = format!("{ROOT_WS}/categories/{}", two.category.id);

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
        .put(&format!("{path}/container-order"), &[two.board_id.as_str()])
        .await;
    check_error(&response, StatusCode::NOT_FOUND, "category not found");

    // Reorder the categories: they are not among the intruder's.
    let order: Vec<&str> = ids(&before);
    let response = intruder
        .put(&format!("{ROOT_WS}/category-order"), &order)
        .await;
    response.check_bad_request();

    // The same answers as for a category nobody has.
    for nobodys in [UNKNOWN_ID, "nope"] {
        let nobody = &format!("{ROOT_WS}/categories/{nobodys}");
        let response = intruder.put(nobody, &as_intruder).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
        let response = intruder.delete(nobody).await;
        check_error(&response, StatusCode::NOT_FOUND, "category not found");
    }

    // Hide and show.
    let place = format!("{path}/containers/{}", two.board_id);
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
