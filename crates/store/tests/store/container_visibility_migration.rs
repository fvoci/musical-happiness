//! Migration `0070` (`containers.visibility`, `public_links`, `files`, and
//! `deleted_containers.container` JSON upgrade) exercised on a populated SQLite and
//! PostgreSQL database seeded at version `0069`.

use fvoci_model::{ContainerKind, ContainerVisibility, FileRecord, PublicLink, new_id};
use fvoci_store::{DbType, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, SharedContainer, Store, Tenant};
use serde_json::json;
use sqlx::Connection as _;

use crate::common::{self, db_type};

const PRE0070_FILES_TABLE: &str = "file_info";
const PRE0070_LINKS_TABLE: &str = "sharing";
const OLD_DEL_KEY: &str = concat!("delete", "At");

#[derive(Clone, Copy)]
enum TemplateKind {
    Regular,
    BuiltIn,
}

struct Pre0070Container<'a> {
    id: &'a str,
    workspace_id: &'a str,
    raw_type: &'a str,
    kind: &'a str,
    title: &'a str,
    template: TemplateKind,
    card_props: &'a str,
    user_id: Option<&'a str>,
}

struct Pre0070SecondOrg {
    org_id: String,
    bob: String,
    open_board_id: String,
    trashed_board_id: String,
    file_id: String,
}

struct Pre0070Fixture {
    org_a: String,
    own_org: String,
    alice: String,
    template_id: String,
    template_file: FileRecord,
    open_board_id: String,
    private_wiki_id: String,
    private_course_id: String,
    diary_id: String,
    trashed_open_id: String,
    trashed_open_file: FileRecord,
    trashed_private_id: String,
    kept_file: FileRecord,
    unreferenced_file: FileRecord,
    other_org: Option<Pre0070SecondOrg>,
}

async fn insert_pre0070_container(scoped: &Store, spec: &Pre0070Container<'_>) {
    let creator = spec
        .user_id
        .map_or_else(|| "NULL".to_owned(), |u| format!("'{u}'"));
    let is_template = matches!(spec.template, TemplateKind::BuiltIn);
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, type, kind, title, description, icon, \
              show_description, is_template, is_builtin, properties, card_properties, create_at, update_at) \
             VALUES ('{}', '{}', {creator}, {creator}, '{}', '{}', \
              '{}', '', '', false, {is_template}, {is_template}, '{{}}', '{}', 100, 200)",
            spec.id, spec.workspace_id, spec.raw_type, spec.kind, spec.title, spec.card_props,
        ))
        .await
        .expect("pre-0070 container inserted");
    if let Some(uid) = spec.user_id {
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO container_members \
                 (container_id, user_id, {}, {}, {}, {}) \
                 VALUES ('{}', '{uid}', true, true, false, false)",
                concat!("scheme", "_admin"),
                concat!("scheme", "_editor"),
                concat!("scheme", "_commenter"),
                concat!("scheme", "_viewer"),
                spec.id,
            ))
            .await
            .expect("pre-0070 container admin inserted");
    }
}

async fn insert_pre0070_file(scoped: &Store, file: &FileRecord, unreferenced_at: Option<i64>) {
    let unref = unreferenced_at.map_or_else(|| "NULL".to_owned(), |ts| ts.to_string());
    let creator = if file.created_by.is_empty() {
        "NULL".to_owned()
    } else {
        format!("'{}'", file.created_by)
    };
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO {PRE0070_FILES_TABLE} \
             (id, container_id, name, original_name, extension, size, path, create_at, \
              unreferenced_at, version, created_by) \
             VALUES ('{}', '{}', '{}', '{}', '{}', {}, '{}', {}, {unref}, {}, {creator})",
            file.id,
            file.container_id,
            file.stored_name,
            file.name,
            file.extension,
            file.size,
            file.path,
            file.create_at,
            file.version,
        ))
        .await
        .expect("pre-0070 file inserted");
}

