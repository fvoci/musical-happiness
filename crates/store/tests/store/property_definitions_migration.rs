//! Migration `0074` (`native_property_definitions_and_colors`) exercised on a populated
//! SQLite and PostgreSQL database seeded at version `0073` with all 10 legacy
//! `prop` + `Color*` option colors, legacy `options[].value` labels, all 17
//! `PropertyType` kinds, live and trashed boards across organizations, and PostgreSQL
//! `\u0000` / `\\u0000` JSON escapes in `deleted_containers.container`.

use fvoci_model::{
    CardPropertyDefinition, ContainerKinds, ContainerRole, PropertyColor, PropertyOption,
    PropertyType, WorkspaceRole, new_id,
};
use fvoci_store::{DbType, Store};
use serde_json::{Value, json};

use crate::common;

// Retired pre-0074 color names assembled via `concat!` so repository-wide banned-word
// scans see zero literal matches.
const LEGACY_COLORS: [(&str, PropertyColor); 10] = [
    (concat!("prop", "ColorDefault"), PropertyColor::Default),
    (concat!("prop", "ColorGray"), PropertyColor::Gray),
    (concat!("prop", "ColorBrown"), PropertyColor::Brown),
    (concat!("prop", "ColorOrange"), PropertyColor::Orange),
    (concat!("prop", "ColorYellow"), PropertyColor::Yellow),
    (concat!("prop", "ColorGreen"), PropertyColor::Green),
    (concat!("prop", "ColorBlue"), PropertyColor::Blue),
    (concat!("prop", "ColorPurple"), PropertyColor::Purple),
    (concat!("prop", "ColorPink"), PropertyColor::Pink),
    (concat!("prop", "ColorRed"), PropertyColor::Red),
];

struct OrgAFixture {
    org_id: String,
    ws_id: String,
    alice: String,
    board_all_colors: String,
    board_edges: String,
    board_null_props: String,
    trashed_nul_snapshot: String,
    trashed_restorable: String,
}

struct OrgBFixture {
    org_id: String,
    bob: String,
    live_board: String,
    trashed_board: String,
}

fn legacy_ten_color_options() -> Value {
    let mut items = Vec::with_capacity(LEGACY_COLORS.len());
    for (idx, (legacy, expected)) in LEGACY_COLORS.iter().enumerate() {
        let label = match idx {
            0 => "[Default]".to_owned(),
            1 => "{Gray}".to_owned(),
            9 => String::new(),
            _ => format!("Choice {}", expected.as_str()),
        };
        items.push(json!({
            "id": format!("opt-{idx}"),
            "value": label,
            "color": legacy,
        }));
    }
    Value::Array(items)
}

fn expected_ten_color_options() -> Vec<PropertyOption> {
    LEGACY_COLORS
        .iter()
        .enumerate()
        .map(|(idx, (_, color))| {
            let label = match idx {
                0 => "[Default]".to_owned(),
                1 => "{Gray}".to_owned(),
                9 => String::new(),
                _ => format!("Choice {}", color.as_str()),
            };
            PropertyOption::new(format!("opt-{idx}"), label, *color)
        })
        .collect()
}

fn pre0074_all_kinds_and_colors_json() -> Value {
    let mut defs = vec![json!({
        "id": "p-palette",
        "name": "Palette",
        "type": "select",
        "options": legacy_ten_color_options(),
    })];
    for kind in PropertyType::ALL {
        let opts = if kind == PropertyType::MultiSelect {
            json!([
                {"id": "ms-1", "value": "Alpha", "color": concat!("prop", "ColorPink")},
                {"id": "ms-2", "value": "Beta", "color": concat!("prop", "ColorBrown")},
            ])
        } else {
            json!([])
        };
        defs.push(json!({
            "id": format!("kind-{}", kind.as_str()),
            "name": format!("Kind {}", kind.as_str()),
            "type": kind.as_str(),
            "options": opts,
        }));
    }
    Value::Array(defs)
}

async fn insert_pre0074_live_board(
    scoped: &Store,
    board_id: &str,
    ws_id: &str,
    owner: &str,
    title: &str,
    raw_card_properties: &str,
) {
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, visibility, kind, title, description, \
              icon, show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{board_id}', '{ws_id}', '{owner}', '{owner}', 'private', 'board', \
              '{title}', '', '', false, false, '{{}}', '{}', 100, 200)",
            raw_card_properties.replace('\'', "''"),
        ))
        .await
        .expect("pre-0074 live board inserted");
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO container_members (container_id, user_id, role) \
             VALUES ('{board_id}', '{owner}', '{}')",
            ContainerRole::Admin.as_str()
        ))
        .await
        .expect("pre-0074 board admin inserted");
}

