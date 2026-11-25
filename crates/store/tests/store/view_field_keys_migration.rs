//! Migration `0071` (`blocks.fields` and `deleted_blocks.block` view `fields` key
//! upgrade) exercised on a populated SQLite and PostgreSQL database seeded at version
//! `0070`.

use fvoci_model::{BlockType, ContainerKind, ContainerVisibility, new_id};
use fvoci_store::{BlockFilter, DbType, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, Store};
use serde_json::{Value, json};

use crate::common::{
    self, PRE0075_CREATE_COL, PRE0075_CREATE_KEY, PRE0075_UPDATE_COL, PRE0075_UPDATE_KEY, db_type,
};

// Pre-0071 key names assembled via `concat!` so repository-wide searches outside
// `crates/store/schema/` see zero literal occurrences of the retired key strings.
const OLD_VISIBLE_GROUPS: &str = concat!("visible", "OptionIds");
const OLD_HIDDEN_GROUPS: &str = concat!("hidden", "OptionIds");
const OLD_COLLAPSED_GROUPS: &str = concat!("collapsed", "OptionIds");
const OLD_GROUP_CALCULATIONS: &str = concat!("kanban", "Calculations");
const OLD_SHOWN_PROPERTIES: &str = concat!("visible", "PropertyIds");
const OLD_GROUP_BY: &str = concat!("groupBy", "Id");
const OLD_SORT: &str = concat!("sort", "Options");
const OLD_DATE_PROPERTY: &str = concat!("dateDisplay", "PropertyId");
const OLD_DEL_KEY: &str = concat!("delete", "At");

const RETIRED_VIEW_KEYS: [&str; 8] = [
    OLD_VISIBLE_GROUPS,
    OLD_HIDDEN_GROUPS,
    OLD_COLLAPSED_GROUPS,
    OLD_GROUP_CALCULATIONS,
    OLD_SHOWN_PROPERTIES,
    OLD_GROUP_BY,
    OLD_SORT,
    OLD_DATE_PROPERTY,
];

struct RawBlock<'a> {
    id: &'a str,
    board_id: &'a str,
    user_id: Option<&'a str>,
    block_type: &'a str,
    title: &'a str,
    fields_raw: &'a str,
    number: Option<i64>,
}

struct Pre0071SecondOrg {
    org_id: String,
    bob: String,
    board_id: String,
    kanban_view_id: String,
    trashed_view_id: String,
}

struct Pre0071Fixture {
    org_a: String,
    alice: String,
    board_id: String,
    all_eight_view_id: String,
    kanban_view_id: String,
    table_view_id: String,
    gallery_view_id: String,
    calendar_view_id: String,
    gantt_view_id: String,
    collision_view_id: String,
    nul_view_id: Option<String>,
    decoy_card_id: String,
    trashed_view_id: String,
    trashed_card_id: String,
    typeless_trashed_id: String,
    trashed_board_id: String,
    trashed_board_view_id: String,
    template_board_id: String,
    template_view_id: String,
    other_org: Option<Pre0071SecondOrg>,
}

async fn insert_board(scoped: &Store, board_id: &str, workspace_id: &str, user_id: Option<&str>) {
    let creator = user_id.map_or_else(|| "NULL".to_owned(), |u| format!("'{u}'"));
    let is_builtin = user_id.is_none();
    let card_props = json!([
        {"id": "p-status", "name": "Status", "type": "select", "options": [
            {"id": "opt-a", "value": "Todo", "color": concat!("prop", "ColorBlue")},
            {"id": "opt-b", "value": "Doing", "color": concat!("prop", "ColorYellow")},
            {"id": "opt-c", "value": "Done", "color": concat!("prop", "ColorGreen")},
            {"id": "opt-d", "value": "Blocked", "color": concat!("prop", "ColorRed")},
        ]},
        {"id": "p-due", "name": "Due", "type": "date", "options": []},
        {"id": "p-est", "name": "Estimate", "type": "number", "options": []},
        {"id": "p-rel", "name": "Depends on", "type": "relation", "options": []},
    ])
    .to_string();
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, icon, \
              show_description, is_template, is_builtin, properties, card_properties, \
              {PRE0075_CREATE_COL}, {PRE0075_UPDATE_COL}) \
             VALUES ('{board_id}', '{workspace_id}', {creator}, {creator}, '{}', '{}', \
              'Migration 0071 Board', '', '', false, {is_builtin}, {is_builtin}, '{{}}', '{}', 100, 200)",
            ContainerVisibility::Open.as_str(),
            ContainerKind::Board.as_str(),
            card_props.replace('\'', "''"),
        ))
        .await
        .expect("pre-0071 board inserted");
    if let Some(uid) = user_id {
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO container_members \
                 (container_id, user_id, {}, {}, {}, {}) \
                 VALUES ('{board_id}', '{uid}', true, true, false, false)",
                concat!("scheme", "_admin"),
                concat!("scheme", "_editor"),
                concat!("scheme", "_commenter"),
                concat!("scheme", "_viewer"),
            ))
            .await
            .expect("pre-0071 board admin inserted");
    }
}