async fn seed_pre0070_template(pre: &Store) -> (String, FileRecord) {
    let sys_org = pre.for_org(SYSTEM_ORG_ID).expect("system org");
    let template_id = new_id();
    insert_pre0070_container(
        &sys_org,
        &Pre0070Container {
            id: &template_id,
            workspace_id: SYSTEM_WORKSPACE_ID,
            raw_type: "O",
            kind: "board",
            title: "Built-in Roadmap",
            template: TemplateKind::BuiltIn,
            card_props: r#"[{"id":"p0","name":"Stage","type":"select"}]"#,
            user_id: None,
        },
    )
    .await;
    let file = FileRecord {
        id: new_id(),
        container_id: template_id.clone(),
        stored_name: "7tpl.png".to_owned(),
        name: "template-banner.png".to_owned(),
        extension: ".png".to_owned(),
        size: 512,
        path: format!("templates/{template_id}/7tpl.png"),
        create_at: 100,
        version: 1,
        created_by: String::new(),
    };
    insert_pre0070_file(&sys_org, &file, None).await;
    (template_id, file)
}

async fn seed_pre0070_second_org(pre: &Store) -> Pre0070SecondOrg {
    let (org_id, ws_b) = common::organization(pre, "org-b").await;
    let scoped_b = pre.for_org(&org_id).expect("org-b");
    let bob = common::pre0072_user(&scoped_b, &org_id, &ws_b, "bob").await;
    let open_board_id = new_id();
    insert_pre0070_container(
        &scoped_b,
        &Pre0070Container {
            id: &open_board_id,
            workspace_id: &ws_b,
            raw_type: "O",
            kind: "board",
            title: "Org B Open",
            template: TemplateKind::Regular,
            card_props: "[]",
            user_id: Some(&bob),
        },
    )
    .await;
    scoped_b
        .execute_unchecked(&format!(
            "INSERT INTO {PRE0070_LINKS_TABLE} (container_id, enabled, token, modified_by, update_at) \
             VALUES ('{open_board_id}', true, 'tok-org-b', '{bob}', 300)"
        ))
        .await
        .expect("org-b public link");
    let file = FileRecord {
        id: new_id(),
        container_id: open_board_id.clone(),
        stored_name: "7bob.png".to_owned(),
        name: "bob.png".to_owned(),
        extension: ".png".to_owned(),
        size: 128,
        path: format!("boards/{open_board_id}/7bob.png"),
        create_at: 150,
        version: 1,
        created_by: bob.clone(),
    };
    insert_pre0070_file(&scoped_b, &file, None).await;
    let trashed_board_id = new_id();
    let deletion_id = new_id();
    let doc = json!({
        "id": trashed_board_id,
        "workspaceId": ws_b,
        "createdBy": bob,
        "modifiedBy": bob,
        "type": "P",
        "kind": "board",
        "title": "Org B Trashed",
        "cardProperties": [{"id": "pb", "name": "Priority", "type": "select"}],
        "createAt": 100,
        "updateAt": 200,
        (OLD_DEL_KEY): 250
    })
    .to_string();
    scoped_b
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board_id}', '{ws_b}', 250, '{bob}', '{deletion_id}', '{doc}', '[]', '[]', '[]')"
        ))
        .await
        .expect("org-b trashed board");
    Pre0070SecondOrg {
        org_id,
        bob,
        open_board_id,
        trashed_board_id,
        file_id: file.id,
    }
}

