//! Who watched the blocks in the trash: the subscriptions to a block go into the trash
//! with it, in its JSON of `deleted_blocks` (under [`KEY`]), and come back with it. A
//! subscriber whose account is gone, or erased, meanwhile comes back to nothing.

use std::collections::HashMap;

use serde_json::Value;

use crate::db::{Dialect, Id, Row, Scope};
use crate::error::StoreError;

/// Where a block of the trash keeps its subscriptions: no member of a block's own JSON.
const KEY: &str = "watchedBy";

/// How many identifiers one statement binds.
const BATCH: usize = 300;

/// A subscription as the trash keeps it, without its block: its own type, so that what
/// is stored does not change with the store's interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredWatcher {
    block_type: String,
    subscriber_type: String,
    subscriber_id: String,
    notified_at: i64,
    create_at: i64,
}

/// The subscriptions to those of `block_ids` that have any, by block, each block's in
/// the order they were made.
pub(super) async fn watchers_of(
    scope: &mut Scope<'_>,
    block_ids: &[&str],
) -> Result<HashMap<String, Vec<StoredWatcher>>, StoreError> {
    let mut watchers: HashMap<String, Vec<StoredWatcher>> = HashMap::new();
    for batch in block_ids.chunks(BATCH) {
        let mut sql = scope.sql(
            "SELECT block_id, block_type, subscriber_type, subscriber_id, notified_at, \
             create_at FROM subscriptions WHERE block_id IN (",
        );
        sql.bind_list(batch.iter().map(|id| Id(id)))
            .push(") ORDER BY block_id, create_at, subscriber_id");
        for row in scope.fetch_all(sql).await? {
            watchers.entry(row.id(0)?).or_default().push(StoredWatcher {
                block_type: row.text(1)?,
                subscriber_type: row.text(2)?,
                subscriber_id: row.id(3)?,
                notified_at: row.int(4)?,
                create_at: row.int(5)?,
            });
        }
    }
    Ok(watchers)
}

/// The JSON of a block of the trash, `in_trash` as it is, with `watchers` beside it.
pub(super) fn with_watchers(
    in_trash: &impl serde::Serialize,
    watchers: Option<Vec<StoredWatcher>>,
) -> Result<Value, StoreError> {
    let corrupt = |error: serde_json::Error| StoreError::Corrupt {
        column: "block",
        reason: error.to_string(),
    };
    let mut value = serde_json::to_value(in_trash).map_err(corrupt)?;
    if let (Some(watchers), Some(members)) = (watchers, value.as_object_mut()) {
        members.insert(
            KEY.to_owned(),
            serde_json::to_value(watchers).map_err(corrupt)?,
        );
    }
    Ok(value)
}

/// The subscriptions the block of a row of the trash kept: none for one that kept none,
/// as what went into the trash before they were kept.
pub(super) fn kept_in(row: &Row, index: usize) -> Result<Vec<StoredWatcher>, StoreError> {
    let mut block = row.json(index, "block")?;
    match block.get_mut(KEY).map(Value::take) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(kept) => serde_json::from_value(kept).map_err(|error| StoreError::Corrupt {
            column: "block",
            reason: error.to_string(),
        }),
    }
}

/// Makes again the subscriptions `block_id` kept in the trash, of the subscribers that
/// are accounts still; the block is back among the blocks. One made since stays as it is.
pub(super) async fn watch_again(
    scope: &mut Scope<'_>,
    block_id: &str,
    watchers: &[StoredWatcher],
) -> Result<(), StoreError> {
    let postgres = scope.dialect() == Dialect::Postgres;
    for watcher in watchers {
        let mut sql = scope.sql(if postgres {
            "INSERT INTO subscriptions (org_id, block_type, block_id, subscriber_type, \
             subscriber_id, notified_at, create_at) SELECT b.org_id, "
        } else {
            "INSERT INTO subscriptions (block_type, block_id, subscriber_type, \
             subscriber_id, notified_at, create_at) SELECT "
        });
        sql.bind(watcher.block_type.as_str())
            .push_bind(", b.id, ", watcher.subscriber_type.as_str())
            .push_bind(", u.id, ", watcher.notified_at)
            .push_bind(", ", watcher.create_at)
            .push_bind(
                " FROM blocks AS b JOIN users AS u ON u.id = ",
                Id(&watcher.subscriber_id),
            )
            .push_bind(" WHERE u.erased_at = 0 AND b.id = ", Id(block_id))
            .push(" ON CONFLICT (block_id, subscriber_id) DO NOTHING");
        scope.execute(sql).await?;
    }
    Ok(())
}
