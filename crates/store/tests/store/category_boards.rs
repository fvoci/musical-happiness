//! Port of `server/services/store/storetests/categoryBoards.go`.
//!
//! One test per test function of the original; its subtests are the commented sections.
//! The tests after [`set_board_visibility`] are not in the original: they pin behavior
//! the original's tests leave open.
//!
//! Two of the original's tests first create boards with `CreateBoardsAndBlocksWithAdmin`.
//! They then only use the identifiers of those boards: the store never reads the boards
//! table for a category, the `category_boards` table has no foreign key to it, and the
//! same tests go on to add boards that were never created. The ports therefore use
//! generated board identifiers instead of board rows.

use fvoci_model::{Category, CategoryBoardMetadata, CategoryBoards, IdKind, new_id};
use fvoci_store::{BoardVisibility, DbType, Store};

use crate::categories::{ids, now_millis, visible};
use crate::common;

/// Creates a category of `user_id` in `team_id`.
async fn create_category(store: &Store, id: &str) {
    store
        .create_category(&Category {
            id: id.to_owned(),
            name: format!("Category {id}"),
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");
}

/// The boards of the category with the given identifier, in display order.
async fn boards_of(store: &Store, category_id: &str) -> Vec<CategoryBoardMetadata> {
    store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed")
        .into_iter()
        .find(|category_boards| category_boards.category.id == category_id)
        .expect("the category is listed")
        .board_metadata
}

/// The identifiers of the boards, in the given order.
fn board_ids(metadata: &[CategoryBoardMetadata]) -> Vec<&str> {
    metadata
        .iter()
        .map(|board| board.board_id.as_str())
        .collect()
}

/// `testGetUserCategoryBoards`.
#[tokio::test]
async fn get_user_category_boards() {
    let test = common::setup().await;
    let store = &test.store;

    let now = now_millis();
    let category1 = Category {
        id: "category_id_1".to_owned(),
        name: "Category 1".to_owned(),
        user_id: "user_id_1".to_owned(),
        team_id: "team_id_1".to_owned(),
        create_at: now,
        update_at: now,
        delete_at: 0,
        collapsed: false,
        ..Category::default()
    };
    store
        .create_category(&category1)
        .await
        .expect("category 1 is created");

    let category2 = Category {
        id: "category_id_2".to_owned(),
        name: "Category 2".to_owned(),
        user_id: "user_id_1".to_owned(),
        team_id: "team_id_1".to_owned(),
        create_at: now,
        update_at: now,
        delete_at: 0,
        collapsed: false,
        ..Category::default()
    };
    store
        .create_category(&category2)
        .await
        .expect("category 2 is created");

    let category3 = Category {
        id: "category_id_3".to_owned(),
        name: "Category 3".to_owned(),
        user_id: "user_id_1".to_owned(),
        team_id: "team_id_1".to_owned(),
        create_at: now,
        update_at: now,
        delete_at: 0,
        collapsed: false,
        ..Category::default()
    };
    store
        .create_category(&category3)
        .await
        .expect("category 3 is created");

    // Adding Board 1 and Board 2 to Category 1
    // The boards don't need to exists in DB for this test
    store
        .add_update_category_board("user_id_1", "category_id_1", &ids(&["board_1"]))
        .await
        .expect("board 1 is added");

    store
        .add_update_category_board("user_id_1", "category_id_1", &ids(&["board_2"]))
        .await
        .expect("board 2 is added");

    // Adding Board 3 to Category 2
    store
        .add_update_category_board("user_id_1", "category_id_2", &ids(&["board_3"]))
        .await
        .expect("board 3 is added");

    // we'll leave category 3 empty

    let user_category_boards = store
        .get_user_category_boards("user_id_1", "team_id_1")
        .await
        .expect("the categories are listed");

    // we created 3 categories for the user
    assert_eq!(user_category_boards.len(), 3);

    let mut category1_board_category = CategoryBoards::default();
    let mut category2_board_category = CategoryBoards::default();
    let mut category3_board_category = CategoryBoards::default();

    for category_boards in &user_category_boards {
        match category_boards.category.id.as_str() {
            "category_id_1" => category1_board_category = category_boards.clone(),
            "category_id_2" => category2_board_category = category_boards.clone(),
            "category_id_3" => category3_board_category = category_boards.clone(),
            _ => {}
        }
    }

    // The original asserts three times that the first category was found, and never that
    // the second and third were.
    assert_ne!(category1_board_category, CategoryBoards::default());
    assert_eq!(category1_board_category.board_metadata.len(), 2);

    assert_ne!(category1_board_category, CategoryBoards::default());
    assert_eq!(category2_board_category.board_metadata.len(), 1);

    assert_ne!(category1_board_category, CategoryBoards::default());
    assert_eq!(category3_board_category.board_metadata.len(), 0);

    // "get empty category boards"
    {
        let user_category_boards = store
            .get_user_category_boards("nonexistent-user-id", "nonexistent-team-id")
            .await
            .expect("an unknown user has no categories");
        assert!(
            user_category_boards.is_empty(),
            "got {user_category_boards:?}"
        );
    }

    test.teardown().await;
}

/// `testAddUpdateCategoryBoard`.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "the original's test function is one sequence of steps on one store"
)]
async fn add_update_category_board() {
    let test = common::setup().await;
    let store = &test.store;

    // creating few boards and categories to later associoate with the category
    //
    // The original creates the boards "board_id_1" and "board_id_2" in team "team_id" and
    // uses nothing but their identifiers afterwards; see the module documentation.
    let board_id_1 = new_id(IdKind::Board);
    let board_id_2 = new_id(IdKind::Board);

    store
        .create_category(&Category {
            id: "category_id".to_owned(),
            name: "Category".to_owned(),
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");

    // adding a few boards to the category
    store
        .add_update_category_board(
            "user_id",
            "category_id",
            &[board_id_1.clone(), board_id_2.clone()],
        )
        .await
        .expect("the boards are added");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 2);
    assert!(metadata.contains(&visible(&board_id_1)), "{metadata:?}");
    assert!(metadata.contains(&visible(&board_id_2)), "{metadata:?}");

    // adding new boards to the same category
    store
        .add_update_category_board("user_id", "category_id", &ids(&["board_id_3"]))
        .await
        .expect("a third board is added");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 3);
    assert!(metadata.contains(&visible("board_id_3")), "{metadata:?}");

    // passing empty array
    store
        .add_update_category_board("user_id", "category_id", &[])
        .await
        .expect("no boards are added");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    assert_eq!(category_boards[0].board_metadata.len(), 3);

    // passing duplicate data in input
    store
        .add_update_category_board(
            "user_id",
            "category_id",
            &ids(&["board_id_4", "board_id_4"]),
        )
        .await
        .expect("a board named twice is added once");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 4);
    assert!(metadata.contains(&visible("board_id_4")), "{metadata:?}");

    // adding already added board
    store
        .add_update_category_board("user_id", "category_id", &[board_id_1.clone(), board_id_2])
        .await
        .expect("boards that are in the category already are accepted");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    assert_eq!(category_boards[0].board_metadata.len(), 4);

    // passing already added board along with a new board
    store
        .add_update_category_board(
            "user_id",
            "category_id",
            &[board_id_1, "board_id_5".to_owned()],
        )
        .await
        .expect("a known and a new board are accepted together");

    // verify inserted data
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 5);
    assert!(metadata.contains(&visible("board_id_5")), "{metadata:?}");

    test.teardown().await;
}

