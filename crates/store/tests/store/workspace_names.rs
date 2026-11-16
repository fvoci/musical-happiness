//! The migration that makes a workspace's name its own among the workspaces its
//! organization has in use (23): names stored twice before keep the oldest workspace's,
//! and the others are told apart, whatever names are there already. And the name the
//! first user gives the organization, which is its first workspace's.

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
    common::execute_raw(&common::raw_connection(&database), &statements.concat()).await;

    let store = Store::connect(database.db_type(), database.database_connection())
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

/// The organization's name is its first workspace's too, and a name is one workspace's
/// among those an organization has in use: where the organization there is was given
/// another workspace of that name before anybody signed up, the name is refused and
/// nothing is stored. No other way in makes a workspace: an invitation leads into one
/// that is there, and who starts an organization of their own starts it with its one.
#[tokio::test]
async fn the_first_user_names_the_organization_as_no_other_workspace_of_it_is_named() {
    let test = common::setup().await;
    // Stored past the organization's own way of making a workspace, which is its
    // administrators' and so nobody's before the first user.
    test.system()
        .execute_unchecked(&format!(
            "INSERT INTO workspaces (id, settings, create_at, update_at, org_id, name) \
             SELECT '{}', '{{}}', 1, 1, org_id, 'Design' FROM workspaces WHERE id = '{}'",
            new_id(),
            test.workspace_id
        ))
        .await
        .expect("another workspace of the organization");

    let mut first = fvoci_model::User {
        id: new_id(),
        username: "first".to_owned(),
        email: "first@example.com".to_owned(),
        ..fvoci_model::User::default()
    };
    let error = test
        .sign_up(&mut first, "Design", fvoci_store::Founding::First)
        .await
        .expect_err("the name is another workspace's");
    assert!(
        matches!(
            error,
            fvoci_store::StoreError::Conflict(fvoci_store::Taken::WorkspaceName)
        ),
        "{error:?}"
    );
    assert!(
        !test.is_bootstrapped().await.expect("the flag is read"),
        "nothing was stored, nothing is decided"
    );
    let workspace_id = test
        .sign_up(&mut first, "Design studio", fvoci_store::Founding::First)
        .await
        .expect("under another name the first user starts the organization");
    assert_eq!(workspace_id, test.workspace_id);
    assert!(first.is_admin);

    test.teardown().await;
}
