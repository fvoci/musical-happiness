//! Migration `0076` (`user_settings_client_state_and_block_cleanup`) exercised on a
//! populated SQLite and PostgreSQL database seeded at version `0075`:
//! - Splitting `preferences` into `user_settings` (14 static columns) and
//!   `user_client_state` (`(user_id, key)`), then dropping `preferences`.
//! - Dropping `blocks."schema"` and stripping top-level `"schema"` from
//!   `deleted_blocks.block` JSON snapshots (preserving nested `"schema"` fields and
//!   PostgreSQL `\u0000` / `\\u0000` escapes).
//! - Renaming `organizations.kind` from the retired value to `'shared'`, updating the
//!   column default, `CHECK` constraints, partial index `organizations_of_people_idx`,
//!   and SQLite triggers.

use std::collections::BTreeMap;

use fvoci_model::{
    BlockType, CardOpen, ClientStatePatch, ContainerRole, Density, HourCycle, OrgKind,
    Subscription, Theme, UserSettings, UserSettingsPatch, WeekStart, new_id,
};
use fvoci_store::{DbType, SYSTEM_ORG_ID, Store};
use serde_json::{Value, json};
use sqlx::Connection as _;

use crate::common::{self, db_type};

const RETIRED_ORG_KIND: &str = concat!("te", "am");

struct Pre0076OrgB {
    org_id: String,
    bob_own_org: String,
    live_card_id: String,
    trashed_card_id: String,
}

struct Pre0076Fixture {
    org_a: String,
    ws_a: String,
    alice: String,
    alice_own_org: String,
    bob: String,
    carol: String,
    dave: String,
    board_id: String,
    card_id: String,
    comment_id: String,
    trashed_card_id: String,
    trashed_nul_card_id: String,
    org_b: Option<Pre0076OrgB>,
}

async fn seed_user(sys: &Store, username: &str) -> String {
    let id = new_id();
    sys.execute_unchecked(&format!(
        "INSERT INTO users (id, username, email, password_hash, created_at, updated_at, is_admin, display_name) \
         VALUES ('{id}', '{username}', '{username}@example.com', 'hash', 100, 100, false, '')"
    ))
    .await
    .expect("pre-0076 user seeded");
    id
}

async fn seed_personal_space(pre: &Store, user_id: &str) -> String {
    let sys = pre.system();
    let (own_org, own_ws) = (new_id(), new_id());
    for sql in [
        format!(
            "INSERT INTO organizations (id, name, kind, owner_id, settings, created_at, updated_at) \
             VALUES ('{own_org}', '', 'personal', '{user_id}', '{{}}', 110, 110)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{own_ws}', '{own_org}', '', '{{}}', 110, 110)"
        ),
        format!(
            "UPDATE organizations SET default_workspace_id = '{own_ws}' WHERE id = '{own_org}'"
        ),
        format!(
            "INSERT INTO organization_members (org_id, user_id, role, created_at) \
             VALUES ('{own_org}', '{user_id}', 'member', 110)"
        ),
    ] {
        sys.execute_unchecked(&sql)
            .await
            .expect("personal space org seeded");
    }
    pre.for_org(&own_org)
        .expect("own org handle")
        .execute_unchecked(&format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
             VALUES ('{own_ws}', '{user_id}', 'member', 110)"
        ))
        .await
        .expect("personal space workspace_member seeded");
    own_org
}

async fn seed_shared_org(sys: &Store, name: &str) -> (String, String) {
    let (org_id, ws_id) = (new_id(), new_id());
    for sql in [
        format!(
            "INSERT INTO organizations (id, name, kind, settings, created_at, updated_at) \
             VALUES ('{org_id}', '{name}', '{RETIRED_ORG_KIND}', '{{}}', 100, 200)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{ws_id}', '{org_id}', 'Main', '{{}}', 100, 200)"
        ),
        format!("UPDATE organizations SET default_workspace_id = '{ws_id}' WHERE id = '{org_id}'"),
    ] {
        sys.execute_unchecked(&sql)
            .await
            .expect("shared org seeded");
    }
    (org_id, ws_id)
}

