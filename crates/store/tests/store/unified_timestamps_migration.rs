//! Migration `0075` (`unified_created_updated_timestamps`) exercised on a populated
//! SQLite and PostgreSQL database seeded at version `0074` across all 36 tables whose
//! timestamp columns are unified to `created_at` / `updated_at`, plus
//! `deleted_containers` (`container` and `files`) and `deleted_blocks` (`block` and
//! `watchedBy`) JSON snapshots, PostgreSQL `\u0000` / `\\u0000` escapes, multi-tenant
//! RLS isolation, and `audit_log` identity-erasure triggers.

use fvoci_model::{BlockType, ContainerRole, FileRecord, Subscription, new_id};
use fvoci_store::{DbType, Store};
use serde_json::{Value, json};
use sqlx::Connection as _;

use crate::common::{
    self, PRE0075_CREATE_COL as C_COL, PRE0075_CREATE_KEY as C_KEY, PRE0075_UPDATE_COL as U_COL,
    PRE0075_UPDATE_KEY as U_KEY, db_type,
};

struct Pre0075OrgB {
    org_id: String,
    bob: String,
    live_board_id: String,
    trashed_board_id: String,
    trashed_file: FileRecord,
    trashed_card_id: String,
}

struct Pre0075Fixture {
    org_a: String,
    ws_a: String,
    alice: String,
    signup_id: String,
    audit_id: String,
    provider_id: String,
    invitation_id: String,
    board_id: String,
    course_id: String,
    card_id: String,
    unit_id: String,
    category_id: String,
    cat_container_id: String,
    file_id: String,
    upload_id: String,
    notif_id: String,
    time_entry_id: String,
    note_id: String,
    review_id: String,
    meeting_id: String,
    exam_id: String,
    trashed_board_id: String,
    trashed_file_1: FileRecord,
    trashed_file_2: FileRecord,
    trashed_nul_board_id: String,
    trashed_card_id: String,
    trashed_nul_card_id: String,
    org_b: Option<Pre0075OrgB>,
}

async fn seed_auth_and_oidc(
    pre: &Store,
    scoped_a: &Store,
    org_a: &str,
    ws_a: &str,
    alice: &str,
) -> (String, String, String, String) {
    let sys = pre.system();
    let (signup_id, audit_id, provider_id, invitation_id) =
        (new_id(), new_id(), new_id(), new_id());
    for sql in [
        format!("UPDATE users SET {C_COL} = 1001, {U_COL} = 2001 WHERE id = '{alice}'"),
        format!("UPDATE organizations SET {C_COL} = 1006, {U_COL} = 2006 WHERE id = '{org_a}'"),
        format!(
            "UPDATE organization_members SET {C_COL} = 1007 \
             WHERE org_id = '{org_a}' AND user_id = '{alice}'"
        ),
        format!("UPDATE workspaces SET {C_COL} = 1008, {U_COL} = 2008 WHERE id = '{ws_a}'"),
        format!(
            "UPDATE workspace_members SET {C_COL} = 1009 \
             WHERE workspace_id = '{ws_a}' AND user_id = '{alice}'"
        ),
        format!(
            "INSERT INTO user_identities (issuer, subject, user_id, email, {C_COL}) \
             VALUES ('https://idp.example.test', 'sub-alice', '{alice}', 'alice@example.com', 1002)"
        ),
        format!(
            "INSERT INTO email_changes (user_id, new_email, token_hash, token_expires_at, {C_COL}, expires_at) \
             VALUES ('{alice}', 'alice-new@example.com', 'chg-tok', 9000000000000, 1003, 9000000000000)"
        ),
        format!(
            "INSERT INTO email_reverts (token_hash, user_id, email, {C_COL}, expires_at) \
             VALUES ('rev-tok', '{alice}', 'alice-old@example.com', 1004, 9000000000000)"
        ),
        format!(
            "INSERT INTO signups (id, username, email, password_hash, organization_name, {C_COL}, expires_at) \
             VALUES ('{signup_id}', 'pending0075', 'pending0075@example.com', 'hash', 'NewOrg', 1005, 9000000000000)"
        ),
    ] {
        sys.execute_unchecked(&sql)
            .await
            .expect("auth/global pre-0075 row seeded");
    }

    for sql in [
        format!(
            "INSERT INTO invitations \
             (id, org_id, workspace_id, role, token_hash, created_by, {C_COL}, expires_at) \
             VALUES ('{invitation_id}', '{org_a}', '{ws_a}', 'member', 'inv-tok', '{alice}', 1010, 9000000000000)"
        ),
        format!(
            "INSERT INTO audit_log \
             (id, org_id, actor_id, actor_kind, action, target_id, subject_id, {C_COL}, detail, identity) \
             VALUES ('{audit_id}', '{org_a}', '{alice}', 'person', 'oidc_identity_carried_over', \
              '{org_a}', '{alice}', 1011, 'https://idp.example.test', 'alice-idp@example.test')"
        ),
        format!(
            "INSERT INTO oidc_providers \
             (id, org_id, issuer, client_id, client_secret, {C_COL}, {U_COL}) \
             VALUES ('{provider_id}', '{org_a}', 'https://idp.example.test', 'client-a', '', 1012, 2012)"
        ),
        format!(
            "INSERT INTO oidc_former_providers \
             (org_id, issuer, client_id, client_secret, {C_COL}, expires_at) \
             VALUES ('{org_a}', 'https://former.example.test', 'former-c', '', 1013, 9000000000000)"
        ),
        format!(
            "INSERT INTO oidc_pending_providers \
             (org_id, provider_id, issuer, client_id, client_secret, {C_COL}) \
             VALUES ('{org_a}', '{provider_id}', 'https://next.example.test', 'next-c', '', 1014)"
        ),
        format!(
            "INSERT INTO oidc_held_sign_ins \
             (held_hash, org_id, provider_id, revision, issuer, subject, email, email_unverified, \
              name, preferred_username, {C_COL}, expires_at) \
             VALUES ('held-0075', '{org_a}', '{provider_id}', 1, 'https://idp.example.test', \
              'sub-held', 'held@example.com', false, 'Held', 'held', 1015, 9000000000000)"
        ),
        format!(
            "INSERT INTO oidc_login_flows \
             (state_hash, browser_hash, nonce, pkce_verifier, org_id, provider_id, {C_COL}, expires_at) \
             VALUES ('state-0075', 'browser-0075', 'nonce', 'pkce', '{org_a}', '{provider_id}', 1016, 9000000000000)"
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("oidc/org pre-0075 row seeded");
    }
    (signup_id, audit_id, provider_id, invitation_id)
}

async fn seed_containers_and_blocks(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
    targets: (&str, &str, &str, &str),
) {
    let (board_id, course_id, card_id, unit_id) = targets;
    let empty_bytes = match scoped_a.db_type() {
        DbType::Sqlite => "X'0102'",
        DbType::Postgres => "'\\x0102'::bytea",
    };
    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, {C_COL}, {U_COL}) \
             VALUES \
             ('{board_id}', '{ws_a}', '{alice}', '{alice}', 'open', 'board', 'Board A', '', '', \
              false, false, '{{}}', '[]', 1017, 2017), \
             ('{course_id}', '{ws_a}', '{alice}', '{alice}', 'private', 'course', 'Course A', '', '', \
              false, false, '{{}}', '[]', 1018, 2018)"
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) VALUES \
             ('{board_id}', '{alice}', '{}'), ('{course_id}', '{alice}', '{}')",
            ContainerRole::Admin.as_str(),
            ContainerRole::Admin.as_str(),
        ),
        format!(
            "INSERT INTO blocks \
             (id, container_id, parent_id, created_by, modified_by, \"schema\", type, title, \
              fields, {C_COL}, {U_COL}, number) VALUES \
             ('{card_id}', '{board_id}', NULL, '{alice}', '{alice}', 1, 'card', 'Card A', \
              '{{}}', 1019, 2019, 1), \
             ('{unit_id}', '{course_id}', NULL, '{alice}', '{alice}', 1, 'unit', 'Unit A', \
              '{{\"kind\":\"unit\",\"order\":\"V\"}}', 1020, 2020, NULL)"
        ),
        format!(
            "INSERT INTO body_docs \
             (block_id, state, state_seq, log_end, projected_seq, body_digest, epoch, \
              last_writer, {C_COL}, {U_COL}) \
             VALUES ('{card_id}', {empty_bytes}, 1, 1, 1, {empty_bytes}, 1, '{alice}', 1021, 2021)"
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("containers/blocks pre-0075 row seeded");
    }
}

