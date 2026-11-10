//! Changes of the stored data that the schema migrations leave to the server: each is
//! named by a row of `data_steps`, which the migration that needs it adds, and is done
//! once, after the migrations, in one transaction that also removes its row.

use std::collections::{HashMap, HashSet};

use fvoci_model::{date_property_ids, rewrite_previous_date_values};

use crate::Store;
use crate::db::{Id, Param, Scope};
use crate::error::StoreError;

/// The step of migration 4: date values to the shape of `docs/s6-design.md` 5.7 (B).
const DATE_VALUES: &str = "date_values";

/// Does the steps that are still to be done.
///
/// # Errors
///
/// Returns [`StoreError::Database`] if a statement fails, and [`StoreError::Corrupt`] if
/// stored JSON cannot be read; nothing of the failed step is kept then.
pub(crate) async fn run(store: &Store) -> Result<(), StoreError> {
    store
        .transaction(async |scope| {
            let sql = scope.sql("SELECT name FROM data_steps");
            let pending = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect::<Result<Vec<_>, _>>()?;
            if pending.iter().any(|name| name == DATE_VALUES) {
                let changed = date_values(scope).await?;
                tracing::info!(changed, "date values were changed to their new shape");
                let mut sql = scope.sql("DELETE FROM data_steps WHERE name = ");
                sql.bind(DATE_VALUES);
                scope.execute(sql).await?;
            }
            Ok(())
        })
        .await
}

/// The identifiers of each board's date properties, live boards and those in the trash.
type DateProperties = HashMap<String, HashSet<String>>;

/// Rewrites the date values of cards and the date filters of views, live and in the
/// trash. Returns how many blocks changed.
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
    let sql = scope.sql("SELECT id, board_id, fields FROM blocks WHERE type IN ('card', 'view')");
    for row in scope.fetch_all(sql).await? {
        let (id, board_id) = (row.id(0)?, row.id(1)?);
        let mut fields = row.json(2, "fields")?;
        if let Some(dates) = dates.get(&board_id)
            && rewrite_previous_date_values(&mut fields, dates)
        {
            let mut sql = scope.sql("UPDATE blocks SET fields = ");
            sql.bind(Param::json(&fields)?)
                .push_bind(" WHERE id = ", Id(&id));
            scope.execute(sql).await?;
            changed += 1;
        }
    }
    let sql = scope.sql("SELECT id, board_id, block FROM deleted_blocks");
    for row in scope.fetch_all(sql).await? {
        let (id, board_id) = (row.id(0)?, row.id(1)?);
        let mut block = row.json(2, "block")?;
        if let Some(dates) = dates.get(&board_id)
            && block["fields"].is_object()
            && rewrite_previous_date_values(&mut block["fields"], dates)
        {
            let mut sql = scope.sql("UPDATE deleted_blocks SET block = ");
            sql.bind(Param::json(&block)?)
                .push_bind(" WHERE id = ", Id(&id));
            scope.execute(sql).await?;
            changed += 1;
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