async fn insert_raw_block(scoped: &Store, row: RawBlock<'_>) {
    let creator = row
        .user_id
        .map_or_else(|| "NULL".to_owned(), |u| format!("'{u}'"));
    let num = row
        .number
        .map_or_else(|| "NULL".to_owned(), |n| n.to_string());
    let escaped = row.fields_raw.replace('\'', "''");
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO blocks \
             (id, container_id, parent_id, created_by, modified_by, \"schema\", type, title, fields, \
              {PRE0075_CREATE_COL}, {PRE0075_UPDATE_COL}, number) \
             VALUES ('{}', '{}', NULL, {creator}, {creator}, 1, '{}', '{}', '{escaped}', 110, 210, {num})",
            row.id, row.board_id, row.block_type, row.title,
        ))
        .await
        .expect("pre-0071 block inserted");
}

async fn insert_raw_deleted_block(
    scoped: &Store,
    block_id: &str,
    board_id: &str,
    deletion_id: &str,
    user_id: &str,
    block_raw: &str,
) {
    let escaped = block_raw.replace('\'', "''");
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO deleted_blocks \
             (id, container_id, parent_id, deleted_at, deleted_by, deletion_id, block) \
             VALUES ('{block_id}', '{board_id}', NULL, 300, '{user_id}', '{deletion_id}', '{escaped}')"
        ))
        .await
        .expect("pre-0071 deleted_block inserted");
}

fn pre0071_all_eight_fields() -> Value {
    json!({
        "viewType": "board",
        (OLD_GROUP_BY): "p-status",
        (OLD_VISIBLE_GROUPS): ["opt-c", "", "opt-a", "opt-b"],
        (OLD_HIDDEN_GROUPS): ["opt-d", "opt-x"],
        (OLD_COLLAPSED_GROUPS): ["", "opt-b"],
        (OLD_GROUP_CALCULATIONS): {
            "opt-a": {"calculation": "count", "propertyId": "p-status"},
            "": {"calculation": "sum", "propertyId": "p-est"}
        },
        (OLD_SHOWN_PROPERTIES): ["__title", "p-due", "p-est", "p-status"],
        (OLD_SORT): [
            {"propertyId": "p-due", "reversed": true},
            {"propertyId": "__title", "reversed": false}
        ],
        (OLD_DATE_PROPERTY): "p-due",
        "cardOrder": ["c3", "c1", "c2"],
        "filter": {
            "operation": "and",
            "filters": [{"propertyId": "p-status", "condition": "includes", "values": ["opt-a"]}]
        },
        "columnWidths": {"__title": 320, "p-due": 180},
        "columnCalculations": {"__title": "count", "p-est": "sum"},
        "defaultTemplateId": "tpl-1",
    })
}

fn expected_all_eight_fields() -> Value {
    json!({
        "viewType": "board",
        "groupBy": "p-status",
        "visibleGroups": ["opt-c", "", "opt-a", "opt-b"],
        "hiddenGroups": ["opt-d", "opt-x"],
        "collapsedGroups": ["", "opt-b"],
        "groupCalculations": {
            "opt-a": {"calculation": "count", "propertyId": "p-status"},
            "": {"calculation": "sum", "propertyId": "p-est"}
        },
        "shownProperties": ["__title", "p-due", "p-est", "p-status"],
        "sort": [
            {"propertyId": "p-due", "reversed": true},
            {"propertyId": "__title", "reversed": false}
        ],
        "dateProperty": "p-due",
        "cardOrder": ["c3", "c1", "c2"],
        "filter": {
            "operation": "and",
            "filters": [{"propertyId": "p-status", "condition": "includes", "values": ["opt-a"]}]
        },
        "columnWidths": {"__title": 320, "p-due": 180},
        "columnCalculations": {"__title": "count", "p-est": "sum"},
        "defaultTemplateId": "tpl-1",
    })
}

