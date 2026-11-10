//! Port of `TestBlock2Card` from `server/model/card_test.go`, plus tests for the card
//! conversions and patches the original leaves untested.

use fvoci_model::{Block, BlockType, Card, CardError, CardPatch, JsonMap, new_id};
use serde_json::{Value, json};

const NOW: i64 = 1_660_046_400_000;

fn sample_fields() -> JsonMap {
    let fields = json!({
        "contentOrder": [
            "acdxa8r8aht85pyoeuj1ed7tu8w",
            "73urm1huoupd4idzkdq5yaeuyay",
            "ay6sogs9owtd9xbyn49qt3395ko"
        ],
        "icon": "🎨",
        "isTemplate": false,
        "properties": {
            "aa7swu9zz3ofdkcna3h867cum4y": "212-444-1234",
            "af6fcbb8-ca56-4b73-83eb-37437b9a667d": "77c539af-309c-4db1-8329-d20ef7e9eacd",
            "aiwt9ibi8jjrf9hzi1xzk8no8mo": "foo",
            "aj65h4s6ghr6wgh3bnhqbzzmiaa": "77",
            "ajy6xbebzopojaenbnmfpgtdwso": "{\"from\":1660046400000}",
            "amc8wnk1xqj54rymkoqffhtw7ie": "zhqsoeqs1pg9i8gk81k9ryy83h",
            "aooz77t119y7xtfmoyeiy4up75c": "someone@example.com",
            "auskzaoaccsn55icuwarf4o3tfe": "https://www.google.com",
            "aydsk41h6cs1z7nmghaw16jqcia": [
                "aw565znut6zphbxqhbwyawiuggy",
                "aefd3pxciomrkur4rc6smg1usoc",
                "a6c96kwrqaskbtochq9wunmzweh",
                "atyexeuq993fwwb84bxoqixxqqr"
            ],
            "d6b1249b-bc18-45fc-889e-bec48fce80ef": "9a090e33-b110-4268-8909-132c5002c90e",
            "d9725d14-d5a8-48e5-8de1-6f8c004a9680": "3245a32d-f688-463b-87f4-8e7142c1b397"
        }
    });
    match fields {
        Value::Object(map) => map,
        other => panic!("sample fields must be an object, got {other}"),
    }
}

fn sample_block() -> Block {
    let board_id = new_id();
    let user_id = new_id();
    Block {
        id: new_id(),
        // A card is directly on its board: it has no parent.
        parent_id: String::new(),
        created_by: user_id.clone(),
        modified_by: user_id,
        schema: 1,
        block_type: BlockType::Card,
        title: "My card title".to_owned(),
        fields: sample_fields(),
        create_at: NOW,
        update_at: NOW,
        delete_at: 0,
        board_id,
    }
}

/// `TestBlock2Card` / "Good block"
#[test]
fn card_from_block_good_block() {
    let block = sample_block();

    let card = Card::from_block(&block, NOW).expect("the block is a well-formed card");

    assert_eq!(card.id, block.id);
    assert_eq!(
        card.content_order,
        [
            "acdxa8r8aht85pyoeuj1ed7tu8w",
            "73urm1huoupd4idzkdq5yaeuyay",
            "ay6sogs9owtd9xbyn49qt3395ko"
        ]
    );
    assert_eq!(json!(card.icon), block.fields["icon"]);
    assert_eq!(json!(card.is_template), block.fields["isTemplate"]);
    assert_eq!(Value::Object(card.properties), block.fields["properties"]);
}

/// `TestBlock2Card` / "Not a card"
#[test]
fn card_from_block_not_a_card() {
    assert_eq!(
        Card::from_block(&Block::default(), NOW),
        Err(CardError::NotCardBlock)
    );
}

#[test]
fn card_from_block_fills_missing_fields_and_times() {
    let block = Block {
        id: "card-id".to_owned(),
        board_id: "board-id".to_owned(),
        block_type: BlockType::Card,
        ..Block::default()
    };

    let card = Card::from_block(&block, NOW).expect("a card block with no fields is valid");

    assert_eq!(card.content_order, Vec::<String>::new());
    assert_eq!(card.icon, "");
    assert!(!card.is_template, "isTemplate defaults to false");
    assert!(card.properties.is_empty(), "properties default to empty");
    assert_eq!((card.create_at, card.update_at), (NOW, NOW));
}

#[test]
fn card_from_block_rejects_fields_of_the_wrong_type() {
    let cases = [
        (json!({ "contentOrder": "abc" }), "contentOrder"),
        (
            json!({ "contentOrder": ["a", ["b", "c"]] }),
            "contentOrder item",
        ),
        (json!({ "icon": 1 }), "icon"),
        (json!({ "isTemplate": "yes" }), "isTemplate"),
        (json!({ "properties": [] }), "properties"),
    ];
    for (fields, field) in cases {
        let Value::Object(fields) = fields else {
            panic!("case fields must be an object");
        };
        let block = Block {
            block_type: BlockType::Card,
            fields,
            ..Block::default()
        };
        assert_eq!(
            Card::from_block(&block, NOW),
            Err(CardError::InvalidFieldType { field }),
            "field {field}"
        );
    }
}

