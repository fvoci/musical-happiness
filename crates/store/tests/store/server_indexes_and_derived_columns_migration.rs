//! Migration `0077` (`server_indexes_and_derived_columns`) exercised on a populated
//! SQLite and PostgreSQL database seeded at version `0076`:
//! - Backfilling `blocks.body_first_image` in document order (`get_card_previews` reads
//!   the derived column without loading `body`).
//! - Backfilling `deleted_blocks.block_type` (including PostgreSQL `\u0000` escapes).
//! - Backfilling `users.email_verified_at` (`created_at` for active non-empty email, `0`
//!   for empty-email OIDC accounts and erased accounts).
//! - Aligning PostgreSQL `containers.visibility` column default to `'private'` to match
//!   SQLite (`0070`).
//! - Dropping redundant indexes (`blocks_parent_id_idx`, `deleted_blocks_container_id_idx`)
//!   and adding composite/partial/author/erasure indexes.
//! - Enforcing the 6 course child table integrity triggers (`unit_progress`, `unit_reviews`,
//!   `unit_notes`, `course_meetings`, `course_exams`, `exam_scores`) against live and
//!   trashed courses.

use fvoci_model::{BlockType, ContainerRole, new_id};
use fvoci_store::{DbType, Store, StoreError};
use serde_json::json;

use crate::common::{self, db_type};

struct Pre0077Fixture {
    org_id: String,
    ws_id: String,
    verified_user: String,
    oidc_user: String,
    erased_user: String,
    board_id: String,
    live_course_id: String,
    trashed_course_id: String,
    image_card_id: String,
    plain_card_id: String,
    trashed_card_id: String,
    trashed_comment_id: String,
    trashed_nul_page_id: String,
}

async fn seed_pre0077(pre: &Store) -> Pre0077Fixture {
    let sys = pre.system();
    let (org_id, ws_id) = (new_id(), new_id());
    let (verified_user, oidc_user, erased_user) = (new_id(), new_id(), new_id());
    for sql in [
        format!(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at, is_admin, display_name) \
             VALUES ('{verified_user}', 'verified0077', 'v@example.com', 'hash', 1234, 1234, true, '')"
        ),
        format!(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at, is_admin, display_name) \
             VALUES ('{oidc_user}', 'oidc0077', '', '', 2345, 2345, false, '')"
        ),
        format!(
            "INSERT INTO users (id, username, email, password_hash, created_at, updated_at, deleted_at, erase_started_at, erased_at, is_admin, display_name) \
             VALUES ('{erased_user}', '', 'ghost@example.com', '', 3456, 4000, 3900, 3900, 4000, false, '')"
        ),
        format!(
            "INSERT INTO organizations (id, name, kind, settings, created_at, updated_at) \
             VALUES ('{org_id}', 'Org 0077', 'shared', '{{}}', 100, 200)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{ws_id}', '{org_id}', 'Main', '{{}}', 100, 200)"
        ),
        format!("UPDATE organizations SET default_workspace_id = '{ws_id}' WHERE id = '{org_id}'"),
        format!(
            "INSERT INTO organization_members (org_id, user_id, role, created_at) \
             VALUES ('{org_id}', '{verified_user}', 'admin', 100)"
        ),
    ] {
        sys.execute_unchecked(&sql)
            .await
            .expect("seed pre-0077 global");
    }

    let fx = Pre0077Fixture {
        org_id,
        ws_id,
        verified_user,
        oidc_user,
        erased_user,
        board_id: new_id(),
        live_course_id: new_id(),
        trashed_course_id: new_id(),
        image_card_id: new_id(),
        plain_card_id: new_id(),
        trashed_card_id: new_id(),
        trashed_comment_id: new_id(),
        trashed_nul_page_id: new_id(),
    };
    seed_pre0077_org_rows(pre, &fx).await;
    fx
}

