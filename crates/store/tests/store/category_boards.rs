//! Port of `server/services/store/storetests/categoryBoards.go`.
//!
//! One test per test function of the original; its subtests are the commented sections.
//! The tests after [`set_board_visibility`] are not in the original: they pin behavior
//! the original's tests leave open.
//!
//! The original names its categories, users, teams and boards with fixed strings, and
//! puts boards into categories that were never created. Here every identifier is
//! generated, and what a row refers to is stored first: the user and the team of a
//! category, and the category, the user and the board of a board's place in one.

use fvoci_model::{Category, CategoryBoardMetadata, CategoryBoards, new_id};
use fvoci_store::{BoardVisibility, Store, StoreError};

use crate::categories::{create_boards, ids, now_millis, visible};
use crate::common;

/// The user and the team whose sidebar a test looks at.
struct Sidebar {
    user_id: String,
    team_id: String,
}

impl Sidebar {
    /// Stores a user and a team.
    async fn create(store: &Store) -> Self {
        Self {
            user_id: common::user(store, "user").await,
            team_id: common::team(store).await,
        }
    }

    /// Creates a category of the user in the team and returns its identifier.
    async fn create_category(&self, store: &Store, name: &str) -> String {
        create_category(store, &self.user_id, &self.team_id, name).await
    }

    /// The categories of the user in the team, each with its boards.
    async fn category_boards(&self, store: &Store) -> Vec<CategoryBoards> {
        store
            .get_user_category_boards(&self.user_id, &self.team_id)
            .await
            .expect("the categories are listed")
    }

    /// The boards of the category with the given identifier, in display order.
    async fn boards_of(&self, store: &Store, category_id: &str) -> Vec<CategoryBoardMetadata> {
        self.category_boards(store)
            .await
            .into_iter()
            .find(|category_boards| category_boards.category.id == category_id)
            .expect("the category is listed")
            .board_metadata
    }
}

/// Creates a category of a user in a team and returns its identifier.
async fn create_category(store: &Store, user_id: &str, team_id: &str, name: &str) -> String {
    let category = Category {
        id: new_id(),
        name: name.to_owned(),
        user_id: user_id.to_owned(),
        team_id: team_id.to_owned(),
        ..Category::default()
    };
    store
        .create_category(&category)
        .await
        .expect("the category is created");
    category.id
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
    let user_id = common::user(store, "user").await;
    let team_id = common::team(store).await;

    let now = now_millis();
    let category1 = Category {
        id: new_id(),
        name: "Category 1".to_owned(),
        user_id: user_id.clone(),
        team_id: team_id.clone(),
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
        id: new_id(),
        name: "Category 2".to_owned(),
        user_id: user_id.clone(),
        team_id: team_id.clone(),
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
        id: new_id(),
        name: "Category 3".to_owned(),
        user_id: user_id.clone(),
        team_id: team_id.clone(),
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
    // The original leaves the boards out of the database for this test; a board in a
    // category exists here.
    let [board1, board2, board3] = create_boards(store, &team_id, &user_id).await;
    store
        .add_update_category_board(&user_id, &category1.id, std::slice::from_ref(&board1))
        .await
        .expect("board 1 is added");

    store
        .add_update_category_board(&user_id, &category1.id, std::slice::from_ref(&board2))
        .await
        .expect("board 2 is added");

    // Adding Board 3 to Category 2
    store
        .add_update_category_board(&user_id, &category2.id, std::slice::from_ref(&board3))
        .await
        .expect("board 3 is added");

    // we'll leave category 3 empty

    let user_category_boards = store
        .get_user_category_boards(&user_id, &team_id)
        .await
        .expect("the categories are listed");

    // we created 3 categories for the user
    assert_eq!(
        user_category_boards.len(),
        3,
        "got {user_category_boards:?}"
    );

    // The original asserts three times that the first category was found, and never that
    // the second and third were. Here each is.
    let board_count = |category: &Category| {
        user_category_boards
            .iter()
            .find(|category_boards| category_boards.category.id == category.id)
            .map(|category_boards| category_boards.board_metadata.len())
    };
    assert_eq!(board_count(&category1), Some(2), "boards 1 and 2");
    assert_eq!(board_count(&category2), Some(1), "board 3");
    assert_eq!(board_count(&category3), Some(0), "an empty category");

    // "get empty category boards"
    {
        let user_category_boards = store
            .get_user_category_boards(&new_id(), &new_id())
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
async fn add_update_category_board() {
    let test = common::setup().await;
    let store = &test.store;
    let sidebar = Sidebar::create(store).await;
    let user_id = &sidebar.user_id;

    // creating few boards and categories to later associoate with the category
    //
    // The original creates the boards "board_id_1" and "board_id_2" in team "team_id" and
    // later adds three more that it never created. A board in a category exists here.
    let [board1, board2, board3, board4, board5] =
        create_boards(store, &sidebar.team_id, user_id).await;

    let category_id = sidebar.create_category(store, "Category").await;

    // adding a few boards to the category
    store
        .add_update_category_board(user_id, &category_id, &ids(&[&board1, &board2]))
        .await
        .expect("the boards are added");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 2, "got {metadata:?}");
    assert!(metadata.contains(&visible(&board1)), "{metadata:?}");
    assert!(metadata.contains(&visible(&board2)), "{metadata:?}");

    // adding new boards to the same category
    store
        .add_update_category_board(user_id, &category_id, std::slice::from_ref(&board3))
        .await
        .expect("a third board is added");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 3, "got {metadata:?}");
    assert!(metadata.contains(&visible(&board3)), "{metadata:?}");

    // passing empty array
    store
        .add_update_category_board(user_id, &category_id, &[])
        .await
        .expect("no boards are added");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 3, "got {metadata:?}");

    // passing duplicate data in input
    store
        .add_update_category_board(user_id, &category_id, &ids(&[&board4, &board4]))
        .await
        .expect("a board named twice is added once");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 4, "got {metadata:?}");
    assert!(metadata.contains(&visible(&board4)), "{metadata:?}");

    // adding already added board
    store
        .add_update_category_board(user_id, &category_id, &ids(&[&board1, &board2]))
        .await
        .expect("boards that are in the category already are accepted");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 4, "got {metadata:?}");

    // passing already added board along with a new board
    store
        .add_update_category_board(user_id, &category_id, &ids(&[&board1, &board5]))
        .await
        .expect("a known and a new board are accepted together");

    // verify inserted data
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 5, "got {metadata:?}");
    assert!(metadata.contains(&visible(&board5)), "{metadata:?}");

    test.teardown().await;
}

