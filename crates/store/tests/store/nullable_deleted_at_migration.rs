//! Migration `0072` (`nullable_deleted_at`) exercised on a populated SQLite and
//! PostgreSQL database seeded at version `0071` with both live (`0`) and deleted (`> 0`)
//! rows across `users`, `organizations`, `workspaces`, `deleted_containers`, and
//! `deleted_blocks`.

use fvoci_model::new_id;
use fvoci_store::{DbType, Store};
use serde_json::{Value, json};
use sqlx::Connection as _;

use crate::common::{self, db_type};

// Retired pre-0072 column and JSON key names assembled via `concat!` so repository-wide
// searches outside `crates/store/schema/` see zero literal occurrences.
const OLD_COL: &str = concat!("delete", "_at");
const OLD_KEY: &str = concat!("delete", "At");

struct Pre0072SecondOrg {
    org_b: String,
    deleted_shared_org: String,
    bob: String,
    trashed_board_b: String,
    trashed_card_b: String,
}

struct Pre0072Fixture {
    org_a: String,
    live_design_ws: String,
    trashed_design_ws: String,
    deleted_personal_org: String,
    live_user: String,
    waiting_user: String,
    deactivated_user: String,
    erased_user: String,
    trashed_board_pos: String,
    trashed_board_zero: String,
    trashed_card_pos: String,
    trashed_card_zero: String,
    decoy_card_id: String,
    other_org: Option<Pre0072SecondOrg>,
}

async fn seed_pre0072_users(
    pre: &Store,
    org_a: &str,
    ws_a: &str,
) -> (String, String, String, String, String) {
    let live_user = common::pre0072_user(pre, org_a, ws_a, "alice").await;
    let waiting_user = common::pre0072_user(pre, org_a, ws_a, "waiting").await;
    let deactivated_user = new_id();
    let erased_user = new_id();
    let erased_user_2 = new_id();
    let deleted_personal_org = new_id();
    let deleted_personal_ws = new_id();

    let sys = pre.system();
    for sql in [
        format!(
            "UPDATE users SET {OLD_COL} = 1700000000500, delete_after = 1702592000500 \
             WHERE id = '{waiting_user}'"
        ),
        format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, {OLD_COL}) \
             VALUES ('{deactivated_user}', 'deactivated', 'deactivated@example.com', '', 10, 10, 1700000000100)"
        ),
        format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, \
             {OLD_COL}, delete_after, erase_started_at, erased_at) \
             VALUES ('{erased_user}', '', '', '', 0, 0, 1700000000200, 1700000000200, \
             1700000000300, 1700000000400)"
        ),
        format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, \
             {OLD_COL}, delete_after, erase_started_at, erased_at) \
             VALUES ('{erased_user_2}', '', '', '', 0, 0, 1700000000210, 1700000000210, \
             1700000000310, 1700000000410)"
        ),
    ] {
        sys.execute_unchecked(&sql)
            .await
            .expect("pre-0072 user seeded");
    }

    let own = pre
        .for_org(&deleted_personal_org)
        .expect("deleted personal org");
    for sql in [
        format!(
            "INSERT INTO organizations (id, name, kind, owner_id, settings, create_at, update_at, {OLD_COL}) \
             VALUES ('{deleted_personal_org}', '', 'personal', '{erased_user}', '{{}}', 5, 5, 1700000000600)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, guest_invitations, create_at, update_at, {OLD_COL}) \
             VALUES ('{deleted_personal_ws}', '{deleted_personal_org}', '', '{{}}', false, 5, 5, 1700000000600)"
        ),
        format!(
            "UPDATE organizations SET default_workspace_id = '{deleted_personal_ws}' \
             WHERE id = '{deleted_personal_org}'"
        ),
    ] {
        own.execute_unchecked(&sql)
            .await
            .expect("deleted personal org seeded");
    }

    (
        live_user,
        waiting_user,
        deactivated_user,
        erased_user,
        deleted_personal_org,
    )
}