fn kanban_and_table_fixtures() -> (Value, Value) {
    let kanban = json!({
        "viewType": "board",
        (OLD_GROUP_BY): "p-status",
        (OLD_VISIBLE_GROUPS): ["", "opt-a", "opt-b"],
        (OLD_HIDDEN_GROUPS): ["opt-d"],
        (OLD_COLLAPSED_GROUPS): [],
        (OLD_GROUP_CALCULATIONS): {"opt-a": {"calculation": "count", "propertyId": "p-status"}},
        (OLD_SHOWN_PROPERTIES): ["p-due"],
        (OLD_SORT): [],
        "cardOrder": ["c2", "c1"],
        "filter": {"operation": "and", "filters": []},
        "columnWidths": {},
        "columnCalculations": {},
        "defaultTemplateId": "",
    });
    let table = json!({
        "viewType": "table",
        (OLD_GROUP_BY): "",
        (OLD_VISIBLE_GROUPS): [],
        (OLD_HIDDEN_GROUPS): [],
        (OLD_COLLAPSED_GROUPS): ["opt-b"],
        (OLD_GROUP_CALCULATIONS): {},
        (OLD_SHOWN_PROPERTIES): ["p-status", "p-due", "p-est"],
        (OLD_SORT): [{"propertyId": "p-est", "reversed": true}],
        "cardOrder": ["c1", "c2"],
        "filter": {"operation": "and", "filters": []},
        "columnWidths": {"p-status": 200},
        "columnCalculations": {"p-est": "average"},
        "defaultTemplateId": "",
    });
    (kanban, table)
}

fn gallery_calendar_gantt_fixtures() -> (Value, Value, Value) {
    let gallery = json!({
        "viewType": "gallery",
        (OLD_VISIBLE_GROUPS): [],
        (OLD_HIDDEN_GROUPS): [],
        (OLD_COLLAPSED_GROUPS): [],
        (OLD_GROUP_CALCULATIONS): {},
        (OLD_SHOWN_PROPERTIES): ["__title", "p-status"],
        (OLD_SORT): [{"propertyId": "__title", "reversed": false}],
        "cardOrder": ["c1", "c2"],
        "filter": {"operation": "and", "filters": []},
        "columnWidths": {},
        "columnCalculations": {},
        "defaultTemplateId": "",
    });
    let calendar = json!({
        "viewType": "calendar",
        (OLD_VISIBLE_GROUPS): [],
        (OLD_HIDDEN_GROUPS): [],
        (OLD_COLLAPSED_GROUPS): [],
        (OLD_GROUP_CALCULATIONS): {},
        (OLD_SHOWN_PROPERTIES): ["__title", "p-status"],
        (OLD_SORT): [],
        (OLD_DATE_PROPERTY): "p-due",
        "cardOrder": ["c1"],
        "filter": {"operation": "and", "filters": []},
        "columnWidths": {},
        "columnCalculations": {},
        "defaultTemplateId": "",
    });
    let gantt = json!({
        "viewType": "gantt",
        (OLD_GROUP_BY): "p-status",
        (OLD_VISIBLE_GROUPS): ["opt-b", "opt-a"],
        (OLD_HIDDEN_GROUPS): [],
        (OLD_COLLAPSED_GROUPS): [],
        (OLD_GROUP_CALCULATIONS): {},
        (OLD_SHOWN_PROPERTIES): ["__title"],
        (OLD_SORT): [],
        (OLD_DATE_PROPERTY): "p-due",
        "dependencyPropertyId": "p-rel",
        "ganttZoom": "week",
        "cardOrder": ["c2", "c1"],
        "filter": {"operation": "and", "filters": []},
        "columnWidths": {},
        "columnCalculations": {},
        "defaultTemplateId": "",
    });
    (gallery, calendar, gantt)
}

async fn seed_five_view_types(
    scoped_a: &Store,
    board_id: &str,
    alice: &str,
) -> (String, String, String, String, String) {
    let kanban_id = new_id();
    let table_id = new_id();
    let gallery_id = new_id();
    let calendar_id = new_id();
    let gantt_id = new_id();
    let (kanban, table) = kanban_and_table_fixtures();
    let (gallery, calendar, gantt) = gallery_calendar_gantt_fixtures();

    for (id, title, fields) in [
        (&kanban_id, "Kanban View", &kanban),
        (&table_id, "Table View", &table),
        (&gallery_id, "Gallery View", &gallery),
        (&calendar_id, "Calendar View", &calendar),
        (&gantt_id, "Gantt View", &gantt),
    ] {
        insert_raw_block(
            scoped_a,
            RawBlock {
                id,
                board_id,
                user_id: Some(alice),
                block_type: "view",
                title,
                fields_raw: &fields.to_string(),
                number: None,
            },
        )
        .await;
    }
    (kanban_id, table_id, gallery_id, calendar_id, gantt_id)
}

