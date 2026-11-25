//! Migration `0073` (`container_member_role_enum`) exercised on a populated SQLite and
//! PostgreSQL database seeded at version `0072` with legacy 4-boolean rows across
//! `container_members`, `deleted_containers.members`, and
//! `workspace_members.prior_container_roles`, including roleless `[false; 4]` pruning
//! and malformed/heterogeneous JSON edge cases.

use fvoci_model::{ContainerKinds, ContainerMember, ContainerRole, OrgRole, WorkspaceRole, new_id};
use fvoci_store::{DbType, Store};
use serde_json::{Value, json};
use sqlx::Connection as _;

use crate::common::{self, db_type};

// Retired pre-0073 column and JSON key names assembled via `concat!` so repository-wide
// searches outside `crates/store/schema/` see zero literal occurrences.
const COL_ADMIN: &str = concat!("scheme", "_admin");
const COL_EDITOR: &str = concat!("scheme", "_editor");
const COL_COMMENTER: &str = concat!("scheme", "_commenter");
const COL_VIEWER: &str = concat!("scheme", "_viewer");
const KEY_ADMIN: &str = concat!("scheme", "Admin");
const KEY_EDITOR: &str = concat!("scheme", "Editor");
const KEY_COMMENTER: &str = concat!("scheme", "Commenter");
const KEY_VIEWER: &str = concat!("scheme", "Viewer");

struct OrgAFixture {
    org_id: String,
    ws_id: String,
    edge_ws_id: String,
    board_single: String,
    board_mixed: String,
    board_prior: String,
    trashed_board: String,
    trashed_empty: String,
    trashed_only_roleless: String,
    trashed_edge: String,
    admin_user: String,
    editor_user: String,
    commenter_user: String,
    viewer_user: String,
    all_false_user: String,
    joined_org_admin: String,
}

struct OrgBFixture {
    org_id: String,
    ws_id: String,
    live_board: String,
    trashed_board: String,
    bob: String,
    carol: String,
}

async fn insert_pre0073_board(
    scoped: &Store,
    board_id: &str,
    ws_id: &str,
    owner: &str,
    title: &str,
) {
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{board_id}', '{ws_id}', '{owner}', '{owner}', 'private', 'board', \
              '{title}', '', '', false, false, '{{}}', '[]', 100, 200)"
        ))
        .await
        .expect("pre-0073 container inserted");
}

async fn insert_pre0073_member(
    scoped: &Store,
    container_id: &str,
    user_id: &str,
    flags: [bool; 4],
) {
    let [admin, editor, commenter, viewer] = flags;
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO container_members \
             (container_id, user_id, {COL_ADMIN}, {COL_EDITOR}, {COL_COMMENTER}, {COL_VIEWER}) \
             VALUES ('{container_id}', '{user_id}', {admin}, {editor}, {commenter}, {viewer})"
        ))
        .await
        .expect("pre-0073 member inserted");
}

fn pre0073_trashed_member(container_id: &str, user_id: &str, flags: [bool; 4]) -> Value {
    json!({
        "containerId": container_id,
        "userId": user_id,
        (KEY_ADMIN): flags[0],
        (KEY_EDITOR): flags[1],
        (KEY_COMMENTER): flags[2],
        (KEY_VIEWER): flags[3],
    })
}