async fn seed_pre0070_trash_in_org_a(
    scoped_a: &Store,
    ws_a: &str,
    alice: &str,
) -> (String, FileRecord, String) {
    let trashed_open_id = new_id();
    let trashed_file = FileRecord {
        id: new_id(),
        container_id: trashed_open_id.clone(),
        stored_name: "7trashed.png".to_owned(),
        name: "trashed.png".to_owned(),
        extension: ".png".to_owned(),
        size: 64,
        path: format!("boards/{trashed_open_id}/7trashed.png"),
        create_at: 110,
        version: 1,
        created_by: alice.to_owned(),
    };
    let files_json = json!([{
        "id": trashed_file.id,
        "container_id": trashed_file.container_id,
        "stored_name": trashed_file.stored_name,
        "name": trashed_file.name,
        "extension": trashed_file.extension,
        "size": trashed_file.size,
        "path": trashed_file.path,
        "create_at": trashed_file.create_at,
        "version": trashed_file.version,
        "created_by": trashed_file.created_by,
    }])
    .to_string();
    let members_json = json!([{
        "containerId": trashed_open_id,
        "userId": alice,
        (concat!("scheme", "Admin")): true,
        (concat!("scheme", "Editor")): true,
        (concat!("scheme", "Commenter")): false,
        (concat!("scheme", "Viewer")): false
    }])
    .to_string();
    let open_doc = json!({
        "id": trashed_open_id,
        "workspaceId": ws_a,
        "createdBy": alice,
        "modifiedBy": alice,
        "type": "O",
        "kind": "board",
        "title": "Trashed Open Board",
        "cardProperties": [{"id": "p1", "name": "Status", "type": "select"}],
        "createAt": 100,
        "updateAt": 200,
        (OLD_DEL_KEY): 260
    })
    .to_string();
    let del1 = new_id();
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_open_id}', '{ws_a}', 260, '{alice}', '{del1}', '{open_doc}', \
              '{members_json}', '{files_json}', '[]')"
        ))
        .await
        .expect("trashed open container");

    let trashed_private_id = new_id();
    let del2 = new_id();
    let desc = if scoped_a.db_type() == DbType::Postgres {
        r"nul:\u0000 literal:\\u0000"
    } else {
        "plain description"
    };
    let priv_doc = format!(
        r#"{{"id":"{trashed_private_id}","workspaceId":"{ws_a}","createdBy":"{alice}","modifiedBy":"{alice}","type":"P","kind":"board","title":"Trashed Private","description":"{desc}","cardProperties":[{{"id":"p2","name":"Kind","type":"multiSelect"}}],"createAt":100,"updateAt":200,"{OLD_DEL_KEY}":270}}"#
    );
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_private_id}', '{ws_a}', 270, '{alice}', '{del2}', '{priv_doc}', \
              '[]', '[]', '[]')"
        ))
        .await
        .expect("trashed private container");
    (trashed_open_id, trashed_file, trashed_private_id)
}

async fn seed_pre0070_files_in_org_a(
    scoped_a: &Store,
    open_board_id: &str,
    alice: &str,
) -> (FileRecord, FileRecord) {
    let card_id = new_id();
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO blocks (id, container_id, parent_id, created_by, modified_by, \"schema\", \
             type, title, fields, create_at, update_at, number) \
             VALUES ('{card_id}', '{open_board_id}', NULL, '{alice}', '{alice}', 1, 'card', \
             'Card 1', '{{}}', 120, 120, 1)"
        ))
        .await
        .expect("card seeded");

    let kept_file = FileRecord {
        id: new_id(),
        container_id: open_board_id.to_owned(),
        stored_name: "7kept.png".to_owned(),
        name: "kept.png".to_owned(),
        extension: ".png".to_owned(),
        size: 1024,
        path: format!("boards/{open_board_id}/7kept.png"),
        create_at: 130,
        version: 2,
        created_by: alice.to_owned(),
    };
    let unreferenced_file = FileRecord {
        id: new_id(),
        container_id: open_board_id.to_owned(),
        stored_name: "7stale.png".to_owned(),
        name: "stale.png".to_owned(),
        extension: ".png".to_owned(),
        size: 256,
        path: format!("boards/{open_board_id}/7stale.png"),
        create_at: 140,
        version: 1,
        created_by: alice.to_owned(),
    };
    insert_pre0070_file(scoped_a, &kept_file, None).await;
    insert_pre0070_file(scoped_a, &unreferenced_file, Some(500)).await;
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO file_refs (block_id, container_id, name) \
             VALUES ('{card_id}', '{open_board_id}', '7kept.png')"
        ))
        .await
        .expect("file_ref seeded");
    (kept_file, unreferenced_file)
}