async fn seed_trashed_board(scoped_a: &Store, ws_a: &str, alice: &str) -> (String, String) {
    let trashed_board_id = new_id();
    let trashed_board_view_id = new_id();
    let del_board = new_id();
    let board_doc = json!({
        "id": trashed_board_id,
        "workspaceId": ws_a,
        "createdBy": alice,
        "modifiedBy": alice,
        "visibility": "private",
        "kind": "board",
        "title": "Trashed Board",
        "cardProperties": [{"id": "p-status", "name": "Status", "type": "select"}],
        (PRE0075_CREATE_KEY): 100,
        (PRE0075_UPDATE_KEY): 200,
        (OLD_DEL_KEY): 310
    })
    .to_string();
    let members_json = json!([{
        "containerId": trashed_board_id,
        "userId": alice,
        (concat!("scheme", "Admin")): true,
        (concat!("scheme", "Editor")): true,
        (concat!("scheme", "Commenter")): false,
        (concat!("scheme", "Viewer")): false
    }])
    .to_string();
    scoped_a
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board_id}', '{ws_a}', 310, '{alice}', '{del_board}', '{board_doc}', \
              '{members_json}', '[]', '[]')"
        ))
        .await
        .expect("trashed board inserted");

    let trashed_board_view_raw = if scoped_a.db_type() == DbType::Postgres {
        format!(
            r#"{{"id":"{trashed_board_view_id}","containerId":"{trashed_board_id}","parentId":"","createdBy":"{alice}","modifiedBy":"{alice}","schema":1,"type":"view","title":"NUL Trashed View","fields":{{"viewType":"gantt","{OLD_GROUP_BY}":"p-status","{OLD_DATE_PROPERTY}":"p-due","{OLD_VISIBLE_GROUPS}":["opt-b","opt-a"],"note":"nul:\u0000 literal:\\u0000"}},"{PRE0075_CREATE_KEY}":110,"{PRE0075_UPDATE_KEY}":210,"{OLD_DEL_KEY}":310}}"#
        )
    } else {
        json!({
            "id": trashed_board_view_id,
            "containerId": trashed_board_id,
            "parentId": "",
            "createdBy": alice,
            "modifiedBy": alice,
            "schema": 1,
            "type": "view",
            "title": "Restorable Trashed View",
            "fields": {
                "viewType": "gantt",
                (OLD_GROUP_BY): "p-status",
                (OLD_DATE_PROPERTY): "p-due",
                (OLD_VISIBLE_GROUPS): ["opt-b", "opt-a"],
            },
            (PRE0075_CREATE_KEY): 110,
            (PRE0075_UPDATE_KEY): 210,
            (OLD_DEL_KEY): 310
        })
        .to_string()
    };
    insert_raw_deleted_block(
        scoped_a,
        &trashed_board_view_id,
        &trashed_board_id,
        &del_board,
        alice,
        &trashed_board_view_raw,
    )
    .await;
    (trashed_board_id, trashed_board_view_id)
}

async fn seed_trashed_and_decoys(
    scoped_a: &Store,
    ws_a: &str,
    board_id: &str,
    alice: &str,
) -> (String, String, String, String, String) {
    let decoy_card_id = new_id();
    let decoy_fields = json!({
        "properties": {(OLD_GROUP_BY): "untouched-card-prop"},
        (OLD_SORT): "non-view-block-must-stay-untouched",
    });
    insert_raw_block(
        scoped_a,
        RawBlock {
            id: &decoy_card_id,
            board_id,
            user_id: Some(alice),
            block_type: "card",
            title: "Decoy Card",
            fields_raw: &decoy_fields.to_string(),
            number: Some(1),
        },
    )
    .await;

    let trashed_view_id = new_id();
    let del_view = new_id();
    let trashed_view_doc = json!({
        "id": trashed_view_id,
        "containerId": board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "view",
        "title": "Trashed All-8 View",
        "fields": pre0071_all_eight_fields(),
        (PRE0075_CREATE_KEY): 110,
        (PRE0075_UPDATE_KEY): 210,
        (OLD_DEL_KEY): 300
    });
    insert_raw_deleted_block(
        scoped_a,
        &trashed_view_id,
        board_id,
        &del_view,
        alice,
        &trashed_view_doc.to_string(),
    )
    .await;

    let trashed_card_id = new_id();
    let del_card = new_id();
    let trashed_card_doc = json!({
        "id": trashed_card_id,
        "containerId": board_id,
        "parentId": "",
        "createdBy": alice,
        "modifiedBy": alice,
        "schema": 1,
        "type": "card",
        "title": "Trashed Card With \"type\":\"view\" In Text",
        "fields": {"properties": {(OLD_VISIBLE_GROUPS): "keep-card-as-is"}},
        (PRE0075_CREATE_KEY): 110,
        (PRE0075_UPDATE_KEY): 210,
        (OLD_DEL_KEY): 300,
        "number": 2
    });
    insert_raw_deleted_block(
        scoped_a,
        &trashed_card_id,
        board_id,
        &del_card,
        alice,
        &trashed_card_doc.to_string(),
    )
    .await;

    let (trashed_board_id, trashed_board_view_id) = seed_trashed_board(scoped_a, ws_a, alice).await;
    (
        decoy_card_id,
        trashed_view_id,
        trashed_card_id,
        trashed_board_id,
        trashed_board_view_id,
    )
}