async fn seed_pre0077_org_rows(pre: &Store, fx: &Pre0077Fixture) {
    let scoped = pre.for_org(&fx.org_id).expect("org handle");
    let (board_id, live_course_id, trashed_course_id) =
        (&fx.board_id, &fx.live_course_id, &fx.trashed_course_id);
    let (image_card_id, plain_card_id) = (&fx.image_card_id, &fx.plain_card_id);
    let (trashed_card_id, trashed_comment_id, trashed_nul_page_id) = (
        &fx.trashed_card_id,
        &fx.trashed_comment_id,
        &fx.trashed_nul_page_id,
    );
    let (ws_id, verified_user) = (&fx.ws_id, &fx.verified_user);

    let nested_body = json!({
        "type": "doc",
        "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "intro"}]},
            {"type": "blockquote", "content": [
                {"type": "image", "attrs": {"fileId": "7first.png", "alt": "first"}}
            ]},
            {"type": "image", "attrs": {"fileId": "7second.png", "alt": "second"}}
        ]
    })
    .to_string()
    .replace('\'', "''");
    let plain_body = json!({
        "type": "doc",
        "content": [{"type": "paragraph", "content": [{"type": "text", "text": "no image"}]}]
    })
    .to_string()
    .replace('\'', "''");
    let (body_cast, trash_cast) = if pre.db_type() == DbType::Postgres {
        ("::jsonb", "::json")
    } else {
        ("", "")
    };
    let (trashed_card_doc, trashed_comment_doc, trashed_nul_doc, trashed_course_doc) =
        build_pre0077_trashed_docs(pre.db_type(), fx);

    for sql in [
        format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
             VALUES ('{ws_id}', '{verified_user}', 'admin', 100)"
        ),
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, created_at, updated_at) VALUES \
             ('{board_id}', '{ws_id}', '{verified_user}', '{verified_user}', 'open', 'board', 'Board 0077', '', '', false, false, '{{}}', '[]', 1000, 1000), \
             ('{live_course_id}', '{ws_id}', '{verified_user}', '{verified_user}', 'private', 'course', 'Course 0077', '', '', false, false, '{{}}', '[]', 1000, 1000)"
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) VALUES \
             ('{board_id}', '{verified_user}', '{}'), \
             ('{live_course_id}', '{verified_user}', '{}')",
            ContainerRole::Admin.as_str(),
            ContainerRole::Admin.as_str()
        ),
        format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_course_id}', '{ws_id}', 2500, '{verified_user}', '{}', '{trashed_course_doc}'{trash_cast}, '[]'{trash_cast}, '[]'{trash_cast}, '[]'{trash_cast})",
            new_id()
        ),
        format!(
            "INSERT INTO files \
             (id, container_id, name, original_name, extension, size, path, created_at, version, created_by) VALUES \
             ('{}', '{board_id}', '7first.png', 'first.png', 'png', 10, 'p/7first.png', 1000, 1, '{verified_user}'), \
             ('{}', '{board_id}', '7second.png', 'second.png', 'png', 10, 'p/7second.png', 1000, 1, '{verified_user}')",
            new_id(),
            new_id()
        ),
        format!(
            "INSERT INTO blocks \
             (id, container_id, parent_id, created_by, modified_by, type, title, fields, body, created_at, updated_at, number) VALUES \
             ('{image_card_id}', '{board_id}', NULL, '{verified_user}', '{verified_user}', 'card', 'With Images', '{{}}'{body_cast}, '{nested_body}'{body_cast}, 1010, 1010, 1), \
             ('{plain_card_id}', '{board_id}', NULL, '{verified_user}', '{verified_user}', 'card', 'Plain Card', '{{}}'{body_cast}, '{plain_body}'{body_cast}, 1020, 1020, 3)"
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) VALUES \
             ('{trashed_card_id}', '{board_id}', NULL, 2000, '{verified_user}', '{}', '{trashed_card_doc}'{trash_cast}), \
             ('{trashed_comment_id}', '{board_id}', '{image_card_id}', 2001, '{verified_user}', '{}', '{trashed_comment_doc}'{trash_cast}), \
             ('{trashed_nul_page_id}', '{board_id}', NULL, 2002, '{verified_user}', '{}', '{trashed_nul_doc}'{trash_cast})",
            new_id(),
            new_id(),
            new_id()
        ),
    ] {
        scoped
            .execute_unchecked(&sql)
            .await
            .expect("seed pre-0077 org");
    }
    seed_pre0077_sqlite_orphan(pre.db_type(), &scoped, verified_user).await;
}

async fn seed_pre0077_sqlite_orphan(db: DbType, scoped: &Store, verified_user: &str) {
    if db == DbType::Sqlite {
        let orphan_course = new_id();
        let orphan_unit = new_id();
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO unit_progress \
                 (container_id, unit_id, user_id, org_id, completed_at, last_position, updated_at) \
                 VALUES ('{orphan_course}', '{orphan_unit}', '{verified_user}', '', 1000, '', 1000)"
            ))
            .await
            .expect("seed pre-0077 sqlite orphan unit_progress");
    }
}