async fn seed_pre0070_diary(pre: &Store, alice: &str) -> (String, String) {
    let own_rows = pre
        .system()
        .texts_unchecked(&format!(
            "SELECT CAST(id AS TEXT) || '|' || CAST(default_workspace_id AS TEXT) \
             FROM organizations WHERE owner_id = '{alice}'"
        ))
        .await
        .expect("own space");
    let (own_org, own_ws) = own_rows[0].split_once('|').expect("own org|ws");
    let (own_org, own_ws) = (own_org.to_owned(), own_ws.to_owned());
    let own = pre.for_org(&own_org).expect("own org");
    let diary_id = new_id();
    insert_pre0070_container(
        &own,
        &Pre0070Container {
            id: &diary_id,
            workspace_id: &own_ws,
            raw_type: "P",
            kind: "diary",
            title: "Personal Diary",
            template: TemplateKind::Regular,
            card_props: "[]",
            user_id: Some(alice),
        },
    )
    .await;
    (own_org, diary_id)
}

async fn seed_pre0070(pre: &Store) -> Pre0070Fixture {
    let (template_id, template_file) = seed_pre0070_template(pre).await;
    let (org_a, ws_a) = common::organization(pre, "org-a").await;
    let scoped_a = pre.for_org(&org_a).expect("org-a");
    let alice = common::pre0072_user(&scoped_a, &org_a, &ws_a, "alice").await;

    let (open_board_id, private_wiki_id, private_course_id) = (new_id(), new_id(), new_id());
    for (id, raw_type, kind, title, card_props) in [
        (
            &open_board_id,
            "O",
            "board",
            "Open Board",
            r#"[{"id":"p1","name":"Status","type":"select"}]"#,
        ),
        (&private_wiki_id, "P", "wiki", "Private Wiki", "[]"),
        (&private_course_id, "P", "course", "Private Course", "[]"),
    ] {
        insert_pre0070_container(
            &scoped_a,
            &Pre0070Container {
                id,
                workspace_id: &ws_a,
                raw_type,
                kind,
                title,
                template: TemplateKind::Regular,
                card_props,
                user_id: Some(&alice),
            },
        )
        .await;
    }

    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO {PRE0070_LINKS_TABLE} (container_id, enabled, token, modified_by, update_at) VALUES \
             ('{open_board_id}', true, 'tok-open-board', '{alice}', 310), \
             ('{private_wiki_id}', false, '', '{alice}', 320)"
        ))
        .await
        .expect("pre-0070 links seeded");

    let (kept_file, unreferenced_file) =
        seed_pre0070_files_in_org_a(&scoped_a, &open_board_id, &alice).await;
    let (trashed_open_id, trashed_open_file, trashed_private_id) =
        seed_pre0070_trash_in_org_a(&scoped_a, &ws_a, &alice).await;
    let (own_org, diary_id) = seed_pre0070_diary(pre, &alice).await;
    let other_org = if pre.db_type() == DbType::Postgres {
        Some(seed_pre0070_second_org(pre).await)
    } else {
        None
    };

    Pre0070Fixture {
        org_a,
        own_org,
        alice,
        template_id,
        template_file,
        open_board_id,
        private_wiki_id,
        private_course_id,
        diary_id,
        trashed_open_id,
        trashed_open_file,
        trashed_private_id,
        kept_file,
        unreferenced_file,
        other_org,
    }
}