async fn seed_sidebar_files_and_activity(
    scoped_a: &Store,
    org_a: &str,
    ws_a: &str,
    alice: &str,
    board_id: &str,
    card_id: &str,
) -> (String, String, String, String, String, String) {
    let (category_id, cat_container_id, file_id, upload_id, notif_id, time_entry_id) =
        (new_id(), new_id(), new_id(), new_id(), new_id(), new_id());
    for sql in [
        format!(
            "INSERT INTO categories (id, name, user_id, workspace_id, {C_COL}, {U_COL}, type) \
             VALUES ('{category_id}', 'Pinned', '{alice}', '{ws_a}', 1022, 2022, 'custom')"
        ),
        format!(
            "INSERT INTO category_containers \
             (id, user_id, category_id, container_id, {C_COL}, {U_COL}) \
             VALUES ('{cat_container_id}', '{alice}', '{category_id}', '{board_id}', 1023, 2023)"
        ),
        format!(
            "INSERT INTO files \
             (id, container_id, name, original_name, extension, size, path, {C_COL}, created_by) \
             VALUES ('{file_id}', '{board_id}', '7live.png', 'live.png', '.png', 64, \
              'boards/{board_id}/7live.png', 1024, '{alice}')"
        ),
        format!(
            "INSERT INTO loose_objects (path, state, {C_COL}, retry_at) \
             VALUES ('loose/0075.bin', 'writing', 1025, 2025)"
        ),
        format!(
            "INSERT INTO public_links (container_id, enabled, token, modified_by, {U_COL}) \
             VALUES ('{board_id}', true, 'pub-tok-0075', '{alice}', 2026)"
        ),
        format!(
            "INSERT INTO subscriptions (block_type, block_id, user_id, notified_at, {C_COL}) \
             VALUES ('card', '{card_id}', '{alice}', 900, 1027)"
        ),
        format!(
            "INSERT INTO uploads \
             (id, user_id, container_id, name, size, content_type, part_size, {C_COL}, \
              expires_at, closed_at, multipart_id) \
             VALUES ('{upload_id}', '{alice}', '{board_id}', 'up.bin', 8, 'application/octet-stream', \
              8, 1028, 9000000000000, 0, '')"
        ),
        format!(
            "INSERT INTO notifications \
             (id, org_id, recipient_id, kind, source_id, block_id, container_id, actor_id, \
              {C_COL}, {U_COL}) \
             VALUES ('{notif_id}', '{org_a}', '{alice}', 'mention', '{card_id}', '{card_id}', \
              '{board_id}', '{alice}', 1029, 2029)"
        ),
        format!(
            "INSERT INTO time_entries \
             (id, org_id, user_id, workspace_id, container_id, block_id, kind, started_at, \
              ended_at, note, {C_COL}, {U_COL}) \
             VALUES ('{time_entry_id}', '{org_a}', '{alice}', '{ws_a}', '{board_id}', \
              '{card_id}', 'work', 500, 800, 'focus', 1030, 2030)"
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("sidebar/files/activity pre-0075 row seeded");
    }
    (
        category_id,
        cat_container_id,
        file_id,
        upload_id,
        notif_id,
        time_entry_id,
    )
}

async fn seed_course_tables(
    scoped_a: &Store,
    org_a: &str,
    alice: &str,
    course_id: &str,
    unit_id: &str,
) -> (String, String, String, String) {
    let (note_id, review_id, meeting_id, exam_id) = (new_id(), new_id(), new_id(), new_id());
    for sql in [
        format!(
            "INSERT INTO course_schedules \
             (container_id, org_id, start_date, target_date, study_days, {U_COL}) \
             VALUES ('{course_id}', '{org_a}', '2026-11-01', '2026-11-30', 31, 2031)"
        ),
        format!(
            "INSERT INTO unit_progress \
             (unit_id, container_id, org_id, user_id, completed_at, {U_COL}) \
             VALUES ('{unit_id}', '{course_id}', '{org_a}', '{alice}', 1500, 2032)"
        ),
        format!(
            "INSERT INTO unit_notes \
             (id, org_id, unit_id, container_id, user_id, kind, content, {C_COL}, {U_COL}) \
             VALUES ('{note_id}', '{org_a}', '{unit_id}', '{course_id}', '{alice}', \
              'question', 'Why?', 1033, 2033)"
        ),
        format!(
            "INSERT INTO unit_reviews \
             (id, org_id, unit_id, container_id, user_id, scheduled_date, interval_days, \
              {C_COL}, {U_COL}) \
             VALUES ('{review_id}', '{org_a}', '{unit_id}', '{course_id}', '{alice}', \
              '2026-11-28', 3, 1034, 2034)"
        ),
        format!(
            "INSERT INTO course_meetings \
             (id, org_id, container_id, title, starts_at, {C_COL}, {U_COL}) \
             VALUES ('{meeting_id}', '{org_a}', '{course_id}', 'Week 1', 1700000000000, 1035, 2035)"
        ),
        format!(
            "INSERT INTO meeting_attendance \
             (meeting_id, org_id, user_id, status, {U_COL}) \
             VALUES ('{meeting_id}', '{org_a}', '{alice}', 'present', 2036)"
        ),
        format!(
            "INSERT INTO course_exams \
             (id, org_id, container_id, title, max_score, {C_COL}, {U_COL}) \
             VALUES ('{exam_id}', '{org_a}', '{course_id}', 'Midterm', 100, 1037, 2037)"
        ),
        format!(
            "INSERT INTO exam_scores \
             (exam_id, org_id, container_id, user_id, score, {U_COL}) \
             VALUES ('{exam_id}', '{org_a}', '{course_id}', '{alice}', 95, 2038)"
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("course pre-0075 row seeded");
    }
    (note_id, review_id, meeting_id, exam_id)
}

fn pre0075_file_json(file: &FileRecord) -> Value {
    json!({
        "id": file.id,
        "container_id": file.container_id,
        "stored_name": file.stored_name,
        "name": file.name,
        "extension": file.extension,
        "size": file.size,
        "path": file.path,
        (C_COL): file.created_at,
        "version": file.version,
        "created_by": file.created_by,
    })
}

async fn seed_restorable_trash(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
    board_id: &str,
) -> (String, FileRecord, FileRecord, String) {
    let (trashed_board_id, trashed_card_id) = (new_id(), new_id());
    let file_1 = FileRecord {
        id: new_id(),
        container_id: trashed_board_id.clone(),
        stored_name: "7f1.png".to_owned(),
        name: "f1.png".to_owned(),
        extension: ".png".to_owned(),
        size: 120,
        path: format!("boards/{trashed_board_id}/7f1.png"),
        created_at: 3002,
        version: 1,
        created_by: alice.to_owned(),
    };
    let file_2 = FileRecord {
        id: new_id(),
        container_id: trashed_board_id.clone(),
        stored_name: "7f2.pdf".to_owned(),
        name: "f2.pdf".to_owned(),
        extension: ".pdf".to_owned(),
        size: 240,
        path: format!("boards/{trashed_board_id}/7f2.pdf"),
        created_at: 3003,
        version: 2,
        created_by: alice.to_owned(),
    };
    let board_doc = json!({
        "id": trashed_board_id,
        "workspaceId": ws_a,
        "createdBy": alice,
        "modifiedBy": alice,
        "visibility": "open",
        "kind": "board",
        "title": "Restorable 0075 Board",
        "cardProperties": [],
        (C_KEY): 3001,
        (U_KEY): 4001,
        "deletedAt": 5001,
    })
    .to_string();
    let files_doc =
        Value::Array(vec![pre0075_file_json(&file_1), pre0075_file_json(&file_2)]).to_string();
    let members_doc =
        json!([{"containerId": trashed_board_id, "userId": alice, "role": "admin"}]).to_string();
    let card_doc = json!({
        "id": trashed_card_id,
        "containerId": board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "card",
        "title": "Restorable 0075 Card",
        "fields": {
            (C_KEY): "nested-must-stay",
            (U_KEY): "nested-must-stay",
            "watchedBy": [{(C_KEY): "nested-watcher-must-stay"}],
        },
        "watchedBy": [{"userId": alice, "blockType": "card", "notifiedAt": 50, (C_KEY): 3021}],
        (C_KEY): 3020,
        (U_KEY): 4020,
        "deletedAt": 5020,
        "number": 2,
    })
    .to_string();
    for sql in [
        format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board_id}', '{ws_a}', 5001, '{alice}', '{}', '{}', '{}', '{}', '[]')",
            new_id(),
            board_doc.replace('\'', "''"),
            members_doc.replace('\'', "''"),
            files_doc.replace('\'', "''"),
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{trashed_card_id}', '{board_id}', NULL, 5020, '{alice}', '{}', '{}')",
            new_id(),
            card_doc.replace('\'', "''"),
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("restorable trash pre-0075 seeded");
    }
    (trashed_board_id, file_1, file_2, trashed_card_id)
}

async fn seed_nul_trash(scoped_a: &Store, org_a: &str, alice: &str) -> (String, String) {
    let (trashed_nul_board_id, trashed_nul_card_id) = (new_id(), new_id());
    let ws_nul = common::pre0075_workspace(scoped_a, org_a).await;
    let nul_desc = if scoped_a.db_type() == DbType::Postgres {
        r"nul:\u0000 literal:\\u0000"
    } else {
        "plain nul fallback"
    };
    let nul_board_doc = format!(
        r#"{{"id":"{trashed_nul_board_id}","workspaceId":"{ws_nul}","createdBy":"{alice}","modifiedBy":"{alice}","visibility":"private","kind":"board","title":"NUL 0075 Board","description":"{nul_desc}","cardProperties":[],"{C_KEY}":3010,"{U_KEY}":4010,"deletedAt":5010}}"#
    );
    let nul_files_doc = format!(
        r#"[{{"id":"{}","container_id":"{trashed_nul_board_id}","stored_name":"7nul.txt","name":"{nul_desc}","extension":".txt","size":9,"path":"boards/{trashed_nul_board_id}/7nul.txt","{C_COL}":3011,"version":1,"created_by":"{alice}"}}]"#,
        new_id()
    );
    let nul_card_doc = format!(
        r#"{{"id":"{trashed_nul_card_id}","containerId":"{trashed_nul_board_id}","parentId":"","createdBy":"{alice}","modifiedBy":"{alice}","schema":1,"type":"card","title":"NUL 0075 Card","fields":{{"note":"{nul_desc}"}},"watchedBy":[{{"userId":"{alice}","blockType":"card","notifiedAt":60,"{C_KEY}":3031}}],"{C_KEY}":3030,"{U_KEY}":4030,"deletedAt":5030,"number":1}}"#
    );
    let del_nul = new_id();
    for sql in [
        format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_nul_board_id}', '{ws_nul}', 5010, '{alice}', '{del_nul}', '{}', '[]', '{}', '[]')",
            nul_board_doc.replace('\'', "''"),
            nul_files_doc.replace('\'', "''"),
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{trashed_nul_card_id}', '{trashed_nul_board_id}', NULL, 5030, '{alice}', '{del_nul}', '{}')",
            nul_card_doc.replace('\'', "''"),
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("nul trash pre-0075 seeded");
    }
    (trashed_nul_board_id, trashed_nul_card_id)
}