/// `testSetBoardVisibility`.
#[tokio::test]
async fn set_board_visibility() {
    let test = common::setup().await;
    let store = &test.store;
    let sidebar = Sidebar::create(store).await;
    let user_id = &sidebar.user_id;

    // As the original does, create the board the category is to hold.
    let [board1] = create_boards(store, &sidebar.team_id, user_id).await;

    let category_id = sidebar.create_category(store, "Category").await;

    // adding a few boards to the category
    store
        .add_update_category_board(user_id, &category_id, std::slice::from_ref(&board1))
        .await
        .expect("the board is added");

    store
        .set_board_visibility(user_id, &category_id, &board1, BoardVisibility::Visible)
        .await
        .expect("the board is made visible");

    // verify set visibility
    let category_boards = sidebar.category_boards(store).await;
    assert_eq!(category_boards.len(), 1, "got {category_boards:?}");
    assert_eq!(category_boards[0].category.id, category_id, "the category");
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 1, "got {metadata:?}");
    assert!(!metadata[0].hidden, "got {metadata:?}");

    store
        .set_board_visibility(user_id, &category_id, &board1, BoardVisibility::Hidden)
        .await
        .expect("the board is hidden");

    // verify set visibility
    let category_boards = sidebar.category_boards(store).await;
    let metadata = &category_boards[0].board_metadata;
    assert!(metadata[0].hidden, "got {metadata:?}");

    test.teardown().await;
}