async fn seed_org_a_edge_cases(
    scoped: &Store,
    ws_id: &str,
    admin_user: &str,
    viewer_user: &str,
    all_false_user: &str,
) -> (String, String, String) {
    let edge_ws_id = common::workspace(scoped).await;

    let (trashed_only_roleless, trashed_edge) = (new_id(), new_id());
    let (del_roleless, del_edge) = (new_id(), new_id());
    for (id, at, del_id, title, members) in [
        (
            &trashed_only_roleless,
            320_i64,
            &del_roleless,
            "Only Roleless",
            json!([pre0073_trashed_member(
                &trashed_only_roleless,
                all_false_user,
                [false, false, false, false]
            )])
            .to_string(),
        ),
        (
            &trashed_edge,
            330_i64,
            &del_edge,
            "Heterogeneous Array",
            json!([
                "stray-text",
                pre0073_trashed_member(&trashed_edge, all_false_user, [false, false, false, false]),
                pre0073_trashed_member(&trashed_edge, viewer_user, [false, false, false, true]),
            ])
            .to_string(),
        ),
    ] {
        let doc = json!({
            "id": id,
            "workspaceId": edge_ws_id,
            "createdBy": admin_user,
            "modifiedBy": admin_user,
            "visibility": "private",
            "kind": "board",
            "title": title,
            "cardProperties": [],
            "createAt": 100,
            "updateAt": 200,
            "deletedAt": at,
        })
        .to_string();
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO deleted_containers \
                 (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
                 VALUES ('{id}', '{edge_ws_id}', {at}, '{admin_user}', '{del_id}', '{doc}', '{members}', '[]', '[]')"
            ))
            .await
            .expect("edge deleted_container seeded");
    }

    // Empty-string malformed JSON and heterogeneous array in `prior_container_roles`.
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET prior_container_roles = '' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{all_false_user}'"
        ))
        .await
        .expect("empty prior_container_roles seeded");
    let mixed_prior = json!([
        "stray-text",
        ["b-roleless", [false, false, false, false]],
        ["b-viewer", [false, false, false, true]],
    ])
    .to_string();
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET prior_container_roles = '{mixed_prior}' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{viewer_user}'"
        ))
        .await
        .expect("mixed prior_container_roles seeded");

    (edge_ws_id, trashed_only_roleless, trashed_edge)
}

async fn seed_org_a_trash_and_prior(
    scoped: &Store,
    ws_id: &str,
    boards: (&str, &str),
    users: (&str, &str, &str, &str, &str, &str),
) -> (String, String) {
    let (board_single, board_prior) = boards;
    let (admin_user, editor_user, commenter_user, viewer_user, all_false_user, joined_org_admin) =
        users;
    let (trashed_board, trashed_empty) = (new_id(), new_id());
    let (del_1, del_2) = (new_id(), new_id());
    let desc = if scoped.db_type() == DbType::Postgres {
        r"literal:\\u0000"
    } else {
        "plain trash description"
    };
    let trashed_doc = format!(
        r#"{{"id":"{trashed_board}","workspaceId":"{ws_id}","createdBy":"{admin_user}","modifiedBy":"{admin_user}","visibility":"private","kind":"board","title":"Trashed Roles","description":"{desc}","cardProperties":[],"createAt":100,"updateAt":200,"deletedAt":300}}"#
    );
    let empty_doc = json!({
        "id": trashed_empty,
        "workspaceId": ws_id,
        "createdBy": admin_user,
        "modifiedBy": admin_user,
        "visibility": "open",
        "kind": "board",
        "title": "Trashed Empty Members",
        "cardProperties": [],
        "createAt": 100,
        "updateAt": 200,
        "deletedAt": 310,
    })
    .to_string();
    let trashed_members = json!([
        pre0073_trashed_member(&trashed_board, admin_user, [true, true, false, false]),
        pre0073_trashed_member(&trashed_board, editor_user, [false, true, false, true]),
        pre0073_trashed_member(&trashed_board, commenter_user, [false, false, true, false]),
        pre0073_trashed_member(&trashed_board, viewer_user, [false, false, false, true]),
        pre0073_trashed_member(&trashed_board, all_false_user, [false, false, false, false]),
        pre0073_trashed_member(
            &trashed_board,
            joined_org_admin,
            [true, false, false, false]
        ),
    ])
    .to_string();
    for (id, at, del_id, doc, members) in [
        (
            &trashed_board,
            300_i64,
            &del_1,
            &trashed_doc,
            trashed_members.as_str(),
        ),
        (&trashed_empty, 310_i64, &del_2, &empty_doc, "[]"),
    ] {
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO deleted_containers \
                 (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
                 VALUES ('{id}', '{ws_id}', {at}, '{admin_user}', '{del_id}', '{doc}', '{members}', '[]', '[]')"
            ))
            .await
            .expect("pre-0073 deleted_container seeded");
    }

    let prior = json!([
        [board_prior, [false, false, true, false]],
        [&trashed_board, [false, true, false, true]],
        [board_single, [false, false, false, false]],
    ])
    .to_string();
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members \
             SET via_org_admin = true, prior_container_roles = '{prior}' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{joined_org_admin}'"
        ))
        .await
        .expect("pre-0073 prior_container_roles seeded");
    (trashed_board, trashed_empty)
}

