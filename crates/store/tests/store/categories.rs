//! Port of `server/services/store/storetests/category.go`.
//!
//! One test per test function of the original; its subtests are the commented sections.
//! The tests after [`reorder_category_boards`] are not in the original: they pin behavior
//! the original's tests leave open.

use std::time::{SystemTime, UNIX_EPOCH};

use fvoci_model::{Category, CategoryBoardMetadata, CategoryType};
use fvoci_store::{DbType, StoreError};
use sqlx::Connection as _;
use sqlx::postgres::PgConnection;
use sqlx::sqlite::SqliteConnection;

use crate::common::{self, TABLE_PREFIX, TestStore};

/// `utils.GetMillis`.
pub(crate) fn now_millis() -> i64 {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock is past 1970");
    i64::try_from(since_epoch.as_millis()).expect("the time fits 64 bits")
}

/// Owned identifiers from literals.
pub(crate) fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|&id| id.to_owned()).collect()
}

/// `model.CategoryBoardMetadata{BoardID: board_id, Hidden: false}`.
pub(crate) fn visible(board_id: &str) -> CategoryBoardMetadata {
    CategoryBoardMetadata {
        board_id: board_id.to_owned(),
        hidden: false,
    }
}

/// The identifiers of the categories, in the given order.
fn category_ids(categories: &[Category]) -> Vec<&str> {
    categories
        .iter()
        .map(|category| category.id.as_str())
        .collect()
}

/// Runs one statement on the test's database, bypassing the store, to stage rows the
/// store itself never writes.
async fn execute_raw(test: &TestStore, sql: &str) {
    let dbconfig = test.dbconfig();
    let sql = sqlx::AssertSqlSafe(sql.to_owned());
    match common::db_type() {
        DbType::Sqlite => {
            let mut conn = SqliteConnection::connect(&format!("sqlite://{dbconfig}"))
                .await
                .expect("the test database file opens");
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
        DbType::Postgres => {
            let (url, schema) = dbconfig
                .rsplit_once("search_path=")
                .expect("the test dbconfig names its schema");
            let mut conn = PgConnection::connect(url.trim_end_matches(['?', '&']))
                .await
                .expect("the test PostgreSQL server accepts connections");
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!("SET search_path TO {schema}")))
                .execute(&mut conn)
                .await
                .expect("the schema is selected");
            sqlx::raw_sql(sql)
                .execute(&mut conn)
                .await
                .expect("the statement runs");
            conn.close().await.expect("the connection closes");
        }
    }
}

/// `testGetCreateCategory`.
#[tokio::test]
async fn get_create_category() {
    let test = common::setup().await;
    let store = &test.store;

    // "save uncollapsed category"
    {
        let now = now_millis();
        let category = Category {
            id: "category_id_1".to_owned(),
            name: "Category".to_owned(),
            user_id: "user_id_1".to_owned(),
            team_id: "team_id_1".to_owned(),
            create_at: now,
            update_at: now,
            delete_at: 0,
            collapsed: false,
            ..Category::default()
        };

        store
            .create_category(&category)
            .await
            .expect("the category is created");

        let created_category = store
            .get_category("category_id_1")
            .await
            .expect("the category is found");
        assert_eq!(created_category.name, "Category");
        assert_eq!(created_category.user_id, "user_id_1");
        assert_eq!(created_category.team_id, "team_id_1");
        assert!(!created_category.collapsed);
    }

    // "save collapsed category"
    {
        let now = now_millis();
        let category = Category {
            id: "category_id_2".to_owned(),
            name: "Category".to_owned(),
            user_id: "user_id_1".to_owned(),
            team_id: "team_id_1".to_owned(),
            create_at: now,
            update_at: now,
            delete_at: 0,
            collapsed: true,
            ..Category::default()
        };

        store
            .create_category(&category)
            .await
            .expect("the category is created");

        let created_category = store
            .get_category("category_id_2")
            .await
            .expect("the category is found");
        assert_eq!(created_category.name, "Category");
        assert_eq!(created_category.user_id, "user_id_1");
        assert_eq!(created_category.team_id, "team_id_1");
        assert!(created_category.collapsed);
    }

    // "get nonexistent category"
    {
        // The original also asserts that the returned category is nil; a `Result` cannot
        // hold both a category and an error.
        let error = store
            .get_category("nonexistent")
            .await
            .expect_err("there is no such category");
        assert!(matches!(error, StoreError::NotFound(_)), "got {error:?}");
        assert!(error.is_not_found(), "got {error:?}");
        assert_eq!(error.to_string(), "{category ID=nonexistent} not found");
    }

    test.teardown().await;
}