async fn seed_preferences(sys: &Store, alice: &str, bob: &str, carol: &str, ws_a: &str) {
    let alice_rows = [
        ("settings", "language", "ko-KR"),
        ("settings", "theme", "dark"),
        ("settings", "density", "compact"),
        ("settings", "cardOpen", "side"),
        ("settings", "haptics", "false"),
        ("settings", "weekStart", "monday"),
        ("settings", "hourCycle", "h23"),
        ("settings", "timeZone", "Asia/Seoul"),
        ("settings", "notifyMentions", "false"),
        ("settings", "notifyAssignments", "false"),
        ("settings", "notifyComments", "false"),
        ("settings", "unknownSetting", "ignored"),
        ("client", "onboarding.step", "done"),
        ("client", "sidebar.width", "280"),
        ("client", "theme", "client-shadow"),
        ("retired", "legacy.key", "dropped"),
    ];
    for (cat, name, val) in alice_rows {
        sys.execute_unchecked(&format!(
            "INSERT INTO preferences (user_id, category, name, value) \
             VALUES ('{alice}', '{cat}', '{name}', '{val}')"
        ))
        .await
        .expect("alice preference seeded");
    }

    let bob_rows = [
        ("settings", "theme", "light"),
        ("settings", "cardOpen", "full"),
        ("settings", "notifyComments", "false"),
        ("settings", "density", "invalid-density"),
        ("settings", "haptics", "not-a-boolean"),
        ("client", "draft.1", "{\"text\":\"hello\"}"),
        ("client", "pinned", "true"),
    ];
    for (cat, name, val) in bob_rows {
        let escaped = val.replace('\'', "''");
        sys.execute_unchecked(&format!(
            "INSERT INTO preferences (user_id, category, name, value) \
             VALUES ('{bob}', '{cat}', '{name}', '{escaped}')"
        ))
        .await
        .expect("bob preference seeded");
    }

    sys.execute_unchecked(&format!(
        "INSERT INTO preferences (user_id, category, name, value) \
         VALUES ('{carol}', 'client', 'lastWorkspace', '{ws_a}')"
    ))
    .await
    .expect("carol preference seeded");
}

async fn seed_blocks_and_trash(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
) -> (String, String, String, String, String) {
    let (board_id, card_id, comment_id, trashed_card_id, trashed_nul_card_id) =
        (new_id(), new_id(), new_id(), new_id(), new_id());
    let empty_bytes = match scoped_a.db_type() {
        DbType::Sqlite => "X'0102'",
        DbType::Postgres => "'\\x0102'::bytea",
    };
    let card_fields = json!({"icon": "🚀", "schema": "nested-live-kept"}).to_string();
    let trashed_doc = json!({
        "id": trashed_card_id,
        "containerId": board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "card",
        "title": "Restorable 0076 Card",
        "fields": {
            "schema": "nested-must-stay",
            "properties": { "schema": "prop-must-stay" }
        },
        "watchedBy": [{"userId": alice, "blockType": "Card", "notifiedAt": 50, "createdAt": 3021}],
        "createdAt": 3020,
        "updatedAt": 4020,
        "deletedAt": 5020,
        "number": 2,
    })
    .to_string();
    let nul_desc = if scoped_a.db_type() == DbType::Postgres {
        r"nul:\u0000 literal:\\u0000"
    } else {
        "plain nul fallback"
    };
    let nul_card_doc = format!(
        r#"{{"id":"{trashed_nul_card_id}","containerId":"{board_id}","parentId":"","createdBy":"{alice}","modifiedBy":"{alice}","schema":1,"type":"card","title":"NUL 0076 Card","fields":{{"note":"{nul_desc}","schema":"nested-nul-kept"}},"createdAt":3030,"updatedAt":4030,"deletedAt":5030,"number":3}}"#
    );

    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, created_at, updated_at) \
             VALUES ('{board_id}', '{ws_a}', '{alice}', '{alice}', 'open', 'board', 'Board 0076', '', '', \
              false, false, '{{}}', '[]', 1000, 2000)"
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) \
             VALUES ('{board_id}', '{alice}', '{}')",
            ContainerRole::Admin.as_str()
        ),
        format!(
            "INSERT INTO blocks \
             (id, container_id, parent_id, created_by, modified_by, \"schema\", type, title, \
              fields, created_at, updated_at, number) VALUES \
             ('{card_id}', '{board_id}', NULL, '{alice}', '{alice}', 1, 'card', 'Live Card', \
              '{}', 1010, 2010, 1), \
             ('{comment_id}', '{board_id}', '{card_id}', '{alice}', '{alice}', 1, 'comment', '', \
              '{{}}', 1020, 2025, NULL)",
            card_fields.replace('\'', "''"),
        ),
        format!(
            "INSERT INTO comment_versions (block_id, version, body, files, written_by, written_at) \
             VALUES ('{comment_id}', 1, '{{\"type\":\"doc\",\"content\":[]}}', '[]', '{alice}', 2025)"
        ),
        format!(
            "INSERT INTO subscriptions (block_type, block_id, user_id, notified_at, created_at) VALUES \
             ('card', '{card_id}', '{alice}', 900, 1015), \
             ('board', '{comment_id}', '{alice}', 905, 1025)"
        ),
        format!(
            "INSERT INTO body_docs \
             (block_id, state, state_seq, log_end, projected_seq, body_digest, epoch, \
              last_writer, created_at, updated_at) \
             VALUES ('{card_id}', {empty_bytes}, 1, 1, 1, {empty_bytes}, 1, '{alice}', 1010, 2010)"
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) VALUES \
             ('{trashed_card_id}', '{board_id}', NULL, 5020, '{alice}', '{}', '{}'), \
             ('{trashed_nul_card_id}', '{board_id}', NULL, 5030, '{alice}', '{}', '{}')",
            new_id(),
            trashed_doc.replace('\'', "''"),
            new_id(),
            nul_card_doc.replace('\'', "''"),
        ),
    ] {
        scoped_a
            .execute_unchecked(&sql)
            .await
            .expect("blocks/trash pre-0076 seeded");
    }
    (
        board_id,
        card_id,
        comment_id,
        trashed_card_id,
        trashed_nul_card_id,
    )
}