/// `testSetBoardVisibility`.
#[tokio::test]
async fn set_board_visibility() {
    let test = common::setup().await;
    let store = &test.store;

    // The original creates the board "board_id_1" in team "team_id" and uses nothing but
    // its identifier afterwards; see the module documentation.
    let board_id_1 = new_id(IdKind::Board);

    store
        .create_category(&Category {
            id: "category_id".to_owned(),
            name: "Category".to_owned(),
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");

    // adding a few boards to the category
    store
        .add_update_category_board("user_id", "category_id", std::slice::from_ref(&board_id_1))
        .await
        .expect("the board is added");

    store
        .set_board_visibility(
            "user_id",
            "category_id",
            &board_id_1,
            BoardVisibility::Visible,
        )
        .await
        .expect("the board is made visible");

    // verify set visibility
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    assert_eq!(category_boards[0].category.id, "category_id");
    assert_eq!(category_boards[0].board_metadata.len(), 1);
    assert!(!category_boards[0].board_metadata[0].hidden);

    store
        .set_board_visibility(
            "user_id",
            "category_id",
            &board_id_1,
            BoardVisibility::Hidden,
        )
        .await
        .expect("the board is hidden");

    // verify set visibility
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert!(category_boards[0].board_metadata[0].hidden);

    test.teardown().await;
}

/// Not in the original. Where boards land when they are added, reordered and moved, and
/// what a move keeps.
#[tokio::test]
async fn sort_orders_of_added_reordered_and_moved_boards() {
    let test = common::setup().await;
    let store = &test.store;
    create_category(store, "a").await;
    create_category(store, "b").await;

    // The second board's identifier would end the string literal the original splices it
    // into (`server/services/store/sqlstore/category_boards.go:144`).
    store
        .add_update_category_board("user_id", "a", &ids(&["one", "t'wo", "three"]))
        .await
        .expect("the boards are added");
    let echoed = store
        .reorder_category_boards(
            "a",
            ids(&["three", "x' THEN 5 ELSE 5 END --", "one", "t'wo"]),
        )
        .await
        .expect("identifiers with SQL syntax in them are accepted");
    assert_eq!(echoed, ["three", "x' THEN 5 ELSE 5 END --", "one", "t'wo"]);
    assert_eq!(
        board_ids(&boards_of(store, "a").await),
        ["three", "one", "t'wo"]
    );

    // A board added later sorts before the reordered ones.
    store
        .add_update_category_board("user_id", "a", &ids(&["four"]))
        .await
        .expect("a fourth board is added");
    assert_eq!(
        board_ids(&boards_of(store, "a").await),
        ["four", "three", "one", "t'wo"]
    );

    // A board moved to another category keeps its hidden flag and its sort order.
    store
        .set_board_visibility("user_id", "a", "t'wo", BoardVisibility::Hidden)
        .await
        .expect("the board is hidden");
    store
        .add_update_category_board("user_id", "b", &ids(&["t'wo", "one", "t'wo"]))
        .await
        .expect("the boards are moved");
    assert_eq!(board_ids(&boards_of(store, "a").await), ["four", "three"]);
    assert_eq!(
        boards_of(store, "b").await,
        [
            visible("one"),
            CategoryBoardMetadata {
                board_id: "t'wo".to_owned(),
                hidden: true,
            },
        ]
    );

    // Visibility is keyed by user, category and board; a mismatch changes nothing.
    store
        .set_board_visibility("user_id", "a", "t'wo", BoardVisibility::Visible)
        .await
        .expect("a board of another category is not found, which is not an error");
    store
        .set_board_visibility("someone_else", "b", "t'wo", BoardVisibility::Visible)
        .await
        .expect("a board of another user is not found, which is not an error");
    assert!(boards_of(store, "b").await[1].hidden);

    // Rows are unique per user and board, so another user may hold the same board in a
    // category of their own.
    store
        .add_update_category_board("someone_else", "elsewhere", &ids(&["one"]))
        .await
        .expect("the board is added for another user");
    assert_eq!(board_ids(&boards_of(store, "b").await), ["one", "t'wo"]);

    // A category lists the rows of every user: a board that another user puts into this
    // category shows up in it.
    store
        .add_update_category_board("someone_else", "b", &ids(&["theirs"]))
        .await
        .expect("another user's board is added to the category");
    assert_eq!(
        board_ids(&boards_of(store, "b").await),
        ["theirs", "one", "t'wo"]
    );

    // An empty order changes nothing.
    let echoed = store
        .reorder_category_boards("b", Vec::new())
        .await
        .expect("an empty order is accepted");
    assert!(echoed.is_empty(), "got {echoed:?}");
    assert_eq!(
        board_ids(&boards_of(store, "b").await),
        ["theirs", "one", "t'wo"]
    );

    test.teardown().await;
}

/// Not in the original. A board of the category that a new order leaves out loses its
/// sort order, because the original's statement has no `ELSE`
/// (`server/services/store/sqlstore/category_boards.go:146`).
#[tokio::test]
async fn reorder_category_boards_clears_the_sort_order_of_unlisted_boards() {
    let test = common::setup().await;
    let store = &test.store;
    create_category(store, "category_id").await;

    store
        .add_update_category_board("user_id", "category_id", &ids(&["x", "y", "z"]))
        .await
        .expect("the boards are added");
    store
        .reorder_category_boards("category_id", ids(&["x", "y", "z"]))
        .await
        .expect("the boards are ordered");
    assert_eq!(
        board_ids(&boards_of(store, "category_id").await),
        ["x", "y", "z"]
    );

    // A board without a sort order comes first on SQLite and last on PostgreSQL.
    store
        .reorder_category_boards("category_id", ids(&["x", "y"]))
        .await
        .expect("an order that leaves out a board is accepted");
    let expected = match common::db_type() {
        DbType::Sqlite => ["z", "x", "y"],
        DbType::Postgres => ["x", "y", "z"],
    };
    assert_eq!(board_ids(&boards_of(store, "category_id").await), expected);

    store
        .reorder_category_boards("category_id", ids(&["z", "y"]))
        .await
        .expect("an order that leaves out a board is accepted");
    let expected = match common::db_type() {
        DbType::Sqlite => ["x", "z", "y"],
        DbType::Postgres => ["z", "y", "x"],
    };
    assert_eq!(board_ids(&boards_of(store, "category_id").await), expected);

    test.teardown().await;
}
