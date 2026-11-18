//! The wall between organizations: what PostgreSQL's row-level security keeps apart
//! whatever a query asks for, and the role the tests connect as, which the policies
//! hold. SQLite has one organization and no such wall.

use std::collections::BTreeMap;
use std::path::Path;

use fvoci_model::{
    AuditAction, BlockType, Category, Container, ContainerKind, ContainerMember, ContainerType,
    OrgRole, SubscriberType, Subscription, Upload, UploadPart, User, Workspace, WorkspaceRole,
    is_id, new_id,
};
use fvoci_store::testing::{GLOBAL_TABLES, ORG_TABLES};
use fvoci_store::{
    AuditEntry, DbType, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, SharedContainer, SharingChange, Store,
    StoreError, Tenant,
};
use sqlx::Connection as _;

use crate::common;

/// The policies hold the role the tests connect as, on tables it owns: a table whose
/// row-level security is forced and that has no policy reads as empty to it. A
/// superuser, a role with `BYPASSRLS`, and the owner of a table whose row-level
/// security is not forced read every row; as one of those, every test of the wall
/// would pass with the wall gone.
#[tokio::test]
async fn the_policies_hold_the_role_the_tests_connect_as() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let database = common::empty_database().await;
    let mut conn = common::connect_past_the_store(database.database_connection()).await;
    sqlx::raw_sql(
        "CREATE TABLE probe (x INT); INSERT INTO probe VALUES (1); \
         ALTER TABLE probe ENABLE ROW LEVEL SECURITY",
    )
    .execute(&mut conn)
    .await
    .expect("the probe is made");
    let not_forced: i64 = sqlx::query_scalar("SELECT count(*) FROM probe")
        .fetch_one(&mut conn)
        .await
        .expect("the probe is read");
    sqlx::raw_sql("ALTER TABLE probe FORCE ROW LEVEL SECURITY")
        .execute(&mut conn)
        .await
        .expect("row-level security is forced");
    let forced: i64 = sqlx::query_scalar("SELECT count(*) FROM probe")
        .fetch_one(&mut conn)
        .await
        .expect("the probe is read");
    conn.close().await.expect("the connection closes");
    database.remove().await;

    assert_eq!(
        not_forced, 1,
        "the role owns its tables: without FORCE the policies do not hold it"
    );
    assert_eq!(
        forced, 0,
        "with FORCE and no policy the table reads as empty"
    );
}

/// Stores an open board of the workspace by the user through `store` and returns its
/// identifier.
async fn board(store: &Store, workspace_id: &str, user_id: &str, title: &str) -> String {
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Open,
        title: title.to_owned(),
        ..Container::default()
    };
    store
        .insert_container(&mut board, user_id)
        .await
        .expect("the board is stored");
    board.id
}

/// What two organizations store in one table stays apart, whatever the statement asks
/// for: each reads its own rows, a handle that names no organization reads none and
/// stores none, the system's handle reads all, and nobody stores a row as another's.
#[tokio::test]
async fn the_boards_of_two_organizations_are_out_of_each_others_reach() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let (other_org, their_workspace) = common::organization(&unset, "the other").await;
    let ours = &test.store;
    let theirs = test.for_org(&other_org).expect("an identifier");

    let user_id = common::user(ours, "anybody").await;
    let workspace_id = common::workspace(ours).await;
    let our_board = board(ours, &workspace_id, &user_id, "ours").await;
    let their_board = board(&theirs, &their_workspace, &user_id, "theirs").await;
    board(&theirs, &their_workspace, &user_id, "theirs too").await;

    let count = async |store: &Store| {
        store
            .count_rows("containers")
            .await
            .expect("the boards are counted")
    };
    assert_eq!(count(ours).await, 1, "ours");
    assert_eq!(count(&theirs).await, 2, "theirs");
    assert_eq!(count(&unset).await, 0, "no organization named");
    assert_eq!(count(&test.system()).await, 3, "the system's handle");

    // By its identifier too: what is another's is not there.
    assert_eq!(
        ours.get_container(&our_board).await.expect("ours").title,
        "ours"
    );
    let missing = ours
        .get_container(&their_board)
        .await
        .expect_err("their board is out of reach");
    assert!(missing.is_not_found(), "got {missing:?}");
    // Nor is it changed or deleted: the statement finds no row.
    let changed = ours
        .execute_unchecked(&format!(
            "UPDATE containers SET title = 'taken' WHERE id = '{their_board}'"
        ))
        .await
        .expect("the statement runs");
    assert_eq!(changed, 0, "an update of their board");
    let deleted = ours
        .execute_unchecked("DELETE FROM containers")
        .await
        .expect("the statement runs");
    assert_eq!(deleted, 1, "a delete of everything deletes ours");
    assert_eq!(count(&theirs).await, 2, "and leaves theirs");

    // A row is stored as the organization's own or not at all.
    // In their workspace: a board is of its workspace's organization, which the schema holds it
    // to apart from the wall.
    let insert = |org: &str| {
        format!(
            "INSERT INTO containers (id, org_id, workspace_id, type, title, description, icon, \
             show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{}', {org}, '{their_workspace}', 'O', '', '', '', false, false, '{{}}', '[]', 0, 0)",
            new_id()
        )
    };
    let as_theirs = ours
        .execute_unchecked(&insert(&format!("'{other_org}'")))
        .await
        .expect_err("a row of another organization is refused");
    assert!(
        as_theirs.to_string().contains("row-level security"),
        "{as_theirs}"
    );
    let as_nobody = unset
        .execute_unchecked(&insert("DEFAULT"))
        .await
        .expect_err("a row of no organization is refused");
    // The policy refuses it before the column does.
    assert!(
        as_nobody.to_string().contains("row-level security"),
        "{as_nobody}"
    );
    // The system's handle stores for whom it says, and for nobody if it does not say.
    test.system()
        .execute_unchecked(&insert(&format!("'{other_org}'")))
        .await
        .expect("the system stores a row of an organization it names");
    let unnamed = test
        .system()
        .execute_unchecked(&insert("DEFAULT"))
        .await
        .expect_err("the system names no organization by itself");
    assert!(unnamed.to_string().contains("org_id"), "{unnamed}");
    assert_eq!(count(&theirs).await, 3, "theirs, with the system's row");

    test.teardown().await;
}

/// The organization a transaction reaches is the transaction's and no longer: a
/// connection that goes back to the pool carries nothing of it to the next operation,
/// whichever handle that is of.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_of_an_organization_stays_on_a_pooled_connection() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let (other_org, their_workspace) = common::organization(&unset, "the other").await;
    let theirs = test.for_org(&other_org).expect("an identifier");
    let user_id = common::user(&test, "anybody").await;
    let workspace_id = common::workspace(&test).await;
    board(&test, &workspace_id, &user_id, "ours").await;
    board(&theirs, &their_workspace, &user_id, "theirs").await;
    board(&theirs, &their_workspace, &user_id, "theirs too").await;

    // Many more operations than the pool has connections, of every handle at once:
    // every connection serves each of them in turn.
    let mut reads = tokio::task::JoinSet::new();
    for round in 0..40 {
        for (store, expected) in [
            (test.store.clone(), 1),
            (theirs.clone(), 2),
            (unset.clone(), 0),
            (test.system(), 3),
        ] {
            reads.spawn(async move {
                let counted = store.count_rows("containers").await.expect("counted");
                (round, store.tenant(), counted, expected)
            });
        }
    }
    while let Some(read) = reads.join_next().await {
        let (round, tenant, counted, expected) = read.expect("the read ends");
        assert_eq!(counted, expected, "round {round}, {tenant:?}");
    }

    // And the connections themselves, looked at past the store's transactions, which
    // would set the organization anew: none carries one, or the system's reach.
    let left = test.settings_left_on_the_connections().await;
    assert!(!left.is_empty(), "the pool has connections to look at");
    for (org, system) in &left {
        assert_eq!(org, "", "an organization stayed on a connection: {left:?}");
        assert!(
            system.is_empty() || system == "off",
            "the system's reach stayed on a connection: {left:?}"
        );
    }

    test.teardown().await;
}

/// The tables of the test's schema, by name.
async fn tables(database_connection: &str) -> Vec<String> {
    let mut conn = common::connect_past_the_store(database_connection).await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name::text FROM information_schema.tables \
         WHERE table_schema = current_schema() ORDER BY table_name",
    )
    .fetch_all(&mut conn)
    .await
    .expect("the tables are listed");
    conn.close().await.expect("the connection closes");
    tables
}