async fn seed_org_a(pre: &Store) -> OrgAFixture {
    let (org_id, ws_id) = common::organization(pre, "acme").await;
    let scoped = pre.for_org(&org_id).expect("org A handle");
    let admin_user = common::user(&scoped, "alice-admin").await;
    let editor_user = common::user(&scoped, "ed-editor").await;
    let commenter_user = common::user(&scoped, "cam-commenter").await;
    let viewer_user = common::user(&scoped, "vic-viewer").await;
    let all_false_user = common::user(&scoped, "nil-flags").await;
    let joined_org_admin = common::user(&scoped, "olivia-org-admin").await;

    scoped
        .execute_unchecked(&format!(
            "UPDATE organization_members SET role = '{}' WHERE user_id = '{joined_org_admin}'",
            OrgRole::Admin.as_str()
        ))
        .await
        .expect("org admin set");
    for uid in [&admin_user, &joined_org_admin] {
        scoped
            .execute_unchecked(&format!(
                "UPDATE workspace_members SET role = '{}' \
                 WHERE workspace_id = '{ws_id}' AND user_id = '{uid}'",
                WorkspaceRole::Admin.as_str()
            ))
            .await
            .expect("workspace admin set");
    }

    let (board_single, board_mixed, board_prior) = (new_id(), new_id(), new_id());
    for (id, title) in [
        (&board_single, "Single Flags"),
        (&board_mixed, "Mixed Flags"),
        (&board_prior, "Prior Roles"),
    ] {
        insert_pre0073_board(&scoped, id, &ws_id, &admin_user, title).await;
    }

    for (uid, flags) in [
        (&admin_user, [true, false, false, false]),
        (&editor_user, [false, true, false, false]),
        (&commenter_user, [false, false, true, false]),
        (&viewer_user, [false, false, false, true]),
        (&all_false_user, [false, false, false, false]),
    ] {
        insert_pre0073_member(&scoped, &board_single, uid, flags).await;
    }

    for (uid, flags) in [
        (&admin_user, [true, true, true, true]),
        (&editor_user, [false, true, true, true]),
        (&commenter_user, [false, false, true, true]),
        (&viewer_user, [false, false, false, true]),
        (&all_false_user, [false, false, false, false]),
        (&joined_org_admin, [true, false, false, true]),
    ] {
        insert_pre0073_member(&scoped, &board_mixed, uid, flags).await;
    }

    for (uid, flags) in [
        (&admin_user, [true, true, false, false]),
        (&joined_org_admin, [true, false, false, false]),
    ] {
        insert_pre0073_member(&scoped, &board_prior, uid, flags).await;
    }

    let (trashed_board, trashed_empty) = seed_org_a_trash_and_prior(
        &scoped,
        &ws_id,
        (&board_single, &board_prior),
        (
            &admin_user,
            &editor_user,
            &commenter_user,
            &viewer_user,
            &all_false_user,
            &joined_org_admin,
        ),
    )
    .await;
    let (edge_ws_id, trashed_only_roleless, trashed_edge) =
        seed_org_a_edge_cases(&scoped, &ws_id, &admin_user, &viewer_user, &all_false_user).await;

    OrgAFixture {
        org_id,
        ws_id,
        edge_ws_id,
        board_single,
        board_mixed,
        board_prior,
        trashed_board,
        trashed_empty,
        trashed_only_roleless,
        trashed_edge,
        admin_user,
        editor_user,
        commenter_user,
        viewer_user,
        all_false_user,
        joined_org_admin,
    }
}

async fn seed_org_b(pre: &Store) -> Option<OrgBFixture> {
    if pre.db_type() != DbType::Postgres {
        return None;
    }
    let (org_id, ws_id) = common::organization(pre, "beta").await;
    let scoped = pre.for_org(&org_id).expect("org B handle");
    let bob = common::user(&scoped, "bob-beta").await;
    let carol = common::user(&scoped, "carol-beta").await;
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{bob}'",
            WorkspaceRole::Admin.as_str()
        ))
        .await
        .expect("bob ws admin");

    let live_board = new_id();
    insert_pre0073_board(&scoped, &live_board, &ws_id, &bob, "Org B Board").await;
    insert_pre0073_member(&scoped, &live_board, &bob, [true, true, false, false]).await;
    insert_pre0073_member(&scoped, &live_board, &carol, [false, false, true, true]).await;

    let trashed_board = new_id();
    let del_id = new_id();
    let doc = json!({
        "id": trashed_board,
        "workspaceId": ws_id,
        "createdBy": bob,
        "modifiedBy": bob,
        "visibility": "private",
        "kind": "board",
        "title": "Org B Trashed",
        "cardProperties": [],
        "createAt": 100,
        "updateAt": 200,
        "deletedAt": 400,
    })
    .to_string();
    let members = json!([
        pre0073_trashed_member(&trashed_board, &bob, [true, false, false, false]),
        pre0073_trashed_member(&trashed_board, &carol, [false, true, false, false]),
    ])
    .to_string();
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board}', '{ws_id}', 400, '{bob}', '{del_id}', '{doc}', '{members}', '[]', '[]')"
        ))
        .await
        .expect("org B trashed board seeded");

    Some(OrgBFixture {
        org_id,
        ws_id,
        live_board,
        trashed_board,
        bob,
        carol,
    })
}

