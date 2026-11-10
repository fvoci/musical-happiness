//! Port of `TestGenerateBlockIDs` and `TestStampModificationMetadata` from
//! `server/model/block_test.go`. Each test maps to one subtest of the original.

use fvoci_model::{
    Block, IdKind, JsonMap, generate_block_ids, new_id, stamp_modification_metadata,
};
use serde_json::{Value, json};

fn block(id: &str, board_id: &str, parent_id: &str) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        ..Block::default()
    }
}

fn fields(value: Value) -> JsonMap {
    match value {
        Value::Object(map) => map,
        other => panic!("test fields must be a JSON object, got {other}"),
    }
}

fn id() -> String {
    new_id(IdKind::Block)
}

/// "Should generate a new ID for a single block with no references"
#[test]
fn generate_block_ids_single_block_with_no_references() {
    let block_id = id();
    let mut blocks = vec![block(&block_id, "", "")];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id);
    assert_eq!(blocks[0].board_id, "");
    assert_eq!(blocks[0].parent_id, "");
}

/// "Should generate a new ID for a single block with references"
#[test]
fn generate_block_ids_single_block_with_references() {
    let (block_id, board_id, parent_id) = (id(), id(), id());
    let mut blocks = vec![block(&block_id, &board_id, &parent_id)];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id);
    assert_eq!(blocks[0].board_id, board_id);
    assert_eq!(blocks[0].parent_id, parent_id);
}

/// "Should generate IDs and link multiple blocks with existing references"
#[test]
fn generate_block_ids_links_multiple_blocks_with_existing_references() {
    let (block_id1, board_id1, parent_id1) = (id(), id(), id());
    let (block_id2, parent_id2) = (id(), id());
    let board_id2 = block_id1.clone();
    let mut blocks = vec![
        block(&block_id1, &board_id1, &parent_id1),
        block(&block_id2, &board_id2, &parent_id2),
    ];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id1);
    assert_eq!(blocks[0].board_id, board_id1);
    assert_eq!(blocks[0].parent_id, parent_id1);

    assert_ne!(blocks[1].id, block_id2);
    assert_ne!(blocks[1].board_id, board_id2);
    assert_eq!(blocks[1].parent_id, parent_id2);

    // The first block was referenced, so the reference follows its new identifier.
    assert_eq!(blocks[1].board_id, blocks[0].id);
}

/// "Should generate new IDs but not modify nonexisting references"
#[test]
fn generate_block_ids_does_not_modify_nonexisting_references() {
    let (block_id1, parent_id1) = (id(), id());
    let (block_id2, board_id2) = (id(), id());
    let mut blocks = vec![
        block(&block_id1, "", &parent_id1),
        block(&block_id2, &board_id2, ""),
    ];

    generate_block_ids(&mut blocks);

    // Only the identifiers changed.
    assert_ne!(blocks[0].id, block_id1);
    assert_eq!(blocks[0].board_id, "");
    assert_eq!(blocks[0].parent_id, parent_id1);

    assert_ne!(blocks[1].id, block_id2);
    assert_eq!(blocks[1].board_id, board_id2);
    assert_eq!(blocks[1].parent_id, "");
}

/// "Should modify correctly multiple blocks with existing and nonexisting references"
#[test]
fn generate_block_ids_multiple_blocks_with_existing_and_nonexisting_references() {
    let (block_id1, board_id1, parent_id1) = (id(), id(), id());
    let first = block(&block_id1, &board_id1, &parent_id1);

    // Linked to 1.
    let (block_id2, parent_id2) = (id(), id());
    let board_id2 = block_id1.clone();
    let second = block(&block_id2, &board_id2, &parent_id2);

    // Linked to 2.
    let (block_id3, parent_id3) = (id(), id());
    let board_id3 = block_id2.clone();
    let third = block(&block_id3, &board_id3, &parent_id3);

    // Linked to 1.
    let (block_id4, parent_id4) = (id(), id());
    let board_id4 = block_id1.clone();
    let fourth = block(&block_id4, &board_id4, &parent_id4);

    // The blocks are shuffled.
    let mut blocks = vec![fourth, second, first, third];

    generate_block_ids(&mut blocks);

    // Block 1.
    assert_ne!(blocks[2].id, block_id1);
    assert_eq!(blocks[2].board_id, board_id1);
    assert_eq!(blocks[2].parent_id, parent_id1);

    // Block 2.
    assert_ne!(blocks[1].id, block_id2);
    assert_ne!(blocks[1].board_id, board_id2);
    assert_eq!(blocks[1].board_id, blocks[2].id, "block 2 links to block 1");
    assert_eq!(blocks[1].parent_id, parent_id2);

    // Block 3.
    assert_ne!(blocks[3].id, block_id3);
    assert_ne!(blocks[3].board_id, board_id3);
    assert_eq!(blocks[3].board_id, blocks[1].id, "block 3 links to block 2");
    assert_eq!(blocks[3].parent_id, parent_id3);

    // Block 4.
    assert_ne!(blocks[0].id, block_id4);
    assert_ne!(blocks[0].board_id, board_id4);
    assert_eq!(blocks[0].board_id, blocks[2].id, "block 4 links to block 1");
    assert_eq!(blocks[0].parent_id, parent_id4);
}