/// A policy as the catalog has it: its command, whether it is permissive, its
/// condition, and its condition for new rows.
type Policy = (String, String, Option<String>, Option<String>);

/// A row of `pg_policies`: the table and the name of a policy, and the policy.
type PolicyRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// The policies of the test's schema, by table and name.
async fn policies(conn: &mut sqlx::postgres::PgConnection) -> BTreeMap<(String, String), Policy> {
    let rows: Vec<PolicyRow> = sqlx::query_as(
        "SELECT tablename::text, policyname::text, cmd::text, permissive::text, qual, \
         with_check FROM pg_policies WHERE schemaname = current_schema()",
    )
    .fetch_all(conn)
    .await
    .expect("the policies are read");
    rows.into_iter()
        .map(|(table, policy, cmd, permissive, qual, check)| {
            ((table, policy), (cmd, permissive, qual, check))
        })
        .collect()
}

/// Checks that a table is behind the wall: row-level security enabled and forced on
/// the owner, the policy `org_wall` as `wall` is, and a column `org_id` that takes no
/// NULL and, where the server does not write it, defaults to the organization of the
/// transaction.
async fn assert_behind_the_wall(
    conn: &mut sqlx::postgres::PgConnection,
    table: &str,
    wall: &Policy,
    policies: &BTreeMap<(String, String), Policy>,
) {
    let (enabled, forced): (bool, bool) = sqlx::query_as(
        "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid = to_regclass($1)",
    )
    .bind(table)
    .fetch_one(&mut *conn)
    .await
    .expect("the table is in the catalog");
    assert!(enabled, "{table}: row-level security is enabled");
    assert!(forced, "{table}: and forced on the owner");
    let policy = policies
        .get(&(table.to_owned(), "org_wall".to_owned()))
        .unwrap_or_else(|| panic!("{table} has the policy org_wall"));
    if table == "organizations" {
        // The same wall, by the organization's own identifier.
        let (command, permissive, using, check) = wall;
        let using = using.as_deref().map(|using| using.replace("org_id", "id"));
        assert_eq!(
            policy,
            &(command.clone(), permissive.clone(), using, check.clone()),
            "{table}: the policy is the one of boards, by its identifier"
        );
        return;
    }
    assert_eq!(policy, wall, "{table}: the policy is the one of boards");
    let (nullable, default): (String, Option<String>) = sqlx::query_as(
        "SELECT is_nullable::text, column_default::text FROM information_schema.columns \
         WHERE table_schema = current_schema() AND table_name = $1 AND column_name = 'org_id'",
    )
    .bind(table)
    .fetch_one(&mut *conn)
    .await
    .unwrap_or_else(|error| panic!("{table} has the column org_id: {error}"));
    assert_eq!(nullable, "NO", "{table}: a row is some organization's");
    // The server names the organization itself in two tables; elsewhere a row gets
    // the organization of the transaction that stores it.
    if ["workspaces", "organization_members"].contains(&table) {
        assert_eq!(default, None, "{table}: the server writes the organization");
    } else {
        let default = default.unwrap_or_default();
        assert!(default.contains("fvoci.org"), "{table}: {default}");
    }
}

/// Every table of the schema is one of the few that are no organization's, or is
/// behind the wall: its row-level security is enabled and forced on its owner, and its
/// policy `org_wall` is the one policy that lets rows be changed, the same on every
/// table. A table that a later migration adds and forgets here fails this test.
#[tokio::test]
async fn every_table_is_no_organizations_or_behind_the_wall() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let all = tables(&test.database_connection()).await;
    for global in GLOBAL_TABLES {
        assert!(
            all.iter().any(|table| table == global),
            "{global} is a table"
        );
    }
    let walled: Vec<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|table| !GLOBAL_TABLES.contains(table))
        .collect();
    assert_eq!(
        walled, ORG_TABLES,
        "every table that is not nobody's is in the list of the organizations' tables"
    );

    let mut conn = common::connect_past_the_store(&test.database_connection()).await;
    let policies = policies(&mut conn).await;
    let wall = policies
        .get(&("containers".to_owned(), "org_wall".to_owned()))
        .expect("boards has the policy")
        .clone();
    let (command, permissive, using, check) = &wall;
    let using = using.as_deref().expect("the policy has a condition");
    assert!(
        using.contains("fvoci.org") && using.contains("fvoci.system") && using.contains("org_id"),
        "{using}"
    );
    assert_eq!(command, "ALL", "for every command");
    assert_eq!(permissive, "PERMISSIVE");
    assert_eq!(check, &None, "new rows are held to the same condition");
    for table in &walled {
        assert_behind_the_wall(&mut conn, table, &wall, &policies).await;
    }

    // Besides the wall, only the reading of the system organization's templates.
    for ((table, policy), (command, permissive, using, _)) in &policies {
        if policy == "org_wall" {
            continue;
        }
        assert_eq!(policy, "org_templates", "{table} has the policy {policy}");
        assert!(
            ["containers", "blocks"].contains(&table.as_str()),
            "{table} lets the templates be read"
        );
        assert_eq!(
            (command.as_str(), permissive.as_str()),
            ("SELECT", "PERMISSIVE")
        );
        assert!(
            using.as_deref().unwrap_or_default().contains(SYSTEM_ORG_ID),
            "{table}: {using:?}"
        );
    }
    for global in GLOBAL_TABLES {
        let enabled: bool =
            sqlx::query_scalar("SELECT relrowsecurity FROM pg_class WHERE oid = to_regclass($1)")
                .bind(global)
                .fetch_one(&mut conn)
                .await
                .expect("the table is in the catalog");
        assert!(!enabled, "{global} is nobody's and has no wall");
    }
    conn.close().await.expect("the connection closes");
    test.teardown().await;
}

/// Stores, through `store`, an upload with a part, an object being written and a file's version.
async fn in_the_files(store: &Store, user_id: &str, board_id: &str, username: &str) {
    let upload = Upload {
        id: new_id(),
        user_id: user_id.to_owned(),
        container_id: board_id.to_owned(),
        name: "upload.bin".to_owned(),
        size: 1,
        part_size: 1,
        create_at: 1,
        expires_at: i64::MAX,
        ..Upload::default()
    };
    store
        .create_upload(&upload)
        .await
        .expect("an upload is opened");
    store
        .put_upload_part(
            &upload.id,
            &UploadPart {
                number: 1,
                size: 1,
                part_id: String::new(),
            },
        )
        .await
        .expect("a part is recorded");
    store
        .mark_writing(&format!("boards/{username}/being-written"), i64::MAX)
        .await
        .expect("an object is being written");
    let added = store.add_file_version(board_id, "7first.png", 1, "7second.png", user_id);
    added.await.expect("a second version of a file");
}

/// Makes the user an administrator of the workspace: a role is recorded as given by a
/// board's manager only while they administer its workspace.
async fn administer(store: &Store, workspace_id: &str, user_id: &str) {
    let made = store
        .add_workspace_member(workspace_id, user_id, WorkspaceRole::Admin)
        .await;
    if made.is_err() {
        store
            .execute_unchecked(&format!(
                "UPDATE workspace_members SET role = 'admin' WHERE user_id = '{user_id}' \
                 AND workspace_id = '{workspace_id}'"
            ))
            .await
            .expect("the user administers the workspace");
    }
}