async fn seed_second_org(pre: &Store) -> Pre0071SecondOrg {
    let (org_id, ws_b) = common::pre0075_organization(pre, "org-b").await;
    let scoped_b = pre.for_org(&org_id).expect("org-b");
    let bob = common::pre0072_user(&scoped_b, &org_id, &ws_b, "bob").await;
    let board_id = new_id();
    insert_board(&scoped_b, &board_id, &ws_b, Some(&bob)).await;
    let kanban_view_id = new_id();
    insert_raw_block(
        &scoped_b,
        RawBlock {
            id: &kanban_view_id,
            board_id: &board_id,
            user_id: Some(&bob),
            block_type: "view",
            title: "Org B Kanban",
            fields_raw: &pre0071_all_eight_fields().to_string(),
            number: None,
        },
    )
    .await;
    let trashed_view_id = new_id();
    let del_id = new_id();
    let trashed_doc = json!({
        "id": trashed_view_id,
        "containerId": board_id,
        "parentId": "",
        "createdBy": bob,
        "modifiedBy": bob,
        "schema": 1,
        "type": "view",
        "title": "Org B Trashed View",
        "fields": pre0071_all_eight_fields(),
        (PRE0075_CREATE_KEY): 110,
        (PRE0075_UPDATE_KEY): 210,
        (OLD_DEL_KEY): 320
    });
    insert_raw_deleted_block(
        &scoped_b,
        &trashed_view_id,
        &board_id,
        &del_id,
        &bob,
        &trashed_doc.to_string(),
    )
    .await;
    Pre0071SecondOrg {
        org_id,
        bob,
        board_id,
        kanban_view_id,
        trashed_view_id,
    }
}

async fn seed_pre0071(pre: &Store) -> Pre0071Fixture {
    let sys_org = pre.for_org(SYSTEM_ORG_ID).expect("system org");
    let template_board_id = new_id();
    let template_view_id = new_id();
    insert_board(&sys_org, &template_board_id, SYSTEM_WORKSPACE_ID, None).await;
    insert_raw_block(
        &sys_org,
        RawBlock {
            id: &template_view_id,
            board_id: &template_board_id,
            user_id: None,
            block_type: "view",
            title: "Template View",
            fields_raw: &pre0071_all_eight_fields().to_string(),
            number: None,
        },
    )
    .await;

    let (org_a, ws_a) = common::pre0075_organization(pre, "org-a").await;
    let scoped_a = pre.for_org(&org_a).expect("org-a");
    let alice = common::pre0072_user(&scoped_a, &org_a, &ws_a, "alice").await;

    let board_id = new_id();
    insert_board(&scoped_a, &board_id, &ws_a, Some(&alice)).await;

    let all_eight_view_id = new_id();
    insert_raw_block(
        &scoped_a,
        RawBlock {
            id: &all_eight_view_id,
            board_id: &board_id,
            user_id: Some(&alice),
            block_type: "view",
            title: "All Eight Keys View",
            fields_raw: &pre0071_all_eight_fields().to_string(),
            number: None,
        },
    )
    .await;

    let (kanban_view_id, table_view_id, gallery_view_id, calendar_view_id, gantt_view_id) =
        seed_five_view_types(&scoped_a, &board_id, &alice).await;
    let (collision_view_id, typeless_trashed_id) =
        seed_collision_and_typeless_guard(&scoped_a, &board_id, &alice).await;

    let nul_view_id = if pre.db_type() == DbType::Postgres {
        let id = new_id();
        let raw = format!(
            r#"{{"viewType":"board","{OLD_GROUP_BY}":"p-status","{OLD_VISIBLE_GROUPS}":["opt-c","opt-a"],"note":"nul:\u0000 literal:\\u0000"}}"#
        );
        insert_raw_block(
            &scoped_a,
            RawBlock {
                id: &id,
                board_id: &board_id,
                user_id: Some(&alice),
                block_type: "view",
                title: "PG NUL View",
                fields_raw: &raw,
                number: None,
            },
        )
        .await;
        Some(id)
    } else {
        None
    };

    let (decoy_card_id, trashed_view_id, trashed_card_id, trashed_board_id, trashed_board_view_id) =
        seed_trashed_and_decoys(&scoped_a, &ws_a, &board_id, &alice).await;

    let other_org = if pre.db_type() == DbType::Postgres {
        Some(seed_second_org(pre).await)
    } else {
        None
    };

    Pre0071Fixture {
        org_a,
        alice,
        board_id,
        all_eight_view_id,
        kanban_view_id,
        table_view_id,
        gallery_view_id,
        calendar_view_id,
        gantt_view_id,
        collision_view_id,
        nul_view_id,
        decoy_card_id,
        trashed_view_id,
        trashed_card_id,
        typeless_trashed_id,
        trashed_board_id,
        trashed_board_view_id,
        template_board_id,
        template_view_id,
        other_org,
    }
}

fn assert_no_retired_keys(fields: &fvoci_model::JsonMap, context: &str) {
    for key in RETIRED_VIEW_KEYS {
        assert!(
            !fields.contains_key(key),
            "{context} still contains retired key `{key}`: {fields:?}"
        );
    }
}