async fn assert_post0070_sqlite_catalog(
    database: &fvoci_store::testing::TestDatabase,
    sys: &Store,
    old_files: &str,
    old_links: &str,
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

    let cols = sys
        .texts_unchecked("SELECT name FROM pragma_table_info('containers')")
        .await
        .expect("containers cols");
    assert!(cols.iter().any(|c| c == "visibility"), "{cols:?}");
    assert!(!cols.iter().any(|c| c == "type"), "{cols:?}");

    let names = sys
        .texts_unchecked(
            "SELECT type || ':' || name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
        )
        .await
        .expect("sqlite_master");
    for expected in [
        "table:public_links",
        "table:files",
        "index:files_unreferenced_at_idx",
        "index:files_path_idx",
        "trigger:public_links_of_no_diary",
        "trigger:public_links_stay_of_no_diary",
        "trigger:diaries_of_a_space",
        "trigger:diaries_stay_of_a_space",
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "missing {expected} in {names:?}"
        );
    }
    for gone in [old_files, old_links] {
        assert!(
            !names.iter().any(|n| n.contains(gone)),
            "retired name `{gone}` still in sqlite_master: {names:?}"
        );
    }
}

async fn assert_post0070_postgres_catalog(sys: &Store, old_files: &str, old_links: &str) {
    let cols = sys
        .texts_unchecked(
            "SELECT column_name::text FROM information_schema.columns \
             WHERE table_schema = current_schema() AND table_name = 'containers'",
        )
        .await
        .expect("pg containers cols");
    assert!(cols.iter().any(|c| c == "visibility"), "{cols:?}");
    assert!(!cols.iter().any(|c| c == "type"), "{cols:?}");

    let tables = sys
        .texts_unchecked(
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = current_schema()",
        )
        .await
        .expect("pg tables");
    assert!(
        tables.iter().any(|t| t == "public_links") && tables.iter().any(|t| t == "files"),
        "expected public_links and files tables in {tables:?}"
    );
    assert!(
        !tables.iter().any(|t| t == old_links || t == old_files),
        "retired table names remain in {tables:?}"
    );

    let catalog_leftovers = sys
        .texts_unchecked(&format!(
            "SELECT relname::text FROM pg_class \
             WHERE relnamespace = current_schema()::regnamespace \
               AND (relname LIKE '{old_links}%' OR relname LIKE '{old_files}%') \
             UNION ALL \
             SELECT conname::text FROM pg_constraint \
             WHERE connamespace = current_schema()::regnamespace \
               AND (conname LIKE '{old_links}%' OR conname LIKE '{old_files}%' \
                    OR conname = 'containers_type_check') \
             UNION ALL \
             SELECT tg.tgname::text FROM pg_trigger AS tg \
             JOIN pg_class AS tc ON tc.oid = tg.tgrelid \
             WHERE tc.relnamespace = current_schema()::regnamespace \
               AND NOT tg.tgisinternal AND tg.tgname LIKE '{old_links}%'"
        ))
        .await
        .expect("pg leftovers");
    assert!(
        catalog_leftovers.is_empty(),
        "retired PG catalog names remain: {catalog_leftovers:?}"
    );
    let triggers = sys
        .texts_unchecked(
            "SELECT tg.tgname::text FROM pg_trigger AS tg \
             JOIN pg_class AS tc ON tc.oid = tg.tgrelid \
             WHERE tc.relnamespace = current_schema()::regnamespace AND NOT tg.tgisinternal",
        )
        .await
        .expect("pg triggers");
    assert!(
        triggers.iter().any(|t| t == "public_links_of_no_diary"),
        "expected public_links_of_no_diary in {triggers:?}"
    );
}

async fn assert_post0070_catalog(database: &fvoci_store::testing::TestDatabase, migrated: &Store) {
    let sys = migrated.system();
    match migrated.db_type() {
        DbType::Sqlite => {
            assert_post0070_sqlite_catalog(
                database,
                &sys,
                PRE0070_FILES_TABLE,
                PRE0070_LINKS_TABLE,
            )
            .await;
        }
        DbType::Postgres => {
            assert_post0070_postgres_catalog(&sys, PRE0070_FILES_TABLE, PRE0070_LINKS_TABLE).await;
        }
    }
}