/// Stores, through `store`, something in every table of an organization: a workspace, a
/// board with a member and the record of a change of its roles, a card and a comment
/// whose text was changed, files, sharing, a subscription, a category with the board in
/// it, an upload with a part, an object being written, and a board and a card in the
/// trash. The user is made through `store`
/// too, and so joins its organization. Returns the identifiers of the card that went to
/// the trash and of the user.
async fn something_in_every_table(store: &Store, username: &str) -> (String, String) {
    // As on a server that has a search server: what changes is noted for it.
    store.keep_search_outbox();
    let user_id = common::user(store, username).await;
    let comment = crate::file_info::posted_comment(store, &user_id).await;
    let board_id = comment.container_id.clone();
    let board = store.get_container(&board_id).await.expect("the board");
    store
        .patch_block(
            &comment.id,
            &crate::file_info::title_and_body(Some("v2")),
            &user_id,
        )
        .await
        .expect("the comment is changed");
    store
        .save_member(&ContainerMember {
            container_id: board_id.clone(),
            user_id: user_id.clone(),
            scheme_editor: true,
            ..ContainerMember::default()
        })
        .await
        .expect("a member is stored");
    let as_viewer = ContainerMember {
        container_id: board_id.clone(),
        user_id: user_id.clone(),
        scheme_viewer: true,
        ..ContainerMember::default()
    };
    let entry = AuditEntry::new(
        &board.workspace_id,
        &user_id,
        AuditAction::SelfAddedToContainer,
        &board_id,
    );
    administer(store, &board.workspace_id, &user_id).await;
    store
        .change_member_recorded(&as_viewer, &entry)
        .await
        .expect("a change of roles is recorded");
    store
        .set_sharing(&board_id, SharingChange::On, &new_id(), &user_id)
        .await
        .expect("the board is shared");
    store
        .create_subscription(Subscription {
            block_type: BlockType::Card,
            block_id: comment.parent_id.clone(),
            subscriber_type: SubscriberType::User,
            subscriber_id: user_id.clone(),
            notified_at: 0,
            create_at: 0,
            delete_at: 0,
        })
        .await
        .expect("a subscription is stored");
    let category = Category {
        id: new_id(),
        name: "a category".to_owned(),
        user_id: user_id.clone(),
        workspace_id: board.workspace_id.clone(),
        ..Category::default()
    };
    store
        .create_category(&category)
        .await
        .expect("a category is stored");
    store
        .add_update_category_container(&user_id, &category.id, std::slice::from_ref(&board_id))
        .await
        .expect("the board is filed");
    in_the_files(store, &user_id, &board_id, username).await;
    a_body_edited_together(store, &board_id, &user_id).await;
    a_team_on_board(store, &board, &user_id).await;
    // The trash: a card of the live board, and a whole board.
    let mut card = fvoci_model::Block {
        id: new_id(),
        container_id: board_id.clone(),
        block_type: BlockType::Card,
        ..fvoci_model::Block::default()
    };
    common::insert_blocks(store, std::slice::from_mut(&mut card), &user_id).await;
    store
        .delete_block(&card.id, &user_id)
        .await
        .expect("a card goes to the trash");
    let gone = common::create_test_boards(store, &board.workspace_id, &user_id, 1).await;
    store
        .delete_container(&gone[0].id, &user_id)
        .await
        .expect("a board goes to the trash");
    common::sign_in_under_way(store, username).await;
    a_link_for_a_guest(store, &board, &user_id).await;
    (card.id, user_id)
}

/// A link that invites a guest to the board, made by the user.
async fn a_link_for_a_guest(store: &Store, board: &Container, user_id: &str) {
    store
        .create_invitation(&fvoci_store::NewLink {
            workspace_id: &board.workspace_id,
            guest_of: Some((&board.id, fvoci_model::ContainerRole::Viewer)),
            token_hash: &format!("link of {user_id}"),
            created_by: user_id,
            now: 0,
            expires_at: i64::MAX,
            max_uses: Some(1),
            mailed: None,
        })
        .await
        .expect("a link is stored");
}

/// The document of a card's body that is edited together: an update logged, a version
/// kept. The body names a block, which is a row of what bodies name.
async fn a_body_edited_together(store: &Store, board_id: &str, user_id: &str) {
    let mut edited = fvoci_model::Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        block_type: BlockType::Card,
        body: crate::body_links::body_naming("kept", &[&new_id()]),
        ..fvoci_model::Block::default()
    };
    common::insert_blocks(store, std::slice::from_mut(&mut edited), user_id).await;
    let body = edited.body.clone();
    crate::body_versions::type_body(store, &edited.id, body, user_id).await;
    crate::body_versions::keep(store, &edited.id).await;
}

/// What two organizations store stays apart in every table of theirs, whatever the
/// statement asks for: counted with no condition, each handle reaches its own rows, one
/// that names no organization reaches none, and the system's reaches all.
#[tokio::test]
async fn what_two_organizations_store_stays_apart_in_every_table() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let (other_org, _) = common::organization(&unset, "the other").await;
    let ours = &test.store;
    let theirs = test.for_org(&other_org).expect("an identifier");
    let system_org = test.for_org(SYSTEM_ORG_ID).expect("an identifier");

    something_in_every_table(ours, "ours").await;
    // Twice as much of everything for them, so that no count is the same by chance.
    something_in_every_table(&theirs, "theirs").await;
    something_in_every_table(&theirs, "theirs.too").await;

    let walled: Vec<String> = tables(&test.database_connection())
        .await
        .into_iter()
        .filter(|table| !GLOBAL_TABLES.contains(&table.as_str()))
        .collect();
    for table in &walled {
        let count = async |store: &Store| store.count_rows(table).await.expect("counted");
        let (our_rows, their_rows) = (count(ours).await, count(&theirs).await);
        assert!(our_rows > 0, "{table}: the test stores a row of ours");
        // An organization, its provider of sign-ins, the one it moved away from and
        // its pending settings are one of each.
        let one_of_each = [
            "organizations",
            "oidc_providers",
            "oidc_former_providers",
            "oidc_pending_providers",
        ];
        if one_of_each.contains(&table.as_str()) {
            assert_eq!((our_rows, their_rows), (1, 1), "each reaches its own alone");
        } else {
            assert!(
                their_rows > our_rows,
                "{table}: theirs are more ({their_rows}) than ours ({our_rows})"
            );
        }
        assert_eq!(count(&unset).await, 0, "{table}: no organization named");
        assert_eq!(
            count(&test.system()).await,
            our_rows + their_rows + count(&system_org).await,
            "{table}: the system's handle reaches all, and nothing is nobody's"
        );
    }

    test.teardown().await;
}

/// The trash of one organization is not purged by another: what is not there for a
/// handle is not gone. The server purges as the system, which reaches all of it.
#[tokio::test]
async fn one_organization_does_not_purge_the_trash_of_another() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let (other_org, _) = common::organization(&unset, "the other").await;
    let theirs = test.for_org(&other_org).expect("an identifier");
    let (our_card, _) = something_in_every_table(&test, "ours").await;
    let (their_card, _) = something_in_every_table(&theirs, "theirs").await;

    // Everything in the trash is old enough to go.
    let whenever = i64::MAX;
    let purged = unset.purge_trash(whenever).await.expect("purged");
    assert_eq!(
        purged.records, 0,
        "a handle of no organization purges nothing"
    );
    let purged = test.purge_trash(whenever).await.expect("purged");
    assert!(purged.records > 0, "ours is purged: {purged:?}");
    let gone = test
        .get_deleted_block(&our_card)
        .await
        .expect_err("our card is gone for good");
    assert!(gone.is_not_found(), "got {gone:?}");
    theirs
        .get_deleted_block(&their_card)
        .await
        .expect("their card is still in their trash");
    assert!(
        theirs.count_rows("file_refs").await.expect("counted") > 0,
        "and what their blocks show is still put down"
    );
    assert!(
        theirs.count_rows("body_links").await.expect("counted") > 0,
        "and what their bodies name"
    );

    let purged = test.system().purge_trash(whenever).await.expect("purged");
    assert!(purged.records > 0, "the system purges theirs: {purged:?}");
    let gone = theirs
        .get_deleted_block(&their_card)
        .await
        .expect_err("their card is gone for good");
    assert!(gone.is_not_found(), "got {gone:?}");

    test.teardown().await;
}

/// A workspace that is not in use is nobody's to work in, a member's neither: the
/// system's workspace, a workspace in the trash, and the workspaces of an organization
/// that was deleted are not found and not listed.
#[tokio::test]
async fn a_member_does_not_find_a_workspace_that_is_not_in_use() {
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let user_id = common::user(&test, "member").await;
    let second_workspace = common::workspace(&test).await;
    test.add_workspace_member(&second_workspace, &user_id, WorkspaceRole::Member)
        .await
        .expect("the user is let into the second workspace");
    test.for_org(SYSTEM_ORG_ID)
        .expect("an identifier")
        .add_workspace_member(SYSTEM_WORKSPACE_ID, &user_id, WorkspaceRole::Member)
        .await
        .expect("a membership of the system's workspace is stored");

    let found = async |workspace_id: &str| {
        unset
            .resolve_workspace(&user_id, workspace_id)
            .await
            .expect("the workspace is resolved")
            .is_some()
    };
    let listed = async || {
        unset
            .workspaces_of_user(&user_id, None)
            .await
            .expect("the workspaces are listed")
            .into_iter()
            .map(|workspace| workspace.id)
            .collect::<Vec<String>>()
    };
    assert!(!found(SYSTEM_WORKSPACE_ID).await, "the system's workspace");
    assert_eq!(
        listed().await,
        [test.workspace_id.clone(), second_workspace.clone()],
        "the two of the organization, and not the system's"
    );

    let system = test.system();
    let trashed = system
        .execute_unchecked(&format!(
            "UPDATE workspaces SET delete_at = 5 WHERE id = '{second_workspace}'"
        ))
        .await
        .expect("the workspace is put in the trash");
    assert_eq!(trashed, 1, "the second workspace");
    assert!(!found(&second_workspace).await, "a workspace in the trash");
    assert!(found(&test.workspace_id).await, "the other is still there");
    assert_eq!(
        listed().await,
        std::slice::from_ref(&test.workspace_id),
        "without the one in the trash"
    );

    let deleted = system
        .execute_unchecked(&format!(
            "UPDATE organizations SET delete_at = 5 WHERE id = '{}'",
            test.org_id
        ))
        .await
        .expect("the organization is deleted");
    assert_eq!(deleted, 1, "the organization");
    assert!(
        !found(&test.workspace_id).await,
        "of a deleted organization"
    );
    assert_eq!(
        listed().await,
        Vec::<String>::new(),
        "nothing of it is listed"
    );

    test.teardown().await;
}

