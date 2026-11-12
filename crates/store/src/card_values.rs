//! What a write may give the property values of a card: values that fit the kinds of
//! its board's properties, and for a copy, what still fits them.

use fvoci_model::{Block, BlockError, validate_card_property_kinds};
use serde_json::Value;

use crate::boards;
use crate::db::Scope;
use crate::error::StoreError;

/// Checks that the values a write gives a card fit the kinds of the board's properties
/// (`docs/s6-design.md` 5.7): a date value for a date property, and none for another.
/// Only the values the write changes are checked; one that a change of the definitions
/// left behind stays until it is written.
pub(crate) async fn check_written_kinds(
    scope: &mut Scope<'_>,
    block: &Block,
    existing: Option<&Block>,
) -> Result<(), StoreError> {
    let Some(Value::Object(values)) = block.fields.get("properties") else {
        return Ok(());
    };
    let before = existing
        .and_then(|existing| existing.fields.get("properties"))
        .and_then(Value::as_object);
    let written: fvoci_model::JsonMap = values
        .iter()
        .filter(|(id, value)| before.and_then(|before| before.get(*id)) != Some(*value))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    if written.is_empty() {
        return Ok(());
    }
    let Some(board) = boards::find_board(scope, &block.board_id).await? else {
        return Ok(());
    };
    validate_card_property_kinds(&written, &board.card_properties).map_err(|error| {
        StoreError::InvalidBlock {
            id: block.id.clone(),
            source: BlockError::from(error),
        }
    })
}

/// Clears what copied blocks can no longer be stored with: malformed fields
/// ([`Block::drop_malformed_fields`]), and the values of cards that no longer fit their
/// board's definitions, as [`fvoci_model::clear_values_of_another_kind`] does.
pub(crate) fn clear_copied_values(blocks: &mut [Block], definitions: &[fvoci_model::JsonMap]) {
    for block in blocks {
        block.drop_malformed_fields();
        if let Some(Value::Object(values)) = block.fields.get_mut("properties") {
            fvoci_model::clear_values_of_another_kind(values, definitions);
        }
    }
}