fn build_pre0077_trashed_docs(db: DbType, fx: &Pre0077Fixture) -> (String, String, String, String) {
    let trashed_card_doc = json!({
        "id": fx.trashed_card_id,
        "containerId": fx.board_id,
        "parentId": "",
        "createdBy": fx.verified_user,
        "modifiedBy": fx.verified_user,
        "type": "card",
        "title": "Trashed Card",
        "fields": {},
        "createdAt": 1000,
        "updatedAt": 1000,
        "deletedAt": 2000,
        "number": 2
    })
    .to_string()
    .replace('\'', "''");
    let trashed_comment_doc = json!({
        "id": fx.trashed_comment_id,
        "containerId": fx.board_id,
        "parentId": fx.image_card_id,
        "createdBy": fx.verified_user,
        "modifiedBy": fx.verified_user,
        "type": "comment",
        "title": "",
        "fields": {},
        "createdAt": 1000,
        "updatedAt": 1000,
        "deletedAt": 2001
    })
    .to_string()
    .replace('\'', "''");
    let nul_text = if db == DbType::Postgres {
        r"nul:\u0000"
    } else {
        "plain"
    };
    let trashed_nul_doc = format!(
        r#"{{"id":"{}","containerId":"{}","parentId":"","createdBy":"{}","modifiedBy":"{}","type":"page","title":"{nul_text}","fields":{{}},"createdAt":1000,"updatedAt":1000,"deletedAt":2002}}"#,
        fx.trashed_nul_page_id, fx.board_id, fx.verified_user, fx.verified_user
    )
    .replace('\'', "''");
    let trashed_course_doc = json!({
        "id": fx.trashed_course_id,
        "workspaceId": fx.ws_id,
        "createdBy": fx.verified_user,
        "modifiedBy": fx.verified_user,
        "visibility": "private",
        "kind": "course",
        "title": "Trashed Course",
        "createdAt": 1000,
        "updatedAt": 1000,
        "deletedAt": 2500
    })
    .to_string()
    .replace('\'', "''");
    (
        trashed_card_doc,
        trashed_comment_doc,
        trashed_nul_doc,
        trashed_course_doc,
    )
}

#[tokio::test]
async fn migration_0077_backfills_derived_columns_aligns_visibility_default_and_guards_course_children()
 {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_server_indexes_and_derived_columns(&database).await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open at 0076");
    let fx = seed_pre0077(&pre).await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migration 0077 applies cleanly");
    let sys = migrated.system();
    let scoped = migrated.for_org(&fx.org_id).expect("org handle");
    let _ = &fx.ws_id;

    // 1. `blocks.body_first_image` backfilled & `get_listed_blocks(..., Previews::With)` returns first image without body.
    let previews = scoped
        .get_listed_blocks(
            &fvoci_store::BlockFilter::container(&fx.board_id).with_type(BlockType::Card),
            fvoci_store::Previews::With,
        )
        .await
        .expect("previews");
    assert_eq!(previews.len(), 2, "two cards returned");
    let img_prev = previews
        .iter()
        .find(|p| p.block.id == fx.image_card_id)
        .expect("image card preview");
    assert_eq!(
        img_prev
            .preview
            .as_ref()
            .and_then(|p| p.first_image.as_deref()),
        Some("7first.png"),
        "document-order first image backfilled"
    );
    assert!(
        img_prev.block.body.is_none(),
        "preview must not load 8 MiB body"
    );
    let plain_prev = previews
        .iter()
        .find(|p| p.block.id == fx.plain_card_id)
        .expect("plain card preview");
    assert_eq!(
        plain_prev
            .preview
            .as_ref()
            .and_then(|p| p.first_image.as_deref()),
        None,
        "card without image has None first_image"
    );

    // 2. `deleted_blocks.block_type` backfilled across normal and `\u0000` rows.
    for (id, expected) in [
        (&fx.trashed_card_id, BlockType::Card.as_str()),
        (&fx.trashed_comment_id, BlockType::Comment.as_str()),
        (&fx.trashed_nul_page_id, BlockType::Page.as_str()),
    ] {
        let kinds = scoped
            .texts_unchecked(&format!(
                "SELECT block_type FROM deleted_blocks WHERE id = '{id}'"
            ))
            .await
            .expect("block_type read");
        assert_eq!(kinds, [expected], "backfilled block_type for {id}");
    }

    // 3. `users.email_verified_at` backfilled accurately.
    let verified_vals = sys
        .texts_unchecked(&format!(
            "SELECT CAST(email_verified_at AS TEXT) FROM users WHERE id IN ('{}', '{}', '{}') ORDER BY created_at",
            fx.verified_user, fx.oidc_user, fx.erased_user
        ))
        .await
        .expect("email_verified_at read");
    assert_eq!(
        verified_vals,
        ["1234", "0", "0"],
        "email_verified_at backfilled for active vs OIDC/erased accounts"
    );

    // 4. `containers.visibility` default parity (`'private'`) and index checks.
    assert_visibility_default_and_indexes(&sys).await;

    // 5. Course child integrity triggers allow live & trashed courses and reject boards/missing,
    // and SQLite orphan rows with `org_id = ''` were removed during 0077.
    if migrated.db_type() == DbType::Sqlite {
        let orphans = scoped
            .texts_unchecked("SELECT container_id FROM unit_progress WHERE org_id = ''")
            .await
            .expect("check sqlite orphan cleanup");
        assert!(orphans.is_empty(), "0077 must delete orphan org_id='' rows");
    }
    assert_course_child_triggers(&scoped, &fx).await;

    migrated.close().await;
    database.remove().await;
}