/// The schema makes the system organization and its workspace under the identifiers
/// the server knows, and nothing lists them among an organization's own.
#[tokio::test]
async fn the_schema_makes_the_system_organization_and_its_workspace() {
    let test = common::setup().await;
    assert!(
        is_id(SYSTEM_ORG_ID) && is_id(SYSTEM_WORKSPACE_ID),
        "identifiers"
    );
    let system_org = test.for_org(SYSTEM_ORG_ID).expect("an identifier");
    let workspaces = system_org.get_workspaces().await.expect("the workspaces");
    let ids: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(ids, [SYSTEM_WORKSPACE_ID], "the system organization's one");

    let ours = test.get_workspaces().await.expect("the workspaces");
    assert!(
        ours.iter()
            .all(|workspace| workspace.id != SYSTEM_WORKSPACE_ID),
        "it is not ours: {ours:?}"
    );
    let missing = test
        .get_workspace(SYSTEM_WORKSPACE_ID)
        .await
        .expect_err("nor is it found by its identifier");
    assert!(missing.is_not_found(), "got {missing:?}");
    // It is not the organization the server works in, old as it is.
    let found = test
        .scoped(Tenant::Unset)
        .instance_organization()
        .await
        .expect("the organization")
        .expect("an existing organization");
    assert_eq!(found, test.org_id);

    test.teardown().await;
}

/// An SQLite database holds one organization besides the system's: a second is refused
/// by the database itself.
#[tokio::test]
async fn an_sqlite_database_holds_one_organization() {
    if common::db_type() != DbType::Sqlite {
        return;
    }
    let test = common::setup().await;
    let second = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let refused = test
        .scoped(Tenant::Unset)
        .create_organization("a second", &second)
        .await
        .expect_err("a second organization is refused");
    assert!(
        matches!(&refused, StoreError::Database(error)
            if error.to_string().contains("holds one organization")),
        "got {refused:?}"
    );
    let missing = test
        .get_workspace(&second.id)
        .await
        .expect_err("nothing of it is stored");
    assert!(missing.is_not_found(), "got {missing:?}");
    let refused = common::try_execute_raw(
        &test.database_connection(),
        "UPDATE organizations SET is_system = NOT is_system",
    )
    .await
    .expect_err("an organization stays what it is");
    assert!(refused.to_string().contains("the system"), "{refused}");

    test.teardown().await;
}

/// The Rust files below a directory.
fn rust_sources(directory: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(directory).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// The files that may ask for the system's reach, by which a handle reaches the rows of
/// every organization: the store itself, the directory, the records of the server's
/// operator in each organization of a person, the changes of the data after a
/// migration, and the one function of the server that its housekeeping works through;
/// and the revisions of the cached checks, which name it to tell what its writes move.
/// Asked by the handle or by the setting of a transaction alike. Anywhere else, a
/// request would be let past the wall it is there to be held by. (The system
/// organization, which holds the built-in templates, is an
/// organization like another to the wall: `Tenant::org(SYSTEM_ORG_ID)` is not this.)
#[test]
fn the_systems_reach_is_asked_for_in_few_places() {
    const ALLOWED: [&str; 8] = [
        "store/src/lib.rs",
        "store/src/directory.rs",
        "store/src/directory/sign_in.rs",
        "store/src/operator_records.rs",
        "store/src/signups.rs",
        "store/src/data_steps.rs",
        "store/src/visibility.rs",
        "server/src/app/housekeeping.rs",
    ];
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    for source in ["store/src", "server/src"] {
        rust_sources(&crates.join(source), &mut files);
    }
    assert!(files.len() > 50, "the sources are found: {}", files.len());
    let mut asking = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(crates)
            .expect("under the crates directory")
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&file).expect("a source file");
        // A file's unit tests, in its `mod tests` at the end, are compiled into no
        // server; an item that is `#[cfg(test)]` alone among the rest is still read.
        let asks = text
            .lines()
            .take_while(|line| !line.trim_start().starts_with("mod tests"))
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| {
                // By the variant, also where it is imported by its bare name, and by
                // the method, however it is called.
                line.contains("Tenant::System")
                    || (line.contains("Tenant::{") && line.contains("System"))
                    || line.contains(".system()")
                    || line.contains("::system(")
                    || line.contains("fvoci.system")
            });
        // The test support is compiled into no server.
        if asks && name != "store/src/testing.rs" {
            asking.push(name);
        }
    }
    asking.sort();
    let mut allowed = ALLOWED.map(str::to_owned).to_vec();
    allowed.sort();
    assert_eq!(asking, allowed, "the files that ask for the system's reach");
}