async fn seed_org_b(pre: &Store) -> Option<Pre0075OrgB> {
    if pre.db_type() != DbType::Postgres {
        return None;
    }
    let (org_id, ws_b) = common::pre0075_organization(pre, "beta-0075").await;
    let scoped = pre.for_org(&org_id).expect("org B handle");
    let bob = common::pre0072_user(&scoped, &org_id, &ws_b, "bob-0075").await;
    let (live_board_id, trashed_board_id, trashed_card_id) = (new_id(), new_id(), new_id());
    let trashed_file = FileRecord {
        id: new_id(),
        container_id: trashed_board_id.clone(),
        stored_name: "7bob.png".to_owned(),
        name: "bob.png".to_owned(),
        extension: ".png".to_owned(),
        size: 32,
        path: format!("boards/{trashed_board_id}/7bob.png"),
        created_at: 3091,
        version: 1,
        created_by: bob.clone(),
    };
    let board_doc = json!({
        "id": trashed_board_id,
        "workspaceId": ws_b,
        "createdBy": bob,
        "modifiedBy": bob,
        "visibility": "private",
        "kind": "board",
        "title": "Org B Trashed",
        "cardProperties": [],
        (C_KEY): 3090,
        (U_KEY): 4090,
        "deletedAt": 5090,
    })
    .to_string();
    let files_doc = Value::Array(vec![pre0075_file_json(&trashed_file)]).to_string();
    let members_doc =
        json!([{"containerId": trashed_board_id, "userId": bob, "role": "admin"}]).to_string();
    let card_doc = json!({
        "id": trashed_card_id,
        "containerId": live_board_id,
        "parentId": "",
        "createdBy": bob,
        "modifiedBy": bob,
        "schema": 1,
        "type": "card",
        "title": "Org B Card",
        "fields": {},
        "watchedBy": [{"userId": bob, "blockType": "card", "notifiedAt": 70, (C_KEY): 3093}],
        (C_KEY): 3092,
        (U_KEY): 4092,
        "deletedAt": 5092,
        "number": 1,
    })
    .to_string();
    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, {C_COL}, {U_COL}) \
             VALUES ('{live_board_id}', '{ws_b}', '{bob}', '{bob}', 'open', 'board', \
              'Org B Live', '', '', false, false, '{{}}', '[]', 1090, 2090)"
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) \
             VALUES ('{live_board_id}', '{bob}', 'admin')"
        ),
        format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board_id}', '{ws_b}', 5090, '{bob}', '{}', '{}', '{}', '{}', '[]')",
            new_id(),
            board_doc.replace('\'', "''"),
            members_doc.replace('\'', "''"),
            files_doc.replace('\'', "''"),
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{trashed_card_id}', '{live_board_id}', NULL, 5092, '{bob}', '{}', '{}')",
            new_id(),
            card_doc.replace('\'', "''"),
        ),
    ] {
        scoped
            .execute_unchecked(&sql)
            .await
            .expect("org B pre-0075 row seeded");
    }
    Some(Pre0075OrgB {
        org_id,
        bob,
        live_board_id,
        trashed_board_id,
        trashed_file,
        trashed_card_id,
    })
}

