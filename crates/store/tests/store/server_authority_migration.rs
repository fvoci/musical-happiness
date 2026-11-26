//! Migration `0078` (`server_authority`) and store-level invariants for atomic board
//! operations, `'Today'` queries, `due_soon` personal-space notifications, and Gantt
//! dependency cycle rejection.

use fvoci_model::{
    Block, BlockError, BlockPatch, BlockType, CardMovesRequest, CardPropertyDefinition,
    ContainerPatch, DiaryDate, DoneWhen, NewBoardView, PropertyChanges, PropertyColor,
    PropertyOption, PropertyType, RelationError, RelationProblem, TodayWorkspaceMeta, ViewType,
    new_id, utc_date,
};
use fvoci_store::{DbType, Store, StoreError, TodayWindow};
use serde_json::json;

use crate::common::{self, db_type};

struct Pre0078Fixture {
    sso_org_id: String,
    open_org_id: String,
    welcomed_ws_id: String,
    fresh_ws_id: String,
    alice: String,
    bob: String,
    timestamp_card_id: String,
}

async fn seed_pre0078(pre: &Store) -> Pre0078Fixture {
    let sys = pre.system();
    let (sso_org_id, open_org_id) = (new_id(), new_id());
    let (sso_ws_id, welcomed_ws_id, fresh_ws_id) = (new_id(), new_id(), new_id());
    let (alice, bob, provider_id) = (new_id(), new_id(), new_id());
    let (board_id, calendar_card_id, timestamp_card_id) = (new_id(), new_id(), new_id());

    for sql in [
        format!(
            "INSERT INTO users (id, username, email, email_verified_at, password_hash, created_at, updated_at, is_admin, display_name) \
             VALUES ('{alice}', 'alice0078', 'alice@example.com', 1000, 'hash', 1000, 1000, true, 'Alice')"
        ),
        format!(
            "INSERT INTO users (id, username, email, email_verified_at, password_hash, created_at, updated_at, is_admin, display_name) \
             VALUES ('{bob}', 'bob0078', 'bob@example.com', 1000, 'hash', 1000, 1000, false, 'Bob')"
        ),
        format!(
            "INSERT INTO organizations (id, name, kind, settings, created_at, updated_at) \
             VALUES ('{sso_org_id}', 'SSO Org', 'shared', '{{}}', 1100, 1100)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{sso_ws_id}', '{sso_org_id}', 'SSO Main', '{{}}', 1100, 1100)"
        ),
        format!(
            "INSERT INTO oidc_providers (id, org_id, issuer, client_id, client_secret, sso_required, created_at, updated_at) \
             VALUES ('{provider_id}', '{sso_org_id}', 'https://sso.example.com', 'cid', 'sec', TRUE, 1100, 1100)"
        ),
        format!(
            "INSERT INTO organizations (id, name, kind, settings, created_at, updated_at) \
             VALUES ('{open_org_id}', 'Open Org', 'shared', '{{}}', 1200, 1200)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{welcomed_ws_id}', '{open_org_id}', 'Welcomed', '{{}}', 1200, 1200)"
        ),
        format!(
            "INSERT INTO workspaces (id, org_id, name, settings, created_at, updated_at) \
             VALUES ('{fresh_ws_id}', '{open_org_id}', 'Fresh', '{{}}', 1300, 1300)"
        ),
        format!(
            "INSERT INTO organization_members (org_id, user_id, role, created_at) \
             VALUES ('{open_org_id}', '{alice}', 'admin', 1200)"
        ),
        format!(
            "INSERT INTO organization_members (org_id, user_id, role, created_at) \
             VALUES ('{open_org_id}', '{bob}', 'member', 1200)"
        ),
    ] {
        if db_type() == DbType::Sqlite && sql.contains(&sso_org_id) {
            continue;
        }
        sys.execute_unchecked(&sql)
            .await
            .expect("seed pre-0078 global");
    }

    seed_pre0078_board_and_cards(
        pre,
        &open_org_id,
        &welcomed_ws_id,
        &alice,
        &bob,
        &board_id,
        &calendar_card_id,
        &timestamp_card_id,
    )
    .await;

    Pre0078Fixture {
        sso_org_id,
        open_org_id,
        welcomed_ws_id,
        fresh_ws_id,
        alice,
        bob,
        timestamp_card_id,
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "test fixture helper passing pre-generated identifiers"
)]
async fn seed_pre0078_board_and_cards(
    pre: &Store,
    org_id: &str,
    ws_id: &str,
    alice: &str,
    bob: &str,
    board_id: &str,
    calendar_card_id: &str,
    timestamp_card_id: &str,
) {
    let scoped = pre.for_org(org_id).expect("org handle");
    let card_props = json!([
        {"id": "p_owner", "name": "Owner", "type": "person", "options": []},
        {"id": "p_watchers", "name": "Watchers", "type": "multiPerson", "options": []},
        {"id": "p_due", "name": "Due", "type": "date", "options": []}
    ])
    .to_string()
    .replace('\'', "''");
    let cal_fields = json!({
        "properties": {
            "p_owner": alice,
            "p_due": {"from": "2026-11-26"}
        }
    })
    .to_string()
    .replace('\'', "''");
    let ts_millis: i64 = 1_795_737_600_000;
    let ts_fields = json!({
        "properties": {
            "p_watchers": [alice, bob],
            "p_due": {"from": ts_millis}
        }
    })
    .to_string()
    .replace('\'', "''");

    // A card whose values are not of their properties' kinds: a time past what a bigint
    // holds, and one person where several are. The migration passes over them.
    let odd_card_id = new_id();
    let odd_fields = format!(
        r#"{{"properties":{{"p_owner":"{alice}","p_watchers":"{bob}","p_due":{{"from":123456789012345678901234}}}}}}"#
    );
    let cast = if db_type() == DbType::Postgres {
        ("::jsonb", "::json")
    } else {
        ("", "")
    };
    for sql in [
        format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
             VALUES ('{ws_id}', '{alice}', 'admin', 1200)"
        ),
        format!(
            "INSERT INTO workspace_members (workspace_id, user_id, role, created_at) \
             VALUES ('{ws_id}', '{bob}', 'member', 1200)"
        ),
        format!(
            "INSERT INTO containers (id, workspace_id, created_by, modified_by, visibility, minimum_role, kind, title, description, icon, show_description, is_template, is_builtin, template_version, properties, card_properties, created_at, updated_at) \
             VALUES ('{board_id}', '{ws_id}', '{alice}', '{alice}', 'open', 'editor', 'board', 'Roadmap', '', '', FALSE, FALSE, FALSE, 0, '{{}}'{}, '{card_props}'{}, 1250, 1250)",
            cast.0, cast.0
        ),
        format!(
            "INSERT INTO container_members (container_id, user_id, role) \
             VALUES ('{board_id}', '{alice}', 'admin')"
        ),
        format!(
            "INSERT INTO blocks (id, parent_id, container_id, created_by, modified_by, type, title, fields, created_at, updated_at) \
             VALUES ('{calendar_card_id}', NULL, '{board_id}', '{alice}', '{alice}', 'card', 'Ship 0078', '{cal_fields}'{}, 1260, 1260)",
            cast.1
        ),
        format!(
            "INSERT INTO blocks (id, parent_id, container_id, created_by, modified_by, type, title, fields, created_at, updated_at) \
             VALUES ('{timestamp_card_id}', NULL, '{board_id}', '{alice}', '{alice}', 'card', 'Timestamped', '{ts_fields}'{}, 1270, 1270)",
            cast.1
        ),
        format!(
            "INSERT INTO blocks (id, parent_id, container_id, created_by, modified_by, type, title, fields, created_at, updated_at) \
             VALUES ('{odd_card_id}', NULL, '{board_id}', '{alice}', '{alice}', 'card', 'Odd values', '{odd_fields}'{}, 1280, 1280)",
            cast.1
        ),
    ] {
        scoped
            .execute_unchecked(&sql)
            .await
            .expect("seed pre-0078 org rows");
    }
}