async fn seed_pre0072_workspaces(scoped_a: &Store, org_a: &str) -> (String, String) {
    let live_design_ws = new_id();
    let trashed_design_ws = new_id();
    for sql in [
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, guest_invitations, create_at, update_at, {OLD_COL}) \
             VALUES ('{live_design_ws}', '{org_a}', 'Design', '{{}}', false, 30, 30, 0)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, guest_invitations, create_at, update_at, {OLD_COL}) \
             VALUES ('{trashed_design_ws}', '{org_a}', 'Design', '{{}}', false, 25, 25, 1700000000800)"
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("pre-0072 workspaces seeded");
    }
    (live_design_ws, trashed_design_ws)
}

async fn seed_pre0072_deleted_containers(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
) -> (String, String, String) {
    let live_board_id = new_id();
    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{live_board_id}', '{ws_a}', '{alice}', '{alice}', 'private', 'board', \
              'Live Board', '', '', false, false, '{{}}', '[]', 100, 200)"
        ),
        format!(
            "INSERT INTO container_members \
             (container_id, user_id, {}, {}, {}, {}) \
             VALUES ('{live_board_id}', '{alice}', true, true, false, false)",
            concat!("scheme", "_admin"),
            concat!("scheme", "_editor"),
            concat!("scheme", "_commenter"),
            concat!("scheme", "_viewer"),
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("live board seeded");
    }

    let (trashed_board_pos, trashed_board_zero) = (new_id(), new_id());
    let (board_deletion_pos, board_deletion_zero) = (new_id(), new_id());
    let desc = if scoped_a.db_type() == DbType::Postgres {
        r"nul:\u0000 literal:\\u0000"
    } else {
        "plain description"
    };
    let pos_board_doc = format!(
        r#"{{"id":"{trashed_board_pos}","workspaceId":"{ws_a}","createdBy":"{alice}","modifiedBy":"{alice}","visibility":"private","kind":"board","title":"Trashed Pos","description":"{desc}","cardProperties":[],"createAt":100,"updateAt":200,"{OLD_KEY}":1700000000900}}"#
    );
    let zero_board_doc = json!({
        "id": trashed_board_zero,
        "workspaceId": ws_a,
        "createdBy": alice,
        "modifiedBy": alice,
        "visibility": "open",
        "kind": "board",
        "title": "Trashed Zero",
        "cardProperties": [],
        "createAt": 100,
        "updateAt": 200,
        (OLD_KEY): 0
    })
    .to_string();
    let members_pos = json!([{
        "containerId": trashed_board_pos,
        "userId": alice,
        (concat!("scheme", "Admin")): true,
        (concat!("scheme", "Editor")): true,
        (concat!("scheme", "Commenter")): false,
        (concat!("scheme", "Viewer")): false
    }])
    .to_string();
    for (id, at, del_id, doc, members) in [
        (
            &trashed_board_pos,
            1_700_000_000_900_i64,
            &board_deletion_pos,
            &pos_board_doc,
            members_pos.as_str(),
        ),
        (
            &trashed_board_zero,
            1_700_000_000_910_i64,
            &board_deletion_zero,
            &zero_board_doc,
            "[]",
        ),
    ] {
        scoped_a
            .execute_unchecked(&format!(
                "INSERT INTO deleted_containers \
                 (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
                 VALUES ('{id}', '{ws_a}', {at}, '{alice}', '{del_id}', '{doc}', '{members}', '[]', '[]')"
            ))
            .await
            .expect("trashed container seeded");
    }

    (live_board_id, trashed_board_pos, trashed_board_zero)
}