async fn seed_org_b(pre: &Store, bob: &str) -> Option<Pre0076OrgB> {
    if pre.db_type() != DbType::Postgres {
        return None;
    }
    let sys = pre.system();
    let bob_own_org = seed_personal_space(pre, bob).await;
    let (org_id, ws_b) = seed_shared_org(&sys, "Beta 0076").await;
    sys.execute_unchecked(&format!(
        "INSERT INTO organization_members (org_id, user_id, role, created_at) \
         VALUES ('{org_id}', '{bob}', 'admin', 100)"
    ))
    .await
    .expect("bob in org_b");

    let scoped_b = pre.for_org(&org_id).expect("org B handle");
    scoped_b
        .execute_unchecked(&format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
             VALUES ('{ws_b}', '{bob}', 'admin', 100)"
        ))
        .await
        .expect("bob in ws_b");
    let (board_b, live_card_id, trashed_card_id) = (new_id(), new_id(), new_id());
    let trashed_doc = json!({
        "id": trashed_card_id,
        "containerId": board_b,
        "parentId": "",
        "createdBy": bob,
        "modifiedBy": bob,
        "schema": 1,
        "type": "card",
        "title": "Org B Trashed Card",
        "fields": {},
        "createdAt": 3100,
        "updatedAt": 4100,
        "deletedAt": 5100,
        "number": 2,
    })
    .to_string();
    for sql in [
        format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, created_at, updated_at) \
             VALUES ('{board_b}', '{ws_b}', '{bob}', '{bob}', 'private', 'board', 'Org B', '', '', \
              false, false, '{{}}', '[]', 1100, 2100)"
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) \
             VALUES ('{board_b}', '{bob}', 'admin')"
        ),
        format!(
            "INSERT INTO blocks \
             (id, container_id, parent_id, created_by, modified_by, \"schema\", type, title, \
              fields, created_at, updated_at, number) \
             VALUES ('{live_card_id}', '{board_b}', NULL, '{bob}', '{bob}', 1, 'card', \
              'Org B Live Card', '{{}}', 1110, 2110, 1)"
        ),
        format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{trashed_card_id}', '{board_b}', NULL, 5100, '{bob}', '{}', '{}')",
            new_id(),
            trashed_doc.replace('\'', "''"),
        ),
    ] {
        scoped_b
            .execute_unchecked(&sql)
            .await
            .expect("org B rows seeded");
    }

    Some(Pre0076OrgB {
        org_id,
        bob_own_org,
        live_card_id,
        trashed_card_id,
    })
}

