//! Changes of the stored data that the schema migrations leave to the server: each is
//! named by a row of `data_steps`, which the migration that needs it adds, and is done
//! once, after the migrations, in one transaction that also removes its row.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
use fvoci_model::BlockType;
use fvoci_model::{DATE_PROPERTY_TYPE, rewrite_previous_date_values};

use crate::db::{Dialect, Row};
use crate::db::{Id, Param, Scope};
use crate::error::StoreError;
use crate::{Store, block_derived, file_refs, search_outbox};

/// The step of migration 4: date values to the shape of `docs/s6-design.md` 5.7 (B).
const DATE_VALUES: &str = "date_values";

/// The step of migration 8: the content blocks stored before a card's content became its
/// body are deleted (user decision 2026-11-11 11:05 KST).
const CONTENT_BLOCKS: &str = "content_blocks";

/// The block types that stay, by their type strings, as migration 8 brought the step:
/// those a block may be given ([`BlockType::is_current`]), the page, which its own
/// operations make, and `board`, a legacy board-block type (not a content block) that an
/// older database may still hold.
const KEPT_TYPES: [&str; 6] = ["card", "view", "comment", "attachment", "page", "board"];

/// The step of migration 42: legacy blocks of type `board`, kept by migration 8, are
/// deleted together with every block below them.
const BOARD_BLOCKS: &str = "board_blocks";

/// The type string of the retired `board` block type.
const BOARD_TYPE: &str = "board";