#[tokio::test]
async fn migration_0078_backfills_welcomed_allow_copy_out_assignees_and_due_columns() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_server_authority(&database).await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open at 0077");
    let fx = seed_pre0078(&pre).await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migration 0078 applies cleanly");
    let open_org = migrated.for_org(&fx.open_org_id).expect("open org");

    assert!(
        open_org
            .is_workspace_welcomed(&fx.welcomed_ws_id)
            .await
            .expect("welcomed check"),
        "workspace with existing container is marked welcomed"
    );
    assert!(
        !open_org
            .is_workspace_welcomed(&fx.fresh_ws_id)
            .await
            .expect("fresh check"),
        "empty workspace stays unwelcomed"
    );
    assert!(
        open_org
            .organization()
            .await
            .expect("open org")
            .allow_copy_out,
        "non-SSO org defaults allow_copy_out = true"
    );
    if db_type() == DbType::Postgres {
        let sso_org = migrated.for_org(&fx.sso_org_id).expect("sso org");
        assert!(
            !sso_org
                .organization()
                .await
                .expect("sso org")
                .allow_copy_out,
            "SSO-enforced org defaults allow_copy_out = false"
        );
    }

    let meta = TodayWorkspaceMeta {
        workspace_id: fx.welcomed_ws_id.clone(),
        workspace_name: "Welcomed".to_owned(),
        org_id: fx.open_org_id.clone(),
        org_name: "Open Org".to_owned(),
        is_personal: false,
    };
    let today: DiaryDate = "2026-11-26".parse().expect("today");
    let window = TodayWindow {
        today,
        soon_date: "2099-12-31",
        sql_max_due_at: "2099-12-31",
        now_millis: 1_000,
        meetings_until_millis: 2_000,
        day_of_millis: utc_date,
    };
    let alice_today = open_org
        .workspace_today(&fx.welcomed_ws_id, meta.clone(), &fx.alice, &window)
        .await
        .expect("alice today");
    assert_eq!(
        alice_today.cards.len(),
        2,
        "Alice sees both person and multiPerson backfilled cards, and no card of a time \
         past the years of a calendar"
    );
    let bob_today = open_org
        .workspace_today(&fx.welcomed_ws_id, meta, &fx.bob, &window)
        .await
        .expect("bob today");
    assert_eq!(
        bob_today.cards.len(),
        1,
        "unjoined workspace member Bob sees multiPerson card on open board"
    );
    assert_eq!(bob_today.cards[0].card_id, fx.timestamp_card_id);
    assert!(bob_today.cards[0].due_at_millis.is_some());

    migrated.close().await;
    database.remove().await;
}