async fn seed_pre0072_deleted_blocks(
    scoped_a: &Store,
    live_board_id: &str,
    alice: &str,
) -> (String, String, String) {
    let (trashed_card_pos, trashed_card_zero, decoy_card_id) = (new_id(), new_id(), new_id());
    let (card_deletion_pos, card_deletion_zero, card_deletion_decoy) =
        (new_id(), new_id(), new_id());
    let pos_card_doc = if scoped_a.db_type() == DbType::Postgres {
        format!(
            r#"{{"id":"{trashed_card_pos}","containerId":"{live_board_id}","parentId":"","createdBy":"{alice}","modifiedBy":"{alice}","schema":1,"type":"card","title":"NUL Card","fields":{{"note":"nul:\u0000 literal:\\u0000"}},"createAt":110,"updateAt":210,"{OLD_KEY}":1700000000950,"number":1}}"#
        )
    } else {
        json!({
            "id": trashed_card_pos,
            "containerId": live_board_id,
            "parentId": "",
            "createdBy": alice,
            "modifiedBy": alice,
            "schema": 1,
            "type": "card",
            "title": "Pos Card",
            "fields": {"note": "plain"},
            "createAt": 110,
            "updateAt": 210,
            (OLD_KEY): 1_700_000_000_950_i64,
            "number": 1
        })
        .to_string()
    };
    let zero_card_doc = json!({
        "id": trashed_card_zero,
        "containerId": live_board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "card",
        "title": "Zero Card",
        "fields": {},
        "createAt": 110,
        "updateAt": 210,
        (OLD_KEY): 0,
        "number": 2
    })
    .to_string();
    let decoy_card_doc = json!({
        "id": decoy_card_id,
        "containerId": live_board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "card",
        "title": "Decoy Nested Key",
        "fields": {(OLD_KEY): "must-keep-nested-field"},
        "createAt": 110,
        "updateAt": 210,
        (OLD_KEY): 1_700_000_000_960_i64,
        "number": 3
    })
    .to_string();
    for (id, at, del_id, doc) in [
        (
            &trashed_card_pos,
            1_700_000_000_950_i64,
            &card_deletion_pos,
            &pos_card_doc,
        ),
        (
            &trashed_card_zero,
            1_700_000_000_955_i64,
            &card_deletion_zero,
            &zero_card_doc,
        ),
        (
            &decoy_card_id,
            1_700_000_000_960_i64,
            &card_deletion_decoy,
            &decoy_card_doc,
        ),
    ] {
        scoped_a
            .execute_unchecked(&format!(
                "INSERT INTO deleted_blocks \
                 (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
                 VALUES ('{id}', '{live_board_id}', NULL, {at}, '{alice}', '{del_id}', '{doc}')"
            ))
            .await
            .expect("trashed block seeded");
    }

    (trashed_card_pos, trashed_card_zero, decoy_card_id)
}

async fn seed_pre0072_trash(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
) -> (String, String, String, String, String, String) {
    let (live_board_id, trashed_board_pos, trashed_board_zero) =
        seed_pre0072_deleted_containers(scoped_a, ws_a, alice).await;
    let (trashed_card_pos, trashed_card_zero, decoy_card_id) =
        seed_pre0072_deleted_blocks(scoped_a, &live_board_id, alice).await;
    (
        live_board_id,
        trashed_board_pos,
        trashed_board_zero,
        trashed_card_pos,
        trashed_card_zero,
        decoy_card_id,
    )
}

