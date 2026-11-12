//! The checks of the card property values a write gives a block, beyond their shape
//! ([`fvoci_model::Block::validate`]): what fits the kinds of the board's properties
//! (`docs/s6-design.md` 5.7), and what a card relation names.
//!
//! A relation value names cards of the block's board, in the trash or out of it. Which
//! ones it names is asked once every block of a write is stored, so that two new cards
//! of one write may name each other. A card that is another board's is answered as one
//! that is not there at all: who may not see that board learns nothing of it.

use std::collections::{HashMap, HashSet};

use fvoci_model::{
    Block, BlockError, BlockType, JsonMap, RELATION_LIMIT, RelationError, RelationProblem,
    check_relation_value, relation_property_ids, tidy_copied_relations,
    validate_card_property_kinds,
};
use serde_json::Value;

use crate::boards;
use crate::db::{Dialect, Id, Scope};
use crate::error::StoreError;

/// How many identifiers one query of [`types_on_board`] asks for, twice (live and in the
/// trash): within what either database binds in one statement.
const LOOKUP_CHUNK: usize = 500;

/// The cards a relation value of a written block names that it did not name before:
/// those are asked for when the write is stored ([`check_named_cards`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NamedCards {
    block_id: String,
    board_id: String,
    property: String,
    ids: Vec<String>,
}

/// Checks the property values a write changes of a block, against what `existing` (the
/// block as it is stored, if it is) has: a date value for a date property and none for
/// another, and as a relation value a list of other cards, each once, at most
/// [`fvoci_model::RELATION_LIMIT`]. Only the values the write changes are checked; one
/// that was stored before the rules, or that a change of the definitions left behind,
/// stays until it is written.
///
/// Returns what the relation values name that they did not name before, to be asked for
/// once the write is stored: an identifier a value had is not asked for again.
///
/// # Errors
///
/// Returns [`StoreError::InvalidBlock`] for the first value that is refused.
pub(crate) async fn check_written(
    scope: &mut Scope<'_>,
    block: &Block,
    existing: Option<&Block>,
) -> Result<Vec<NamedCards>, StoreError> {
    let Some(Value::Object(values)) = block.fields.get("properties") else {
        return Ok(Vec::new());
    };
    let before = existing
        .and_then(|existing| existing.fields.get("properties"))
        .and_then(Value::as_object);
    let written: JsonMap = values
        .iter()
        .filter(|(id, value)| before.and_then(|before| before.get(*id)) != Some(*value))
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    if written.is_empty() {
        return Ok(Vec::new());
    }
    let Some(board) = boards::find_board(scope, &block.board_id).await? else {
        return Ok(Vec::new());
    };
    let invalid = |source: BlockError| StoreError::InvalidBlock {
        id: block.id.clone(),
        source,
    };
    validate_card_property_kinds(&written, &board.card_properties)
        .map_err(|error| invalid(error.into()))?;
    let relations = relation_property_ids(&board.card_properties);
    let mut named = Vec::new();
    for (property, value) in &written {
        if !relations.contains(property.as_str()) {
            continue;
        }
        let ids = check_relation_value(&block.id, property, value)
            .map_err(|error| invalid(error.into()))?;
        let had: HashSet<&str> = before
            .and_then(|before| before.get(property))
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| !had.contains(id))
            .map(str::to_owned)
            .collect();
        if !ids.is_empty() {
            named.push(NamedCards {
                block_id: block.id.clone(),
                board_id: block.board_id.clone(),
                property: property.clone(),
                ids,
            });
        }
    }
    Ok(named)
}