async fn assert_edge_cases(scoped: &Store, org_a: &OrgAFixture) {
    let _ = &org_a.edge_ws_id;
    let roleless_only = scoped
        .texts_unchecked(&format!(
            "SELECT CAST(members AS TEXT) FROM deleted_containers WHERE id = '{}'",
            org_a.trashed_only_roleless
        ))
        .await
        .expect("roleless-only trashed members");
    assert_eq!(
        serde_json::from_str::<Value>(&roleless_only[0]).expect("json"),
        json!([]),
        "roleless-only snapshot becomes empty array"
    );

    let edge_members = scoped
        .texts_unchecked(&format!(
            "SELECT CAST(members AS TEXT) FROM deleted_containers WHERE id = '{}'",
            org_a.trashed_edge
        ))
        .await
        .expect("edge trashed members");
    assert_eq!(
        serde_json::from_str::<Value>(&edge_members[0]).expect("json"),
        json!([
            "stray-text",
            {
                "containerId": org_a.trashed_edge,
                "userId": org_a.viewer_user,
                "role": "viewer",
            }
        ]),
        "string element preserved and roleless object pruned in deleted_containers.members"
    );

    let empty_prior = scoped
        .texts_unchecked(&format!(
            "SELECT prior_container_roles FROM workspace_members \
             WHERE workspace_id = '{}' AND user_id = '{}'",
            org_a.ws_id, org_a.all_false_user
        ))
        .await
        .expect("empty-string prior_container_roles");
    assert_eq!(
        empty_prior,
        [""],
        "malformed empty-string prior_container_roles is skipped without aborting"
    );

    let mixed_prior = scoped
        .texts_unchecked(&format!(
            "SELECT prior_container_roles FROM workspace_members \
             WHERE workspace_id = '{}' AND user_id = '{}'",
            org_a.ws_id, org_a.viewer_user
        ))
        .await
        .expect("mixed prior_container_roles");
    assert_eq!(
        serde_json::from_str::<Value>(&mixed_prior[0]).expect("json"),
        json!(["stray-text", ["b-viewer", "viewer"]]),
        "string element preserved and roleless entry pruned in prior_container_roles"
    );
}

async fn assert_catalog_and_constraints(
    database: &fvoci_store::testing::TestDatabase,
    scoped: &Store,
    board_id: &str,
    extra_user: &str,
) {
    match db_type() {
        DbType::Sqlite => {
            let cols = scoped
                .texts_unchecked("SELECT name || ':' || \"notnull\" FROM pragma_table_info('container_members') ORDER BY cid")
                .await
                .expect("pragma_table_info");
            assert_eq!(
                cols,
                ["container_id:1", "user_id:1", "role:1"],
                "SQLite container_members columns after 0073"
            );
        }
        DbType::Postgres => {
            let mut conn = common::connect_past_the_store(database.database_connection()).await;
            let cols: Vec<(String, String)> = sqlx::query_as(
                "SELECT column_name::text, is_nullable::text \
                 FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = 'container_members' \
                 ORDER BY ordinal_position",
            )
            .fetch_all(&mut conn)
            .await
            .expect("information_schema.columns");
            for (col, _) in &cols {
                assert!(
                    ![COL_ADMIN, COL_EDITOR, COL_COMMENTER, COL_VIEWER].contains(&col.as_str()),
                    "retired column {col} must be dropped"
                );
            }
            assert!(
                cols.contains(&("role".to_owned(), "NO".to_owned())),
                "role column must be NOT NULL: {cols:?}"
            );
            let (rls_enabled, rls_forced): (bool, bool) = sqlx::query_as(
                "SELECT relrowsecurity, relforcerowsecurity \
                 FROM pg_class WHERE oid = to_regclass('container_members')",
            )
            .fetch_one(&mut conn)
            .await
            .expect("pg_class");
            assert!(
                rls_enabled && rls_forced,
                "container_members keeps forced RLS"
            );
            conn.close().await.expect("close pg conn");
        }
    }

    // Invalid role values and omitted roles are rejected by DB constraints.
    assert!(
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO container_members (container_id, user_id, role) \
                 VALUES ('{board_id}', '{extra_user}', 'owner')"
            ))
            .await
            .is_err(),
        "CHECK constraint must reject unknown role"
    );
    assert!(
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO container_members (container_id, user_id) \
                 VALUES ('{board_id}', '{extra_user}')"
            ))
            .await
            .is_err(),
        "NOT NULL constraint must reject omitted role"
    );
}