/// Does the steps that are still to be done.
///
/// # Errors
///
/// Returns [`StoreError::Database`] if a statement fails, and [`StoreError::Corrupt`] if
/// stored JSON cannot be read; nothing of the failed step is kept then.
pub(crate) async fn run(store: &Store) -> Result<(), StoreError> {
    // The steps change what is stored, whosever it is. Those of migrations 4 and 8 name
    // the rows of the boards by identifier alone, which since the keys of the
    // organizations (migration 15) may name one row of each organization: each is done
    // once, just after the migrations of the server that brought it, on a database that
    // holds one organization then. The step of migration 42 names a row by its
    // organization and its identifier.
    store
        .system()
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
            if claim(scope, BOARD_BLOCKS).await? {
                let deleted = board_blocks(scope).await?;
                tracing::info!(deleted, "retired board-type blocks were deleted");
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

/// Deletes the content blocks (every type but [`KEPT_TYPES`]) with every block below
/// them, as deleting a block does, on the boards and in the trash, and the references to
/// the files they showed. A file that no block shows any more is marked, to be collected
/// after the grace period like any other. Returns how many blocks were deleted.
async fn content_blocks(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let org = org_column(scope);
    // `UNION`, not `UNION ALL`: a cycle of parents ends.
    let mut sql = match org {
        Some(_) => scope.sql(
            "WITH RECURSIVE tree(org_id, id) AS (SELECT org_id, id FROM blocks WHERE type NOT IN (",
        ),
        None => scope.sql("WITH RECURSIVE tree(id) AS (SELECT id FROM blocks WHERE type NOT IN ("),
    };
    for (index, block_type) in KEPT_TYPES.iter().enumerate() {
        sql.push_bind(if index == 0 { "" } else { ", " }, *block_type);
    }
    match org {
        Some(_) => {
            sql.push(
                ") UNION SELECT b.org_id, b.id FROM blocks AS b \
                 JOIN tree ON b.parent_id = tree.id AND b.org_id = tree.org_id) \
                 DELETE FROM blocks WHERE (org_id, id) IN (SELECT org_id, id FROM tree) \
                 RETURNING org_id, id",
            );
        }
        None => {
            sql.push(
                ") UNION SELECT b.id FROM blocks AS b JOIN tree ON b.parent_id = tree.id) \
                 DELETE FROM blocks WHERE id IN (SELECT id FROM tree) RETURNING id",
            );
        }
    }
    let live = keys_of(&scope.fetch_all(sql).await?, org)?;
    let mut deleted = u64::try_from(live.len()).unwrap_or(u64::MAX);

    // The trash keeps a block as JSON: its type is read there.
    deleted += delete_from_trash(scope, org, &live, |block_type| {
        !KEPT_TYPES.contains(&block_type)
    })
    .await?;

    file_refs::forget_gone_blocks(scope).await?;
    file_refs::mark(scope, None).await?;
    Ok(deleted)
}

/// Deletes the blocks of the type `board` with every block below them, on the boards
/// and in the trash, of every organization, and what they leave behind
/// ([`block_derived::forget_gone_blocks`]); the search server is told of the boards
/// they were on. Where there were none, nothing changes. Returns how many blocks were
/// deleted.
///
/// Unlike the steps before it, this one runs on a database that may hold several
/// organizations: a block is named by its organization and its identifier.
async fn board_blocks(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let org = org_column(scope);
    let containers = touched_containers(scope, org).await?;
    // `UNION`, not `UNION ALL`: a cycle of parents ends.
    let mut sql = match org {
        Some(_) => scope
            .sql("WITH RECURSIVE tree(org_id, id) AS (SELECT org_id, id FROM blocks WHERE type = "),
        None => scope.sql("WITH RECURSIVE tree(id) AS (SELECT id FROM blocks WHERE type = "),
    };
    sql.bind(BOARD_TYPE);
    match org {
        Some(_) => {
            sql.push(
                " UNION SELECT b.org_id, b.id FROM blocks AS b \
                 JOIN tree ON b.parent_id = tree.id AND b.org_id = tree.org_id) \
                 DELETE FROM blocks WHERE (org_id, id) IN (SELECT org_id, id FROM tree) \
                 RETURNING org_id, id",
            );
        }
        None => {
            sql.push(
                " UNION SELECT b.id FROM blocks AS b JOIN tree ON b.parent_id = tree.id) \
                 DELETE FROM blocks WHERE id IN (SELECT id FROM tree) RETURNING id",
            );
        }
    }
    let live = keys_of(&scope.fetch_all(sql).await?, org)?;
    let mut deleted = u64::try_from(live.len()).unwrap_or(u64::MAX);
    deleted += delete_from_trash(scope, org, &live, |block_type| block_type == BOARD_TYPE).await?;
    // A database that had no such block is left as it was.
    if deleted > 0 {
        block_derived::forget_gone_blocks(scope).await?;
        for (org_id, container_id) in &containers {
            note_touched_container(scope, org, org_id, container_id).await?;
        }
    }
    Ok(deleted)
}

/// The column that names a row's organization where the database holds several: the
/// steps run in the system's scope, which names none.
fn org_column(scope: &Scope<'_>) -> Option<&'static str> {
    debug_assert!(!scope.has_org(), "the data steps run in the system's scope");
    (scope.dialect() == Dialect::Postgres).then_some("org_id")
}

/// The blocks the rows name: by organization (empty where the database holds one) and
/// identifier.
fn keys_of(rows: &[Row], org: Option<&str>) -> Result<Vec<(String, String)>, StoreError> {
    rows.iter()
        .map(|row| match org {
            Some(_) => Ok((row.id(0)?, row.id(1)?)),
            None => Ok((String::new(), row.id(0)?)),
        })
        .collect()
}

/// The `(org_id, container_id)` pairs that hold a live block of the type `board`.
async fn touched_containers(
    scope: &mut Scope<'_>,
    org: Option<&'static str>,
) -> Result<Vec<(String, String)>, StoreError> {
    let mut sql = match org {
        Some(_) => scope.sql("SELECT DISTINCT org_id, container_id FROM blocks WHERE type = "),
        None => scope.sql("SELECT DISTINCT container_id FROM blocks WHERE type = "),
    };
    sql.bind(BOARD_TYPE);
    keys_of(&scope.fetch_all(sql).await?, org)
}

async fn note_touched_container(
    scope: &mut Scope<'_>,
    org: Option<&'static str>,
    org_id: &str,
    container_id: &str,
) -> Result<(), StoreError> {
    if org.is_none() {
        return search_outbox::note_container_regardless(scope, container_id).await;
    }
    let now = scope.now_millis();
    let mut sql = scope.sql(
        "INSERT INTO search_outbox (scope, id, org_id, seq, due_at) \
         SELECT 'container', bo.id, w.org_id, ",
    );
    sql.bind(now)
        .push_bind(", ", now)
        .push(
            " FROM containers AS bo JOIN workspaces AS w \
             ON w.id = bo.workspace_id AND w.org_id = bo.org_id \
             WHERE bo.org_id = ",
        )
        .bind(Id(org_id))
        .push_bind(" AND bo.id = ", Id(container_id))
        .push(
            " ON CONFLICT (org_id, scope, id) DO UPDATE SET seq = CASE \
             WHEN search_outbox.seq >= excluded.seq THEN search_outbox.seq + 1 \
             ELSE excluded.seq END, due_at = excluded.due_at, attempts = 0",
        );
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes from the trash the blocks whose type `is_gone` says and those whose parent
/// is among `deleted` (the live blocks the step deleted, by organization and
/// identifier), with every block below them there, each organization's apart (`org`,
/// the column that names it, where the database holds several). Returns how many went.
async fn delete_from_trash(
    scope: &mut Scope<'_>,
    org: Option<&'static str>,
    deleted: &[(String, String)],
    is_gone: impl Fn(&str) -> bool,
) -> Result<u64, StoreError> {
    let sql = match org {
        Some(_) => scope.sql("SELECT id, parent_id, block, org_id FROM deleted_blocks"),
        None => scope.sql("SELECT id, parent_id, block FROM deleted_blocks"),
    };
    // A block of the trash by its organization (empty where there is one) and its id.
    let mut children: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut gone: Vec<(String, String)> = Vec::new();
    for row in scope.fetch_all(sql).await? {
        let id = row.id(0)?;
        let parent_id = row.id(1)?;
        let block = row.json(2, "block")?;
        let org_id = org.map(|_| row.id(3)).transpose()?.unwrap_or_default();
        if is_gone(block["type"].as_str().unwrap_or_default()) {
            gone.push((org_id.clone(), id.clone()));
        }
        children.entry((org_id, parent_id)).or_default().push(id);
    }
    // A block deleted on its own below a live block that went goes with it: it would
    // have nothing to come back to.
    for parent in deleted {
        let below = children.get(parent).into_iter().flatten();
        gone.extend(below.map(|child| (parent.0.clone(), child.clone())));
    }
    let mut in_trash: HashMap<String, HashSet<String>> = HashMap::new();
    while let Some((org_id, id)) = gone.pop() {
        if in_trash
            .entry(org_id.clone())
            .or_default()
            .insert(id.clone())
        {
            let below = children.get(&(org_id.clone(), id)).into_iter().flatten();
            gone.extend(below.map(|child| (org_id.clone(), child.clone())));
        }
    }
    let mut removed = 0;
    for (org_id, ids) in in_trash {
        let ids: Vec<String> = ids.into_iter().collect();
        for chunk in ids.chunks(500) {
            let mut sql = scope.sql("DELETE FROM deleted_blocks WHERE ");
            if let Some(column) = org {
                sql.push(column).push_bind(" = ", Id(&org_id)).push(" AND ");
            }
            sql.push("id IN (")
                .bind_list(chunk.iter().map(|id| Id(id)))
                .push(")");
            removed += scope.execute(sql).await?;
        }
    }
    Ok(removed)
}

/// The identifiers of each board's date properties, keyed by `(org_id, container_id)`.
type DateProperties = HashMap<(String, String), HashSet<String>>;

/// Rewrites the date values of cards and the date filters of views, live and in the
/// trash, one `(org_id, container_id)` at a time so cross-organization ID collisions on
/// PostgreSQL never leak or overwrite another organization's blocks.
async fn date_values(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let org = org_column(scope);
    let mut dates = DateProperties::new();
    let sql = match org {
        Some(_) => scope.sql("SELECT id, card_properties, org_id FROM containers"),
        None => scope.sql("SELECT id, card_properties FROM containers"),
    };
    for row in scope.fetch_all(sql).await? {
        let properties = row.json(1, "card_properties")?;
        let org_id = org.map(|_| row.id(2)).transpose()?.unwrap_or_default();
        dates.insert((org_id, row.id(0)?), date_ids(&properties));
    }
    let sql = match org {
        Some(_) => scope.sql("SELECT id, container, org_id FROM deleted_containers"),
        None => scope.sql("SELECT id, container FROM deleted_containers"),
    };
    for row in scope.fetch_all(sql).await? {
        let container = row.json(1, "container")?;
        let org_id = org.map(|_| row.id(2)).transpose()?.unwrap_or_default();
        dates.insert((org_id, row.id(0)?), date_ids(&container["cardProperties"]));
    }

    let mut changed = 0;
    for ((org_id, container_id), dates) in dates.iter().filter(|(_, dates)| !dates.is_empty()) {
        let mut sql = scope.sql("SELECT id, fields FROM blocks WHERE type IN ('card', 'view')");
        sql.push_bind(" AND container_id = ", Id(container_id));
        if let Some(column) = org {
            sql.push([" AND ", column, " = "]).bind(Id(org_id));
        }
        for row in scope.fetch_all(sql).await? {
            let id = row.id(0)?;
            let mut fields = row.json(1, "fields")?;
            if rewrite_previous_date_values(&mut fields, dates) {
                let mut sql = scope.sql("UPDATE blocks SET fields = ");
                sql.bind(Param::json(&fields)?)
                    .push_bind(" WHERE id = ", Id(&id))
                    .push_bind(" AND container_id = ", Id(container_id));
                if let Some(column) = org {
                    sql.push([" AND ", column, " = "]).bind(Id(org_id));
                }
                scope.execute(sql).await?;
                changed += 1;
            }
        }
        let mut sql = scope.sql("SELECT id, block FROM deleted_blocks WHERE container_id = ");
        sql.bind(Id(container_id));
        if let Some(column) = org {
            sql.push([" AND ", column, " = "]).bind(Id(org_id));
        }
        for row in scope.fetch_all(sql).await? {
            let id = row.id(0)?;
            let mut block = row.json(1, "block")?;
            if block["fields"].is_object()
                && rewrite_previous_date_values(&mut block["fields"], dates)
            {
                let mut sql = scope.sql("UPDATE deleted_blocks SET block = ");
                sql.bind(Param::json(&block)?)
                    .push_bind(" WHERE id = ", Id(&id))
                    .push_bind(" AND container_id = ", Id(container_id));
                if let Some(column) = org {
                    sql.push([" AND ", column, " = "]).bind(Id(org_id));
                }
                scope.execute(sql).await?;
                changed += 1;
            }
        }
    }
    Ok(changed)
}

/// The identifiers of the date properties in a stored JSON list of definitions.
fn date_ids(list: &serde_json::Value) -> HashSet<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_object)
        .filter(|definition| {
            definition.get("type").and_then(serde_json::Value::as_str) == Some(DATE_PROPERTY_TYPE)
        })
        .filter_map(|definition| definition.get("id").and_then(serde_json::Value::as_str))
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The step keeps every current block type plus `page` and the retired `board` block
    /// type, so newly added current types are kept as well.
    #[test]
    fn the_kept_types_are_the_current_ones_and_the_page_and_the_board() {
        for block_type in BlockType::ALL.into_iter().filter(|kind| kind.is_current()) {
            assert!(KEPT_TYPES.contains(&block_type.as_str()), "{block_type}");
        }
        let besides: Vec<&str> = KEPT_TYPES
            .into_iter()
            .filter(|kept| !kept.parse::<BlockType>().is_ok_and(BlockType::is_current))
            .collect();
        assert_eq!(besides, ["page", "board"]);
    }
}