/// Checks that what relation values name are cards of their blocks' boards, in the
/// trash or out of it, as the write that gave them the values left the boards.
///
/// # Errors
///
/// Returns [`StoreError::InvalidBlock`] with a [`RelationError`] for the first value
/// that names a block of its board that is no card
/// ([`RelationProblem::NotACard`]), or a card that is not on its board
/// ([`RelationProblem::CardNotFound`]): none of that identifier, or another board's.
pub(crate) async fn check_named_cards(
    scope: &mut Scope<'_>,
    named: &[NamedCards],
) -> Result<(), StoreError> {
    let mut by_board: HashMap<&str, HashSet<&str>> = HashMap::new();
    for value in named {
        by_board
            .entry(&value.board_id)
            .or_default()
            .extend(value.ids.iter().map(String::as_str));
    }
    let mut types: HashMap<&str, HashMap<String, BlockType>> = HashMap::new();
    for (board_id, ids) in by_board {
        let ids: Vec<&str> = ids.into_iter().collect();
        types.insert(board_id, types_on_board(scope, board_id, &ids).await?);
    }
    for value in named {
        let found = types.get(value.board_id.as_str());
        for id in &value.ids {
            let problem = match found.and_then(|found| found.get(id)) {
                Some(BlockType::Card) => continue,
                Some(_) => RelationProblem::NotACard,
                None => RelationProblem::CardNotFound,
            };
            return Err(StoreError::InvalidBlock {
                id: value.block_id.clone(),
                source: RelationError::new(&value.property, problem).into(),
            });
        }
    }
    Ok(())
}

/// The types of the blocks of a board among `ids`, in the trash or out of it.
async fn types_on_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    ids: &[&str],
) -> Result<HashMap<String, BlockType>, StoreError> {
    // The type of a block in the trash is in the stored block.
    let trashed_type = match scope.dialect() {
        Dialect::Sqlite => "json_extract(block, '$.type')",
        Dialect::Postgres => "block->>'type'",
    };
    let mut types = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(LOOKUP_CHUNK) {
        let mut sql = scope.sql("SELECT id, type FROM blocks WHERE board_id = ");
        sql.bind(Id(board_id))
            .push(" AND id IN (")
            .bind_list(chunk.iter().map(|id| Id(id)))
            .push(&format!(
                ") UNION ALL SELECT id, {trashed_type} FROM deleted_blocks WHERE board_id = "
            ))
            .bind(Id(board_id))
            .push(" AND id IN (")
            .bind_list(chunk.iter().map(|id| Id(id)))
            .push(")");
        for row in scope.fetch_all(sql).await? {
            types.insert(row.id(0)?, BlockType::from(row.text(1)?));
        }
    }
    Ok(types)
}

/// Clears what copied blocks can no longer be stored with: malformed fields
/// ([`Block::drop_malformed_fields`]), the values of cards that no longer fit their
/// board's definitions, as [`fvoci_model::clear_values_of_another_kind`] does, and of
/// relation values what a write would refuse ([`tidy_copied_relations`]).
pub(crate) fn clear_copied_values(blocks: &mut [Block], definitions: &[JsonMap]) {
    for block in blocks {
        block.drop_malformed_fields();
        let id = block.id.clone();
        if let Some(Value::Object(values)) = block.fields.get_mut("properties") {
            fvoci_model::clear_values_of_another_kind(values, definitions);
            tidy_copied_relations(values, &id, definitions);
        }
    }
}

/// Leaves out of the relation values of copied blocks of a board what names no card of
/// the board, in the trash or out of it, and of the rest what is past
/// [`fvoci_model::RELATION_LIMIT`]: copies of cards stored before the rules may name
/// cards that are gone for good, or blocks that are no cards.
pub(crate) async fn keep_cards_of_board(
    scope: &mut Scope<'_>,
    blocks: &mut [Block],
    board_id: &str,
    definitions: &[JsonMap],
) -> Result<(), StoreError> {
    let relations = relation_property_ids(definitions);
    let relation_values = |block: &Block| -> Vec<String> {
        let Some(Value::Object(values)) = block.fields.get("properties") else {
            return Vec::new();
        };
        relations
            .iter()
            .filter_map(|property| values.get(*property).and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let named: HashSet<String> = blocks.iter().flat_map(relation_values).collect();
    if named.is_empty() {
        return Ok(());
    }
    let named: Vec<&str> = named.iter().map(String::as_str).collect();
    let types = types_on_board(scope, board_id, &named).await?;
    for block in blocks.iter_mut() {
        let Some(Value::Object(values)) = block.fields.get_mut("properties") else {
            continue;
        };
        for property in &relations {
            if let Some(Value::Array(items)) = values.get_mut(*property) {
                items.retain(|item| {
                    item.as_str()
                        .is_some_and(|id| types.get(id) == Some(&BlockType::Card))
                });
                items.truncate(RELATION_LIMIT);
            }
        }
    }
    Ok(())
}