async fn assert_post0070_containers_and_trash(migrated: &Store, fx: &Pre0070Fixture) {
    let post_a = migrated.for_org(&fx.org_a).expect("org-a");
    let own = migrated.for_org(&fx.own_org).expect("own-org");

    let board = post_a
        .get_container(&fx.open_board_id)
        .await
        .expect("open board");
    assert_eq!(
        board.visibility,
        ContainerVisibility::Open,
        "open board visibility migrated"
    );
    assert_eq!(
        board.card_properties[0].property_type,
        fvoci_model::PropertyType::Select,
        "nested card_properties.type preserved"
    );

    let wiki = post_a
        .get_container(&fx.private_wiki_id)
        .await
        .expect("wiki");
    assert_eq!(
        (wiki.kind, wiki.visibility),
        (ContainerKind::Wiki, ContainerVisibility::Private),
        "private wiki migrated"
    );

    let course = post_a
        .get_container(&fx.private_course_id)
        .await
        .expect("course");
    assert_eq!(
        (course.kind, course.visibility),
        (ContainerKind::Course, ContainerVisibility::Private),
        "private course migrated"
    );

    let diary = own.get_container(&fx.diary_id).await.expect("diary");
    assert_eq!(
        (diary.kind, diary.visibility),
        (ContainerKind::Diary, ContainerVisibility::Private),
        "personal diary migrated"
    );

    for rejected in ["O", "P", "other"] {
        let bad = migrated
            .system()
            .execute_unchecked(&format!(
                "UPDATE containers SET visibility = '{rejected}' WHERE id = '{}'",
                fx.open_board_id
            ))
            .await;
        assert!(bad.is_err(), "CHECK must reject visibility `{rejected}`");
    }

    assert_post0070_trash_rows(&post_a, fx).await;
}