async fn seed_pre0076(pre: &Store) -> Pre0076Fixture {
    let sys = pre.system();
    let (org_a, ws_a) = seed_shared_org(&sys, "Acme 0076").await;
    let alice = seed_user(&sys, "alice-0076").await;
    let bob = seed_user(&sys, "bob-0076").await;
    let carol = seed_user(&sys, "carol-0076").await;
    let dave = seed_user(&sys, "dave-0076").await;
    sys.execute_unchecked("UPDATE instance SET bootstrapped = true")
        .await
        .expect("bootstrapped");
    let scoped_a = pre.for_org(&org_a).expect("org A handle");
    for (uid, role) in [(&alice, "admin"), (&bob, "member")] {
        sys.execute_unchecked(&format!(
            "INSERT INTO organization_members (org_id, user_id, role, created_at) \
             VALUES ('{org_a}', '{uid}', '{role}', 100)"
        ))
        .await
        .expect("org_a member");
        scoped_a
            .execute_unchecked(&format!(
                "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
                 VALUES ('{ws_a}', '{uid}', '{role}', 100)"
            ))
            .await
            .expect("ws_a member");
    }
    let alice_own_org = seed_personal_space(pre, &alice).await;
    seed_preferences(&sys, &alice, &bob, &carol, &ws_a).await;

    let (board_id, card_id, comment_id, trashed_card_id, trashed_nul_card_id) =
        seed_blocks_and_trash(&scoped_a, &ws_a, &alice).await;
    let org_b = seed_org_b(pre, &bob).await;

    Pre0076Fixture {
        org_a,
        ws_a,
        alice,
        alice_own_org,
        bob,
        carol,
        dave,
        board_id,
        card_id,
        comment_id,
        trashed_card_id,
        trashed_nul_card_id,
        org_b,
    }
}

const EXPECTED_SETTINGS_COLS: [&str; 15] = [
    "card_open",
    "density",
    "haptics",
    "hour_cycle",
    "language",
    "notify_assignments",
    "notify_comments",
    "notify_due",
    "notify_meetings",
    "notify_mentions",
    "notify_reviews",
    "theme",
    "time_zone",
    "user_id",
    "week_start",
];

async fn assert_sqlite_catalog(database: &fvoci_store::testing::TestDatabase, sys: &Store) {
    let mut conn = sqlx::sqlite::SqliteConnection::connect(&common::raw_connection(database))
        .await
        .expect("sqlite open");
    let fk: Vec<String> = sqlx::query_scalar("SELECT \"table\" FROM pragma_foreign_key_check")
        .fetch_all(&mut conn)
        .await
        .expect("fk check");
    assert_eq!(fk, Vec::<String>::new(), "broken SQLite foreign keys");
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(&mut conn)
        .await
        .expect("integrity check");
    assert_eq!(integrity, ["ok"], "SQLite integrity_check must pass");
    conn.close().await.expect("close");

    let retired = sys
        .texts_unchecked(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'preferences' \
             UNION ALL \
             SELECT name FROM pragma_table_info('blocks') WHERE name = 'schema'",
        )
        .await
        .expect("sqlite retired check");
    assert_eq!(retired, Vec::<String>::new(), "retired SQLite objects left");

    let mut cols = sys
        .texts_unchecked("SELECT name FROM pragma_table_info('user_settings')")
        .await
        .expect("sqlite user_settings cols");
    cols.sort();
    assert_eq!(cols, EXPECTED_SETTINGS_COLS, "SQLite user_settings columns");

    let org_defs = sys
        .texts_unchecked(
            "SELECT sql FROM sqlite_master \
             WHERE name IN ('organizations', 'organizations_of_people_idx', 'organizations_only_one')",
        )
        .await
        .expect("sqlite org defs");
    assert_eq!(
        org_defs.len(),
        3,
        "three SQLite organization catalog entries"
    );
    for def in &org_defs {
        assert!(
            def.contains("'shared'") && !def.contains(&format!("'{RETIRED_ORG_KIND}'")),
            "SQLite definition must use 'shared': {def}"
        );
    }
}