async fn assert_visibility_default_and_indexes(sys: &Store) {
    let default_sql = match db_type() {
        DbType::Sqlite => {
            "SELECT dflt_value FROM pragma_table_info('containers') WHERE name = 'visibility'"
        }
        DbType::Postgres => {
            "SELECT column_default::text FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = 'containers' AND column_name = 'visibility'"
        }
    };
    let defaults = sys
        .texts_unchecked(default_sql)
        .await
        .expect("containers.visibility default");
    assert_eq!(defaults.len(), 1, "expected 1 visibility default row");
    assert!(
        defaults[0] == "'private'" || defaults[0] == "'private'::text",
        "containers.visibility default must be 'private', got {:?}",
        defaults[0]
    );

    let index_sql = match db_type() {
        DbType::Sqlite => "SELECT name FROM sqlite_master WHERE type = 'index'",
        DbType::Postgres => {
            "SELECT indexname::text FROM pg_indexes WHERE schemaname = current_schema()"
        }
    };
    let indexes = sys.texts_unchecked(index_sql).await.expect("indexes");
    for present in [
        "blocks_container_type_created_at_idx",
        "blocks_container_titled_updated_at_idx",
        "deleted_blocks_container_deleted_at_idx",
        "deleted_blocks_container_type_deleted_at_idx",
        "comment_versions_written_by_idx",
        "body_version_editors_user_id_idx",
        "files_created_by_idx",
        "file_versions_created_by_idx",
        "containers_created_by_idx",
        "invitations_created_by_idx",
        "audit_log_actor_identity_idx",
        "audit_log_subject_identity_idx",
    ] {
        assert!(
            indexes.iter().any(|idx| idx == present),
            "missing index {present}: {indexes:?}"
        );
    }
    for dropped in ["blocks_parent_id_idx", "deleted_blocks_container_id_idx"] {
        assert!(
            !indexes.iter().any(|idx| idx == dropped),
            "redundant index {dropped} must be dropped"
        );
    }
}

async fn assert_course_child_triggers(scoped: &Store, fx: &Pre0077Fixture) {
    let unit_id = new_id();
    let exam_id = new_id();
    let trashed_exam_id = new_id();
    let json_cast = if scoped.db_type() == DbType::Postgres {
        "::jsonb"
    } else {
        ""
    };
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO blocks (id, container_id, created_by, modified_by, type, title, fields, created_at, updated_at) \
             VALUES ('{unit_id}', '{}', '{}', '{}', 'unit', 'U1', '{{}}'{json_cast}, 1000, 1000)",
            fx.live_course_id, fx.verified_user, fx.verified_user
        ))
        .await
        .expect("unit on live course");

    for course_id in [&fx.live_course_id, &fx.trashed_course_id] {
        let ex_id = if course_id == &fx.live_course_id {
            &exam_id
        } else {
            &trashed_exam_id
        };
        for sql in [
            format!(
                "INSERT INTO unit_progress (container_id, unit_id, user_id, org_id, completed_at, last_position, updated_at) \
                 VALUES ('{course_id}', '{}', '{}', '{}', 1000, '', 1000)",
                new_id(),
                fx.verified_user,
                fx.org_id
            ),
            format!(
                "INSERT INTO unit_reviews (id, container_id, unit_id, user_id, org_id, scheduled_date, interval_days, created_at, updated_at) \
                 VALUES ('{}', '{course_id}', '{unit_id}', '{}', '{}', '2026-11-26', 1, 1000, 1000)",
                new_id(),
                fx.verified_user,
                fx.org_id
            ),
            format!(
                "INSERT INTO unit_notes (id, container_id, unit_id, user_id, org_id, kind, content, created_at, updated_at) \
                 VALUES ('{}', '{course_id}', '{unit_id}', '{}', '{}', 'review', 'ok', 1000, 1000)",
                new_id(),
                fx.verified_user,
                fx.org_id
            ),
            format!(
                "INSERT INTO course_meetings (id, container_id, org_id, title, starts_at, location, unit_ids, notes, created_at, updated_at) \
                 VALUES ('{}', '{course_id}', '{}', 'M1', 1000, '', '[]'{json_cast}, '', 1000, 1000)",
                new_id(),
                fx.org_id
            ),
            format!(
                "INSERT INTO course_exams (id, container_id, org_id, title, max_score, description, created_at, updated_at) \
                 VALUES ('{ex_id}', '{course_id}', '{}', 'E1', 100.0, '', 1000, 1000)",
                fx.org_id
            ),
            format!(
                "INSERT INTO exam_scores (exam_id, container_id, user_id, org_id, score, note, updated_at) \
                 VALUES ('{ex_id}', '{course_id}', '{}', '{}', 95.0, '', 1000)",
                fx.verified_user, fx.org_id
            ),
        ] {
            scoped
                .execute_unchecked(&sql)
                .await
                .unwrap_or_else(|e| panic!("valid course child insert on {course_id} failed: {e}"));
        }
    }

    assert_course_child_trigger_rejections(scoped, fx, &unit_id, &exam_id, json_cast).await;
}