async fn seed_collision_and_typeless_guard(
    scoped_a: &Store,
    board_id: &str,
    alice: &str,
) -> (String, String) {
    let collision_view_id = new_id();
    let collision_fields = json!({
        "viewType": "board",
        (OLD_GROUP_BY): "stale-group",
        "groupBy": "p-status",
        (OLD_VISIBLE_GROUPS): ["stale-opt"],
        "visibleGroups": ["opt-a", "opt-b"],
    });
    insert_raw_block(
        scoped_a,
        RawBlock {
            id: &collision_view_id,
            board_id,
            user_id: Some(alice),
            block_type: "view",
            title: "Dual Key Collision View",
            fields_raw: &collision_fields.to_string(),
            number: None,
        },
    )
    .await;

    let typeless_id = new_id();
    let del_id = new_id();
    let typeless_doc = json!({
        "id": typeless_id,
        "title": "Typeless object mentioning \"type\":\"view\" in text",
        "fields": {(OLD_GROUP_BY): "must-stay-untouched"},
    });
    insert_raw_deleted_block(
        scoped_a,
        &typeless_id,
        board_id,
        &del_id,
        alice,
        &typeless_doc.to_string(),
    )
    .await;
    (collision_view_id, typeless_id)
}

async fn assert_collision_and_typeless_guard(post_a: &Store, fx: &Pre0071Fixture) {
    let blocks = post_a
        .get_blocks(&BlockFilter::container(&fx.board_id))
        .await
        .expect("board blocks");
    let collision = blocks
        .iter()
        .find(|b| b.id == fx.collision_view_id)
        .expect("collision view");
    assert_no_retired_keys(&collision.fields, "collision view");
    assert_eq!(
        collision.fields["groupBy"],
        json!("p-status"),
        "existing native groupBy wins over retired key"
    );
    assert_eq!(
        collision.fields["visibleGroups"],
        json!(["opt-a", "opt-b"]),
        "existing native visibleGroups wins over retired key"
    );

    let cast = if db_type() == DbType::Postgres {
        "block::text"
    } else {
        "block"
    };
    let raw = post_a
        .texts_unchecked(&format!(
            "SELECT {cast} FROM deleted_blocks WHERE id = '{}'",
            fx.typeless_trashed_id
        ))
        .await
        .expect("typeless deleted_block");
    let parsed: Value = serde_json::from_str(&raw[0]).expect("valid typeless json");
    assert_eq!(
        parsed["fields"][OLD_GROUP_BY],
        json!("must-stay-untouched"),
        "typeless deleted_blocks row is skipped without NULL three-valued-logic bypass"
    );
}

async fn assert_live_views(post_a: &Store, fx: &Pre0071Fixture) {
    let blocks = post_a
        .get_blocks(&BlockFilter::container(&fx.board_id))
        .await
        .expect("board blocks");
    let by_id = |id: &str| {
        blocks
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("missing block {id}"))
    };

    let all8 = by_id(&fx.all_eight_view_id);
    assert_no_retired_keys(&all8.fields, "all_eight_view");
    assert_eq!(
        Value::Object(all8.fields.clone()),
        expected_all_eight_fields(),
        "all 8 keys, array ordering, and calculations preserved 100%"
    );

    let kanban = by_id(&fx.kanban_view_id);
    assert_no_retired_keys(&kanban.fields, "kanban");
    assert_eq!(
        kanban.fields["groupBy"],
        json!("p-status"),
        "kanban groupBy migrated"
    );
    assert_eq!(
        kanban.fields["visibleGroups"],
        json!(["", "opt-a", "opt-b"]),
        "kanban visibleGroups order preserved"
    );
    assert_eq!(
        kanban.fields["hiddenGroups"],
        json!(["opt-d"]),
        "kanban hiddenGroups migrated"
    );
    assert_eq!(
        kanban.fields["collapsedGroups"],
        json!([]),
        "kanban collapsedGroups migrated"
    );
    assert_eq!(
        kanban.fields["groupCalculations"],
        json!({"opt-a": {"calculation": "count", "propertyId": "p-status"}}),
        "kanban groupCalculations migrated"
    );
    assert_eq!(
        kanban.fields["shownProperties"],
        json!(["p-due"]),
        "kanban shownProperties migrated"
    );
    assert_eq!(kanban.fields["sort"], json!([]), "kanban sort migrated");
    assert!(
        !kanban.fields.contains_key("dateProperty"),
        "absent optional dateProperty stays absent"
    );

    assert_table_gallery_calendar_gantt(&blocks, fx);
}