async fn assert_postgres_catalog(sys: &Store) {
    let retired = sys
        .texts_unchecked(
            "SELECT relname::text FROM pg_class \
             WHERE relnamespace = current_schema()::regnamespace AND relname = 'preferences' \
             UNION ALL \
             SELECT attname::text FROM pg_attribute \
             WHERE attrelid = 'blocks'::regclass AND attnum > 0 AND NOT attisdropped \
               AND attname = 'schema'",
        )
        .await
        .expect("pg retired check");
    assert_eq!(retired, Vec::<String>::new(), "retired PG objects left");

    let mut cols = sys
        .texts_unchecked(
            "SELECT attname::text FROM pg_attribute \
             WHERE attrelid = 'user_settings'::regclass AND attnum > 0 AND NOT attisdropped",
        )
        .await
        .expect("pg user_settings cols");
    cols.sort();
    assert_eq!(cols, EXPECTED_SETTINGS_COLS, "PG user_settings columns");

    let rls_global = sys
        .texts_unchecked(
            "SELECT relname::text FROM pg_class \
             WHERE relnamespace = current_schema()::regnamespace \
               AND relname IN ('user_settings', 'user_client_state') \
               AND relrowsecurity",
        )
        .await
        .expect("pg rls check");
    assert_eq!(
        rls_global,
        Vec::<String>::new(),
        "global user tables must not enable RLS"
    );

    let retired_defs = sys
        .texts_unchecked(&format!(
            "WITH org_con AS MATERIALIZED ( \
                 SELECT oid, conname FROM pg_constraint \
                 WHERE conrelid = 'organizations'::regclass \
             ), \
             org_idx AS MATERIALIZED ( \
                 SELECT ix.indexrelid, ic.relname \
                 FROM pg_index AS ix \
                 JOIN pg_class AS ic ON ic.oid = ix.indexrelid \
                 WHERE ix.indrelid = 'organizations'::regclass \
             ) \
             SELECT conname::text FROM org_con \
             WHERE pg_get_constraintdef(oid) LIKE '%{RETIRED_ORG_KIND}%' \
             UNION ALL \
             SELECT relname::text FROM org_idx \
             WHERE pg_get_indexdef(indexrelid) LIKE '%{RETIRED_ORG_KIND}%'"
        ))
        .await
        .expect("pg org constraint/index check");
    assert_eq!(
        retired_defs,
        Vec::<String>::new(),
        "PG organizations constraints/indexes must not mention retired kind"
    );
}

async fn assert_catalog(database: &fvoci_store::testing::TestDatabase, sys: &Store) {
    match db_type() {
        DbType::Sqlite => assert_sqlite_catalog(database, sys).await,
        DbType::Postgres => assert_postgres_catalog(sys).await,
    }
}

async fn assert_migrated_settings(store: &Store, sys: &Store, fx: &Pre0076Fixture) {
    assert_eq!(
        store
            .get_user_settings(&fx.alice)
            .await
            .expect("alice settings"),
        UserSettings {
            language: "ko-KR".parse().expect("tag"),
            theme: Theme::Dark,
            density: Density::Compact,
            card_open: CardOpen::Side,
            haptics: false,
            week_start: WeekStart::Monday,
            hour_cycle: HourCycle::H23,
            time_zone: "Asia/Seoul".parse().expect("tz"),
            notify_mentions: false,
            notify_assignments: false,
            notify_comments: false,
            notify_due: true,
        },
        "every setting of Alice migrated losslessly"
    );
    let upcoming_toggles = sys
        .texts_unchecked(&format!(
            "SELECT CASE WHEN notify_reviews AND notify_meetings AND notify_due \
             THEN 'all_true' ELSE 'unexpected' END FROM user_settings WHERE user_id = '{}'",
            fx.alice
        ))
        .await
        .expect("upcoming toggles");
    assert_eq!(
        upcoming_toggles,
        ["all_true"],
        "upcoming notify toggles default to true"
    );
    assert_eq!(
        store
            .get_user_settings(&fx.bob)
            .await
            .expect("bob settings"),
        UserSettings {
            theme: Theme::Light,
            card_open: CardOpen::Full,
            notify_comments: false,
            density: Density::Auto,
            haptics: true,
            ..UserSettings::default()
        },
        "partial and invalid-fallback settings of Bob migrated cleanly"
    );
    assert_eq!(
        store
            .get_user_settings(&fx.carol)
            .await
            .expect("carol settings"),
        UserSettings::default(),
        "Carol has default settings when only client state existed"
    );
    assert_eq!(
        store
            .get_user_settings(&fx.dave)
            .await
            .expect("dave settings"),
        UserSettings::default(),
        "Dave has default settings when no preferences existed"
    );
}