async fn seed_pre0075(pre: &Store) -> Pre0075Fixture {
    let (org_a, ws_a) = common::pre0075_organization(pre, "acme-0075").await;
    let scoped_a = pre.for_org(&org_a).expect("org A handle");
    let alice = common::pre0072_user(&scoped_a, &org_a, &ws_a, "alice-0075").await;
    let (signup_id, audit_id, provider_id, invitation_id) =
        seed_auth_and_oidc(pre, &scoped_a, &org_a, &ws_a, &alice).await;
    let (board_id, course_id, card_id, unit_id) = (new_id(), new_id(), new_id(), new_id());
    seed_containers_and_blocks(
        &scoped_a,
        &ws_a,
        &alice,
        (&board_id, &course_id, &card_id, &unit_id),
    )
    .await;
    let (category_id, cat_container_id, file_id, upload_id, notif_id, time_entry_id) =
        seed_sidebar_files_and_activity(&scoped_a, &org_a, &ws_a, &alice, &board_id, &card_id)
            .await;
    let (note_id, review_id, meeting_id, exam_id) =
        seed_course_tables(&scoped_a, &org_a, &alice, &course_id, &unit_id).await;
    let (trashed_board_id, trashed_file_1, trashed_file_2, trashed_card_id) =
        seed_restorable_trash(&scoped_a, &ws_a, &alice, &board_id).await;
    let (trashed_nul_board_id, trashed_nul_card_id) =
        seed_nul_trash(&scoped_a, &org_a, &alice).await;
    let org_b = seed_org_b(pre).await;

    Pre0075Fixture {
        org_a,
        ws_a,
        alice,
        signup_id,
        audit_id,
        provider_id,
        invitation_id,
        board_id,
        course_id,
        card_id,
        unit_id,
        category_id,
        cat_container_id,
        file_id,
        upload_id,
        notif_id,
        time_entry_id,
        note_id,
        review_id,
        meeting_id,
        exam_id,
        trashed_board_id,
        trashed_file_1,
        trashed_file_2,
        trashed_nul_board_id,
        trashed_card_id,
        trashed_nul_card_id,
        org_b,
    }
}