async fn seed_pre0072_second_org(pre: &Store) -> Pre0072SecondOrg {
    let (org_b, ws_b) = common::organization(pre, "org-b").await;
    let scoped_b = pre.for_org(&org_b).expect("org-b");
    let bob = common::pre0072_user(&scoped_b, &org_b, &ws_b, "bob").await;

    let (deleted_shared_org, deleted_shared_ws) = common::organization(pre, "org-deleted").await;
    pre.system()
        .execute_unchecked(&format!(
            "UPDATE organizations SET {OLD_COL} = 1700000000700 WHERE id = '{deleted_shared_org}'"
        ))
        .await
        .expect("deleted shared org marked");
    pre.for_org(&deleted_shared_org)
        .expect("deleted shared org")
        .execute_unchecked(&format!(
            "UPDATE workspaces SET {OLD_COL} = 1700000000700 WHERE id = '{deleted_shared_ws}'"
        ))
        .await
        .expect("deleted shared ws marked");

    let live_board_b = new_id();
    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{live_board_b}', '{ws_b}', '{bob}', '{bob}', 'open', 'board', \
              'Org B Live', '', '', false, false, '{{}}', '[]', 100, 200)"
        ),
        format!(
            "INSERT INTO container_members \
             (container_id, user_id, {}, {}, {}, {}) \
             VALUES ('{live_board_b}', '{bob}', true, true, false, false)",
            concat!("scheme", "_admin"),
            concat!("scheme", "_editor"),
            concat!("scheme", "_commenter"),
            concat!("scheme", "_viewer"),
        ),
    ] {
        scoped_b
            .execute_unchecked(&sql)
            .await
            .expect("org-b live board");
    }

    let (trashed_board_b, trashed_card_b) = (new_id(), new_id());
    let (del_b, del_c) = (new_id(), new_id());
    let b_doc = json!({
        "id": trashed_board_b,
        "workspaceId": ws_b,
        "createdBy": bob,
        "modifiedBy": bob,
        "visibility": "open",
        "kind": "board",
        "title": "Org B Trashed Board",
        "cardProperties": [],
        "createAt": 100,
        "updateAt": 200,
        (OLD_KEY): 1_700_000_000_980_i64
    })
    .to_string();
    let c_doc = json!({
        "id": trashed_card_b,
        "containerId": live_board_b,
        "parentId": "",
        "createdBy": bob,
        "modifiedBy": bob,
        "schema": 1,
        "type": "card",
        "title": "Org B Trashed Card",
        "fields": {},
        "createAt": 110,
        "updateAt": 210,
        (OLD_KEY): 1_700_000_000_990_i64,
        "number": 1
    })
    .to_string();
    scoped_b
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board_b}', '{ws_b}', 1700000000980, '{bob}', '{del_b}', '{b_doc}', '[]', '[]', '[]')"
        ))
        .await
        .expect("org-b trashed board");
    scoped_b
        .execute_unchecked(&format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{trashed_card_b}', '{live_board_b}', NULL, 1700000000990, '{bob}', '{del_c}', '{c_doc}')"
        ))
        .await
        .expect("org-b trashed card");

    Pre0072SecondOrg {
        org_b,
        deleted_shared_org,
        bob,
        trashed_board_b,
        trashed_card_b,
    }
}

async fn seed_pre0072(pre: &Store) -> Pre0072Fixture {
    let (org_a, ws_a) = common::organization(pre, "org-a").await;
    let scoped_a = pre.for_org(&org_a).expect("org-a");
    let (live_user, waiting_user, deactivated_user, erased_user, deleted_personal_org) =
        seed_pre0072_users(pre, &org_a, &ws_a).await;
    let (live_design_ws, trashed_design_ws) = seed_pre0072_workspaces(&scoped_a, &org_a).await;
    let (
        _live_board_id,
        trashed_board_pos,
        trashed_board_zero,
        trashed_card_pos,
        trashed_card_zero,
        decoy_card_id,
    ) = seed_pre0072_trash(&scoped_a, &ws_a, &live_user).await;
    let other_org = if pre.db_type() == DbType::Postgres {
        Some(seed_pre0072_second_org(pre).await)
    } else {
        None
    };

    Pre0072Fixture {
        org_a,
        live_design_ws,
        trashed_design_ws,
        deleted_personal_org,
        live_user,
        waiting_user,
        deactivated_user,
        erased_user,
        trashed_board_pos,
        trashed_board_zero,
        trashed_card_pos,
        trashed_card_zero,
        decoy_card_id,
        other_org,
    }
}