async fn assert_settings_and_client_state(store: &Store, sys: &Store, fx: &Pre0076Fixture) {
    assert_migrated_settings(store, sys, fx).await;
    assert_eq!(
        store
            .get_client_state(&fx.alice)
            .await
            .expect("alice client state"),
        BTreeMap::from([
            ("onboarding.step".to_owned(), "done".to_owned()),
            ("sidebar.width".to_owned(), "280".to_owned()),
            ("theme".to_owned(), "client-shadow".to_owned()),
        ]),
        "every client-state entry of Alice migrated losslessly"
    );
    assert_eq!(
        store
            .get_client_state(&fx.bob)
            .await
            .expect("bob client state"),
        BTreeMap::from([
            ("draft.1".to_owned(), "{\"text\":\"hello\"}".to_owned()),
            ("pinned".to_owned(), "true".to_owned()),
        ]),
        "every client-state entry of Bob migrated losslessly"
    );
    assert_eq!(
        store
            .get_client_state(&fx.carol)
            .await
            .expect("carol client state"),
        BTreeMap::from([("lastWorkspace".to_owned(), fx.ws_a.clone())]),
        "Carol client state migrated losslessly"
    );
    assert_eq!(
        store
            .get_client_state(&fx.dave)
            .await
            .expect("dave client state"),
        BTreeMap::new(),
        "Dave client state is empty"
    );

    let patch = UserSettingsPatch::from_members(
        json!({"theme": "dark", "language": "en-US"})
            .as_object()
            .expect("object"),
    )
    .expect("valid patch");
    let updated_settings = store
        .patch_user_settings(&fx.carol, &patch)
        .await
        .expect("patch carol settings");
    assert_eq!(
        updated_settings.theme,
        Theme::Dark,
        "post-migration user_settings write succeeds"
    );

    let updated_bag = store
        .patch_client_state(
            &fx.carol,
            &ClientStatePatch {
                updated_fields: BTreeMap::from([("tour".to_owned(), "seen".to_owned())]),
                deleted_fields: vec!["lastWorkspace".to_owned()],
            },
        )
        .await
        .expect("patch carol client state");
    assert_eq!(
        updated_bag,
        BTreeMap::from([("tour".to_owned(), "seen".to_owned())]),
        "post-migration user_client_state write and delete succeed"
    );
}