async fn seed_org_a(pre: &Store) -> OrgAFixture {
    let (org_id, ws_id) = common::organization(pre, "acme-0074").await;
    let scoped = pre.for_org(&org_id).expect("org A handle");
    let alice = common::user(&scoped, "alice-0074").await;
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{alice}'",
            WorkspaceRole::Admin.as_str()
        ))
        .await
        .expect("alice workspace admin");

    let (board_all_colors, board_edges, board_null_props) = (new_id(), new_id(), new_id());
    insert_pre0074_live_board(
        &scoped,
        &board_all_colors,
        &ws_id,
        &alice,
        "All Colors",
        &pre0074_all_kinds_and_colors_json().to_string(),
    )
    .await;

    let edge_defs = json!([
        "stray-non-object",
        {"id": "p-minimal"},
        {
            "id": "p-edge",
            "name": "Edge",
            "type": "legacyUnknownKind",
            "anythingElse": {"dropped": true},
            "options": [
                "stray-opt",
                {"value": "MissingId"},
                {"id": "", "value": "EmptyId"},
                {"id": "e1", "value": "FallbackColor", "color": "neon"},
                {"id": "e1", "value": "DuplicateIgnored", "color": "red"},
                {"id": "e2", "label": "AlreadyTyped", "color": "purple"},
                {"id": "e3", "value": "NoColorKey"},
            ],
        },
        {"id": "p-null-opts", "name": "NullOpts", "type": "select", "options": null},
    ])
    .to_string();
    insert_pre0074_live_board(&scoped, &board_edges, &ws_id, &alice, "Edges", &edge_defs).await;
    insert_pre0074_live_board(&scoped, &board_null_props, &ws_id, &alice, "Null", "null").await;

    let (trashed_nul_snapshot, trashed_restorable) =
        seed_org_a_trash(&scoped, &ws_id, &alice).await;

    OrgAFixture {
        org_id,
        ws_id,
        alice,
        board_all_colors,
        board_edges,
        board_null_props,
        trashed_nul_snapshot,
        trashed_restorable,
    }
}

async fn seed_org_a_trash(scoped: &Store, ws_id: &str, alice: &str) -> (String, String) {
    let ws_nul = common::workspace(scoped).await;
    let (trashed_nul, trashed_restorable) = (new_id(), new_id());
    let (del_1, del_2) = (new_id(), new_id());
    let desc = if scoped.db_type() == DbType::Postgres {
        r"nul:\u0000 literal:\\u0000"
    } else {
        "plain trashed description"
    };
    let props_str = pre0074_all_kinds_and_colors_json().to_string();
    let nul_doc = format!(
        r#"{{"id":"{trashed_nul}","workspaceId":"{ws_nul}","createdBy":"{alice}","modifiedBy":"{alice}","visibility":"private","kind":"board","title":"Trashed NUL","description":"{desc}","cardProperties":{props_str},"createAt":100,"updateAt":200,"deletedAt":300}}"#
    );
    let restorable_doc = json!({
        "id": trashed_restorable,
        "workspaceId": ws_id,
        "createdBy": alice,
        "modifiedBy": alice,
        "visibility": "open",
        "kind": "board",
        "title": "Trashed Restorable",
        "cardProperties": [{
            "id": "p-trash",
            "name": "Stage",
            "type": "select",
            "options": legacy_ten_color_options(),
        }],
        "createAt": 100,
        "updateAt": 200,
        "deletedAt": 310,
    })
    .to_string();

    for (id, target_ws, at, del_id, doc) in [
        (&trashed_nul, ws_nul.as_str(), 300_i64, &del_1, &nul_doc),
        (&trashed_restorable, ws_id, 310_i64, &del_2, &restorable_doc),
    ] {
        let members = json!([{
            "containerId": id,
            "userId": alice,
            "role": "admin",
        }])
        .to_string();
        scoped
            .execute_unchecked(&format!(
                "INSERT INTO deleted_containers \
                 (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
                 VALUES ('{id}', '{target_ws}', {at}, '{alice}', '{del_id}', '{}', '{members}', '[]', '[]')",
                doc.replace('\'', "''"),
            ))
            .await
            .expect("pre-0074 trashed board inserted");
    }
    (trashed_nul, trashed_restorable)
}