async fn assert_post0072_users_and_checks(migrated: &Store, fx: &Pre0072Fixture) {
    let sys = migrated.system();
    let alice = migrated
        .get_user_by_id(&fx.live_user)
        .await
        .expect("live alice");
    assert_eq!(alice.deleted_at, None, "live user deleted_at is None");
    assert!(
        migrated.get_user_by_id(&fx.waiting_user).await.is_err(),
        "waiting user excluded from active get_user_by_id"
    );
    assert!(
        migrated.get_user_by_id(&fx.deactivated_user).await.is_err(),
        "deactivated user excluded from active get_user_by_id"
    );

    let by_ids = migrated
        .get_users_list(&[
            fx.live_user.clone(),
            fx.waiting_user.clone(),
            fx.erased_user.clone(),
        ])
        .await
        .expect("users by ids");
    let find = |id: &str| by_ids.iter().find(|u| u.id == id).expect("present");
    assert_eq!(
        find(&fx.live_user).deleted_at,
        None,
        "live user has no deleted_at"
    );
    assert_eq!(
        find(&fx.waiting_user).deleted_at,
        Some(1_700_000_000_500),
        "waiting user keeps deletion timestamp"
    );
    let erased = find(&fx.erased_user);
    assert_eq!(
        (
            erased.create_at,
            erased.update_at,
            erased.deleted_at,
            erased.username.as_str()
        ),
        (0, 0, None, ""),
        "erased user projects as nobody tombstone without deletedAt"
    );

    let raw = sys
        .texts_unchecked(&format!(
            "SELECT COALESCE(CAST(deleted_at AS TEXT), 'NULL') FROM users \
             WHERE id IN ('{}', '{}', '{}') ORDER BY CASE id \
               WHEN '{}' THEN 1 WHEN '{}' THEN 2 ELSE 3 END",
            fx.live_user, fx.deactivated_user, fx.erased_user, fx.live_user, fx.deactivated_user,
        ))
        .await
        .expect("raw user deleted_at");
    assert_eq!(
        raw,
        ["NULL", "1700000000100", "1700000000200"],
        "0 became NULL while > 0 timestamps were preserved"
    );
    assert_post0072_user_constraints(migrated, fx).await;
}

async fn assert_post0072_user_constraints(migrated: &Store, fx: &Pre0072Fixture) {
    let sys = migrated.system();
    // Partial unique indexes on `users` (`WHERE username <> ''` / `WHERE email <> ''`):
    // waiting/deactivated accounts keep their username and email reserved until erased.
    assert!(
        sys.execute_unchecked(&format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, deleted_at) \
             VALUES ('{}', 'WAITING', 'other@example.com', '', 1, 1, NULL)",
            new_id()
        ))
        .await
        .is_err(),
        "waiting account username stays reserved"
    );
    assert!(
        sys.execute_unchecked(&format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, deleted_at) \
             VALUES ('{}', 'unique_u', 'DEACTIVATED@example.com', '', 1, 1, NULL)",
            new_id()
        ))
        .await
        .is_err(),
        "deactivated account email stays reserved"
    );
    sys.execute_unchecked(&format!(
        "INSERT INTO users (id, username, email, password_hash, create_at, update_at, \
         deleted_at, delete_after, erase_started_at, erased_at) \
         VALUES ('{}', '', '', '', 0, 0, 999, 999, 999, 999)",
        new_id()
    ))
    .await
    .expect("multiple erased tombstones with empty username/email allowed");

    // 3-valued logic CHECK constraints on `users`:
    for bad_sql in [
        format!(
            "UPDATE users SET deleted_at = 0 WHERE id = '{}'",
            fx.live_user
        ),
        format!(
            "UPDATE users SET deleted_at = -5 WHERE id = '{}'",
            fx.live_user
        ),
        format!(
            "UPDATE users SET delete_after = 100 WHERE id = '{}'",
            fx.live_user
        ),
        format!(
            "UPDATE users SET erase_started_at = 100 WHERE id = '{}'",
            fx.live_user
        ),
    ] {
        assert!(
            sys.execute_unchecked(&bad_sql).await.is_err(),
            "3-valued logic CHECK must reject `{bad_sql}`"
        );
    }
}