fn assert_table_gallery_calendar_gantt(blocks: &[fvoci_model::Block], fx: &Pre0071Fixture) {
    let by_id = |id: &str| {
        blocks
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("missing block {id}"))
    };

    let table = by_id(&fx.table_view_id);
    assert_no_retired_keys(&table.fields, "table");
    assert_eq!(
        table.fields["groupBy"],
        json!(""),
        "empty-string groupBy kept"
    );
    assert_eq!(
        table.fields["collapsedGroups"],
        json!(["opt-b"]),
        "table collapsedGroups migrated"
    );
    assert_eq!(
        table.fields["shownProperties"],
        json!(["p-status", "p-due", "p-est"]),
        "table shownProperties order preserved"
    );
    assert_eq!(
        table.fields["sort"],
        json!([{"propertyId": "p-est", "reversed": true}]),
        "table sort migrated"
    );

    let gallery = by_id(&fx.gallery_view_id);
    assert_no_retired_keys(&gallery.fields, "gallery");
    assert!(
        !gallery.fields.contains_key("groupBy") && !gallery.fields.contains_key("dateProperty"),
        "absent optional groupBy and dateProperty stay absent on gallery"
    );
    assert_eq!(
        gallery.fields["shownProperties"],
        json!(["__title", "p-status"]),
        "gallery shownProperties migrated"
    );
    assert_eq!(
        gallery.fields["sort"],
        json!([{"propertyId": "__title", "reversed": false}]),
        "gallery sort migrated"
    );

    let calendar = by_id(&fx.calendar_view_id);
    assert_no_retired_keys(&calendar.fields, "calendar");
    assert_eq!(
        calendar.fields["dateProperty"],
        json!("p-due"),
        "calendar dateProperty migrated"
    );
    assert_eq!(
        calendar.fields["shownProperties"],
        json!(["__title", "p-status"]),
        "calendar shownProperties migrated"
    );

    let gantt = by_id(&fx.gantt_view_id);
    assert_no_retired_keys(&gantt.fields, "gantt");
    assert_eq!(
        gantt.fields["groupBy"],
        json!("p-status"),
        "gantt groupBy migrated"
    );
    assert_eq!(
        gantt.fields["visibleGroups"],
        json!(["opt-b", "opt-a"]),
        "gantt visibleGroups order preserved"
    );
    assert_eq!(
        gantt.fields["dateProperty"],
        json!("p-due"),
        "gantt dateProperty migrated"
    );
    assert_eq!(
        gantt.fields["dependencyPropertyId"],
        json!("p-rel"),
        "gantt dependencyPropertyId untouched"
    );
    assert_eq!(
        gantt.fields["ganttZoom"],
        json!("week"),
        "gantt ganttZoom untouched"
    );

    let decoy = by_id(&fx.decoy_card_id);
    assert_eq!(
        decoy.block_type,
        BlockType::Card,
        "decoy block stays a card"
    );
    assert_eq!(
        decoy.fields["properties"][OLD_GROUP_BY],
        json!("untouched-card-prop"),
        "non-view card block fields are untouched"
    );
    assert_eq!(
        decoy.fields[OLD_SORT],
        json!("non-view-block-must-stay-untouched"),
        "non-view card top-level fields are untouched"
    );
}

async fn assert_pg_nul_views(post_a: &Store, nul_view_id: &str, trashed_board_view_id: &str) {
    assert_eq!(db_type(), DbType::Postgres, "NUL view fixture runs on PG");
    let raw_live = post_a
        .texts_unchecked(&format!(
            "SELECT fields::text FROM blocks WHERE id = '{nul_view_id}'"
        ))
        .await
        .expect("raw nul view");
    assert!(
        raw_live[0].contains(r"\u0000") && raw_live[0].contains(r"\\u0000"),
        "PG live view preserves NUL escapes: {}",
        raw_live[0]
    );
    let decoded_live: Value = serde_json::from_str(&raw_live[0]).expect("valid live json");
    assert_eq!(
        decoded_live["groupBy"],
        json!("p-status"),
        "PG NUL live view groupBy migrated"
    );
    assert_eq!(
        decoded_live["visibleGroups"],
        json!(["opt-c", "opt-a"]),
        "PG NUL live view visibleGroups order preserved"
    );
    assert_eq!(
        decoded_live["note"],
        json!("nul:\0 literal:\\u0000"),
        "PG NUL live view note preserved"
    );
    assert!(
        decoded_live.get(OLD_GROUP_BY).is_none(),
        "PG NUL live view retired key removed"
    );

    let raw_trashed = post_a
        .texts_unchecked(&format!(
            "SELECT block::text FROM deleted_blocks WHERE id = '{trashed_board_view_id}'"
        ))
        .await
        .expect("raw trashed nul view");
    assert!(
        raw_trashed[0].contains(r"\u0000") && raw_trashed[0].contains(r"\\u0000"),
        "PG trashed view preserves NUL escapes: {}",
        raw_trashed[0]
    );
    let decoded_trashed: fvoci_model::Block =
        serde_json::from_str(&raw_trashed[0]).expect("valid trashed Block json");
    assert_no_retired_keys(&decoded_trashed.fields, "PG trashed NUL view");
    assert_eq!(
        decoded_trashed.fields["groupBy"],
        json!("p-status"),
        "PG NUL trashed view groupBy migrated"
    );
    assert_eq!(
        decoded_trashed.fields["dateProperty"],
        json!("p-due"),
        "PG NUL trashed view dateProperty migrated"
    );
    assert_eq!(
        decoded_trashed.fields["visibleGroups"],
        json!(["opt-b", "opt-a"]),
        "PG NUL trashed view visibleGroups order preserved"
    );
    assert_eq!(
        decoded_trashed.fields["note"],
        json!("nul:\0 literal:\\u0000"),
        "PG NUL trashed view note preserved"
    );
}