async fn assert_catalog_clean(database: &fvoci_store::testing::TestDatabase, sys: &Store) {
    match db_type() {
        DbType::Sqlite => {
            let mut conn =
                sqlx::sqlite::SqliteConnection::connect(&common::raw_connection(database))
                    .await
                    .expect("sqlite open");
            let fk: Vec<String> =
                sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
                    .fetch_all(&mut conn)
                    .await
                    .expect("fk check");
            assert!(fk.is_empty(), "broken SQLite foreign keys: {fk:?}");
            let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
                .fetch_all(&mut conn)
                .await
                .expect("integrity check");
            assert_eq!(integrity, ["ok"], "sqlite integrity check passes");
            conn.close().await.expect("close");

            let leftovers = sys
                .texts_unchecked(&format!(
                    "SELECT m.name || '.' || c.name \
                     FROM sqlite_master AS m, pragma_table_info(m.name) AS c \
                     WHERE m.type = 'table' AND c.name IN ('{C_COL}', '{U_COL}') \
                     UNION ALL \
                     SELECT name FROM sqlite_master \
                     WHERE name LIKE '%{C_COL}%' OR name LIKE '%{U_COL}%'"
                ))
                .await
                .expect("sqlite catalog check");
            assert!(leftovers.is_empty(), "SQLite leftovers: {leftovers:?}");
            let master_sql = sys
                .texts_unchecked(
                    "SELECT COALESCE(sql, '') FROM sqlite_master WHERE sql IS NOT NULL",
                )
                .await
                .expect("sqlite_master sql");
            for def in &master_sql {
                assert!(
                    !def.contains(C_COL) && !def.contains(U_COL),
                    "sqlite_master definition still mentions retired column: {def}"
                );
            }
            let idx = sys
                .texts_unchecked(
                    "SELECT name FROM sqlite_master WHERE name = 'blocks_parent_type_created_at_idx'",
                )
                .await
                .expect("blocks index");
            assert_eq!(
                idx,
                ["blocks_parent_type_created_at_idx"],
                "SQLite blocks index renamed to created_at"
            );
        }
        DbType::Postgres => {
            let leftovers = sys
                .texts_unchecked(&format!(
                    "SELECT table_name::text || '.' || column_name::text \
                     FROM information_schema.columns \
                     WHERE table_schema = current_schema() \
                       AND column_name IN ('{C_COL}', '{U_COL}') \
                     UNION ALL \
                     SELECT relname::text FROM pg_class \
                     WHERE relnamespace = current_schema()::regnamespace \
                       AND (relname LIKE '%{C_COL}%' OR relname LIKE '%{U_COL}%') \
                     UNION ALL \
                     SELECT conname::text FROM pg_constraint \
                     WHERE connamespace = current_schema()::regnamespace \
                       AND (conname LIKE '%{C_COL}%' OR conname LIKE '%{U_COL}%' \
                            OR pg_get_constraintdef(oid) LIKE '%{C_COL}%' \
                            OR pg_get_constraintdef(oid) LIKE '%{U_COL}%') \
                     UNION ALL \
                     SELECT proname::text FROM pg_proc \
                     WHERE pronamespace = current_schema()::regnamespace \
                       AND (prosrc LIKE '%{C_COL}%' OR prosrc LIKE '%{U_COL}%')"
                ))
                .await
                .expect("postgres catalog check");
            assert!(leftovers.is_empty(), "PostgreSQL leftovers: {leftovers:?}");
            let idx = sys
                .texts_unchecked(
                    "SELECT relname::text FROM pg_class \
                     WHERE relnamespace = current_schema()::regnamespace \
                       AND relname = 'blocks_parent_type_created_at_idx'",
                )
                .await
                .expect("pg blocks index");
            assert_eq!(
                idx,
                ["blocks_parent_type_created_at_idx"],
                "PostgreSQL blocks index renamed to created_at"
            );
        }
    }
}

