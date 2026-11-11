//! Changes of the stored data that the schema migrations leave to the server: each is
//! named by a row of `data_steps`, which the migration that needs it adds, and is done
//! once, after the migrations, in one transaction that also removes its row.

use std::collections::{HashMap, HashSet};

use fvoci_model::{BlockType, date_property_ids, rewrite_previous_date_values};

use crate::Store;
use crate::db::{Id, Param, Scope};
use crate::error::StoreError;
use crate::file_refs;

/// The step of migration 4: date values to the shape of `docs/s6-design.md` 5.7 (B).
const DATE_VALUES: &str = "date_values";

/// The step of migration 8: the content blocks stored before a card's content became its
/// body are deleted (user decision 2026-11-11 11:05 KST).
const CONTENT_BLOCKS: &str = "content_blocks";

/// The block types that stay: those a block may be given ([`BlockType::is_current`]),
/// and the legacy board block of the original, which is no content block.
const KEPT_TYPES: [BlockType; 5] = [
    BlockType::Card,
    BlockType::View,
    BlockType::Comment,
    BlockType::Attachment,
    BlockType::Board,
];

/// Does the steps that are still to be done.
///
/// # Errors
///
/// Returns [`StoreError::Database`] if a statement fails, and [`StoreError::Corrupt`] if
/// stored JSON cannot be read; nothing of the failed step is kept then.
pub(crate) async fn run(store: &Store) -> Result<(), StoreError> {
    store
        .transaction(async |scope| {
            // The step is claimed before it is done: of two servers that start at once,
            // the second waits here for the first and then finds nothing to do.
            if claim(scope, DATE_VALUES).await? {
                let changed = date_values(scope).await?;
                tracing::info!(changed, "date values were changed to their new shape");
            }
            if claim(scope, CONTENT_BLOCKS).await? {
                let deleted = content_blocks(scope).await?;
                tracing::info!(
                    deleted,
                    "the content blocks stored before bodies were deleted"
                );
            }
            Ok(())
        })
        .await
}

/// Takes the step's row: whether the step is still to be done.
async fn claim(scope: &mut Scope<'_>, step: &str) -> Result<bool, StoreError> {
    let mut sql = scope.sql("DELETE FROM data_steps WHERE name = ");
    sql.bind(step).push(" RETURNING name");
    Ok(!scope.fetch_all(sql).await?.is_empty())
}

/// Deletes the content blocks (every type but [`KEPT_TYPES`]), on the boards and in the
/// trash, and the references to the files they showed. A file that no
/// block shows any more is marked, to be collected after the grace period like any
/// other. Returns how many blocks were deleted.
async fn content_blocks(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let mut sql = scope.sql("DELETE FROM blocks WHERE type NOT IN (");
    for (index, block_type) in KEPT_TYPES.iter().enumerate() {
        sql.push_bind(if index == 0 { "" } else { ", " }, block_type.as_str());
    }
    sql.push(")");
    let mut deleted = scope.execute(sql).await?;

    // The trash keeps a block as JSON: its type is read here.
    let sql = scope.sql("SELECT id, block FROM deleted_blocks");
    let mut in_trash = Vec::new();
    for row in scope.fetch_all(sql).await? {
        let block = row.json(1, "block")?;
        let block_type = BlockType::from(block["type"].as_str().unwrap_or_default());
        if !KEPT_TYPES.contains(&block_type) {
            in_trash.push(row.id(0)?);
        }
    }
    for chunk in in_trash.chunks(500) {
        let mut sql = scope.sql("DELETE FROM deleted_blocks WHERE id IN (");
        sql.bind_list(chunk.iter().map(|id| Id(id))).push(")");
        deleted += scope.execute(sql).await?;
    }

    file_refs::forget_gone_blocks(scope).await?;
    file_refs::mark(scope, None).await?;
    Ok(deleted)
}

/// The identifiers of each board's date properties, live boards and those in the trash.
type DateProperties = HashMap<String, HashSet<String>>;

/// Rewrites the date values of cards and the date filters of views, live and in the
/// trash, one board at a time. Returns how many blocks changed.
async fn date_values(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let mut dates = DateProperties::new();
    let sql = scope.sql("SELECT id, card_properties FROM boards");
    for row in scope.fetch_all(sql).await? {
        let properties = row.json(1, "card_properties")?;
        dates.insert(row.id(0)?, date_property_ids(definitions(&properties)));
    }
    let sql = scope.sql("SELECT id, board FROM deleted_boards");
    for row in scope.fetch_all(sql).await? {
        let board = row.json(1, "board")?;
        dates.insert(
            row.id(0)?,
            date_property_ids(definitions(&board["cardProperties"])),
        );
    }

    let mut changed = 0;
    for (board_id, dates) in dates.iter().filter(|(_, dates)| !dates.is_empty()) {
        let mut sql = scope.sql("SELECT id, fields FROM blocks WHERE type IN ('card', 'view')");
        sql.push_bind(" AND board_id = ", Id(board_id));
        for row in scope.fetch_all(sql).await? {
            let id = row.id(0)?;
            let mut fields = row.json(1, "fields")?;
            if rewrite_previous_date_values(&mut fields, dates) {
                let mut sql = scope.sql("UPDATE blocks SET fields = ");
                sql.bind(Param::json(&fields)?)
                    .push_bind(" WHERE id = ", Id(&id));
                scope.execute(sql).await?;
                changed += 1;
            }
        }
        let mut sql = scope.sql("SELECT id, block FROM deleted_blocks WHERE board_id = ");
        sql.bind(Id(board_id));
        for row in scope.fetch_all(sql).await? {
            let id = row.id(0)?;
            let mut block = row.json(1, "block")?;
            if block["fields"].is_object()
                && rewrite_previous_date_values(&mut block["fields"], dates)
            {
                let mut sql = scope.sql("UPDATE deleted_blocks SET block = ");
                sql.bind(Param::json(&block)?)
                    .push_bind(" WHERE id = ", Id(&id));
                scope.execute(sql).await?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// The property definitions in a stored list of them.
fn definitions(list: &serde_json::Value) -> impl Iterator<Item = &fvoci_model::JsonMap> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object)
}