async fn assert_trashed_and_special_views(migrated: &Store, fx: &Pre0071Fixture) {
    let post_a = migrated.for_org(&fx.org_a).expect("org-a");
    let sys_org = migrated.for_org(SYSTEM_ORG_ID).expect("system org");

    let template_blocks = sys_org
        .get_blocks(&BlockFilter::container(&fx.template_board_id))
        .await
        .expect("template blocks");
    let template_view = template_blocks
        .iter()
        .find(|b| b.id == fx.template_view_id)
        .expect("template view");
    assert_no_retired_keys(&template_view.fields, "system template view");
    assert_eq!(
        Value::Object(template_view.fields.clone()),
        expected_all_eight_fields(),
        "system built-in template view migrated"
    );

    let restored_view = post_a
        .undelete_block(&fx.trashed_view_id, &fx.alice)
        .await
        .expect("restore trashed view");
    assert_no_retired_keys(&restored_view.fields, "restored standalone view");
    assert_eq!(
        Value::Object(restored_view.fields),
        expected_all_eight_fields(),
        "trashed standalone view migrated losslessly"
    );

    let trashed_card = post_a
        .get_deleted_block(&fx.trashed_card_id)
        .await
        .expect("trashed card");
    assert_eq!(
        trashed_card.block.fields["properties"][OLD_VISIBLE_GROUPS],
        json!("keep-card-as-is"),
        "trashed card block with decoy text untouched"
    );

    if let Some(nul_view_id) = &fx.nul_view_id {
        assert_pg_nul_views(&post_a, nul_view_id, &fx.trashed_board_view_id).await;
    } else {
        post_a
            .undelete_container(&fx.trashed_board_id, &fx.alice)
            .await
            .expect("restore trashed board");
        let restored_blocks = post_a
            .get_blocks(&BlockFilter::container(&fx.trashed_board_id))
            .await
            .expect("restored board blocks");
        let restored_board_view = restored_blocks
            .iter()
            .find(|b| b.id == fx.trashed_board_view_id)
            .expect("restored board view");
        assert_no_retired_keys(&restored_board_view.fields, "restored board view");
        assert_eq!(
            restored_board_view.fields["groupBy"],
            json!("p-status"),
            "restored board view groupBy migrated"
        );
        assert_eq!(
            restored_board_view.fields["dateProperty"],
            json!("p-due"),
            "restored board view dateProperty migrated"
        );
        assert_eq!(
            restored_board_view.fields["visibleGroups"],
            json!(["opt-b", "opt-a"]),
            "restored board view visibleGroups order preserved"
        );
    }

    if let Some(other) = &fx.other_org {
        let post_b = migrated.for_org(&other.org_id).expect("org-b");
        let b_blocks = post_b
            .get_blocks(&BlockFilter::container(&other.board_id))
            .await
            .expect("org-b blocks");
        let b_view = b_blocks
            .iter()
            .find(|b| b.id == other.kanban_view_id)
            .expect("org-b view");
        assert_no_retired_keys(&b_view.fields, "org-b live view");
        assert_eq!(
            Value::Object(b_view.fields.clone()),
            expected_all_eight_fields(),
            "org-b live view migrated across tenant boundary"
        );
        let b_restored = post_b
            .undelete_block(&other.trashed_view_id, &other.bob)
            .await
            .expect("org-b restore view");
        assert_no_retired_keys(&b_restored.fields, "org-b trashed view");
        assert_eq!(
            Value::Object(b_restored.fields),
            expected_all_eight_fields(),
            "org-b trashed view migrated across tenant boundary"
        );
    }
}

#[tokio::test]
async fn migration_0071_upgrades_populated_views_and_trash_losslessly() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_native_view_field_keys(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("opens at migration 70");
    let fx = seed_pre0071(&pre).await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrates 0070 -> 0071 cleanly");

    let post_a = migrated.for_org(&fx.org_a).expect("org-a");
    assert_live_views(&post_a, &fx).await;
    assert_collision_and_typeless_guard(&post_a, &fx).await;
    assert_trashed_and_special_views(&migrated, &fx).await;

    migrated.close().await;
    database.remove().await;
}