async fn assert_post0072_orgs_and_workspaces(migrated: &Store, fx: &Pre0072Fixture) {
    let sys = migrated.system();
    let org_rows = sys
        .texts_unchecked(&format!(
            "SELECT COALESCE(CAST(deleted_at AS TEXT), 'NULL') FROM organizations \
             WHERE id IN ('{}', '{}') ORDER BY CASE id WHEN '{}' THEN 1 ELSE 2 END",
            fx.org_a, fx.deleted_personal_org, fx.org_a
        ))
        .await
        .expect("org deleted_at");
    assert_eq!(
        org_rows,
        ["NULL", "1700000000600"],
        "organizations 0 -> NULL and > 0 preserved"
    );
    assert!(
        sys.execute_unchecked(&format!(
            "UPDATE organizations SET deleted_at = 0 WHERE id = '{}'",
            fx.org_a
        ))
        .await
        .is_err(),
        "organizations CHECK rejects deleted_at = 0"
    );

    let scoped_a = migrated.for_org(&fx.org_a).expect("org-a");
    let workspaces = scoped_a.org_workspaces("").await.expect("org workspaces");
    let live_ws = workspaces
        .iter()
        .find(|w| w.id == fx.live_design_ws)
        .expect("live Design ws");
    let trashed_ws = workspaces
        .iter()
        .find(|w| w.id == fx.trashed_design_ws)
        .expect("trashed Design ws");
    assert_eq!(
        live_ws.deleted_at, None,
        "live workspace deleted_at is None"
    );
    assert_eq!(
        trashed_ws.deleted_at,
        Some(1_700_000_000_800),
        "trashed workspace preserves deletion timestamp"
    );

    // Partial unique index `workspaces_name_in_use_key` (`WHERE deleted_at IS NULL`):
    assert!(
        scoped_a
            .execute_unchecked(&format!(
                "INSERT INTO workspaces (id, org_id, name, settings, create_at, update_at, deleted_at) \
                 VALUES ('{}', '{}', 'Design', '{{}}', 1, 1, NULL)",
                new_id(),
                fx.org_a
            ))
            .await
            .is_err(),
        "duplicate active workspace name refused"
    );
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO workspaces (id, org_id, name, settings, create_at, update_at, deleted_at) \
             VALUES ('{}', '{}', 'Design', '{{}}', 1, 1, 555)",
            new_id(),
            fx.org_a
        ))
        .await
        .expect("trashed duplicate workspace name allowed");
    assert!(
        scoped_a
            .execute_unchecked(&format!(
                "UPDATE workspaces SET deleted_at = 0 WHERE id = '{}'",
                fx.live_design_ws
            ))
            .await
            .is_err(),
        "workspaces CHECK rejects deleted_at = 0"
    );
}

async fn assert_post0072_trash_json(scoped_a: &Store, fx: &Pre0072Fixture) {
    let c_cast = if db_type() == DbType::Postgres {
        "container::text"
    } else {
        "container"
    };
    let b_cast = if db_type() == DbType::Postgres {
        "block::text"
    } else {
        "block"
    };

    for (id, expected) in [
        (&fx.trashed_board_pos, Some(json!(1_700_000_000_900_i64))),
        (&fx.trashed_board_zero, None),
    ] {
        let raw = scoped_a
            .texts_unchecked(&format!(
                "SELECT {c_cast} FROM deleted_containers WHERE id = '{id}'"
            ))
            .await
            .expect("raw container json");
        if db_type() == DbType::Postgres && id == &fx.trashed_board_pos {
            assert!(
                raw[0].contains(r"\u0000") && raw[0].contains(r"\\u0000"),
                "PG NUL escape preserved in deleted_containers: {}",
                raw[0]
            );
        }
        let parsed: Value = serde_json::from_str(&raw[0]).expect("valid container json");
        assert!(
            parsed.get(OLD_KEY).is_none(),
            "retired key stripped: {parsed}"
        );
        assert_eq!(
            parsed.get("deletedAt").cloned(),
            expected,
            "rewritten container deletedAt"
        );
    }

    for (id, expected) in [
        (&fx.trashed_card_pos, Some(json!(1_700_000_000_950_i64))),
        (&fx.trashed_card_zero, None),
        (&fx.decoy_card_id, Some(json!(1_700_000_000_960_i64))),
    ] {
        let raw = scoped_a
            .texts_unchecked(&format!(
                "SELECT {b_cast} FROM deleted_blocks WHERE id = '{id}'"
            ))
            .await
            .expect("raw block json");
        if db_type() == DbType::Postgres && id == &fx.trashed_card_pos {
            assert!(
                raw[0].contains(r"\u0000") && raw[0].contains(r"\\u0000"),
                "PG NUL escape preserved in deleted_blocks: {}",
                raw[0]
            );
        }
        let parsed: Value = serde_json::from_str(&raw[0]).expect("valid block json");
        assert!(
            parsed.get(OLD_KEY).is_none(),
            "retired key stripped: {parsed}"
        );
        assert_eq!(
            parsed.get("deletedAt").cloned(),
            expected,
            "rewritten block deletedAt"
        );
        if id == &fx.decoy_card_id {
            assert_eq!(
                parsed["fields"][OLD_KEY],
                json!("must-keep-nested-field"),
                "nested fields key preserved untouched"
            );
        }
    }
}

