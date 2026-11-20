//! An account's own space: made with the account, one for each, of the shape the
//! accounts that were there got theirs in, and none of what the instance counts as an
//! organization.

use fvoci_model::{OrgKind, User, new_id};
use fvoci_store::{DbType, Founding, Tenant};

use crate::common;

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
        .workspaces_of_user(&member, None)
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
            .workspaces_of_user(&founder.id, None)
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
            "INSERT INTO organizations (id, name, kind, owner_id, settings, create_at, update_at) \
             VALUES ('{}', '', '{kind}', {owner}, '{{}}', 1, 1)",
            new_id()
        )
    };
    let theirs = format!("'{owner}'");
    for (statement, why) in [
        (insert(own, &theirs), "a second space of the account"),
        (insert(own, "NULL"), "a space of nobody"),
        (insert(shared, &theirs), "an organization of people, owned"),
        (
            format!(
                "UPDATE organizations SET kind = '{shared}', owner_id = NULL \
                 WHERE owner_id = '{owner}'"
            ),
            "a space made an organization of people",
        ),
        (
            format!("UPDATE organizations SET owner_id = NULL WHERE owner_id = '{owner}'"),
            "a space taken from its account",
        ),
        (
            format!(
                "UPDATE organizations SET kind = '{own}', owner_id = '{owner}' WHERE id = '{}'",
                test.org_id
            ),
            "an organization of people made a space",
        ),
    ] {
        let refused = system.execute_unchecked(&statement).await;
        assert!(refused.is_err(), "{why}: {refused:?}");
    }
    assert_eq!(
        common::own_space_shape(&system, &owner).await,
        common::own_space(),
        "the space is as it was"
    );
    test.teardown().await;
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
            "UPDATE organizations SET create_at = 1 WHERE id = '{own_org}'"
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
    test.teardown().await;
}
