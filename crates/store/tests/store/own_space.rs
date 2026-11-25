//! An account's own space: made with the account, one for each, of the shape the
//! accounts that were there got theirs in, and none of what the instance counts as an
//! organization.

use fvoci_model::{OrgKind, User, new_id};
use fvoci_store::{DbType, Founding, StoreError, Tenant};

use crate::common::{self, PRE0075_CREATE_COL, PRE0075_UPDATE_COL};

/// Whatever handle stores an account, the account has its own space afterwards: an
/// organization of its own with one workspace, both without a name, the account the
/// administrator of both and nobody else in either. The organization the handle
/// reaches is the one the account joins besides, as before: the wall of the
/// transaction is where it was after the space was stored past it.
#[tokio::test]
async fn an_account_is_made_with_its_own_space() {
    let test = common::setup().await;
    let system = test.system();
    let unset = test.scoped(Tenant::Unset);
    let member = common::user(&test, "member").await;
    let outside = common::user(&unset, "outside").await;
    let operator = common::user(&system, "operator").await;

    let mut spaces = Vec::new();
    for user_id in [&member, &outside, &operator] {
        assert_eq!(
            common::own_space_shape(&system, user_id).await,
            common::own_space(),
            "{user_id}"
        );
        spaces.push(common::own_workspace(&test, user_id).await);
    }
    spaces.sort();
    spaces.dedup();
    assert_eq!(spaces.len(), 3, "a space for each account");

    // The member is in the organization of the handle too, listed before their own.
    let listed: Vec<(String, OrgKind)> = unset
        .workspaces_of_user(&member, fvoci_store::Making::with_password())
        .await
        .expect("listed")
        .into_iter()
        .map(|workspace| (workspace.org_id, workspace.kind))
        .collect();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert_eq!(listed[0], (test.org_id.clone(), OrgKind::Team));
    assert_eq!(listed[1].1, OrgKind::Personal);
    test.teardown().await;
}

/// The account that signs up with the name of an organization has its own space
/// beside the organization it starts: the first of the instance, and on a database
/// that holds several whoever founds another.
#[tokio::test]
async fn who_starts_an_organization_has_a_space_beside_it() {
    let test = common::setup().await;
    let system = test.system();
    let mut foundings = vec![("first", Founding::First)];
    if common::db_type() == DbType::Postgres {
        foundings.push(("another", Founding::New));
    }
    for (username, founding) in foundings {
        let mut founder = User {
            id: new_id(),
            username: username.to_owned(),
            email: format!("{username}@example.com"),
            ..User::default()
        };
        let workspace_id = test
            .sign_up(&mut founder, username, founding)
            .await
            .expect("the account starts an organization");
        assert_eq!(
            common::own_space_shape(&system, &founder.id).await,
            common::own_space(),
            "{username}"
        );
        let listed: Vec<(String, OrgKind)> = test
            .workspaces_of_user(&founder.id, fvoci_store::Making::with_password())
            .await
            .expect("listed")
            .into_iter()
            .map(|workspace| (workspace.id, workspace.kind))
            .collect();
        let own = common::own_workspace(&test, &founder.id).await;
        assert_eq!(
            listed,
            [(workspace_id, OrgKind::Team), (own, OrgKind::Personal)],
            "{username}: the workspace of the organization, and their own last"
        );
    }
    assert_eq!(
        test.instance_organization().await.expect("read"),
        Some(test.org_id.clone()),
        "the instance's own is the one the first account took"
    );
    test.teardown().await;
}