/// `testUpdateCategory`.
#[tokio::test]
async fn update_category() {
    let test = common::setup().await;
    let store = &test.store;

    let now = now_millis();
    let category = Category {
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
        .create_category(&category)
        .await
        .expect("the category is created");

    let update_now = now_millis();
    let mut updated_category = Category {
        id: "category_id_1".to_owned(),
        name: "Category 1 New".to_owned(),
        user_id: "user_id_1".to_owned(),
        team_id: "team_id_1".to_owned(),
        create_at: now,
        update_at: update_now,
        delete_at: 0,
        collapsed: true,
        ..Category::default()
    };

    store
        .update_category(&updated_category)
        .await
        .expect("the category is updated");

    let fetched_category = store
        .get_category("category_id_1")
        .await
        .expect("the category is found");
    assert_eq!(fetched_category.id, "category_id_1");
    assert_eq!(fetched_category.name, "Category 1 New");
    assert!(fetched_category.collapsed);

    // now lets try to un-collapse the same category
    updated_category.collapsed = false;
    store
        .update_category(&updated_category)
        .await
        .expect("the category is updated");

    let fetched_category = store
        .get_category("category_id_1")
        .await
        .expect("the category is found");
    assert_eq!(fetched_category.id, "category_id_1");
    assert_eq!(fetched_category.name, "Category 1 New");
    assert!(!fetched_category.collapsed);

    test.teardown().await;
}

/// `testDeleteCategory`.
#[tokio::test]
async fn delete_category() {
    let test = common::setup().await;
    let store = &test.store;

    let now = now_millis();
    let category = Category {
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
        .create_category(&category)
        .await
        .expect("the category is created");

    store
        .delete_category("category_id_1", "user_id_1", "team_id_1")
        .await
        .expect("the category is deleted");

    let deleted_category = store
        .get_category("category_id_1")
        .await
        .expect("a deleted category is still found");
    assert_eq!(deleted_category.id, "category_id_1");
    assert_eq!(deleted_category.name, "Category 1");
    assert!(!deleted_category.collapsed);
    assert!(deleted_category.delete_at > 0, "got {deleted_category:?}");

    test.teardown().await;
}

/// `testGetUserCategories`.
#[tokio::test]
async fn get_user_categories() {
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

    // The original gives the third category the second one's name, "Category 2".
    let category3 = Category {
        id: "category_id_3".to_owned(),
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
        .create_category(&category3)
        .await
        .expect("category 3 is created");

    let user_categories = store
        .get_user_category_boards("user_id_1", "team_id_1")
        .await
        .expect("the categories are listed");
    assert_eq!(user_categories.len(), 3);

    test.teardown().await;
}

/// `testReorderCategories`.
#[tokio::test]
async fn reorder_categories() {
    let test = common::setup().await;
    let store = &test.store;

    // setup
    store
        .create_category(&Category {
            id: "category_id_1".to_owned(),
            name: "Category 1".to_owned(),
            category_type: CategoryType::Custom,
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("category 1 is created");

    store
        .create_category(&Category {
            id: "category_id_2".to_owned(),
            name: "Category 2".to_owned(),
            category_type: CategoryType::Custom,
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("category 2 is created");

    store
        .create_category(&Category {
            id: "category_id_3".to_owned(),
            name: "Category 3".to_owned(),
            category_type: CategoryType::Custom,
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("category 3 is created");

    // verify the current order
    let categories = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(categories.len(), 3);

    // the categories should show up in reverse insertion order (latest one first)
    assert_eq!(categories[0].id, "category_id_3");
    assert_eq!(categories[1].id, "category_id_2");
    assert_eq!(categories[2].id, "category_id_1");

    // re-ordering categories normally
    store
        .reorder_categories(
            "user_id",
            "team_id",
            ids(&["category_id_2", "category_id_3", "category_id_1"]),
        )
        .await
        .expect("the categories are reordered");

    // verify the board order
    let categories = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(categories.len(), 3);
    assert_eq!(categories[0].id, "category_id_2");
    assert_eq!(categories[1].id, "category_id_3");
    assert_eq!(categories[2].id, "category_id_1");

    // lets try specifying a non existing category ID.
    // It shouldn't cause any problem
    store
        .reorder_categories(
            "user_id",
            "team_id",
            ids(&[
                "category_id_1",
                "category_id_2",
                "category_id_3",
                "non-existing-category-id",
            ]),
        )
        .await
        .expect("an unknown identifier is ignored");

    let categories = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(categories.len(), 3);
    assert_eq!(categories[0].id, "category_id_1");
    assert_eq!(categories[1].id, "category_id_2");
    assert_eq!(categories[2].id, "category_id_3");

    test.teardown().await;
}

/// `testReorderCategoryBoards`.
#[tokio::test]
async fn reorder_category_boards() {
    let test = common::setup().await;
    let store = &test.store;

    // setup
    store
        .create_category(&Category {
            id: "category_id_1".to_owned(),
            name: "Category 1".to_owned(),
            category_type: CategoryType::Custom,
            user_id: "user_id".to_owned(),
            team_id: "team_id".to_owned(),
            ..Category::default()
        })
        .await
        .expect("the category is created");

    store
        .add_update_category_board(
            "user_id",
            "category_id_1",
            &ids(&["board_id_1", "board_id_2", "board_id_3", "board_id_4"]),
        )
        .await
        .expect("the boards are added");

    // verify current order
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 4);
    assert!(metadata.contains(&visible("board_id_1")), "{metadata:?}");
    assert!(metadata.contains(&visible("board_id_2")), "{metadata:?}");
    assert!(metadata.contains(&visible("board_id_3")), "{metadata:?}");
    assert!(metadata.contains(&visible("board_id_4")), "{metadata:?}");

    // reordering
    let new_order = store
        .reorder_category_boards(
            "category_id_1",
            ids(&["board_id_3", "board_id_1", "board_id_2", "board_id_4"]),
        )
        .await
        .expect("the boards are reordered");
    assert_eq!(new_order[0], "board_id_3");
    assert_eq!(new_order[1], "board_id_1");
    assert_eq!(new_order[2], "board_id_2");
    assert_eq!(new_order[3], "board_id_4");

    // verify new order
    let category_boards = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_boards.len(), 1);
    let metadata = &category_boards[0].board_metadata;
    assert_eq!(metadata.len(), 4);
    assert_eq!(metadata[0].board_id, "board_id_3");
    assert_eq!(metadata[1].board_id, "board_id_1");
    assert_eq!(metadata[2].board_id, "board_id_2");
    assert_eq!(metadata[3].board_id, "board_id_4");

    test.teardown().await;
}

/// Not in the original. The sort-order arithmetic of creating and reordering, and which
/// rows each statement reaches.
#[tokio::test]
async fn sort_orders_of_created_and_reordered_categories() {
    let test = common::setup().await;
    let store = &test.store;
    let category = |id: &str, user_id: &str, sort_order: i64| Category {
        id: id.to_owned(),
        name: format!("Category {id}"),
        user_id: user_id.to_owned(),
        team_id: "team_id".to_owned(),
        sort_order,
        ..Category::default()
    };
    let sort_order = async |id: &str| {
        store
            .get_category(id)
            .await
            .expect("the category is found")
            .sort_order
    };

    // A new category is bumped together with the ones that were there before it.
    store
        .create_category(&category("other", "other_user", 0))
        .await
        .expect("another user's category is created");
    store
        .create_category(&category("a", "user_id", 0))
        .await
        .expect("a is created");
    assert_eq!(sort_order("a").await, 10);
    store
        .create_category(&category("b", "user_id", 70))
        .await
        .expect("b is created");
    assert_eq!(sort_order("a").await, 20);
    assert_eq!(sort_order("b").await, 80);
    assert_eq!(
        sort_order("other").await,
        10,
        "another user's categories stay where they are"
    );

    // A deleted category keeps its place when others are created, yet is reordered.
    store
        .delete_category("a", "user_id", "team_id")
        .await
        .expect("a is deleted");
    store
        .create_category(&category("c", "user_id", 0))
        .await
        .expect("c is created");
    assert_eq!(sort_order("a").await, 20);
    assert_eq!(sort_order("b").await, 90);
    assert_eq!(sort_order("c").await, 10);

    let echoed = store
        .reorder_categories("user_id", "team_id", ids(&["b", "a", "c", "other"]))
        .await
        .expect("the categories are reordered");
    assert_eq!(echoed, ["b", "a", "c", "other"]);
    assert_eq!(sort_order("b").await, 0);
    assert_eq!(sort_order("a").await, 10);
    assert_eq!(sort_order("c").await, 20);
    assert_eq!(
        sort_order("other").await,
        10,
        "another user's category is out of reach"
    );

    // An empty order changes nothing.
    let echoed = store
        .reorder_categories("user_id", "team_id", Vec::new())
        .await
        .expect("an empty order is accepted");
    assert!(echoed.is_empty(), "got {echoed:?}");
    assert_eq!(sort_order("c").await, 20);

    test.teardown().await;
}

/// Not in the original, whose reorder statements splice the identifiers into the SQL
/// text (`server/services/store/sqlstore/category.go:221`). Here they are data.
#[tokio::test]
async fn reorder_categories_treats_identifiers_as_data() {
    let test = common::setup().await;
    let store = &test.store;

    for id in ["plain", "quo'te"] {
        store
            .create_category(&Category {
                id: id.to_owned(),
                name: id.to_owned(),
                user_id: "user_id".to_owned(),
                team_id: "team_id".to_owned(),
                ..Category::default()
            })
            .await
            .expect("the category is created");
    }

    // Spliced into `CASE id WHEN '<id>' THEN <n> ...`, the first identifier would move
    // every category of the user to position 5.
    store
        .reorder_categories(
            "user_id",
            "team_id",
            ids(&["x' THEN 5 ELSE 5 END --", "quo'te", "plain"]),
        )
        .await
        .expect("identifiers with SQL syntax in them are accepted");

    let categories = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(category_ids(&categories), ["quo'te", "plain"]);
    assert_eq!(
        categories
            .iter()
            .map(|category| category.sort_order)
            .collect::<Vec<i64>>(),
        [10, 20]
    );

    test.teardown().await;
}

/// A system category of `user_id` in `team_id`, for the two tests below.
fn boards_category() -> Category {
    Category {
        id: "category_id".to_owned(),
        name: "Boards".to_owned(),
        user_id: "user_id".to_owned(),
        team_id: "team_id".to_owned(),
        create_at: 1000,
        update_at: 1000,
        category_type: CategoryType::System,
        ..Category::default()
    }
}

/// Not in the original. An update writes the name, the modification time and the
/// collapsed state, and nothing else.
#[tokio::test]
async fn update_category_leaves_other_members_and_rows_alone() {
    let test = common::setup().await;
    let store = &test.store;

    let category = boards_category();
    store
        .create_category(&category)
        .await
        .expect("the category is created");

    store
        .update_category(&Category {
            name: "Renamed".to_owned(),
            user_id: "someone_else".to_owned(),
            team_id: "another_team".to_owned(),
            create_at: 5,
            update_at: 2000,
            delete_at: 3000,
            collapsed: true,
            sort_order: 500,
            category_type: CategoryType::Custom,
            ..category.clone()
        })
        .await
        .expect("the category is updated");
    let stored = store
        .get_category("category_id")
        .await
        .expect("the category is found");
    assert_eq!(
        stored,
        Category {
            name: "Renamed".to_owned(),
            update_at: 2000,
            collapsed: true,
            sort_order: 10,
            ..category.clone()
        }
    );

    // Updating a category that does not exist succeeds and writes nothing.
    store
        .update_category(&Category {
            id: "nonexistent".to_owned(),
            ..category
        })
        .await
        .expect("updating nothing succeeds");
    let error = store
        .get_category("nonexistent")
        .await
        .expect_err("an update does not create a category");
    assert!(error.is_not_found(), "got {error:?}");

    test.teardown().await;
}

/// Not in the original. A deletion needs the owner and the team to match, and a deleted
/// category is out of reach of further updates and deletions.
#[tokio::test]
async fn delete_category_matches_owner_and_team_and_happens_once() {
    let test = common::setup().await;
    let store = &test.store;

    let category = boards_category();
    store
        .create_category(&category)
        .await
        .expect("the category is created");

    store
        .delete_category("category_id", "someone_else", "team_id")
        .await
        .expect("deleting nothing succeeds");
    store
        .delete_category("category_id", "user_id", "another_team")
        .await
        .expect("deleting nothing succeeds");
    let stored = store
        .get_category("category_id")
        .await
        .expect("the category is found");
    assert_eq!(stored.delete_at, 0);

    store
        .delete_category("category_id", "user_id", "team_id")
        .await
        .expect("the category is deleted");
    let deleted = store
        .get_category("category_id")
        .await
        .expect("a deleted category is still found");
    assert!(deleted.delete_at > 0, "got {deleted:?}");
    assert_eq!(deleted.category_type, CategoryType::System);
    let listed = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert!(listed.is_empty(), "a deleted category is not listed");

    store
        .update_category(&Category {
            name: "Renamed again".to_owned(),
            ..category
        })
        .await
        .expect("updating a deleted category succeeds");
    store
        .delete_category("category_id", "user_id", "team_id")
        .await
        .expect("deleting a deleted category succeeds");
    let unchanged = store
        .get_category("category_id")
        .await
        .expect("a deleted category is still found");
    assert_eq!(unchanged, deleted);

    test.teardown().await;
}

/// Not in the original. Rows the store does not write itself: columns left `NULL` by an
/// older schema, and a type this server does not know.
#[tokio::test]
async fn categories_written_by_others() {
    let test = common::setup().await;
    let store = &test.store;
    let categories = format!("{TABLE_PREFIX}categories");
    let category_boards = format!("{TABLE_PREFIX}category_boards");

    execute_raw(
        &test,
        &format!(
            "INSERT INTO {categories} (id, name, user_id, team_id, create_at, update_at, \
             delete_at, type) VALUES \
             ('null_type', 'A', 'user_id', 'team_id', 1, 1, 0, NULL), \
             ('empty_type', 'B', 'user_id', 'team_id', 1, 1, 0, ''), \
             ('system_type', 'C', 'user_id', 'team_id', 1, 1, 0, 'system')"
        ),
    )
    .await;
    execute_raw(
        &test,
        &format!(
            "INSERT INTO {category_boards} (id, user_id, category_id, board_id) \
             VALUES ('row_id', 'user_id', 'null_type', 'board_id')"
        ),
    )
    .await;

    // A missing type reads as a custom category, a missing sort order as zero, and a
    // missing hidden flag as visible.
    let listed = store
        .get_user_category_boards("user_id", "team_id")
        .await
        .expect("the categories are listed");
    assert_eq!(listed.len(), 3);
    for entry in &listed {
        let category = &entry.category;
        let expected_type = match category.id.as_str() {
            "system_type" => CategoryType::System,
            _ => CategoryType::Custom,
        };
        assert_eq!(category.category_type, expected_type, "{category:?}");
        assert_eq!(category.sort_order, 0, "{category:?}");
        assert!(!category.collapsed, "{category:?}");
    }
    let with_board = listed
        .iter()
        .find(|entry| entry.category.id == "null_type")
        .expect("the category is listed");
    assert_eq!(with_board.board_metadata, [visible("board_id")]);

    // An unknown type is an error for every read that meets the row.
    execute_raw(
        &test,
        &format!(
            "INSERT INTO {categories} (id, name, user_id, team_id, create_at, update_at, \
             delete_at, type) VALUES ('odd_type', 'D', 'user_id', 'team_id', 1, 1, 0, 'odd')"
        ),
    )
    .await;
    let error = store
        .get_category("odd_type")
        .await
        .expect_err("the type is not one this server knows");
    assert!(
        matches!(error, StoreError::Corrupt { column: "type", .. }),
        "got {error:?}"
    );
    let error = store
        .get_user_categories("user_id", "team_id")
        .await
        .expect_err("the listing meets the row");
    assert!(
        matches!(error, StoreError::Corrupt { column: "type", .. }),
        "got {error:?}"
    );
    assert!(
        store.get_category("system_type").await.is_ok(),
        "other rows are still readable"
    );

    test.teardown().await;
}