/// Not in the original. Where boards land when they are added, reordered and moved, and
/// what a move keeps.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "each step starts from the places the steps before it left the boards in"
)]
async fn sort_orders_of_added_reordered_and_moved_boards() {
    let test = common::setup().await;
    let store = &test.store;
    let sidebar = Sidebar::create(store).await;
    let user_id = &sidebar.user_id;
    let someone_else = common::user(store, "someone-else").await;
    let a = sidebar.create_category(store, "a").await;
    let b = sidebar.create_category(store, "b").await;
    let elsewhere = create_category(store, &someone_else, &sidebar.team_id, "elsewhere").await;
    let [one, two, three, four, theirs] = create_boards(store, &sidebar.team_id, user_id).await;

    // An identifier that names no board of the category is ignored and comes back with
    // the rest; the place it would have taken stays empty.
    store
        .add_update_category_board(user_id, &a, &ids(&[&one, &two, &three]))
        .await
        .expect("the boards are added");
    let order = ids(&[&three, &new_id(), &one, &two]);
    let echoed = store
        .reorder_category_boards(&a, order.clone())
        .await
        .expect("an identifier that names no board of the category is accepted");
    assert_eq!(echoed, order, "the order comes back as it was given");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &a).await),
        [&three, &one, &two],
        "the order asked for"
    );

    // Text that is not an identifier is refused before the statement runs. The original
    // splices it into the SQL text, where this one would end the string literal
    // (`server/services/store/sqlstore/category_boards.go:144`).
    let spliced = "x' THEN 5 ELSE 5 END --";
    let error = store
        .reorder_category_boards(&a, ids(&[&two, spliced, &one, &three]))
        .await
        .expect_err("text that is not an identifier is refused");
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == spliced),
        "got {error:?}"
    );
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &a).await),
        [&three, &one, &two],
        "the refused order moved nothing"
    );

    // A board added later sorts before the reordered ones.
    store
        .add_update_category_board(user_id, &a, std::slice::from_ref(&four))
        .await
        .expect("a fourth board is added");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &a).await),
        [&four, &three, &one, &two],
        "a new board comes first"
    );

    // A board moved to another category keeps its hidden flag and its sort order.
    store
        .set_board_visibility(user_id, &a, &two, BoardVisibility::Hidden)
        .await
        .expect("the board is hidden");
    store
        .add_update_category_board(user_id, &b, &ids(&[&two, &one, &two]))
        .await
        .expect("the boards are moved");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &a).await),
        [&four, &three],
        "the moved boards have left their category"
    );
    assert_eq!(
        sidebar.boards_of(store, &b).await,
        [
            visible(&one),
            CategoryBoardMetadata {
                board_id: two.clone(),
                hidden: true,
            },
        ],
        "the moved boards keep their order and their hidden flag"
    );

    // Visibility is keyed by user, category and board; a mismatch changes nothing.
    store
        .set_board_visibility(user_id, &a, &two, BoardVisibility::Visible)
        .await
        .expect("a board of another category is not found, which is not an error");
    store
        .set_board_visibility(&someone_else, &b, &two, BoardVisibility::Visible)
        .await
        .expect("a board of another user is not found, which is not an error");
    assert!(
        sidebar.boards_of(store, &b).await[1].hidden,
        "the board is still hidden"
    );

    // Rows are unique per user and board, so another user may hold the same board in a
    // category of their own.
    store
        .add_update_category_board(&someone_else, &elsewhere, std::slice::from_ref(&one))
        .await
        .expect("the board is added for another user");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &b).await),
        [&one, &two],
        "the user's own place for the board is untouched"
    );

    // A category lists the rows of every user: a board that another user puts into this
    // category shows up in it.
    store
        .add_update_category_board(&someone_else, &b, std::slice::from_ref(&theirs))
        .await
        .expect("another user's board is added to the category");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &b).await),
        [&theirs, &one, &two],
        "the other user's board is listed with the category"
    );

    // An empty order changes nothing.
    let echoed = store
        .reorder_category_boards(&b, Vec::new())
        .await
        .expect("an empty order is accepted");
    assert!(echoed.is_empty(), "got {echoed:?}");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &b).await),
        [&theirs, &one, &two],
        "nothing moved"
    );

    test.teardown().await;
}

/// Not in the original. A board of the category that a new order leaves out keeps its
/// sort order. In the original it loses it, because the statement has no `ELSE`
/// (`server/services/store/sqlstore/category_boards.go:146`), and is then listed first
/// on SQLite and last on PostgreSQL.
#[tokio::test]
async fn reorder_category_boards_keeps_the_sort_order_of_unlisted_boards() {
    let test = common::setup().await;
    let store = &test.store;
    let sidebar = Sidebar::create(store).await;
    let category_id = sidebar.create_category(store, "Category").await;
    let [x, y, z] = create_boards(store, &sidebar.team_id, &sidebar.user_id).await;

    store
        .add_update_category_board(&sidebar.user_id, &category_id, &ids(&[&x, &y, &z]))
        .await
        .expect("the boards are added");
    store
        .reorder_category_boards(&category_id, ids(&[&x, &y, &z]))
        .await
        .expect("the boards are ordered");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &category_id).await),
        [&x, &y, &z],
        "the order asked for"
    );

    // The boards have the sort orders 10, 11 and 12. An order that names the first two
    // gives them 10 and 11 again, and the third stays behind them on both databases.
    store
        .reorder_category_boards(&category_id, ids(&[&y, &x]))
        .await
        .expect("an order that leaves out a board is accepted");
    assert_eq!(
        board_ids(&sidebar.boards_of(store, &category_id).await),
        [&y, &x, &z],
        "the board the order leaves out keeps the last place"
    );

    // The numbering starts over at 10 with every order, so a board that is left out may
    // come to share its sort order with one that is named: here `y`, which keeps 10,
    // with `z`. The two come before `x`, in no particular order.
    store
        .reorder_category_boards(&category_id, ids(&[&z, &x]))
        .await
        .expect("an order that leaves out a board is accepted");
    let boards = sidebar.boards_of(store, &category_id).await;
    let mut first_two = board_ids(&boards[..2]);
    first_two.sort_unstable();
    let mut expected = [y.as_str(), z.as_str()];
    expected.sort_unstable();
    assert_eq!(first_two, expected, "the boards with sort order 10");
    assert_eq!(boards[2].board_id, x, "the board with sort order 11");

    test.teardown().await;
}