async fn assert_organizations(
    database: &fvoci_store::testing::TestDatabase,
    store: &Store,
    sys: &Store,
    fx: &Pre0076Fixture,
) {
    for org_id in [SYSTEM_ORG_ID, fx.org_a.as_str()] {
        let kind = sys
            .texts_unchecked(&format!(
                "SELECT kind FROM organizations WHERE id = '{org_id}'"
            ))
            .await
            .expect("org kind");
        assert_eq!(kind, [OrgKind::Shared.as_str()], "org {org_id}");
    }
    let own_kind = sys
        .texts_unchecked(&format!(
            "SELECT kind FROM organizations WHERE id = '{}'",
            fx.alice_own_org
        ))
        .await
        .expect("alice own kind");
    assert_eq!(
        own_kind,
        [OrgKind::Personal.as_str()],
        "personal org preserved"
    );

    let listed: Vec<(String, OrgKind)> = store
        .workspaces_of_user(&fx.alice, fvoci_store::Making::with_password())
        .await
        .expect("workspaces_of_user")
        .into_iter()
        .map(|ws| (ws.org_id, ws.kind))
        .collect();
    assert_eq!(
        listed,
        [
            (fx.org_a.clone(), OrgKind::Shared),
            (fx.alice_own_org.clone(), OrgKind::Personal)
        ],
        "workspaces_of_user returns Shared and Personal"
    );

    assert!(
        common::try_execute_raw(
            &common::raw_connection(database),
            &format!(
                "INSERT INTO organizations (id, name, kind, settings, created_at, updated_at) \
                 VALUES ('{}', 'Bad', '{RETIRED_ORG_KIND}', '{{}}', 1, 1)",
                new_id()
            ),
        )
        .await
        .is_err(),
        "CHECK constraint must reject retired organization kind"
    );
    assert!(
        common::try_execute_raw(
            &common::raw_connection(database),
            &format!(
                "UPDATE organizations SET kind = 'personal', owner_id = '{}' WHERE id = '{}'",
                fx.alice, fx.org_a
            ),
        )
        .await
        .is_err(),
        "organizations_keep_their_kind trigger must reject kind updates"
    );
    if db_type() == DbType::Sqlite {
        assert!(
            common::try_execute_raw(
                &common::raw_connection(database),
                &format!(
                    "INSERT INTO organizations (id, name, settings, created_at, updated_at) \
                     VALUES ('{}', 'Second Shared', '{{}}', 1, 1)",
                    new_id()
                ),
            )
            .await
            .is_err(),
            "SQLite organizations_only_one must reject a second shared organization"
        );
    } else {
        let default_org_id = new_id();
        sys.execute_unchecked(&format!(
            "INSERT INTO organizations (id, name, settings, created_at, updated_at) \
             VALUES ('{default_org_id}', 'Default Shared', '{{}}', 300, 300)"
        ))
        .await
        .expect("PG default org kind insert");
        let kind = sys
            .texts_unchecked(&format!(
                "SELECT kind FROM organizations WHERE id = '{default_org_id}'"
            ))
            .await
            .expect("default kind");
        assert_eq!(
            kind,
            ["shared"],
            "DEFAULT on organizations.kind is 'shared'"
        );
    }
}

async fn assert_live_blocks(scoped_a: &Store, fx: &Pre0076Fixture) {
    let card = scoped_a.get_block(&fx.card_id).await.expect("live card");
    assert_eq!(card.block_type, BlockType::Card, "live card type preserved");
    assert_eq!(card.number, Some(1), "live card number preserved");
    assert_eq!(
        card.fields.get("schema"),
        Some(&json!("nested-live-kept")),
        "nested fields.schema on live block preserved"
    );
    let comment = scoped_a
        .get_block(&fx.comment_id)
        .await
        .expect("live comment");
    assert_eq!(comment.edited_at, 2025, "comment_versions FK intact");
    let sub = |block_type, block_id: &String, notified_at, created_at| Subscription {
        block_type,
        block_id: block_id.clone(),
        user_id: fx.alice.clone(),
        notified_at,
        created_at,
        deleted_at: None,
    };
    assert_eq!(
        scoped_a.get_subscriptions(&fx.alice).await.expect("subs"),
        [
            sub(BlockType::Card, &fx.card_id, 900, 1015),
            sub(BlockType::Comment, &fx.comment_id, 905, 1025),
        ],
        "live subscriptions preserved and pre-0076 non-standard block_type normalized"
    );
    if db_type() == DbType::Postgres {
        assert!(
            scoped_a
                .execute_unchecked(&format!(
                    "UPDATE blocks SET type = 'text' WHERE id = '{}'",
                    fx.card_id
                ))
                .await
                .is_err(),
            "PostgreSQL blocks_type_check rejects non-standard block types"
        );
    }
    assert!(
        scoped_a
            .get_body_doc(&fx.card_id)
            .await
            .expect("body_doc")
            .is_some(),
        "live card Yjs body_doc preserved"
    );
}