/// The files of the server that say whose rows a handle reaches: the way into a
/// workspace, the templates (as the system organization), the start (the one
/// organization of the routes that name no workspace), the housekeeping, the
/// application's own `scoped`, and the documents of bodies edited together, which the
/// server writes to their cards as the organization of each when it starts and stops. All of them are the logic's; no handler names one. A handler that named an organization by what a request
/// says, past the extractor, would open the wall from the inside.
#[test]
fn the_server_names_an_organization_for_a_handle_in_few_places() {
    const ALLOWED: [&str; 5] = [
        "app/housekeeping.rs",
        "app/mod.rs",
        "app/templates.rs",
        "app/workspaces.rs",
        "collab/mod.rs",
    ];
    let server = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory")
        .join("server/src");
    let mut files = Vec::new();
    rust_sources(&server, &mut files);
    let mut naming = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source file");
        let names = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .any(|line| {
                [".scoped(", "for_org(", "Tenant::org(", "Tenant::Org("]
                    .iter()
                    .any(|naming| line.contains(naming))
            });
        if names {
            naming.push(
                file.strip_prefix(&server)
                    .expect("under the server's sources")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    naming.sort();
    assert_eq!(
        naming, ALLOWED,
        "the files that name an organization for a handle"
    );
}

/// What a server that knew nothing of organizations left in every table: three users
/// (an administrator, a member, and one who left), two workspaces, boards of both and a
/// built-in template, and something in every other table, some of it hanging on the
/// template (a membership, a file, sharing, a subscription, a filing: what an older
/// server let a user have on one), some on a board in the trash, some on nothing that
/// is still there. The words that begin with a colon stand for identifiers.
pub(crate) const BEFORE_ORGANIZATIONS: &str = r#"
INSERT INTO users (id, username, email, password_hash, create_at, update_at, delete_at,
                   is_admin, display_name) VALUES
    (':admin', 'admin', 'admin@example.com', '', 10, 10, 0, true, ''),
    (':member', 'member', 'member@example.com', '', 20, 20, 0, false, ''),
    (':left', 'left', 'left@example.com', '', 30, 30, 40, false, '');
UPDATE instance SET bootstrapped = true;
INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at) VALUES
    (':younger_workspace', 'token-2', '{}', NULL, 200, 200),
    (':older_workspace', 'token-1', '{"kept": 1}', ':admin', 100, 100);
INSERT INTO boards (id, team_id, created_by, modified_by, type, title, description, icon,
                    show_description, is_template, is_builtin, template_version, properties,
                    card_properties, create_at, update_at) VALUES
    (':board', ':older_workspace', ':admin', ':admin', 'O', 'a board', '', '', false, false, false,
     0, '{}', '[]', 1, 1),
    (':template', ':older_workspace', NULL, NULL, 'O', 'a template', '', '', false, true, true, 7,
     '{}', '[]', 1, 1),
    (':other_board', ':younger_workspace', ':member', ':member', 'P', 'another', '', '', false,
     false, false, 0, '{}', '[]', 2, 2);
INSERT INTO board_members (board_id, user_id, scheme_admin, scheme_editor, scheme_commenter,
                           scheme_viewer) VALUES
    (':board', ':admin', true, true, false, false),
    (':other_board', ':member', true, true, false, false),
    (':template', ':member', false, true, false, false);
INSERT INTO blocks (id, board_id, parent_id, created_by, modified_by, "schema", type, title,
                    fields, create_at, update_at) VALUES
    (':card', ':board', NULL, ':admin', ':admin', 1, 'card', 'a card', '{}', 1, 1),
    (':template_card', ':template', NULL, NULL, NULL, 1, 'card', 'of the template', '{}', 1, 1);
INSERT INTO blocks (id, board_id, parent_id, created_by, modified_by, "schema", type, title,
                    fields, create_at, update_at) VALUES
    (':comment', ':board', ':card', ':member', ':member', 1, 'comment', '', '{}', 2, 2);
INSERT INTO deleted_boards (id, team_id, deleted_at, deleted_by, deletion_id, board, members,
                            files) VALUES
    (':deleted_board', ':older_workspace', 5, ':admin', ':deletion', '{}', '[]', '[]');
INSERT INTO deleted_blocks (id, board_id, parent_id, deleted_at, deleted_by, deletion_id,
                            block) VALUES
    (':deleted_card', ':board', NULL, 5, ':admin', ':other_deletion', '{"type": "card"}'),
    (':card_of_deleted_board', ':deleted_board', NULL, 5, ':admin', ':deletion',
     '{"type": "card"}');
INSERT INTO categories (id, name, user_id, team_id, create_at, update_at, collapsed, type,
                        sort_order) VALUES
    (':category', 'mine', ':admin', ':older_workspace', 1, 1, false, 'custom', 0),
    (':other_category', 'theirs', ':member', ':older_workspace', 1, 1, false, 'custom', 0);
INSERT INTO category_boards (id, user_id, category_id, board_id, create_at, update_at,
                             sort_order, hidden) VALUES
    (':filed', ':admin', ':category', ':board', 1, 1, 0, false),
    (':filed_template', ':member', ':other_category', ':template', 1, 1, 0, false);
INSERT INTO file_info (id, board_id, name, original_name, extension, size, path, create_at)
VALUES (':file', ':board', '7shown.png', 'shown.png', '.png', 1, 'somewhere', 1),
       (':template_file', ':template', '7template.png', 'template.png', '.png', 1, 'elsewhere', 1);
INSERT INTO file_refs (block_id, board_id, name) VALUES
    (':card', ':board', '7shown.png'),
    (':card_of_deleted_board', ':deleted_board', '7gone.png');
INSERT INTO sharing (board_id, enabled, token, modified_by, update_at)
VALUES (':board', true, 'the-read-token', ':admin', 1),
       (':template', true, 'the-template-token', ':admin', 1);
INSERT INTO subscriptions (block_type, block_id, subscriber_type, subscriber_id, notified_at,
                           create_at) VALUES
    ('card', ':card', 'user', ':member', 0, 1),
    ('card', ':template_card', 'user', ':admin', 0, 1);
INSERT INTO uploads (id, user_id, board_id, name, size, content_type, part_size, create_at,
                     expires_at, closed_at, multipart_id)
VALUES ('an-upload', ':admin', ':board', 'upload.bin', 1, '', 1, 1, 9000000000000, 0, '');
INSERT INTO upload_parts (upload_id, number, size, part_id) VALUES ('an-upload', 1, 1, '');
INSERT INTO comment_versions (block_id, version, body, files, written_by, written_at) VALUES
    (':comment', 1, NULL, '[]', ':admin', 1),
    (':comment_that_is_gone', 1, NULL, '[]', ':admin', 1);
"#;

/// The identifiers of [`BEFORE_ORGANIZATIONS`], by their names there.
pub(crate) struct Before(BTreeMap<&'static str, String>);

impl Before {
    const NAMES: [&'static str; 23] = [
        "admin",
        "member",
        "left",
        "younger_workspace",
        "older_workspace",
        "board",
        "template",
        "other_board",
        "card",
        "template_card",
        "comment",
        "deleted_board",
        "deletion",
        "other_deletion",
        "deleted_card",
        "card_of_deleted_board",
        "category",
        "other_category",
        "filed",
        "filed_template",
        "file",
        "template_file",
        "comment_that_is_gone",
    ];

    pub(crate) fn new() -> Self {
        Self(Self::NAMES.iter().map(|name| (*name, new_id())).collect())
    }

    pub(crate) fn id(&self, name: &str) -> &str {
        &self.0[name]
    }

    /// The statements, with the identifiers in the places of their names: the longest
    /// name first, as some names begin with another.
    pub(crate) fn statements(&self) -> String {
        let mut names: Vec<&&str> = self.0.keys().collect();
        names.sort_by_key(|name| std::cmp::Reverse(name.len()));
        let mut statements = BEFORE_ORGANIZATIONS.to_owned();
        for name in names {
            statements = statements.replace(&format!(":{name}"), &self.0[*name]);
        }
        assert!(!statements.contains("':"), "a name without an identifier");
        statements
    }
}

/// The two workspaces of the old database are the organization's, as they were, every user is in
/// both, and who registers now gets into the oldest.
async fn assert_the_workspaces_are_its_workspaces(
    unset: &Store,
    ours: &Store,
    before: &Before,
    org_id: &str,
) {
    // Both workspaces are its own, as they were, the oldest first.
    let workspaces = ours.get_workspaces().await.expect("the workspaces");
    let ids: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [before.id("older_workspace"), before.id("younger_workspace")],
        "the oldest first"
    );
    assert_eq!(
        workspaces[0].modified_by,
        before.id("admin"),
        "{:?}",
        workspaces[0]
    );
    assert_eq!(workspaces[0].settings["kept"], 1, "{:?}", workspaces[0]);
    assert_eq!(workspaces[0].title, "", "no name yet");

    // Every user is in both, the one who left too; the administrator administers.
    for (user, org_role, role) in [
        ("admin", OrgRole::Admin, WorkspaceRole::Admin),
        ("member", OrgRole::Member, WorkspaceRole::Member),
        ("left", OrgRole::Member, WorkspaceRole::Member),
    ] {
        let listed = unset
            .workspaces_of_user(before.id(user), None)
            .await
            .expect("the workspaces are listed");
        let listed: Vec<(&str, &str, WorkspaceRole)> = listed
            .iter()
            .map(|workspace| {
                (
                    workspace.id.as_str(),
                    workspace.org_id.as_str(),
                    workspace.role,
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (before.id("older_workspace"), org_id, role),
                (before.id("younger_workspace"), org_id, role)
            ],
            "{user}"
        );
        let access = unset
            .resolve_workspace(before.id(user), before.id("younger_workspace"))
            .await
            .expect("the workspace is resolved")
            .expect("a member");
        assert_eq!(access.org_role, Some(org_role), "{user}");
    }
    // The default workspace is the oldest workspace: who registers now gets into it.
    let mut newcomer = User {
        id: new_id(),
        username: "newcomer".to_owned(),
        email: "newcomer@example.com".to_owned(),
        ..User::default()
    };
    ours.create_user(&mut newcomer)
        .await
        .expect("a user registers");
    assert!(!newcomer.is_admin, "the instance had its first user");
    let listed = unset
        .workspaces_of_user(&newcomer.id, None)
        .await
        .expect("the workspaces are listed");
    let ids: Vec<&str> = listed
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(ids, [before.id("older_workspace")], "the default workspace");
}

/// The built-in template is the system organization's and is still read by ours; the
/// boards of the workspaces are where they were.
async fn assert_the_template_is_the_systems(
    unset: &Store,
    ours: &Store,
    before: &Before,
    org_id: &str,
) {
    // The built-in template is the system organization's, in its workspace, and is
    // still read by ours; the boards of the workspaces are where they were.
    let system_org = unset.for_org(SYSTEM_ORG_ID).expect("an identifier");
    let templates = system_org
        .get_builtin_templates()
        .await
        .expect("the built-in templates");
    let found: Vec<(&str, &str)> = templates
        .iter()
        .map(|board| (board.id.as_str(), board.workspace_id.as_str()))
        .collect();
    assert_eq!(
        found,
        [(before.id("template"), SYSTEM_WORKSPACE_ID)],
        "in the system's workspace"
    );
    let read = ours
        .get_container(before.id("template"))
        .await
        .expect("ours reads the template");
    assert!(read.is_builtin, "{read:?}");
    ours.get_block(before.id("template_card"))
        .await
        .expect("and its card");
    // What a user had on the template under an older server is the system's with it:
    // behind PostgreSQL's wall it is out of the user's reach, and the template is
    // nobody's to change. SQLite has no wall, and leaves it where it was.
    let membership = ours
        .get_member_for_container(before.id("template"), before.id("member"))
        .await;
    match unset.db_type() {
        DbType::Postgres => assert!(
            membership.is_err_and(|error| error.is_not_found()),
            "the membership of the template is out of reach"
        ),
        DbType::Sqlite => assert!(
            membership.is_ok(),
            "the membership of the template is where it was"
        ),
    }
    assert_eq!(
        unset
            .shared_container(before.id("template"), "the-template-token")
            .await
            .expect("the shared board is looked up"),
        None,
        "the template is shared with nobody"
    );
    assert_eq!(
        ours.get_container(before.id("board"))
            .await
            .expect("a board")
            .workspace_id,
        before.id("older_workspace"),
        "a board stays in its workspace"
    );
    assert_eq!(
        unset
            .shared_container(before.id("board"), "the-read-token")
            .await
            .expect("the shared board is looked up")
            .map(|shared| shared.org_id),
        Some(org_id.to_owned()),
        "whose a shared board is"
    );
}

/// On PostgreSQL every row that was stored is some organization's and behind the wall.
async fn assert_every_row_is_behind_the_wall(unset: &Store, ours: &Store) {
    let system_org = unset.for_org(SYSTEM_ORG_ID).expect("an identifier");
    // Every row is the organization's, but what hangs on the template, which
    // is the system's; ours reads the template's board and card besides.
    let expected = [
        ("organizations", 1, 1),
        ("workspaces", 2, 1),
        ("organization_members", 4, 0),
        ("workspace_members", 7, 0),
        ("containers", 2, 1),
        ("container_members", 2, 1),
        ("blocks", 2, 1),
        ("deleted_containers", 1, 0),
        ("deleted_blocks", 2, 0),
        ("categories", 2, 0),
        ("category_containers", 2, 0),
        ("file_info", 1, 1),
        ("file_refs", 2, 0),
        ("sharing", 1, 1),
        ("subscriptions", 1, 1),
        ("uploads", 1, 0),
        ("upload_parts", 1, 0),
        ("comment_versions", 2, 0),
    ];
    for (table, of_ours, of_the_system) in expected {
        let count = async |store: &Store| store.count_rows(table).await.expect("counted");
        let read_besides = if ["containers", "blocks"].contains(&table) {
            of_the_system
        } else {
            0
        };
        assert_eq!(count(ours).await, of_ours + read_besides, "{table}: ours");
        assert_eq!(
            count(&system_org).await,
            of_the_system,
            "{table}: the system's"
        );
        assert_eq!(
            count(&unset.system()).await,
            of_ours + of_the_system,
            "{table}: all"
        );
        assert_eq!(count(unset).await, 0, "{table}: no organization named");
    }
}

/// A database that a server left which knew nothing of organizations becomes one
/// organization, whose default workspace is the oldest workspace; every user is a member of
/// it and of each of its workspaces, an administrator of the instance of both as an
/// administrator; and the built-in template is the system organization's. On
/// PostgreSQL every row that was stored is some organization's and behind the wall.
#[tokio::test]
async fn a_database_from_before_organizations_becomes_one_organization() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let before = Before::new();
    common::execute_raw(&common::raw_connection(&database), &before.statements()).await;

    let unset = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("the store migrates the database");
    // The one organization: found, not made.
    let org_id = unset
        .instance_organization()
        .await
        .expect("the organization")
        .expect("an existing organization");
    assert!(is_id(&org_id), "{org_id}");
    assert_eq!(org_id.as_bytes()[14], b'7', "a version 7 UUID: {org_id}");
    assert!(
        b"89ab".contains(&org_id.as_bytes()[19]),
        "of the usual variant: {org_id}"
    );
    let ours = unset.for_org(&org_id).expect("an identifier");

    assert_the_workspaces_are_its_workspaces(&unset, &ours, &before, &org_id).await;
    assert_the_template_is_the_systems(&unset, &ours, &before, &org_id).await;
    match database.db_type() {
        DbType::Postgres => assert_every_row_is_behind_the_wall(&unset, &ours).await,
        DbType::Sqlite => {
            // The table of the workspaces was made again (0014): every row that refers to one
            // still finds it.
            unset.close().await;
            let path = database.sqlite_path().expect("an SQLite file");
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&path.display().to_string())
                .await
                .expect("the database file opens");
            let broken = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut conn)
                .await
                .expect("the keys are checked");
            conn.close().await.expect("the connection closes");
            assert!(broken.is_empty(), "{} rows refer to nothing", broken.len());
        }
    }

    unset.close().await;
    database.remove().await;
}

