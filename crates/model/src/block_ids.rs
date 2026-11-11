//! Regenerating the identifiers of a tree of blocks.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::block::Block;
use crate::id::new_id;

/// Gives every block a new identifier while keeping references between them intact.
///
/// A reference is another block's identifier appearing as a `board_id`, a `parent_id`, or
/// inside the `contentOrder`, `cardOrder` or `defaultTemplateId` fields. References to
/// identifiers outside `blocks` are left as they are, so the tree keeps its shape and
/// stays attached to whatever it pointed at.
///
/// Fields of an unexpected JSON type are left untouched. A block that comes without an
/// identifier gets one like any other, and an empty `parent_id` or `board_id` stays
/// empty.
pub fn generate_block_ids(blocks: &mut [Block]) {
    let mut referenced: HashSet<&str> = HashSet::new();
    for block in blocks.iter() {
        referenced.insert(&block.board_id);
        referenced.insert(&block.parent_id);
        // A view's card order names cards that nothing else may refer to: a card whose
        // content is its body has no blocks below it.
        if let Some(Value::Array(items)) = block.fields.get("cardOrder") {
            referenced.extend(order_ids(items));
        }

        if let Some(content_order) = block.fields.get("contentOrder") {
            let Some(items) = content_order.as_array() else {
                // The original skips the rest of this block, including its
                // `defaultTemplateId`, when `contentOrder` is not an array.
                continue;
            };
            referenced.extend(order_ids(items));
        }
        if let Some(Value::String(template_id)) = block.fields.get("defaultTemplateId") {
            referenced.insert(template_id);
        }
    }

    // No identifier is no reference. A block without a parent would otherwise refer to
    // every block of the request that came without an identifier: those would share
    // one new identifier, and the blocks without a parent would get it as theirs.
    referenced.remove("");

    // The new identifiers are made in the order of the blocks: identifiers sort in the
    // order they were made, and a list of blocks is by creation time and then by
    // identifier, so blocks that are stored in one millisecond come back in the order
    // they were sent in. Blocks that share an identifier share the new one too, which
    // is only kept for the blocks something refers to: no other block needs to learn
    // the new value of the rest.
    let mut renamed: HashMap<String, String> = HashMap::new();
    let new_ids: Vec<String> = blocks
        .iter()
        .map(|block| {
            if referenced.contains(block.id.as_str()) {
                renamed
                    .entry(block.id.clone())
                    .or_insert_with(new_id)
                    .clone()
            } else {
                new_id()
            }
        })
        .collect();

    let renamed_or_same = |id: &mut String| {
        if let Some(new) = renamed.get(id.as_str()) {
            id.clone_from(new);
        }
    };

    for (block, new) in blocks.iter_mut().zip(new_ids) {
        block.id = new;
        renamed_or_same(&mut block.board_id);
        renamed_or_same(&mut block.parent_id);

        for field in ["contentOrder", "cardOrder"] {
            if let Some(Value::Array(items)) = block.fields.get_mut(field) {
                for item in items {
                    match item {
                        Value::String(id) => renamed_or_same(id),
                        Value::Array(column) => {
                            for cell in column {
                                if let Value::String(id) = cell {
                                    renamed_or_same(id);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(Value::String(template_id)) = block.fields.get_mut("defaultTemplateId") {
            renamed_or_same(template_id);
        }
    }
}

/// Yields the identifiers in an order list, which holds strings and lists of strings.
fn order_ids(items: &[Value]) -> impl Iterator<Item = &str> {
    items.iter().flat_map(|item| -> Vec<&str> {
        match item {
            Value::String(id) => vec![id.as_str()],
            Value::Array(column) => column.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
    })
}