#[tokio::test]
async fn board_ops_views_card_moves_done_when_and_gantt_cycle_rejection() {
    let test = common::setup().await;
    let alice = common::user(&test, "alice").await;
    let _bob = common::user(&test, "bob").await;
    let boards = common::create_test_boards(&test, &test.workspace_id, &alice, 1).await;
    let board_id = &boards[0].id;

    let status_id = new_id();
    let todo_id = new_id();
    let done_id = new_id();
    let dep_id = new_id();
    let due_id = new_id();
    let owner_id = new_id();

    test.patch_container(
        board_id,
        &ContainerPatch {
            updated_card_properties: vec![
                CardPropertyDefinition {
                    id: status_id.clone(),
                    name: "Status".to_owned(),
                    property_type: PropertyType::Select,
                    options: vec![
                        PropertyOption::new(todo_id.clone(), "Todo", PropertyColor::Gray),
                        PropertyOption::new(done_id.clone(), "Done", PropertyColor::Green),
                    ],
                },
                CardPropertyDefinition::new(dep_id.clone(), "Depends on", PropertyType::Relation),
                CardPropertyDefinition::new(due_id.clone(), "Due", PropertyType::Date),
                CardPropertyDefinition::new(owner_id.clone(), "Owner", PropertyType::Person),
            ],
            done_when: Some(Some(DoneWhen {
                property_id: status_id.clone(),
                option_ids: vec![done_id.clone()],
            })),
            ..ContainerPatch::default()
        },
        &alice,
    )
    .await
    .expect("patched board");

    let mut gantt_fields = fvoci_model::JsonMap::new();
    gantt_fields.insert(
        "dependencyPropertyId".to_owned(),
        serde_json::Value::String(dep_id.clone()),
    );
    let view_write = test
        .create_board_view(
            board_id,
            &NewBoardView {
                title: "Timeline".to_owned(),
                view_type: ViewType::Gantt,
                after: None,
                fields: Some(gantt_fields),
            },
            &alice,
        )
        .await
        .expect("created gantt view");

    let mut c1 = Block {
        id: new_id(),
        container_id: board_id.clone(),
        block_type: BlockType::Card,
        title: "Card 1".to_owned(),
        ..Block::default()
    };
    let mut c2 = Block {
        id: new_id(),
        container_id: board_id.clone(),
        block_type: BlockType::Card,
        title: "Card 2".to_owned(),
        ..Block::default()
    };
    test.insert_blocks(std::slice::from_mut(&mut c1), &alice)
        .await
        .expect("c1");
    test.insert_blocks(std::slice::from_mut(&mut c2), &alice)
        .await
        .expect("c2");

    assert_cycle(&test, &c1.id, &c2.id, &dep_id, &alice).await;
    verify_card_moves_and_batch_ops(
        &test,
        board_id,
        &view_write.view.id,
        &c1.id,
        &c2.id,
        status_id,
        done_id,
        &alice,
    )
    .await;

    test.teardown().await;
}