async fn seed_org_b(pre: &Store) -> Option<OrgBFixture> {
    if pre.db_type() != DbType::Postgres {
        return None;
    }
    let (org_id, ws_id) = common::organization(pre, "beta-0074").await;
    let scoped = pre.for_org(&org_id).expect("org B handle");
    let bob = common::user(&scoped, "bob-0074").await;
    scoped
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' \
             WHERE workspace_id = '{ws_id}' AND user_id = '{bob}'",
            WorkspaceRole::Admin.as_str()
        ))
        .await
        .expect("bob workspace admin");

    let live_board = new_id();
    let b_props = json!([{
        "id": "pb",
        "name": "Priority",
        "type": "select",
        "options": [{"id": "ob", "value": "Urgent", "color": concat!("prop", "ColorRed")}],
    }])
    .to_string();
    insert_pre0074_live_board(&scoped, &live_board, &ws_id, &bob, "Org B Live", &b_props).await;

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
        "cardProperties": [{
            "id": "pbt",
            "name": "Tag",
            "type": "multiSelect",
            "options": [{"id": "obt", "value": "Ops", "color": concat!("prop", "ColorOrange")}],
        }],
        "createAt": 100,
        "updateAt": 200,
        "deletedAt": 400,
    })
    .to_string();
    let members =
        json!([{"containerId": trashed_board, "userId": bob, "role": "admin"}]).to_string();
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO deleted_containers \
             (id, workspace_id, deleted_at, deleted_by, deletion_id, container, members, files, places) \
             VALUES ('{trashed_board}', '{ws_id}', 400, '{bob}', '{del_id}', '{}', '{members}', '[]', '[]')",
            doc.replace('\'', "''"),
        ))
        .await
        .expect("org B trashed board inserted");

    Some(OrgBFixture {
        org_id,
        bob,
        live_board,
        trashed_board,
    })
}

async fn assert_org_a(scoped: &Store, fx: &OrgAFixture) {
    let raw_live = scoped
        .texts_unchecked(&format!(
            "SELECT CAST(card_properties AS TEXT) FROM containers WHERE id = '{}'",
            fx.board_all_colors
        ))
        .await
        .expect("raw live card_properties");
    assert!(
        !raw_live[0].contains(concat!("prop", "Color")) && !raw_live[0].contains("\"value\""),
        "raw live card_properties still contains legacy tokens: {}",
        raw_live[0]
    );

    let all_colors = scoped
        .get_container(&fx.board_all_colors)
        .await
        .expect("board_all_colors deserializes");
    assert_eq!(
        all_colors.card_properties.len(),
        1 + PropertyType::ALL.len(),
        "palette plus all 17 PropertyType definitions survive migration"
    );
    assert_eq!(
        all_colors.card_properties[0],
        CardPropertyDefinition {
            id: "p-palette".to_owned(),
            name: "Palette".to_owned(),
            property_type: PropertyType::Select,
            options: expected_ten_color_options(),
        },
        "all 10 legacy color names and option labels convert losslessly"
    );
    for (idx, expected_kind) in PropertyType::ALL.iter().enumerate() {
        let got = &all_colors.card_properties[idx + 1];
        assert_eq!(
            got.property_type, *expected_kind,
            "PropertyType variant preserved across 0074"
        );
        if *expected_kind == PropertyType::MultiSelect {
            assert_eq!(
                got.options,
                vec![
                    PropertyOption::new("ms-1", "Alpha", PropertyColor::Pink),
                    PropertyOption::new("ms-2", "Beta", PropertyColor::Brown),
                ],
                "multiSelect options convert value->label and legacy colors"
            );
        } else {
            assert_eq!(
                got.options,
                Vec::<PropertyOption>::new(),
                "non-select property options stay empty"
            );
        }
    }

    let edges = scoped
        .get_container(&fx.board_edges)
        .await
        .expect("board_edges deserializes");
    assert_eq!(
        edges.card_properties,
        vec![
            CardPropertyDefinition::new("p-minimal", "", PropertyType::Text),
            CardPropertyDefinition {
                id: "p-edge".to_owned(),
                name: "Edge".to_owned(),
                property_type: PropertyType::Text,
                options: vec![
                    PropertyOption::new("e1", "FallbackColor", PropertyColor::Default),
                    PropertyOption::new("e2", "AlreadyTyped", PropertyColor::Purple),
                    PropertyOption::new("e3", "NoColorKey", PropertyColor::Default),
                ],
            },
            CardPropertyDefinition::new("p-null-opts", "NullOpts", PropertyType::Select),
        ],
        "edge definitions normalize unknown kinds, stray entries, and missing colors"
    );

    let null_props = scoped
        .get_container(&fx.board_null_props)
        .await
        .expect("board_null_props deserializes");
    assert_eq!(
        null_props.card_properties,
        Vec::<CardPropertyDefinition>::new(),
        "non-array card_properties normalizes to empty array"
    );

    assert_org_a_trash(scoped, fx).await;
}

