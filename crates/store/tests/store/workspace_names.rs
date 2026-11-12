//! The migration that makes a workspace's name its own among the workspaces its
//! organization has in use (23): names stored twice before keep the oldest workspace's,
//! and the others are told apart, whatever names are there already.

use fvoci_model::new_id;
use fvoci_store::Store;

use crate::common;

/// A workspace renamed after its identifier does not take a name another workspace has:
/// the migration goes through, and every name in use is one workspace's.
#[tokio::test]
async fn names_stored_twice_are_told_apart_even_where_the_new_name_is_taken() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_org_management(&database).await;
    let (org, older, newer, other) = (new_id(), new_id(), new_id(), new_id());
    let workspace = |id: &str, name: &str, at: i64| {
        format!(
            "INSERT INTO workspaces (id, settings, create_at, update_at, org_id, name) \
             VALUES ('{id}', '{{}}', {at}, {at}, '{org}', '{name}');"
        )
    };
    let statements = [
        format!(
            "INSERT INTO organizations (id, name, settings, create_at, update_at) \
             VALUES ('{org}', 'Org', '{{}}', 1, 1);"
        ),
        workspace(&older, "Design", 1),
        workspace(&newer, "Design", 2),
        workspace(&other, &format!("Design ({newer})"), 3),
    ];
    common::execute_raw(&common::raw_dbconfig(&database), &statements.concat()).await;

    let store = Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("the migration goes through");
    let mut names: Vec<(String, String)> = store
        .for_org(&org)
        .expect("an identifier")
        .org_workspaces("")
        .await
        .expect("the workspaces")
        .into_iter()
        .map(|workspace| (workspace.id, workspace.name))
        .collect();
    names.sort();
    let name_of = |id: &str| {
        names
            .iter()
            .find(|(of, _)| of == id)
            .map(|(_, name)| name.clone())
    };
    assert_eq!(
        name_of(&other),
        Some(format!("Design ({newer})")),
        "the workspace that had the name before keeps it"
    );
    assert_eq!(
        name_of(&older).as_deref(),
        Some("Design"),
        "the oldest keeps it"
    );
    let mut in_use: Vec<&String> = names
        .iter()
        .map(|(_, name)| name)
        .filter(|name| !name.is_empty())
        .collect();
    in_use.sort();
    let before = in_use.len();
    in_use.dedup();
    assert_eq!(
        in_use.len(),
        before,
        "each name is one workspace's: {names:?}"
    );
    store.close().await;
    database.remove().await;
}