/// An upload session of `size` bytes that the user opens through `store`: an archive to
/// import into a workspace of the store's organization, which is what an upload names.
async fn open_upload(store: &Store, workspace_id: &str, user_id: &str, size: i64) {
    let upload = Upload {
        id: new_id(),
        user_id: user_id.to_owned(),
        purpose: fvoci_model::UploadPurpose::Import,
        workspace_id: workspace_id.to_owned(),
        name: "upload.bin".to_owned(),
        size,
        part_size: size,
        create_at: 1,
        expires_at: i64::MAX,
        ..Upload::default()
    };
    store
        .create_upload(&upload)
        .await
        .expect("an upload is opened");
}

/// The limits on open uploads are a user's and the server's, not an organization's:
/// what they count is the same through the handle of any organization, with the
/// sessions of all of them in it.
#[tokio::test]
async fn the_open_uploads_are_counted_over_every_organization() {
    let test = common::setup().await;
    let user_id = common::user(&test, "uploader").await;
    let other_user = common::user(&test, "another.uploader").await;
    open_upload(&test, &test.workspace_id, &user_id, 10).await;
    open_upload(&test, &test.workspace_id, &other_user, 100).await;
    let mut handles = vec![test.store.clone(), test.scoped(Tenant::Unset)];
    let mut expected = (1, 10, 110);
    if common::db_type() == DbType::Postgres {
        let (other_org, their_workspace) =
            common::organization(&test.scoped(Tenant::Unset), "the other").await;
        let theirs = test.for_org(&other_org).expect("an identifier");
        open_upload(&theirs, &their_workspace, &user_id, 1000).await;
        handles.push(theirs);
        expected = (2, 1010, 1110);
    }
    for store in &handles {
        let usage = store
            .upload_usage(&user_id, 5)
            .await
            .expect("the usage is read");
        assert_eq!(
            (usage.user_sessions, usage.user_bytes, usage.server_bytes),
            expected,
            "through {:?}",
            store.tenant()
        );
    }
    test.teardown().await;
}