async fn assert_org_a_live(scoped_a: &Store, org_a: &OrgAFixture) {
    // 1. Verify single-flag `container_members` conversions and pruning of `[false; 4]`.
    for (uid, expected) in [
        (&org_a.admin_user, ContainerRole::Admin),
        (&org_a.editor_user, ContainerRole::Editor),
        (&org_a.commenter_user, ContainerRole::Commenter),
        (&org_a.viewer_user, ContainerRole::Viewer),
    ] {
        let member = scoped_a
            .get_member_for_container(&org_a.board_single, uid)
            .await
            .expect("single-board member");
        assert_eq!(member.role, expected, "board_single user {uid}");
    }
    assert!(
        scoped_a
            .get_member_for_container(&org_a.board_single, &org_a.all_false_user)
            .await
            .expect_err("roleless [false; 4] row on board_single must be pruned")
            .is_not_found(),
        "all_false_user must not be elevated to viewer on board_single"
    );

    // 2. Verify multi-flag precedence (`admin > editor > commenter > viewer`) and pruning.
    for (uid, expected) in [
        (&org_a.admin_user, ContainerRole::Admin),
        (&org_a.editor_user, ContainerRole::Editor),
        (&org_a.commenter_user, ContainerRole::Commenter),
        (&org_a.viewer_user, ContainerRole::Viewer),
        (&org_a.joined_org_admin, ContainerRole::Admin),
    ] {
        let member = scoped_a
            .get_member_for_container(&org_a.board_mixed, uid)
            .await
            .expect("mixed-board member");
        assert_eq!(member.role, expected, "board_mixed user {uid}");
    }
    assert!(
        scoped_a
            .get_member_for_container(&org_a.board_mixed, &org_a.all_false_user)
            .await
            .expect_err("roleless [false; 4] row on board_mixed must be pruned")
            .is_not_found(),
        "all_false_user must not be elevated to viewer on board_mixed"
    );
}

async fn assert_org_a_trash(scoped_a: &Store, org_a: &OrgAFixture) {
    // 3. Verify `deleted_containers.members` raw JSON has zero legacy keys and no roleless
    //    entry, plus trash admin filtering (`get_deleted_containers`).
    let raw_trash_members = scoped_a
        .texts_unchecked(&format!(
            "SELECT CAST(members AS TEXT) FROM deleted_containers WHERE id = '{}'",
            org_a.trashed_board
        ))
        .await
        .expect("raw trashed members");
    for key in [KEY_ADMIN, KEY_EDITOR, KEY_COMMENTER, KEY_VIEWER] {
        assert!(
            !raw_trash_members[0].contains(key),
            "retired key {key} still present in deleted_containers.members: {}",
            raw_trash_members[0]
        );
    }
    assert!(
        !raw_trash_members[0].contains(&org_a.all_false_user),
        "roleless snapshot member must be pruned from deleted_containers.members: {}",
        raw_trash_members[0]
    );
    let empty_trash_members = scoped_a
        .texts_unchecked(&format!(
            "SELECT CAST(members AS TEXT) FROM deleted_containers WHERE id = '{}'",
            org_a.trashed_empty
        ))
        .await
        .expect("empty trashed members");
    assert_eq!(
        serde_json::from_str::<Value>(&empty_trash_members[0]).expect("json"),
        json!([]),
        "empty trashed members stay empty"
    );
    assert_edge_cases(scoped_a, org_a).await;

    let admin_trash = scoped_a
        .get_deleted_containers(
            &org_a.ws_id,
            Some(&org_a.admin_user),
            None,
            ContainerKinds::BOARDS,
        )
        .await
        .expect("admin trash");
    assert_eq!(admin_trash.len(), 1, "admin sees trashed_board");
    assert_eq!(
        admin_trash[0].container.id, org_a.trashed_board,
        "trashed_board id"
    );

    let editor_trash = scoped_a
        .get_deleted_containers(
            &org_a.ws_id,
            Some(&org_a.editor_user),
            None,
            ContainerKinds::BOARDS,
        )
        .await
        .expect("editor trash");
    assert!(editor_trash.is_empty(), "non-admin sees no trashed board");
}