/// "Should update content order"
#[test]
fn generate_block_ids_updates_content_order() {
    let (block_id1, board_id1, parent_id1) = (id(), id(), id());
    let (block_id2, board_id2, parent_id2) = (id(), id(), id());
    let mut blocks = vec![
        block(&block_id1, &board_id1, &parent_id1),
        Block {
            fields: fields(json!({ "contentOrder": [block_id1] })),
            ..block(&block_id2, &board_id2, &parent_id2)
        },
    ];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id1);
    assert_eq!(blocks[0].board_id, board_id1);
    assert_eq!(blocks[0].parent_id, parent_id1);

    assert_ne!(blocks[1].id, block_id2);
    assert_eq!(blocks[1].board_id, board_id2);
    assert_eq!(blocks[1].parent_id, parent_id2);

    // Block 1 was referenced in block 2's content order, which follows the new identifier.
    assert_eq!(blocks[1].fields["contentOrder"], json!([blocks[0].id]));
}

/// "Should update content order when it contain slices"
#[test]
fn generate_block_ids_updates_content_order_containing_lists() {
    let (block_id1, board_id1, parent_id1) = (id(), id(), id());
    let (block_id2, block_id3) = (id(), id());
    let (block_id4, board_id2, parent_id2) = (id(), id(), id());
    let mut blocks = vec![
        block(&block_id1, &board_id1, &parent_id1),
        block(&block_id2, &board_id1, &parent_id1),
        block(&block_id3, &board_id1, &parent_id1),
        Block {
            fields: fields(json!({ "contentOrder": [block_id1, [block_id2, block_id3]] })),
            ..block(&block_id4, &board_id2, &parent_id2)
        },
    ];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id1);
    assert_eq!(blocks[0].board_id, board_id1);
    assert_eq!(blocks[0].parent_id, parent_id1);

    assert_ne!(blocks[3].id, block_id4);
    assert_eq!(blocks[3].board_id, board_id2);
    assert_eq!(blocks[3].parent_id, parent_id2);

    assert_ne!(blocks[1].id, block_id2);
    assert_ne!(blocks[2].id, block_id3);
    assert_eq!(
        blocks[3].fields["contentOrder"],
        json!([blocks[0].id, [blocks[1].id, blocks[2].id]])
    );
}

/// "Should update Id of default template view"
#[test]
fn generate_block_ids_updates_default_template_id() {
    let (block_id1, board_id1, parent_id1) = (id(), id(), id());
    let (block_id2, board_id2, parent_id2) = (id(), id(), id());
    let mut blocks = vec![
        block(&block_id1, &board_id1, &parent_id1),
        Block {
            fields: fields(json!({ "defaultTemplateId": block_id1 })),
            ..block(&block_id2, &board_id2, &parent_id2)
        },
    ];

    generate_block_ids(&mut blocks);

    assert_ne!(blocks[0].id, block_id1);
    assert_eq!(blocks[0].board_id, board_id1);
    assert_eq!(blocks[0].parent_id, parent_id1);

    assert_ne!(blocks[1].id, block_id2);
    assert_eq!(blocks[1].board_id, board_id2);
    assert_eq!(blocks[1].parent_id, parent_id2);

    assert_eq!(blocks[1].fields["defaultTemplateId"], json!(blocks[0].id));
}

/// Not in the original suite: `cardOrder` is rewritten like `contentOrder`.
#[test]
fn generate_block_ids_updates_card_order() {
    let (card_id, view_id, board_id) = (id(), id(), id());
    let mut blocks = vec![
        block(&card_id, &board_id, &board_id),
        Block {
            fields: fields(json!({ "cardOrder": [card_id, "not-in-this-set"] })),
            ..block(&view_id, &board_id, &card_id)
        },
    ];

    generate_block_ids(&mut blocks);

    assert_eq!(
        blocks[1].fields["cardOrder"],
        json!([blocks[0].id, "not-in-this-set"])
    );
}

/// Not in the original suite: fields of an unexpected type are left as they are.
#[test]
fn generate_block_ids_leaves_fields_of_unexpected_type() {
    let weird = json!({ "contentOrder": "not a list", "cardOrder": 7, "defaultTemplateId": [] });
    let mut blocks = vec![Block {
        fields: fields(weird.clone()),
        ..block(&id(), "", "")
    }];

    generate_block_ids(&mut blocks);

    assert_eq!(Value::Object(blocks[0].fields.clone()), weird);
}

/// `TestStampModificationMetadata` / "base case"
#[test]
fn stamp_modification_metadata_base_case() {
    let mut blocks = vec![Block::default()];
    assert_eq!(blocks[0].modified_by, "");
    assert_eq!(blocks[0].update_at, 0);

    stamp_modification_metadata("user_id_1", &mut blocks, 1_700_000_000_000);

    assert_eq!(blocks[0].modified_by, "user_id_1");
    assert_eq!(blocks[0].update_at, 1_700_000_000_000);
}

/// Not in the original suite: the single-user pseudo user is recorded as no modifier.
#[test]
fn stamp_modification_metadata_blanks_the_single_user() {
    let mut blocks = vec![Block {
        modified_by: "someone".to_owned(),
        ..Block::default()
    }];

    stamp_modification_metadata(fvoci_model::SINGLE_USER_ID, &mut blocks, 5);

    assert_eq!(blocks[0].modified_by, "");
    assert_eq!(blocks[0].update_at, 5);
}