#[test]
fn card_into_block_round_trips() {
    let block = sample_block();
    let card = Card::from_block(&block, NOW).expect("the block is a well-formed card");

    assert_eq!(Block::from(card), block);
}

#[test]
fn card_validate_reports_the_first_problem() {
    let valid = Card {
        id: "card-id".to_owned(),
        board_id: "board-id".to_owned(),
        icon: "🎨".to_owned(),
        create_at: NOW,
        update_at: NOW,
        ..Card::default()
    };
    assert_eq!(valid.validate(), Ok(()));

    let cases = [
        (
            Card {
                id: String::new(),
                ..valid.clone()
            },
            "ID is missing",
        ),
        (
            Card {
                board_id: String::new(),
                ..valid.clone()
            },
            "BoardID is missing",
        ),
        (
            Card {
                icon: "🎨🎨".to_owned(),
                ..valid.clone()
            },
            "Icon can have only one grapheme",
        ),
        (
            Card {
                create_at: 0,
                ..valid.clone()
            },
            "CreateAt",
        ),
        (
            Card {
                update_at: 0,
                ..valid
            },
            "UpdateAt",
        ),
    ];
    for (card, message) in cases {
        assert_eq!(
            card.validate(),
            Err(CardError::Invalid(message)),
            "{message}"
        );
    }
}

#[test]
fn card_icon_may_be_one_multi_scalar_grapheme() {
    // A family emoji is seven scalar values joined into one grapheme cluster.
    let card = Card {
        id: "card-id".to_owned(),
        board_id: "board-id".to_owned(),
        icon: "👨‍👩‍👧‍👦".to_owned(),
        create_at: NOW,
        update_at: NOW,
        ..Card::default()
    };
    assert_eq!(card.validate(), Ok(()));
}

#[test]
fn card_populate_keeps_existing_values() {
    let mut card = Card {
        id: "card-id".to_owned(),
        create_at: 5,
        ..Card::default()
    };

    card.populate(NOW);

    assert_eq!(card.id, "card-id");
    assert_eq!((card.create_at, card.update_at), (5, NOW));

    let mut blank = Card::default();
    blank.populate(NOW);
    assert!(fvoci_model::is_id(&blank.id), "new card id {}", blank.id);
}

#[test]
fn card_patch_apply_merges_properties() {
    let mut card = Card {
        title: "old".to_owned(),
        icon: "a".to_owned(),
        content_order: vec!["x".to_owned()],
        ..Card::default()
    };
    card.properties.insert("kept".to_owned(), json!("1"));
    card.properties.insert("replaced".to_owned(), json!("2"));

    let mut patch = CardPatch {
        title: Some("new".to_owned()),
        content_order: Some(vec!["y".to_owned(), "z".to_owned()]),
        icon: None,
        ..CardPatch::default()
    };
    patch
        .updated_properties
        .insert("replaced".to_owned(), json!("3"));
    patch
        .updated_properties
        .insert("added".to_owned(), json!(["4"]));
    patch.apply(&mut card);

    assert_eq!(card.title, "new");
    assert_eq!(card.icon, "a");
    assert_eq!(card.content_order, ["y", "z"]);
    assert_eq!(
        Value::Object(card.properties),
        json!({ "kept": "1", "replaced": "3", "added": ["4"] })
    );
}

#[test]
fn card_patch_to_block_patch() {
    let mut patch = CardPatch {
        title: Some("new".to_owned()),
        content_order: Some(vec!["y".to_owned()]),
        icon: Some("🎨".to_owned()),
        ..CardPatch::default()
    };
    patch.updated_properties.insert("p".to_owned(), json!("v"));

    let block_patch = patch.to_block_patch().expect("the patch is valid");

    assert_eq!(block_patch.title.as_deref(), Some("new"));
    assert_eq!(block_patch.parent_id, None);
    assert_eq!(
        Value::Object(block_patch.updated_fields),
        json!({ "contentOrder": ["y"], "icon": "🎨", "properties": { "p": "v" } })
    );
    assert!(block_patch.deleted_fields.is_empty(), "nothing is deleted");

    let empty = CardPatch::default()
        .to_block_patch()
        .expect("an empty patch is valid");
    assert!(
        empty.updated_fields.is_empty(),
        "an empty patch updates no field"
    );
}

#[test]
fn card_patch_rejects_an_icon_of_two_graphemes() {
    let patch = CardPatch {
        icon: Some("ab".to_owned()),
        ..CardPatch::default()
    };
    let want = Err(CardError::Invalid("Icon can have only one grapheme"));
    assert_eq!(patch.validate(), want);
    assert_eq!(patch.to_block_patch().map(|_| ()), want);
}