/// The organization of a shared board is told to whoever holds its read token, and to
/// nobody else; and nothing is shared of the system organization, or of a workspace
/// that was deleted.
#[tokio::test]
async fn the_organization_of_a_shared_board_is_for_the_holder_of_its_token() {
    let test = common::setup().await;
    let unset = test.scoped(Tenant::Unset);
    let user_id = common::user(&test, "sharer").await;
    let workspace_id = common::workspace(&test).await;
    let shared = board(&test, &workspace_id, &user_id, "shared").await;
    let not_shared = board(&test, &workspace_id, &user_id, "not shared").await;
    test.set_sharing(&shared, SharingChange::On, "the-token", &user_id)
        .await
        .expect("the board is shared");
    test.set_sharing(&not_shared, SharingChange::On, "another-token", &user_id)
        .await
        .expect("the other board is shared");
    test.set_sharing(&not_shared, SharingChange::Off, "", &user_id)
        .await
        .expect("and shared no more");
    let system_org = test.for_org(SYSTEM_ORG_ID).expect("an identifier");
    let template = board(&system_org, SYSTEM_WORKSPACE_ID, &user_id, "a template").await;
    system_org
        .set_sharing(&template, SharingChange::On, "template-token", &user_id)
        .await
        .expect("the template is shared");

    let org_of = async |board_id: &str, token: &str| {
        unset
            .shared_container(board_id, token)
            .await
            .expect("the shared board is looked up")
            .map(|shared| shared.org_id)
    };
    assert_eq!(
        org_of(&shared, "the-token").await,
        Some(test.org_id.clone())
    );
    // A wiki is shared as a board is, and said to be one: its token reads its pages.
    let mut wiki = Container {
        id: new_id(),
        workspace_id: workspace_id.clone(),
        kind: ContainerKind::Wiki,
        title: "a wiki".to_owned(),
        ..Container::default()
    };
    test.insert_container(&mut wiki, &user_id)
        .await
        .expect("the wiki is stored");
    test.set_sharing(&wiki.id, SharingChange::On, "the-wiki-token", &user_id)
        .await
        .expect("the wiki is shared");
    assert_eq!(
        unset
            .shared_container(&wiki.id, "the-wiki-token")
            .await
            .expect("the shared wiki is looked up"),
        Some(SharedContainer {
            org_id: test.org_id.clone(),
            kind: ContainerKind::Wiki,
        })
    );
    assert_eq!(
        org_of(&shared, "the-wiki-token").await,
        None,
        "a wiki's token opens no board"
    );
    for (board_id, token, why) in [
        (shared.as_str(), "the-toke", "a part of the token"),
        (shared.as_str(), "", "no token"),
        (
            shared.as_str(),
            "another-token",
            "the token of another board",
        ),
        (
            not_shared.as_str(),
            "",
            "a board that is shared no more has no token",
        ),
        (not_shared.as_str(), "another-token", "nor its old one"),
        (
            template.as_str(),
            "template-token",
            "the system organization's",
        ),
        ("no identifier", "the-token", "no board"),
    ] {
        assert_eq!(org_of(board_id, token).await, None, "{why}");
    }

    // A workspace in the trash shares nothing.
    let trashed = test
        .system()
        .execute_unchecked(&format!(
            "UPDATE workspaces SET delete_at = 5 WHERE id = '{workspace_id}'"
        ))
        .await
        .expect("the workspace is put in the trash");
    assert_eq!(trashed, 1, "the workspace");
    assert_eq!(
        org_of(&shared, "the-token").await,
        None,
        "a deleted workspace"
    );

    test.teardown().await;
}

/// The store tells whether the wall holds the role it connects as: the role of the
/// tests it does, a superuser it does not, and SQLite has no role to ask about.
#[tokio::test]
async fn the_store_names_a_role_that_is_past_the_wall() {
    let test = common::setup().await;
    assert_eq!(
        test.role_past_the_wall().await.expect("the role is read"),
        None,
        "the role of the tests is held"
    );
    test.teardown().await;

    if common::db_type() != DbType::Postgres {
        return;
    }
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("store-tests");
    let database = fvoci_store::testing::TestDatabase::create_as_superuser(&directory).await;
    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("a store connects as the superuser");
    let role = store.role_past_the_wall().await.expect("the role is read");
    assert!(role.is_some(), "a superuser is past the wall");
    // Past it indeed: what a handle of no organization must not reach, it reaches.
    common::organization(&store, "anybody's").await;
    assert_eq!(
        store.count_rows("workspaces").await.expect("counted"),
        2,
        "the superuser reads the workspace and the system's through a handle of no organization"
    );
    store.close().await;
    database.remove().await;
}

/// The tests that connect to PostgreSQL as a superuser, whom no policy holds: those of
/// what the server and the store make of such a role, and nothing else. A test that
/// took that connection for convenience would show nothing of the wall.
#[test]
fn few_tests_connect_as_a_superuser() {
    const ALLOWED: [&str; 6] = [
        "server/tests/api/common.rs",
        "server/tests/api/organizations.rs",
        "server/tests/api/table_owner.rs",
        "store/src/testing.rs",
        "store/tests/store/table_owner.rs",
        "store/tests/store/tenancy.rs",
    ];
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crates directory");
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    let mut connecting: Vec<String> = files
        .iter()
        .filter(|file| {
            let text = std::fs::read_to_string(file).expect("a source file");
            [
                "create_as_superuser",
                "create_as_role_with_bypassrls",
                "create_owned_apart",
                "connecting_as_superuser",
                "SUPERUSER_CONN_STRING_ENV",
            ]
            .iter()
            .any(|name| text.contains(name))
        })
        .map(|file| {
            file.strip_prefix(crates)
                .expect("under the crates directory")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    connecting.sort();
    assert_eq!(connecting, ALLOWED, "the files that connect as a superuser");
}

/// The plain case of a database from before: one workspace and three users. It becomes one
/// organization with that workspace, and every user is a member of both.
#[tokio::test]
async fn a_database_with_one_workspace_becomes_one_organization_with_one_workspace() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let (workspace_id, admin, member, left) = (new_id(), new_id(), new_id(), new_id());
    common::execute_raw(
        &common::raw_connection(&database),
        &format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, \
             delete_at, is_admin, display_name) VALUES \
             ('{admin}', 'admin', 'admin@example.com', '', 10, 10, 0, true, ''), \
             ('{member}', 'member', 'member@example.com', '', 20, 20, 0, false, ''), \
             ('{left}', 'left', 'left@example.com', '', 30, 30, 40, false, ''); \
             UPDATE instance SET bootstrapped = true; \
             INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at) \
             VALUES ('{workspace_id}', 'the-token', '{{}}', NULL, 100, 100);"
        ),
    )
    .await;

    let unset = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("the store migrates the database");
    // One organization, and the system's.
    assert_eq!(
        unset
            .system()
            .count_rows("organizations")
            .await
            .expect("counted"),
        2,
        "the organization and the system's"
    );
    let org_id = unset
        .instance_organization()
        .await
        .expect("the organization")
        .expect("an existing organization");
    let ours = unset.for_org(&org_id).expect("an identifier");
    // One workspace: the one of the old database.
    let workspaces = ours.get_workspaces().await.expect("the workspaces");
    let ids: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(ids, [workspace_id.as_str()], "the one workspace");
    assert_eq!(
        unset
            .system()
            .count_rows("workspaces")
            .await
            .expect("counted"),
        2,
        "the workspace and the system's"
    );
    // Every user is a member of the organization and of the workspace.
    for (user, org_role, role) in [
        (&admin, OrgRole::Admin, WorkspaceRole::Admin),
        (&member, OrgRole::Member, WorkspaceRole::Member),
        (&left, OrgRole::Member, WorkspaceRole::Member),
    ] {
        let access = unset
            .resolve_workspace(user, &workspace_id)
            .await
            .expect("the workspace is resolved")
            .expect("a member of the workspace");
        assert_eq!(access.org_id, org_id, "of the one organization");
        assert_eq!(
            (access.org_role, access.workspace_role),
            (Some(org_role), role),
            "a member of both"
        );
        let listed = unset
            .workspaces_of_user(user, None)
            .await
            .expect("the workspaces are listed");
        assert_eq!(listed.len(), 1, "one workspace: {listed:?}");
    }
    assert_eq!(
        unset
            .system()
            .count_rows("organization_members")
            .await
            .expect("counted"),
        3,
        "the three users, and nobody else"
    );
    assert_eq!(
        unset
            .system()
            .count_rows("workspace_members")
            .await
            .expect("counted"),
        3,
        "the three users, in the one workspace"
    );

    unset.close().await;
    database.remove().await;
}

/// The foreign keys of the test's schema: the table, the table it refers to, and the
/// columns of the key in the table, in the key's order.
async fn foreign_keys(conn: &mut sqlx::PgConnection) -> Vec<(String, String, Vec<String>)> {
    sqlx::query_as(
        "SELECT c.conrelid::regclass::text, c.confrelid::regclass::text, \
         (SELECT array_agg(a.attname::text ORDER BY k.position) \
          FROM unnest(c.conkey) WITH ORDINALITY AS k (number, position) \
          JOIN pg_attribute AS a ON a.attrelid = c.conrelid AND a.attnum = k.number) \
         FROM pg_constraint AS c \
         WHERE c.contype = 'f' AND c.connamespace = current_schema()::regnamespace \
         ORDER BY 1, 2, 3",
    )
    .fetch_all(conn)
    .await
    .expect("the keys are in the catalog")
}