async fn assert_blocks_and_trash(store: &Store, sys: &Store, fx: &Pre0076Fixture) {
    let scoped_a = store.for_org(&fx.org_a).expect("org A");
    assert_live_blocks(&scoped_a, fx).await;

    let raw_trashed = scoped_a
        .texts_unchecked(&format!(
            "SELECT CAST(block AS TEXT) FROM deleted_blocks WHERE id IN ('{}', '{}') ORDER BY deleted_at",
            fx.trashed_card_id, fx.trashed_nul_card_id
        ))
        .await
        .expect("raw deleted_blocks");
    assert_eq!(raw_trashed.len(), 2, "two trashed blocks present");
    let doc_0: Value = serde_json::from_str(&raw_trashed[0]).expect("trashed json");
    assert!(
        doc_0.get("schema").is_none(),
        "top-level schema stripped: {doc_0}"
    );
    assert_eq!(
        doc_0["fields"]["schema"],
        json!("nested-must-stay"),
        "nested fields.schema kept"
    );
    assert_eq!(
        doc_0["fields"]["properties"]["schema"],
        json!("prop-must-stay"),
        "nested fields.properties.schema kept"
    );
    assert!(
        !raw_trashed[1].contains(r#"\"schema\":1"#) && !raw_trashed[1].contains(r#"\"schema\": 1"#),
        "top-level schema stripped on NUL-bearing row: {}",
        raw_trashed[1]
    );
    assert!(
        raw_trashed[1].contains("nested-nul-kept"),
        "nested schema preserved: {}",
        raw_trashed[1]
    );
    if db_type() == DbType::Postgres {
        assert!(
            raw_trashed[1].contains(r"\u0000") && raw_trashed[1].contains(r"\\u0000"),
            "PostgreSQL NUL escapes preserved: {}",
            raw_trashed[1]
        );
    }

    let restored = scoped_a
        .undelete_block(&fx.trashed_card_id, &fx.alice)
        .await
        .expect("restores pre-0076 trashed card");
    assert_eq!(
        restored.title, "Restorable 0076 Card",
        "restored pre-0076 card title"
    );
    assert_eq!(restored.number, Some(2), "restored pre-0076 card number");
    let restored_sub = scoped_a
        .get_subscription(&fx.trashed_card_id, &fx.alice)
        .await
        .expect("restored watcher");
    assert_eq!(
        (restored_sub.block_type, restored_sub.created_at),
        (BlockType::Card, 3021),
        "restored watcher normalizes pre-0076 blockType and preserves timestamp"
    );

    if let Some(org_b) = &fx.org_b {
        let kinds = sys
            .texts_unchecked(&format!(
                "SELECT kind FROM organizations WHERE id IN ('{}', '{}') ORDER BY kind",
                org_b.org_id, org_b.bob_own_org
            ))
            .await
            .expect("org B kinds");
        assert_eq!(
            kinds,
            ["personal", "shared"],
            "org B and Bob personal org kinds migrated"
        );
        let scoped_b = store.for_org(&org_b.org_id).expect("org B");
        assert_eq!(
            scoped_b
                .get_block(&org_b.live_card_id)
                .await
                .expect("org B live card")
                .title,
            "Org B Live Card",
            "org B RLS live card preserved"
        );
        let restored_b = scoped_b
            .undelete_block(&org_b.trashed_card_id, &fx.bob)
            .await
            .expect("org B trashed card restores");
        assert_eq!(
            restored_b.title, "Org B Trashed Card",
            "org B RLS trashed card restores"
        );
    }
}

#[tokio::test]
async fn populated_database_upgrades_from_0075_to_0076() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_user_settings_and_block_cleanup(&database).await;
    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open at 0075");
    let fx = seed_pre0076(&pre).await;
    pre.close().await;

    let store = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrates 0075 -> 0076 cleanly");
    let sys = store.system();

    assert_catalog(&database, &sys).await;
    assert_settings_and_client_state(&store, &sys, &fx).await;
    assert_organizations(&database, &store, &sys, &fx).await;
    assert_blocks_and_trash(&store, &sys, &fx).await;

    common::execute_raw(
        &common::raw_connection(&database),
        &format!("DELETE FROM users WHERE id = '{}'", fx.carol),
    )
    .await;
    assert_eq!(
        store
            .get_client_state(&fx.carol)
            .await
            .expect("carol state after cascade"),
        BTreeMap::new(),
        "user_client_state cascades on user delete"
    );
    let left_settings = sys
        .texts_unchecked(&format!(
            "SELECT user_id FROM user_settings WHERE user_id = '{}'",
            fx.carol
        ))
        .await
        .expect("carol settings after cascade");
    assert_eq!(
        left_settings,
        Vec::<String>::new(),
        "user_settings cascades on user delete"
    );

    let _ = &fx.board_id;
    store.close().await;
    database.remove().await;
}