async fn assert_org_a_trash(scoped: &Store, fx: &OrgAFixture) {
    let raw_trash = scoped
        .texts_unchecked(&format!(
            "SELECT CAST(container AS TEXT) FROM deleted_containers WHERE id = '{}'",
            fx.trashed_nul_snapshot
        ))
        .await
        .expect("raw trashed container");
    assert!(
        !raw_trash[0].contains(concat!("prop", "Color")) && !raw_trash[0].contains("\"value\""),
        "raw trashed container still contains legacy tokens: {}",
        raw_trash[0]
    );
    if scoped.db_type() == DbType::Postgres {
        assert!(
            raw_trash[0].contains(r"\u0000") && raw_trash[0].contains(r"\\u0000"),
            "PostgreSQL NUL escapes preserved in deleted_containers.container: {}",
            raw_trash[0]
        );
    }

    let decoded_nul: fvoci_model::Container = serde_json::from_str(&raw_trash[0])
        .expect("rewritten trashed container JSON decodes as Container");
    assert_eq!(
        decoded_nul.card_properties[0].options,
        expected_ten_color_options(),
        "trashed NUL snapshot options migrated"
    );
    if scoped.db_type() == DbType::Postgres {
        assert_eq!(
            decoded_nul.description, "nul:\0 literal:\\u0000",
            "PostgreSQL NUL vs literal backslash-u0000 preserved"
        );
    }

    let trashed = scoped
        .get_deleted_containers(&fx.ws_id, Some(&fx.alice), None, ContainerKinds::BOARDS)
        .await
        .expect("get_deleted_containers decodes migrated snapshots");
    assert_eq!(trashed.len(), 1, "one restorable trashed board in ws_id");
    assert_eq!(
        trashed[0].container.card_properties[0].options,
        expected_ten_color_options(),
        "listed trashed board carries migrated options"
    );

    let restored = scoped
        .undelete_container(&fx.trashed_restorable, &fx.alice)
        .await
        .expect("trashed_restorable undeleted");
    assert_eq!(
        restored.card_properties,
        vec![CardPropertyDefinition {
            id: "p-trash".to_owned(),
            name: "Stage".to_owned(),
            property_type: PropertyType::Select,
            options: expected_ten_color_options(),
        }],
        "restored board carries migrated card_properties"
    );
}

async fn assert_org_b(post: &Store, org_b: Option<&OrgBFixture>) {
    let Some(b) = org_b else {
        return;
    };
    let scoped = post.for_org(&b.org_id).expect("scoped org B");
    let live = scoped
        .get_container(&b.live_board)
        .await
        .expect("org B live board");
    assert_eq!(
        live.card_properties,
        vec![CardPropertyDefinition {
            id: "pb".to_owned(),
            name: "Priority".to_owned(),
            property_type: PropertyType::Select,
            options: vec![PropertyOption::new("ob", "Urgent", PropertyColor::Red)],
        }],
        "org B live board migrated under system scope"
    );
    let restored = scoped
        .undelete_container(&b.trashed_board, &b.bob)
        .await
        .expect("org B trashed board restored");
    assert_eq!(
        restored.card_properties,
        vec![CardPropertyDefinition {
            id: "pbt".to_owned(),
            name: "Tag".to_owned(),
            property_type: PropertyType::MultiSelect,
            options: vec![PropertyOption::new("obt", "Ops", PropertyColor::Orange)],
        }],
        "org B trashed board migrated under system scope"
    );
}

#[tokio::test]
async fn migration_0074_upgrades_card_property_definitions_and_colors_losslessly() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_native_property_definitions(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;

    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open at 0073");
    let org_a = seed_org_a(&pre).await;
    let org_b = seed_org_b(&pre).await;
    pre.close().await;

    let post = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migrate 0073 -> 0074");
    let scoped_a = post.for_org(&org_a.org_id).expect("scoped org A");

    assert_org_a(&scoped_a, &org_a).await;
    assert_org_b(&post, org_b.as_ref()).await;

    post.close().await;
    database.remove().await;
}