/// One account owns one space, a space is of an account and an organization of people
/// of none, and neither becomes the other: the schema refuses each, whatever the
/// server does.
#[tokio::test]
async fn one_account_owns_one_space_which_stays_what_it_is() {
    let test = common::setup().await;
    let system = test.system();
    let owner = common::user(&test, "owner").await;
    let (own, shared) = (OrgKind::Personal.as_str(), OrgKind::Team.as_str());
    let insert = |kind: &str, owner: &str| {
        format!(
            "INSERT INTO organizations (id, name, kind, owner_id, settings, created_at, updated_at) \
             VALUES ('{}', '', '{kind}', {owner}, '{{}}', 1, 1)",
            new_id()
        )
    };
    let theirs = format!("'{owner}'");
    let (unique_owner, owner_check) = match common::db_type() {
        DbType::Sqlite => ("UNIQUE constraint failed", "CHECK constraint failed"),
        DbType::Postgres => (
            "organizations_owner_id_key",
            "organizations_owner_of_personal",
        ),
    };
    let keep_kind = "an organization does not change its kind or its owner";
    for (statement, expected, why) in [
        (
            insert(own, &theirs),
            unique_owner,
            "a second space of the account",
        ),
        (insert(own, "NULL"), owner_check, "a space of nobody"),
        (
            format!(
                "UPDATE organizations SET kind = '{shared}', owner_id = NULL \
                 WHERE owner_id = '{owner}'"
            ),
            keep_kind,
            "a space made an organization of people",
        ),
        (
            format!("UPDATE organizations SET owner_id = NULL WHERE owner_id = '{owner}'"),
            keep_kind,
            "a space taken from its account",
        ),
        (
            format!(
                "UPDATE organizations SET kind = '{own}', owner_id = '{owner}' WHERE id = '{}'",
                test.org_id
            ),
            keep_kind,
            "an organization of people made a space",
        ),
    ] {
        let Err(StoreError::Database(error)) = system.execute_unchecked(&statement).await else {
            panic!("{why}: expected a database constraint refusal");
        };
        let message = error.to_string();
        assert!(
            message.contains(expected),
            "{why}: expected {expected:?}, got {message:?}"
        );
    }
    assert_eq!(
        common::own_space_shape(&system, &owner).await,
        common::own_space(),
        "the space is as it was"
    );
    test.teardown().await;

    // On a database with no shared organization yet (so SQLite's one-shared-organization
    // trigger does not mask the CHECK constraint), an owned shared organization is refused
    // by the CHECK constraint itself on both SQLite and PostgreSQL.
    let fresh_db = common::empty_database().await;
    let fresh = fvoci_store::Store::connect(fresh_db.db_type(), fresh_db.database_connection())
        .await
        .expect("fresh store");
    let fresh_owner = common::user(&fresh.scoped(Tenant::Unset), "fresh-owner").await;
    let owned_org = insert(shared, &format!("'{fresh_owner}'"));
    let Err(StoreError::Database(error)) = fresh.system().execute_unchecked(&owned_org).await
    else {
        panic!("an organization of people, owned: expected a database CHECK constraint refusal");
    };
    let message = error.to_string();
    assert!(
        message.contains(owner_check),
        "an organization of people, owned: expected {owner_check:?}, got {message:?}"
    );
    fresh.close().await;
    fresh_db.remove().await;
}

/// No account's own space is the instance's organization, however old, or the one
/// organization a login page shows without being told which; and nobody is anything in
/// one to the operations of an organization, its owner neither.
#[tokio::test]
async fn an_own_space_is_not_counted_among_the_organizations() {
    let test = common::setup().await;
    let system = test.system();
    let unset = test.scoped(Tenant::Unset);
    let asked = async || {
        let instance = unset.instance_organization().await.expect("read");
        let login = unset.login_organization(None).await.expect("read");
        (instance, login.map(|organization| organization.id))
    };
    let before = asked().await;
    assert_eq!(
        before,
        (Some(test.org_id.clone()), Some(test.org_id.clone())),
        "the one organization"
    );

    let owner = common::user(&unset, "owner").await;
    let own_org = system
        .texts_unchecked(&format!(
            "SELECT CAST(id AS TEXT) FROM organizations WHERE owner_id = '{owner}'"
        ))
        .await
        .expect("read")
        .remove(0);
    // The space is older than every organization there is.
    let aged = system
        .execute_unchecked(&format!(
            "UPDATE organizations SET created_at = 1 WHERE id = '{own_org}'"
        ))
        .await
        .expect("aged");
    assert_eq!(aged, 1);

    assert_eq!(asked().await, before, "the space changes neither");
    assert_eq!(
        unset
            .login_organization(Some(&own_org))
            .await
            .expect("read"),
        None,
        "a space has no login page"
    );
    assert_eq!(
        unset
            .org_role_of_user(&owner, &own_org)
            .await
            .expect("asked"),
        None,
        "its owner administers it by no route of an organization"
    );
    let own_workspace = common::own_workspace(&test, &owner).await;
    let resolved = unset
        .resolve_workspace(&owner, &own_workspace)
        .await
        .expect("resolved")
        .expect("the owner enters their space");
    assert_eq!(
        (resolved.kind, resolved.org_role),
        (OrgKind::Personal, None),
        "resolve_workspace agrees with org_role_of_user: nobody holds an org role in an own space"
    );
    test.teardown().await;
}