/// The wall looks at a row's own organization. That the row and what it hangs on are
/// one organization's is held by keys: wherever a table of an organization refers to
/// another by a key of one column, a key of that column and the organization refers to
/// the same row. A table that a later migration adds with the one and without the
/// other fails this test.
#[tokio::test]
async fn a_row_is_held_to_the_organization_of_what_it_hangs_on() {
    // A category takes a built-in template, which is the system organization's: a
    // trigger checks the board when the row is stored (organization_boundary_round2.rs).
    const NOT_HELD: [(&str, &str); 1] = [("category_containers", "container_id")];
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let mut conn = common::connect_past_the_store(&test.database_connection()).await;
    let keys = foreign_keys(&mut conn).await;
    conn.close().await.expect("the connection closes");
    test.teardown().await;

    let mut held = Vec::new();
    for (table, parent, columns) in &keys {
        let [column] = columns.as_slice() else {
            continue;
        };
        let of_organizations = |name: &String| ORG_TABLES.contains(&name.as_str());
        // A member of an organization hangs on the organization by its own org_id.
        if !of_organizations(table) || !of_organizations(parent) || parent == "organizations" {
            continue;
        }
        if NOT_HELD.contains(&(table.as_str(), column.as_str())) {
            continue;
        }
        // An organization is itself the organization of its default workspace.
        let organization = if table == "organizations" {
            "id"
        } else {
            "org_id"
        };
        let with_the_organization = vec![column.clone(), organization.to_owned()];
        assert!(
            keys.contains(&(table.clone(), parent.clone(), with_the_organization)),
            "{table}.{column} refers to {parent} without the organization: {keys:?}"
        );
        held.push(format!("{table}.{column}"));
    }
    assert_eq!(
        held,
        [
            "blocks.container_id",
            "categories.workspace_id",
            "category_containers.category_id",
            "container_members.container_id",
            "containers.workspace_id",
            "file_info.container_id",
            "organizations.default_workspace_id",
            "sharing.container_id",
            "subscriptions.block_id",
            "upload_parts.upload_id",
            "workspace_members.workspace_id",
        ],
        "the keys that are held to the organization"
    );
}

/// The lookups that reach across every organization do not take the keys on trust:
/// with the keys gone, a member's row that one organization stored under another's
/// workspace lets nobody into it, and a sharing that one stored under another's board
/// opens nothing.
#[tokio::test]
async fn a_row_of_one_organization_under_anothers_parent_counts_for_nothing() {
    if common::db_type() != DbType::Postgres {
        return;
    }
    let test = common::setup().await;
    let user_id = common::user(&test, "anybody").await;
    let (other_org, their_workspace) = common::organization(&test, "the other").await;
    let theirs = test.for_org(&other_org).expect("an identifier");
    let their_board = board(&theirs, &their_workspace, &user_id, "theirs").await;

    // Each row is the first organization's: stored by its handle, with its org_id.
    let forgeries = [
        format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, create_at) \
             VALUES ('{their_workspace}', '{user_id}', 'admin', 0)"
        ),
        format!(
            "INSERT INTO sharing (container_id, enabled, token, update_at) \
             VALUES ('{their_board}', true, 'a-forged-token', 0)"
        ),
    ];
    for forged in &forgeries {
        let refused = test
            .execute_unchecked(forged)
            .await
            .expect_err("the key holds the row to its parent's organization");
        assert!(refused.to_string().contains("foreign key"), "{refused}");
    }

    let mut conn = common::connect_past_the_store(&test.database_connection()).await;
    sqlx::raw_sql(
        "ALTER TABLE workspace_members DROP CONSTRAINT workspace_members_workspace_org_fkey; \
         ALTER TABLE sharing DROP CONSTRAINT sharing_container_org_fkey",
    )
    .execute(&mut conn)
    .await
    .expect("the keys are dropped in the test's own schema");
    conn.close().await.expect("the connection closes");

    for forged in &forgeries {
        assert_eq!(
            test.execute_unchecked(forged)
                .await
                .expect("without the key the row is stored"),
            1
        );
    }
    assert_eq!(
        test.resolve_workspace(&user_id, &their_workspace)
            .await
            .expect("the membership is looked up"),
        None,
        "the row lets nobody into their workspace"
    );
    let listed = test
        .workspaces_of_user(&user_id, None)
        .await
        .expect("the workspaces are listed");
    assert!(
        listed
            .iter()
            .all(|workspace| workspace.id != their_workspace),
        "and their workspace is not among the user's: {listed:?}"
    );
    assert_eq!(
        test.shared_container(&their_board, "a-forged-token")
            .await
            .expect("the sharing is looked up"),
        None,
        "the sharing opens nothing"
    );
    test.teardown().await;
}

/// An open board is open to who works in its workspace: a member of it who is no
/// guest. To a guest of the workspace and to somebody who is not in it, the board is
/// found neither among the boards of its workspace nor by a search, until they are on it.
#[tokio::test]
async fn an_open_board_is_found_by_who_works_in_its_workspace() {
    use fvoci_model::ContainerSearchField::Title;
    use fvoci_store::PublicContainers;

    let test = common::setup().await;
    let (member, guest, outsider) = (
        common::user(&test, "member").await,
        common::user(&test, "guest").await,
        common::user(&test, "outsider").await,
    );
    let workspace_id = common::workspace_of(&test, &[&member]).await;
    test.add_workspace_member(&workspace_id, &guest, WorkspaceRole::Guest)
        .await
        .expect("the guest is let into the workspace");
    let board_id = board(&test, &workspace_id, &member, "open to the workspace").await;

    let found_by = async |user_id: &str| {
        let (open, boards) = (
            PublicContainers::Include,
            fvoci_model::ContainerKinds::Boards,
        );
        let of_the_workspace = test
            .get_containers_for_user_and_workspace(user_id, &workspace_id, open, boards)
            .await
            .expect("the boards of the workspace are read");
        let searched = test
            .search_containers_for_user("open", Title, user_id, open, boards)
            .await
            .expect("the boards are searched");
        (of_the_workspace.len(), searched.len())
    };
    assert_eq!(
        found_by(&member).await,
        (1, 1),
        "who works in the workspace"
    );
    assert_eq!(found_by(&guest).await, (0, 0), "a guest of the workspace");
    assert_eq!(found_by(&outsider).await, (0, 0), "somebody not in it");

    // On the board, a guest finds it as any member of a board does.
    test.save_member(&ContainerMember {
        container_id: board_id,
        user_id: guest.clone(),
        scheme_viewer: true,
        ..ContainerMember::default()
    })
    .await
    .expect("the guest is put on the board");
    assert_eq!(
        found_by(&guest).await,
        (1, 1),
        "a guest who is on the board"
    );
    test.teardown().await;
}

/// Real rows in all three group tables, so every organization wall is exercised.
async fn a_team_on_board(store: &Store, board: &Container, user_id: &str) {
    let group = store
        .create_team(&board.workspace_id, user_id, user_id)
        .await
        .expect("a team is stored");
    store
        .add_team_member(&board.workspace_id, &group, user_id, user_id)
        .await
        .expect("a team member is stored");
    store.execute_unchecked(&format!(
        "INSERT INTO container_teams (container_id, team_id, workspace_id, role) VALUES ('{}', '{group}', '{}', 'editor')",
        board.id, board.workspace_id
    )).await.expect("the team has a board role");
}

/// Whether an organization is the instance's own is asked in the transaction of what
/// depends on it, past the wall on PostgreSQL for that one statement: the oldest
/// organization is, a later one is not, and afterwards the transaction reaches its own
/// organization alone again.
#[tokio::test]
async fn the_instances_organization_is_asked_behind_a_wall_closed_again() {
    let test = common::setup().await;
    let own = test
        .instances_organization_and_reach()
        .await
        .expect("asked");
    // The system's organization, also on SQLite, which holds one organization besides.
    let systems = test
        .for_org(SYSTEM_ORG_ID)
        .expect("organization")
        .instances_organization_and_reach()
        .await
        .expect("asked");
    let later = if common::db_type() == DbType::Postgres {
        let (org, _) = common::organization(&test.system(), "Later").await;
        let later = test.for_org(&org).expect("organization");
        Some(
            later
                .instances_organization_and_reach()
                .await
                .expect("asked"),
        )
    } else {
        None
    };
    test.teardown().await;

    assert!(own.0, "the oldest organization is the instance's");
    assert!(!systems.0, "the system's organization is not");
    if let Some(later) = later {
        assert_eq!(
            later,
            (false, 1),
            "a later one is not, and sees itself alone"
        );
        assert_eq!(own.1, 1, "nor does the instance's see the others");
    }
}