fn identity_and_oidc_checks(fx: &Pre0075Fixture) -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "users",
            format!("SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM users WHERE id = '{}'", fx.alice),
            "1001:2001",
        ),
        (
            "user_identities",
            "SELECT CAST(created_at AS TEXT) FROM user_identities WHERE issuer = 'https://idp.example.test' AND subject = 'sub-alice'".to_owned(),
            "1002",
        ),
        (
            "email_changes",
            format!("SELECT CAST(created_at AS TEXT) FROM email_changes WHERE user_id = '{}'", fx.alice),
            "1003",
        ),
        (
            "email_reverts",
            "SELECT CAST(created_at AS TEXT) FROM email_reverts WHERE token_hash = 'rev-tok'".to_owned(),
            "1004",
        ),
        (
            "signups",
            format!("SELECT CAST(created_at AS TEXT) FROM signups WHERE id = '{}'", fx.signup_id),
            "1005",
        ),
        (
            "organizations",
            format!("SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM organizations WHERE id = '{}'", fx.org_a),
            "1006:2006",
        ),
        (
            "organization_members",
            format!("SELECT CAST(created_at AS TEXT) FROM organization_members WHERE org_id = '{}' AND user_id = '{}'", fx.org_a, fx.alice),
            "1007",
        ),
        (
            "workspaces",
            format!("SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM workspaces WHERE id = '{}'", fx.ws_a),
            "1008:2008",
        ),
        (
            "workspace_members",
            format!("SELECT CAST(created_at AS TEXT) FROM workspace_members WHERE workspace_id = '{}' AND user_id = '{}'", fx.ws_a, fx.alice),
            "1009",
        ),
        (
            "invitations",
            format!("SELECT CAST(created_at AS TEXT) FROM invitations WHERE id = '{}'", fx.invitation_id),
            "1010",
        ),
        (
            "audit_log",
            format!("SELECT CAST(created_at AS TEXT) FROM audit_log WHERE id = '{}'", fx.audit_id),
            "1011",
        ),
        (
            "oidc_providers",
            format!("SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM oidc_providers WHERE id = '{}'", fx.provider_id),
            "1012:2012",
        ),
        (
            "oidc_former_providers",
            format!("SELECT CAST(created_at AS TEXT) FROM oidc_former_providers WHERE org_id = '{}'", fx.org_a),
            "1013",
        ),
        (
            "oidc_pending_providers",
            format!("SELECT CAST(created_at AS TEXT) FROM oidc_pending_providers WHERE org_id = '{}'", fx.org_a),
            "1014",
        ),
        (
            "oidc_held_sign_ins",
            "SELECT CAST(created_at AS TEXT) FROM oidc_held_sign_ins WHERE held_hash = 'held-0075'".to_owned(),
            "1015",
        ),
        (
            "oidc_login_flows",
            "SELECT CAST(created_at AS TEXT) FROM oidc_login_flows WHERE state_hash = 'state-0075'".to_owned(),
            "1016",
        ),
    ]
}

fn content_checks(fx: &Pre0075Fixture) -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "containers",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM containers WHERE id = '{}'",
                fx.board_id
            ),
            "1017:2017",
        ),
        (
            "blocks",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM blocks WHERE id = '{}'",
                fx.card_id
            ),
            "1019:2019",
        ),
        (
            "body_docs",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM body_docs WHERE block_id = '{}'",
                fx.card_id
            ),
            "1021:2021",
        ),
        (
            "categories",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM categories WHERE id = '{}'",
                fx.category_id
            ),
            "1022:2022",
        ),
        (
            "category_containers",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM category_containers WHERE id = '{}'",
                fx.cat_container_id
            ),
            "1023:2023",
        ),
        (
            "files",
            format!(
                "SELECT CAST(created_at AS TEXT) FROM files WHERE id = '{}'",
                fx.file_id
            ),
            "1024",
        ),
        (
            "loose_objects",
            "SELECT CAST(created_at AS TEXT) FROM loose_objects WHERE path = 'loose/0075.bin'"
                .to_owned(),
            "1025",
        ),
        (
            "public_links",
            format!(
                "SELECT CAST(updated_at AS TEXT) FROM public_links WHERE container_id = '{}'",
                fx.board_id
            ),
            "2026",
        ),
        (
            "subscriptions",
            format!(
                "SELECT CAST(created_at AS TEXT) FROM subscriptions WHERE block_id = '{}' AND user_id = '{}'",
                fx.card_id, fx.alice
            ),
            "1027",
        ),
        (
            "uploads",
            format!(
                "SELECT CAST(created_at AS TEXT) FROM uploads WHERE id = '{}'",
                fx.upload_id
            ),
            "1028",
        ),
        (
            "notifications",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM notifications WHERE id = '{}'",
                fx.notif_id
            ),
            "1029:2029",
        ),
        (
            "time_entries",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM time_entries WHERE id = '{}'",
                fx.time_entry_id
            ),
            "1030:2030",
        ),
    ]
}