async fn assert_cycle(
    test: &common::TestStore,
    c1_id: &str,
    c2_id: &str,
    dep_id: &str,
    alice: &str,
) {
    let mut props_2 = fvoci_model::JsonMap::new();
    props_2.insert(dep_id.to_owned(), json!([{"card": c1_id}]));
    test.patch_block(
        c2_id,
        &BlockPatch {
            property_changes: PropertyChanges {
                updated: props_2,
                deleted: Vec::new(),
            },
            ..BlockPatch::default()
        },
        alice,
    )
    .await
    .expect("c2 -> c1 allowed");

    let mut props_1 = fvoci_model::JsonMap::new();
    props_1.insert(dep_id.to_owned(), json!([{"card": c2_id}]));
    let cycle_err = test
        .patch_block(
            c1_id,
            &BlockPatch {
                property_changes: PropertyChanges {
                    updated: props_1,
                    deleted: Vec::new(),
                },
                ..BlockPatch::default()
            },
            alice,
        )
        .await
        .expect_err("cycle rejected");
    assert!(
        matches!(
            cycle_err,
            StoreError::InvalidBlock {
                source: BlockError::InvalidRelation(RelationError {
                    problem: RelationProblem::Cycle,
                    ..
                }),
                ..
            }
        ),
        "expected RelationProblem::Cycle, got {cycle_err:?}"
    );
}

#[expect(
    clippy::too_many_arguments,
    reason = "test helper groups board, view, card, and status identifiers"
)]
async fn verify_card_moves_and_batch_ops(
    test: &common::TestStore,
    board_id: &str,
    view_id: &str,
    c1_id: &str,
    c2_id: &str,
    status_id: String,
    done_id: String,
    alice: &str,
) {
    let (moved_view, moved_cards) = test
        .move_cards_in_view(
            board_id,
            view_id,
            &CardMovesRequest {
                card_ids: vec![c2_id.to_owned()],
                group_property_id: Some(status_id),
                group_value: Some(json!(done_id)),
                after_card_id: Some(String::new()),
            },
            alice,
        )
        .await
        .expect("card-moves");
    assert_eq!(moved_cards.len(), 1, "one card updated");
    assert_eq!(
        moved_view.fields.get("cardOrder"),
        Some(&json!([c2_id, c1_id])),
        "view cardOrder updated"
    );

    let ids = [c1_id.to_owned(), c2_id.to_owned()];
    let deleted = test
        .batch_delete_cards(board_id, &ids, alice)
        .await
        .expect("batch delete");
    assert_eq!(deleted.len(), 2, "two cards deleted");
    let restored = test
        .batch_restore_cards(board_id, &ids, alice)
        .await
        .expect("batch restore");
    assert_eq!(restored.len(), 2, "two cards restored");
}