async fn assert_course_child_trigger_rejections(
    scoped: &Store,
    fx: &Pre0077Fixture,
    unit_id: &str,
    exam_id: &str,
    json_cast: &str,
) {
    // Pointing any of the 6 tables at a `board` container or a missing ID must fail in DB triggers.
    for bad_container in [&fx.board_id, &new_id()] {
        for (table, sql) in [
            (
                "unit_progress",
                format!(
                    "INSERT INTO unit_progress (container_id, unit_id, user_id, org_id, last_position, updated_at) \
                     VALUES ('{bad_container}', '{}', '{}', '{}', '', 1000)",
                    new_id(),
                    fx.verified_user,
                    fx.org_id
                ),
            ),
            (
                "unit_reviews",
                format!(
                    "INSERT INTO unit_reviews (id, container_id, unit_id, user_id, org_id, scheduled_date, interval_days, created_at, updated_at) \
                     VALUES ('{}', '{bad_container}', '{unit_id}', '{}', '{}', '2026-11-26', 1, 1000, 1000)",
                    new_id(),
                    fx.verified_user,
                    fx.org_id
                ),
            ),
            (
                "unit_notes",
                format!(
                    "INSERT INTO unit_notes (id, container_id, unit_id, user_id, org_id, kind, content, created_at, updated_at) \
                     VALUES ('{}', '{bad_container}', '{unit_id}', '{}', '{}', 'review', 'bad', 1000, 1000)",
                    new_id(),
                    fx.verified_user,
                    fx.org_id
                ),
            ),
            (
                "course_meetings",
                format!(
                    "INSERT INTO course_meetings (id, container_id, org_id, title, starts_at, location, unit_ids, notes, created_at, updated_at) \
                     VALUES ('{}', '{bad_container}', '{}', 'Bad', 1000, '', '[]'{json_cast}, '', 1000, 1000)",
                    new_id(),
                    fx.org_id
                ),
            ),
            (
                "course_exams",
                format!(
                    "INSERT INTO course_exams (id, container_id, org_id, title, max_score, description, created_at, updated_at) \
                     VALUES ('{}', '{bad_container}', '{}', 'Bad', 100.0, '', 1000, 1000)",
                    new_id(),
                    fx.org_id
                ),
            ),
            (
                "exam_scores",
                format!(
                    "INSERT INTO exam_scores (exam_id, container_id, user_id, org_id, score, note, updated_at) \
                     VALUES ('{exam_id}', '{bad_container}', '{}', '{}', 50.0, '', 1000)",
                    new_id(),
                    fx.org_id
                ),
            ),
        ] {
            let err = scoped.execute_unchecked(&sql).await;
            assert!(
                matches!(err, Err(StoreError::Database(_))),
                "{table} must reject non-course container {bad_container}: {err:?}"
            );
        }
    }

    // Updating `container_id` to a board is also rejected by the UPDATE triggers.
    for table in [
        "unit_progress",
        "unit_reviews",
        "unit_notes",
        "course_meetings",
        "course_exams",
        "exam_scores",
    ] {
        let err = scoped
            .execute_unchecked(&format!(
                "UPDATE {table} SET container_id = '{}' WHERE container_id = '{}'",
                fx.board_id, fx.live_course_id
            ))
            .await;
        assert!(
            matches!(err, Err(StoreError::Database(_))),
            "{table} UPDATE to non-course container must fail: {err:?}"
        );
    }
}