fn course_checks(fx: &Pre0075Fixture) -> Vec<(&'static str, String, &'static str)> {
    vec![
        (
            "course_schedules",
            format!(
                "SELECT CAST(updated_at AS TEXT) FROM course_schedules WHERE container_id = '{}'",
                fx.course_id
            ),
            "2031",
        ),
        (
            "unit_progress",
            format!(
                "SELECT CAST(updated_at AS TEXT) FROM unit_progress WHERE unit_id = '{}' AND user_id = '{}'",
                fx.unit_id, fx.alice
            ),
            "2032",
        ),
        (
            "unit_notes",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM unit_notes WHERE id = '{}'",
                fx.note_id
            ),
            "1033:2033",
        ),
        (
            "unit_reviews",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM unit_reviews WHERE id = '{}'",
                fx.review_id
            ),
            "1034:2034",
        ),
        (
            "course_meetings",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM course_meetings WHERE id = '{}'",
                fx.meeting_id
            ),
            "1035:2035",
        ),
        (
            "meeting_attendance",
            format!(
                "SELECT CAST(updated_at AS TEXT) FROM meeting_attendance WHERE meeting_id = '{}' AND user_id = '{}'",
                fx.meeting_id, fx.alice
            ),
            "2036",
        ),
        (
            "course_exams",
            format!(
                "SELECT CAST(created_at AS TEXT) || ':' || CAST(updated_at AS TEXT) FROM course_exams WHERE id = '{}'",
                fx.exam_id
            ),
            "1037:2037",
        ),
        (
            "exam_scores",
            format!(
                "SELECT CAST(updated_at AS TEXT) FROM exam_scores WHERE exam_id = '{}' AND user_id = '{}'",
                fx.exam_id, fx.alice
            ),
            "2038",
        ),
    ]
}

async fn assert_all_36_table_timestamps(sys: &Store, fx: &Pre0075Fixture) {
    let mut checks = identity_and_oidc_checks(fx);
    checks.extend(content_checks(fx));
    checks.extend(course_checks(fx));
    assert_eq!(checks.len(), 36, "every renamed SQL table is checked");
    for (table, query, expected) in checks {
        let got = sys
            .texts_unchecked(&query)
            .await
            .unwrap_or_else(|e| panic!("{table} query failed: {e}"));
        assert_eq!(got, [expected], "{table} timestamp preserved losslessly");
    }
}

async fn assert_nul_trash_snapshots(scoped_a: &Store, fx: &Pre0075Fixture) {
    let raw_nul = scoped_a
        .texts_unchecked(&format!(
            "SELECT CAST(container AS TEXT) || '|||' || CAST(files AS TEXT) \
             FROM deleted_containers WHERE id = '{}'",
            fx.trashed_nul_board_id
        ))
        .await
        .expect("nul deleted_containers");
    let (raw_nul_container, raw_nul_files) = raw_nul[0].split_once("|||").expect("split");
    assert!(
        !raw_nul_container.contains(C_KEY) && !raw_nul_container.contains(U_KEY),
        "deleted_containers.container stripped retired keys: {raw_nul_container}"
    );
    assert!(
        !raw_nul_files.contains(C_COL),
        "deleted_containers.files stripped retired key: {raw_nul_files}"
    );
    let parsed_container: Value =
        serde_json::from_str(raw_nul_container).expect("valid container json");
    let parsed_files: Value = serde_json::from_str(raw_nul_files).expect("valid files json");
    assert_eq!(
        (
            parsed_container["createdAt"].as_i64(),
            parsed_container["updatedAt"].as_i64()
        ),
        (Some(3010), Some(4010)),
        "trashed container JSON timestamps migrated"
    );
    assert_eq!(
        parsed_files[0]["created_at"].as_i64(),
        Some(3011),
        "trashed files JSON created_at migrated"
    );
    if db_type() == DbType::Postgres {
        assert!(
            raw_nul_container.contains(r"\u0000") && raw_nul_container.contains(r"\\u0000"),
            "PG NUL escape preserved in container: {raw_nul_container}"
        );
        assert!(
            raw_nul_files.contains(r"\u0000") && raw_nul_files.contains(r"\\u0000"),
            "PG NUL escape preserved in files: {raw_nul_files}"
        );
        assert_eq!(
            parsed_container["description"].as_str(),
            Some("nul:\0 literal:\\u0000"),
            "PG container NUL vs literal escape decoded"
        );
        assert_eq!(
            parsed_files[0]["name"].as_str(),
            Some("nul:\0 literal:\\u0000"),
            "PG file NUL vs literal escape decoded"
        );
    }

    let raw_nul_block = scoped_a
        .texts_unchecked(&format!(
            "SELECT CAST(block AS TEXT) FROM deleted_blocks WHERE id = '{}'",
            fx.trashed_nul_card_id
        ))
        .await
        .expect("nul deleted_blocks");
    assert!(
        !raw_nul_block[0].contains(C_KEY) && !raw_nul_block[0].contains(U_KEY),
        "deleted_blocks.block stripped retired keys: {}",
        raw_nul_block[0]
    );
    let parsed_block: Value = serde_json::from_str(&raw_nul_block[0]).expect("valid block json");
    assert_eq!(
        (
            parsed_block["createdAt"].as_i64(),
            parsed_block["updatedAt"].as_i64(),
            parsed_block["watchedBy"][0]["createdAt"].as_i64(),
        ),
        (Some(3030), Some(4030), Some(3031)),
        "trashed block and watchedBy JSON timestamps migrated"
    );
    if db_type() == DbType::Postgres {
        assert!(
            raw_nul_block[0].contains(r"\u0000") && raw_nul_block[0].contains(r"\\u0000"),
            "PG NUL escape preserved in block: {}",
            raw_nul_block[0]
        );
        assert_eq!(
            parsed_block["fields"]["note"].as_str(),
            Some("nul:\0 literal:\\u0000"),
            "PG block NUL vs literal escape decoded"
        );
    }
}

