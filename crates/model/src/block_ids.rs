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
/// Fields of an unexpected JSON type are left untouched.
pub fn generate_block_ids(blocks: &mut [Block]) {
    let mut referenced: HashSet<&str> = HashSet::new();
    for block in blocks.iter() {
        referenced.insert(&block.board_id);
        referenced.insert(&block.parent_id);

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

    // Blocks that share an identifier share the new one too.
    let renamed: HashMap<String, String> = blocks
        .iter()
        .filter(|block| referenced.contains(block.id.as_str()))
        .map(|block| block.id.as_str())
        .collect::<HashSet<&str>>()
        .into_iter()
        .map(|id| (id.to_owned(), new_id()))
        .collect();

    let renamed_or_same = |id: &mut String| {
        if let Some(new) = renamed.get(id.as_str()) {
            id.clone_from(new);
        }
    };

    for block in blocks {
        block.id = match renamed.get(&block.id) {
            Some(new) => new.clone(),
            // Not referenced by anything, so no other block needs to learn the new value.
            None => new_id(),
        };
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