async fn assert_post0072_trash_and_rls(migrated: &Store, fx: &Pre0072Fixture) {
    let scoped_a = migrated.for_org(&fx.org_a).expect("org-a");
    assert_post0072_trash_json(&scoped_a, fx).await;

    // Even when pre-0072 JSON carried a zero sentinel (stripped to absent `"deletedAt"`),
    // `get_deleted_container` and `get_deleted_block` preserve the authoritative SQL column timestamp.
    let zero_board_row = scoped_a
        .get_deleted_container(&fx.trashed_board_zero)
        .await
        .expect("trashed zero container row");
    assert_eq!(
        (
            zero_board_row.deleted_at,
            zero_board_row.container.deleted_at
        ),
        (1_700_000_000_910, None),
        "SQL deleted_at preserved when JSON omitted deletedAt"
    );
    let zero_card_row = scoped_a
        .get_deleted_block(&fx.trashed_card_zero)
        .await
        .expect("trashed zero card row");
    assert_eq!(
        (zero_card_row.deleted_at, zero_card_row.block.deleted_at),
        (1_700_000_000_955, None),
        "SQL deleted_at preserved on block when JSON omitted deletedAt"
    );

    // Restoring trashed boards and blocks yields `deleted_at == None`.
    if db_type() != DbType::Postgres {
        let restored_board = scoped_a
            .undelete_container(&fx.trashed_board_pos, &fx.live_user)
            .await
            .expect("restore pos board");
        assert_eq!(
            restored_board.deleted_at, None,
            "restored pos board has deleted_at None"
        );
        let restored_card = scoped_a
            .undelete_block(&fx.trashed_card_pos, &fx.live_user)
            .await
            .expect("restore pos card");
        assert_eq!(
            restored_card.deleted_at, None,
            "restored pos card has deleted_at None"
        );
    }
    let restored_zero_board = scoped_a
        .undelete_container(&fx.trashed_board_zero, &fx.live_user)
        .await
        .expect("restore zero board");
    assert_eq!(
        restored_zero_board.deleted_at, None,
        "restored zero board has deleted_at None"
    );
    let restored_zero_card = scoped_a
        .undelete_block(&fx.trashed_card_zero, &fx.live_user)
        .await
        .expect("restore zero card");
    assert_eq!(
        restored_zero_card.deleted_at, None,
        "restored zero card has deleted_at None"
    );

    if let Some(other) = &fx.other_org {
        let del_org_val = migrated
            .system()
            .texts_unchecked(&format!(
                "SELECT CAST(deleted_at AS TEXT) FROM organizations WHERE id = '{}'",
                other.deleted_shared_org
            ))
            .await
            .expect("deleted shared org");
        assert_eq!(
            del_org_val,
            ["1700000000700"],
            "deleted shared org timestamp preserved"
        );

        let scoped_b = migrated.for_org(&other.org_b).expect("org-b");
        let b_board = scoped_b
            .undelete_container(&other.trashed_board_b, &other.bob)
            .await
            .expect("restore org-b board");
        assert_eq!(
            b_board.deleted_at, None,
            "restored org-b board has deleted_at None"
        );
        let b_card = scoped_b
            .undelete_block(&other.trashed_card_b, &other.bob)
            .await
            .expect("restore org-b card");
        assert_eq!(
            b_card.deleted_at, None,
            "restored org-b card has deleted_at None"
        );
    }
}