async fn assert_trash_restore(scoped_a: &Store, fx: &Pre0075Fixture) {
    let restored_board = scoped_a
        .undelete_container(&fx.trashed_board_id, &fx.alice)
        .await
        .expect("undelete migrated board");
    assert_eq!(
        restored_board.created_at, 3001,
        "restored board preserves created_at"
    );
    assert_eq!(
        scoped_a
            .get_file_record(&fx.trashed_board_id, &fx.trashed_file_1.stored_name)
            .await
            .expect("restored file 1"),
        fx.trashed_file_1,
        "restored file 1 preserves created_at"
    );
    assert_eq!(
        scoped_a
            .get_file_record(&fx.trashed_board_id, &fx.trashed_file_2.stored_name)
            .await
            .expect("restored file 2"),
        fx.trashed_file_2,
        "restored file 2 preserves created_at"
    );

    let restored_card = scoped_a
        .undelete_block(&fx.trashed_card_id, &fx.alice)
        .await
        .expect("undelete migrated card");
    assert_eq!(
        restored_card.created_at, 3020,
        "restored card preserves created_at"
    );
    assert_eq!(
        restored_card.fields.get(C_KEY),
        Some(&json!("nested-must-stay")),
        "nested fields key untouched"
    );
    assert_eq!(
        restored_card.fields.get(U_KEY),
        Some(&json!("nested-must-stay")),
        "nested fields key untouched"
    );
    assert_eq!(
        restored_card.fields.get("watchedBy"),
        Some(&json!([{(C_KEY): "nested-watcher-must-stay"}])),
        "nested fields.watchedBy untouched"
    );
    assert_eq!(
        scoped_a
            .get_subscription(&fx.trashed_card_id, &fx.alice)
            .await
            .expect("restored card watcher"),
        Subscription {
            block_type: BlockType::Card,
            block_id: fx.trashed_card_id.clone(),
            user_id: fx.alice.clone(),
            notified_at: 50,
            created_at: 3021,
            deleted_at: None,
        },
        "restored watcher preserves created_at"
    );
}

async fn assert_audit_triggers_and_org_b(post: &Store, scoped_a: &Store, fx: &Pre0075Fixture) {
    assert!(
        scoped_a
            .execute_unchecked(&format!(
                "UPDATE audit_log SET created_at = 0 WHERE id = '{}'",
                fx.audit_id
            ))
            .await
            .is_err(),
        "audit_log immutability rejects created_at mutation"
    );
    assert!(
        scoped_a
            .execute_unchecked(&format!(
                "UPDATE audit_log SET identity = NULL WHERE id = '{}'",
                fx.audit_id
            ))
            .await
            .is_err(),
        "audit_log identity clearing rejected before erase_started_at > 0"
    );
    post.system()
        .execute_unchecked(&format!(
            "UPDATE users SET deleted_at = 1, erase_started_at = 1 WHERE id = '{}'",
            fx.alice
        ))
        .await
        .expect("mark alice erasing");
    scoped_a
        .execute_unchecked(&format!(
            "UPDATE audit_log SET identity = NULL WHERE id = '{}'",
            fx.audit_id
        ))
        .await
        .expect("audit_log_is_kept / audit_log_unchanged permits identity erasure post-0075");

    if let Some(b) = &fx.org_b {
        assert!(
            scoped_a.get_container(&b.live_board_id).await.is_err(),
            "RLS hides org B board from org A"
        );
        let scoped_b = post.for_org(&b.org_id).expect("org B handle");
        let live_b = scoped_b
            .get_container(&b.live_board_id)
            .await
            .expect("org B live board");
        assert_eq!(
            (live_b.created_at, live_b.updated_at),
            (1090, 2090),
            "org B live board timestamps preserved"
        );
        let restored_b = scoped_b
            .undelete_container(&b.trashed_board_id, &b.bob)
            .await
            .expect("org B undelete board");
        assert_eq!(
            restored_b.created_at, 3090,
            "org B restored board created_at preserved"
        );
        assert_eq!(
            scoped_b
                .get_file_record(&b.trashed_board_id, &b.trashed_file.stored_name)
                .await
                .expect("org B restored file"),
            b.trashed_file,
            "org B restored file preserved"
        );
        let restored_card_b = scoped_b
            .undelete_block(&b.trashed_card_id, &b.bob)
            .await
            .expect("org B undelete card");
        assert_eq!(
            restored_card_b.created_at, 3092,
            "org B restored card created_at preserved"
        );
        assert_eq!(
            scoped_b
                .get_subscription(&b.trashed_card_id, &b.bob)
                .await
                .expect("org B restored watcher")
                .created_at,
            3093,
            "org B restored watcher created_at preserved"
        );
    }
}

#[tokio::test]
async fn migration_0075_upgrades_populated_database_losslessly() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_unified_timestamps(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("opens at migration 74");
    let fx = seed_pre0075(&pre).await;
    pre.close().await;

    let post = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrates 0074 -> 0075 cleanly");
    let sys = post.system();
    let scoped_a = post.for_org(&fx.org_a).expect("org A handle");

    assert_catalog_clean(&database, &sys).await;
    assert_all_36_table_timestamps(&sys, &fx).await;
    assert_nul_trash_snapshots(&scoped_a, &fx).await;
    assert_trash_restore(&scoped_a, &fx).await;
    assert_audit_triggers_and_org_b(&post, &scoped_a, &fx).await;

    post.close().await;
    database.remove().await;
}