async fn assert_post0070_trash_rows(post_a: &Store, fx: &Pre0070Fixture) {
    if db_type() == DbType::Postgres {
        let raw = post_a
            .texts_unchecked(&format!(
                "SELECT container::text FROM deleted_containers WHERE id = '{}'",
                fx.trashed_private_id
            ))
            .await
            .expect("raw trashed json");
        assert!(
            raw[0].contains(r#""visibility": "private""#)
                || raw[0].contains(r#""visibility":"private""#),
            "raw={}",
            raw[0]
        );
        assert!(
            !raw[0].contains(r#""type": "P""#) && !raw[0].contains(r#""type":"P""#),
            "raw={}",
            raw[0]
        );
        assert!(
            raw[0].contains("multiSelect"),
            "nested cardProperties kept: {}",
            raw[0]
        );
        assert!(
            raw[0].contains(r"\u0000") && raw[0].contains(r"\\u0000"),
            "NUL escapes kept: {}",
            raw[0]
        );
        let decoded: fvoci_model::Container =
            serde_json::from_str(&raw[0]).expect("rewritten PG trash JSON decodes as Container");
        assert_eq!(
            decoded.visibility,
            ContainerVisibility::Private,
            "PG trashed container visibility decoded"
        );
        assert_eq!(
            decoded.description, "nul:\0 literal:\\u0000",
            "PG trashed container NUL vs literal escape decoded"
        );
        assert_eq!(
            decoded.card_properties[0].property_type,
            fvoci_model::PropertyType::MultiSelect,
            "PG trashed container card_properties decoded"
        );
    } else {
        let restored_priv = post_a
            .undelete_container(&fx.trashed_private_id, &fx.alice)
            .await
            .expect("restore trashed private");
        assert_eq!(
            restored_priv.visibility,
            ContainerVisibility::Private,
            "trashed private board visibility restored"
        );
        assert_eq!(
            restored_priv.card_properties[0].property_type,
            fvoci_model::PropertyType::MultiSelect,
            "trashed private board card_properties preserved"
        );
    }

    let restored_open = post_a
        .undelete_container(&fx.trashed_open_id, &fx.alice)
        .await
        .expect("restore trashed open");
    assert_eq!(
        restored_open.visibility,
        ContainerVisibility::Open,
        "trashed open board visibility restored"
    );
    assert_eq!(
        restored_open.card_properties[0].property_type,
        fvoci_model::PropertyType::Select,
        "trashed open board card_properties preserved"
    );
    assert_eq!(
        post_a
            .get_file_record(&fx.trashed_open_id, &fx.trashed_open_file.stored_name)
            .await
            .expect("restored file"),
        fx.trashed_open_file,
        "trashed board file restored into files table"
    );
}

async fn assert_post0070_links_and_files(migrated: &Store, fx: &Pre0070Fixture) {
    let post_a = migrated.for_org(&fx.org_a).expect("org-a");
    let unset = migrated.scoped(Tenant::Unset);
    let sys = migrated.system();

    assert_eq!(
        post_a
            .get_public_link(&fx.open_board_id)
            .await
            .expect("board link"),
        PublicLink {
            container_id: fx.open_board_id.clone(),
            enabled: true,
            token: "tok-open-board".to_owned(),
            modified_by: fx.alice.clone(),
            update_at: 310,
        },
        "enabled public link migrated intact"
    );
    let disabled = post_a
        .get_public_link(&fx.private_wiki_id)
        .await
        .expect("wiki link");
    assert!(
        !disabled.enabled && disabled.token.is_empty(),
        "disabled public link migrated intact: {disabled:?}"
    );
    let enabled_ids = post_a
        .public_link_container_ids(None)
        .await
        .expect("enabled ids");
    assert_eq!(
        enabled_ids.as_slice(),
        std::slice::from_ref(&fx.open_board_id),
        "only enabled public link container listed"
    );
    assert_eq!(
        unset
            .shared_container(&fx.open_board_id, "tok-open-board")
            .await
            .expect("shared lookup"),
        Some(SharedContainer {
            org_id: fx.org_a.clone(),
            kind: ContainerKind::Board,
        }),
        "shared_container lookup works on public_links"
    );

    assert_diary_public_link_triggers(&sys, fx).await;

    assert_eq!(
        post_a
            .get_file_record(&fx.open_board_id, &fx.kept_file.stored_name)
            .await
            .expect("kept file"),
        fx.kept_file,
        "referenced file migrated intact"
    );
    assert_eq!(
        post_a
            .get_file_record(&fx.open_board_id, &fx.unreferenced_file.stored_name)
            .await
            .expect("unreferenced file before collection"),
        fx.unreferenced_file,
        "unreferenced file migrated intact before collection"
    );
    let paths = sys
        .recorded_paths_after("", 100)
        .await
        .expect("recorded paths");
    assert!(
        paths.contains(&fx.kept_file.path) && paths.contains(&fx.template_file.path),
        "recorded_paths_after reads migrated files table: {paths:?}"
    );

    let collected = sys
        .collect_unreferenced_files(1_000)
        .await
        .expect("collect");
    assert_eq!(
        collected.as_slice(),
        std::slice::from_ref(&fx.unreferenced_file),
        "unreferenced file collected from files table"
    );
    post_a
        .get_file_record(&fx.open_board_id, &fx.kept_file.stored_name)
        .await
        .expect("referenced file kept");
}

async fn assert_diary_public_link_triggers(sys: &Store, fx: &Pre0070Fixture) {
    let diary_link_sql = match db_type() {
        DbType::Sqlite => format!(
            "INSERT INTO public_links (container_id, enabled, token, update_at) \
             VALUES ('{}', 1, 'tok-diary', 1)",
            fx.diary_id
        ),
        DbType::Postgres => format!(
            "INSERT INTO public_links (container_id, org_id, enabled, token, update_at) \
             VALUES ('{}', '{}', true, 'tok-diary', 1)",
            fx.diary_id, fx.own_org
        ),
    };
    assert!(
        sys.execute_unchecked(&diary_link_sql).await.is_err(),
        "public_links_of_no_diary trigger must refuse diary links"
    );
    let diary_retarget_sql = match db_type() {
        DbType::Sqlite => format!(
            "UPDATE public_links SET container_id = '{}' WHERE container_id = '{}'",
            fx.diary_id, fx.open_board_id
        ),
        DbType::Postgres => format!(
            "UPDATE public_links SET container_id = '{}', org_id = '{}' WHERE container_id = '{}'",
            fx.diary_id, fx.own_org, fx.open_board_id
        ),
    };
    assert!(
        sys.execute_unchecked(&diary_retarget_sql).await.is_err(),
        "public_links_stay_of_no_diary trigger must refuse retargeting to diary"
    );
}

async fn assert_post0070_rls(migrated: &Store, fx: &Pre0070Fixture, other: &Pre0070SecondOrg) {
    let post_a = migrated.for_org(&fx.org_a).expect("org-a");
    assert_eq!(
        post_a
            .get_file_record(&fx.template_id, &fx.template_file.stored_name)
            .await
            .expect("org_templates allows reading built-in template file"),
        fx.template_file,
        "org_templates policy allows reading built-in template file"
    );
    assert_eq!(
        post_a
            .execute_unchecked(&format!(
                "DELETE FROM files WHERE id = '{}'",
                fx.template_file.id
            ))
            .await
            .expect("delete statement runs under RLS"),
        0,
        "org_wall blocks deleting system template file"
    );
    assert!(
        post_a.get_container(&other.open_board_id).await.is_err(),
        "org_wall hides other org container"
    );
    assert!(
        post_a.get_public_link(&other.open_board_id).await.is_err(),
        "org_wall hides other org public link"
    );
    assert!(
        post_a
            .get_file_record(&other.open_board_id, "7bob.png")
            .await
            .is_err(),
        "org_wall hides other org file"
    );

    let post_b = migrated.for_org(&other.org_id).expect("org-b");
    assert_eq!(
        post_b
            .get_container(&other.open_board_id)
            .await
            .expect("org-b board")
            .visibility,
        ContainerVisibility::Open,
        "org-b board visibility migrated"
    );
    assert_eq!(
        post_b
            .get_public_link(&other.open_board_id)
            .await
            .expect("org-b link")
            .token,
        "tok-org-b",
        "org-b public link token migrated"
    );
    assert_eq!(
        post_b
            .get_file_record(&other.open_board_id, "7bob.png")
            .await
            .expect("org-b file")
            .id,
        other.file_id,
        "org-b file record migrated"
    );
    let restored_b = post_b
        .undelete_container(&other.trashed_board_id, &other.bob)
        .await
        .expect("org-b restore");
    assert_eq!(
        restored_b.visibility,
        ContainerVisibility::Private,
        "org-b trashed board visibility restored"
    );
    assert_eq!(
        restored_b.card_properties[0].property_type,
        fvoci_model::PropertyType::Select,
        "org-b trashed board card_properties preserved"
    );
}

#[tokio::test]
async fn migration_0070_upgrades_populated_database_losslessly() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_container_visibility(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("opens at migration 69");
    let fx = seed_pre0070(&pre).await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrates 0069 -> 0070 cleanly");

    assert_post0070_catalog(&database, &migrated).await;
    assert_post0070_containers_and_trash(&migrated, &fx).await;
    assert_post0070_links_and_files(&migrated, &fx).await;
    if let Some(other) = &fx.other_org {
        assert_post0070_rls(&migrated, &fx, other).await;
    }

    migrated.close().await;
    database.remove().await;
}