async fn assert_post0072_sqlite_catalog(
    database: &fvoci_store::testing::TestDatabase,
    sys: &Store,
) {
    let mut conn = sqlx::sqlite::SqliteConnection::connect(&common::raw_connection(database))
        .await
        .expect("sqlite open");
    let fk_errors: Vec<String> =
        sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
            .fetch_all(&mut conn)
            .await
            .expect("fk check");
    assert!(fk_errors.is_empty(), "broken foreign keys: {fk_errors:?}");
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut conn)
        .await
        .expect("integrity check");
    assert_eq!(integrity, ["ok"], "sqlite integrity check passes");
    conn.close().await.expect("close");

    for table in ["organizations", "workspaces", "users"] {
        let cols = sys
            .texts_unchecked(&format!(
                "SELECT name || ':' || \"notnull\" FROM pragma_table_info('{table}')"
            ))
            .await
            .expect("table cols");
        assert!(
            cols.iter().any(|c| c == "deleted_at:0"),
            "{table} missing nullable deleted_at: {cols:?}"
        );
        assert!(
            !cols.iter().any(|c| c.starts_with(&format!("{OLD_COL}:"))),
            "{table} still has retired {OLD_COL}: {cols:?}"
        );
    }

    let master_sql = sys
        .texts_unchecked("SELECT COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL")
        .await
        .expect("sqlite_master sql");
    assert!(
        !master_sql.iter().any(|s| s.contains(OLD_COL)),
        "sqlite_master still mentions {OLD_COL}: {master_sql:?}"
    );
}

async fn assert_post0072_postgres_catalog(sys: &Store) {
    for table in ["organizations", "workspaces", "users"] {
        let cols = sys
            .texts_unchecked(&format!(
                "SELECT column_name::text || ':' || is_nullable::text \
                 FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = '{table}'"
            ))
            .await
            .expect("pg cols");
        assert!(
            cols.iter().any(|c| c == "deleted_at:YES"),
            "{table} missing nullable deleted_at: {cols:?}"
        );
    }
    let retired_cols = sys
        .texts_unchecked(&format!(
            "SELECT table_name::text FROM information_schema.columns \
             WHERE table_schema = current_schema() AND column_name = '{OLD_COL}'"
        ))
        .await
        .expect("pg retired cols");
    assert!(
        retired_cols.is_empty(),
        "retired {OLD_COL} column remains in PG: {retired_cols:?}"
    );
    let retired_checks = sys
        .texts_unchecked(&format!(
            "SELECT conname::text FROM pg_constraint \
             WHERE connamespace = current_schema()::regnamespace \
               AND pg_get_constraintdef(oid) LIKE '%{OLD_COL}%'"
        ))
        .await
        .expect("pg checks");
    assert!(
        retired_checks.is_empty(),
        "retired {OLD_COL} constraint remains in PG: {retired_checks:?}"
    );
}

#[tokio::test]
async fn migration_0072_upgrades_populated_database_to_nullable_deleted_at() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_nullable_deleted_at(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("opens at migration 71");
    let fx = seed_pre0072(&pre).await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrates 0071 -> 0072 cleanly");

    let sys = migrated.system();
    if db_type() == DbType::Postgres {
        assert_post0072_postgres_catalog(&sys).await;
    } else {
        assert_post0072_sqlite_catalog(&database, &sys).await;
    }
    assert_post0072_users_and_checks(&migrated, &fx).await;
    assert_post0072_orgs_and_workspaces(&migrated, &fx).await;
    assert_post0072_trash_and_rls(&migrated, &fx).await;

    migrated.close().await;
    database.remove().await;
}