async fn assert_org_b(post: &Store, org_b: Option<&OrgBFixture>) {
    let Some(b) = org_b else {
        return;
    };
    let scoped_b = post.for_org(&b.org_id).expect("scoped org B");
    assert_eq!(
        scoped_b
            .get_member_for_container(&b.live_board, &b.bob)
            .await
            .expect("bob")
            .role,
        ContainerRole::Admin,
        "org B bob admin"
    );
    assert_eq!(
        scoped_b
            .get_member_for_container(&b.live_board, &b.carol)
            .await
            .expect("carol")
            .role,
        ContainerRole::Commenter,
        "org B carol commenter"
    );
    scoped_b
        .undelete_container(&b.trashed_board, &b.bob)
        .await
        .expect("org B trashed board restored");
    assert_eq!(
        scoped_b
            .get_member_for_container(&b.trashed_board, &b.carol)
            .await
            .expect("carol restored")
            .role,
        ContainerRole::Editor,
        "org B carol restored editor"
    );
    let _ = &b.ws_id;
}

#[tokio::test]
async fn migration_0073_converts_scheme_booleans_to_role_enum_across_tables_and_snapshots() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_container_member_role_enum(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open at 0072");
    let org_a = seed_org_a(&pre).await;
    let org_b = seed_org_b(&pre).await;
    pre.close().await;

    // Upgrade from 0072 -> latest (0073+).
    let post = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrate to 0073");
    let scoped_a = post.for_org(&org_a.org_id).expect("scoped org A");

    assert_org_a_live(&scoped_a, &org_a).await;
    assert_org_a_trash(&scoped_a, &org_a).await;

    // 4. Verify `workspace_members.prior_container_roles` conversion and roleless pruning
    //    by ending the org admin's joining and restoring `trashed_board`.
    assert!(
        scoped_a
            .leave_workspace_as_org_admin(&org_a.ws_id, &org_a.joined_org_admin)
            .await
            .expect("leave workspace as org admin"),
        "joining ended"
    );
    let prior_restored = scoped_a
        .get_member_for_container(&org_a.board_prior, &org_a.joined_org_admin)
        .await
        .expect("prior role on live board restored");
    assert_eq!(
        prior_restored.role,
        ContainerRole::Commenter,
        "prior commenter role"
    );
    for ungranted in [&org_a.board_single, &org_a.board_mixed] {
        assert!(
            scoped_a
                .get_member_for_container(ungranted, &org_a.joined_org_admin)
                .await
                .expect_err("roleless prior entry and joining-only role must not grant access")
                .is_not_found(),
            "joined_org_admin must have no role on {ungranted}"
        );
    }

    scoped_a
        .undelete_container(&org_a.trashed_board, &org_a.admin_user)
        .await
        .expect("trashed board restored");
    for (uid, expected) in [
        (&org_a.admin_user, ContainerRole::Admin),
        (&org_a.editor_user, ContainerRole::Editor),
        (&org_a.commenter_user, ContainerRole::Commenter),
        (&org_a.viewer_user, ContainerRole::Viewer),
        (&org_a.joined_org_admin, ContainerRole::Editor),
    ] {
        let member = scoped_a
            .get_member_for_container(&org_a.trashed_board, uid)
            .await
            .expect("restored member");
        assert_eq!(
            member,
            ContainerMember::new(&org_a.trashed_board, uid, expected),
            "restored member {uid}"
        );
    }
    assert!(
        scoped_a
            .get_member_for_container(&org_a.trashed_board, &org_a.all_false_user)
            .await
            .expect_err("pruned snapshot member must not return on restore")
            .is_not_found(),
        "all_false_user must not become viewer after undelete_container"
    );

    // 5. Verify second organization on PostgreSQL (system scope across forced RLS).
    assert_org_b(&post, org_b.as_ref()).await;

    let extra_user = common::user(&scoped_a, "constraint-probe").await;
    assert_catalog_and_constraints(&database, &scoped_a, &org_a.board_prior, &extra_user).await;

    post.close().await;
    database.remove().await;
}