/// Which sessions a space takes is read with its workspace, and kept by who read it
/// until the space's revision moves on. An account's first password moves it, whether
/// its person set it or the server's operator did, and moves no other organization's.
#[tokio::test]
async fn a_first_password_moves_the_revision_of_the_space_alone() {
    let test = common::setup().await;
    common::user(&test, "founder").await;
    let mut moved = Vec::new();
    for name in ["person", "operated"] {
        let user = common::user(&test, name).await;
        let of_owner =
            format!("SELECT CAST(id AS TEXT) FROM organizations WHERE owner_id = '{user}'");
        let space = test
            .system()
            .texts_unchecked(&of_owner)
            .await
            .expect("read");
        let space = test.for_org(&space[0]).expect("an organization");
        let before = (
            test.visibility_version().await,
            space.visibility_version().await,
        );
        if name == "operated" {
            let set = test.reset_user_password(name, "a hash").await;
            assert_eq!(set.expect("set"), user, "the operator's word");
        } else {
            let set = test
                .change_user_password(&user, "", "a hash", &new_id(), "no digest")
                .await;
            assert!(set.expect("set"), "the account's first password");
        }
        let after = (
            test.visibility_version().await,
            space.visibility_version().await,
        );
        moved.push((name, before, after));
    }
    test.teardown().await;
    for (name, before, after) in moved {
        assert_ne!(after.1, before.1, "{name}: the space's revision moves on");
        assert_eq!(after.0, before.0, "{name}: the organization's stays");
    }
}

/// Migration 0050 (`erase_started_at = 0` filter): active accounts and accounts waiting
/// in the deletion grace period both receive a valid own space (and cancelling the
/// waiting account's deletion leaves its space usable), while accounts whose erasure
/// already began or completed receive no own space.
#[tokio::test]
async fn migration_0050_gives_spaces_to_active_and_waiting_accounts_only() {
    let database = common::empty_database().await;
    let connection = common::raw_connection(&database);
    fvoci_store::testing::migrate_up_to(database.db_type(), database.database_connection(), 49)
        .await;

    let legacy_col = concat!("delete", "_at");
    let (active, waiting, mid_erasure, erased) = (new_id(), new_id(), new_id(), new_id());
    let insert = |id: &str,
                  name: &str,
                  legacy_del: i64,
                  delete_after: i64,
                  started: i64,
                  done: i64| {
        format!(
            "INSERT INTO users (id, username, email, password_hash, {PRE0075_CREATE_COL}, {PRE0075_UPDATE_COL}, \
             {legacy_col}, delete_after, erase_started_at, erased_at) \
             VALUES ('{id}', '{name}', '{name}@example.com', '', 1, 1, {legacy_del}, \
             {delete_after}, {started}, {done});"
        )
    };
    common::execute_raw(
        &connection,
        &[
            insert(&active, "active", 0, 0, 0, 0),
            insert(&waiting, "waiting", 100, 9_000_000_000_000, 0, 0),
            insert(&mid_erasure, "mid", 100, 200, 300, 0),
            insert(&erased, "", 100, 200, 300, 400),
        ]
        .concat(),
    )
    .await;

    let store = fvoci_store::Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrated from 49 through 0050 and latest");
    let system = store.system();

    for (user_id, label) in [(&active, "active"), (&waiting, "waiting in grace")] {
        assert_eq!(
            common::own_space_shape(&system, user_id).await,
            common::own_space(),
            "{label} account received a valid own space"
        );
    }
    // Cancelling the waiting account's deletion restores the account with its backfilled
    // own space ready for use.
    assert!(
        store
            .cancel_account_deletion(&waiting, 150)
            .await
            .expect("cancelled"),
        "waiting account deletion can be cancelled after 0050"
    );
    let listed = store
        .workspaces_of_user(&waiting, fvoci_store::Making::with_password())
        .await
        .expect("listed");
    assert_eq!(
        listed.len(),
        1,
        "backfilled own space is listed: {listed:?}"
    );
    assert_eq!(listed[0].kind, OrgKind::Personal);

    for (user_id, label) in [(&mid_erasure, "mid-erasure"), (&erased, "already erased")] {
        let empty: [Vec<String>; 4] = Default::default();
        assert_eq!(
            common::own_space_shape(&system, user_id).await,
            empty,
            "{label} account must not receive an own space"
        );
    }

    store.close().await;
    database.remove().await;
}
