//! Changes of the stored data that the schema migrations leave to the server: each is
//! named by a row of `data_steps`, which the migration that needs it adds, and is done
//! once, after the migrations, in one transaction that also removes its row.

use std::collections::{HashMap, HashSet};

use fvoci_model::{BlockType, date_property_ids, rewrite_previous_date_values};

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
/// operations make, and `board`, the type of the original's board block, which is no
/// content block (and which a block of a database of before the step may have).
const KEPT_TYPES: [&str; 6] = ["card", "view", "comment", "attachment", "page", "board"];

/// The step of migration 42: the blocks of the type `board`, the original's board block,
/// which the step of migration 8 kept, are deleted with every block below them.
const BOARD_BLOCKS: &str = "board_blocks";

/// The type string of the original's board block.
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
                tracing::info!(
                    deleted,
                    "the blocks of the original's board block type were deleted"
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

/// Deletes the content blocks (every type but [`KEPT_TYPES`]) with every block below
/// them, as deleting a block does, on the boards and in the trash, and the references to
/// the files they showed. A file that no block shows any more is marked, to be collected
/// after the grace period like any other. Returns how many blocks were deleted.
async fn content_blocks(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    // `UNION`, not `UNION ALL`: a cycle of parents ends.
    let mut sql =
        scope.sql("WITH RECURSIVE tree(id) AS (SELECT id FROM blocks WHERE type NOT IN (");
    for (index, block_type) in KEPT_TYPES.iter().enumerate() {
        sql.push_bind(if index == 0 { "" } else { ", " }, *block_type);
    }
    let org = org_column(scope);
    sql.push(&format!(
        ") UNION SELECT b.id FROM blocks AS b JOIN tree ON b.parent_id = tree.id) \
         DELETE FROM blocks WHERE id IN (SELECT id FROM tree) RETURNING {}",
        key(org, "")
    ));
    let live = keys_of(&scope.fetch_all(sql).await?, org)?;
    let mut deleted = u64::try_from(live.len()).unwrap_or(u64::MAX);

    // The trash keeps a block as JSON: its type is read there.
    deleted += delete_from_trash(scope, org, &live, |block_type| {
        !KEPT_TYPES.contains(&BlockType::from(block_type).as_str())
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
    let (key, key_of_b) = (key(org, ""), key(org, "b."));
    let containers = touched_containers(scope).await?;
    // `UNION`, not `UNION ALL`: a cycle of parents ends.
    let mut sql = scope.sql(&format!(
        "WITH RECURSIVE tree({key}) AS (SELECT {key} FROM blocks WHERE type = "
    ));
    sql.bind(BOARD_TYPE).push(&format!(
        " UNION SELECT {key_of_b} FROM blocks AS b JOIN tree ON b.parent_id = tree.id{}) \
         DELETE FROM blocks WHERE ({key}) IN (SELECT {key} FROM tree) RETURNING {key}",
        scope.same_organization(&[("b", "tree")]),
    ));
    let live = keys_of(&scope.fetch_all(sql).await?, org)?;
    let mut deleted = u64::try_from(live.len()).unwrap_or(u64::MAX);
    deleted += delete_from_trash(scope, org, &live, |block_type| block_type == BOARD_TYPE).await?;
    // A database that had no such block is left as it was.
    if deleted > 0 {
        block_derived::forget_gone_blocks(scope).await?;
        for container_id in &containers {
            search_outbox::note_container_regardless(scope, container_id).await?;
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

/// The columns that name a block, of the table or alias `prefix` (`""` or `"b."`).
fn key(org: Option<&str>, prefix: &str) -> String {
    match org {
        Some(column) => format!("{prefix}{column}, {prefix}id"),
        None => format!("{prefix}id"),
    }
}

/// The blocks the rows name, as [`key`] selects them: by organization (empty where the
/// database holds one) and identifier.
fn keys_of(rows: &[Row], org: Option<&str>) -> Result<Vec<(String, String)>, StoreError> {
    rows.iter()
        .map(|row| match org {
            Some(_) => Ok((row.id(0)?, row.id(1)?)),
            None => Ok((String::new(), row.id(0)?)),
        })
        .collect()
}

/// The boards that hold a live block of the type `board`.
async fn touched_containers(scope: &mut Scope<'_>) -> Result<Vec<String>, StoreError> {
    let mut sql = scope.sql("SELECT DISTINCT container_id FROM blocks WHERE type = ");
    sql.bind(BOARD_TYPE);
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.id(0))
        .collect()
}

/// Deletes from the trash the blocks whose type `is_gone` says and those whose parent
/// is among `deleted` (the live blocks the step deleted, by organization and
/// identifier), with every block below them there, each organization's apart (`org`,
/// the column that names it, where the database holds several). Returns how many went.
async fn delete_from_trash(
    scope: &mut Scope<'_>,
    org: Option<&str>,
    deleted: &[(String, String)],
    is_gone: impl Fn(&str) -> bool,
) -> Result<u64, StoreError> {
    let org_select = org.map_or_else(String::new, |column| format!(", {column}"));
    let sql = scope.sql(&format!(
        "SELECT id, parent_id, block{org_select} FROM deleted_blocks"
    ));
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

/// The identifiers of each board's date properties, live boards and those in the trash.
type DateProperties = HashMap<String, HashSet<String>>;

/// Rewrites the date values of cards and the date filters of views, live and in the
/// trash, one board at a time. Returns how many blocks changed.
async fn date_values(scope: &mut Scope<'_>) -> Result<u64, StoreError> {
    let mut dates = DateProperties::new();
    let sql = scope.sql("SELECT id, card_properties FROM containers");
    for row in scope.fetch_all(sql).await? {
        let properties = row.json(1, "card_properties")?;
        dates.insert(row.id(0)?, date_property_ids(definitions(&properties)));
    }
    let sql = scope.sql("SELECT id, container FROM deleted_containers");
    for row in scope.fetch_all(sql).await? {
        let container = row.json(1, "container")?;
        dates.insert(
            row.id(0)?,
            date_property_ids(definitions(&container["cardProperties"])),
        );
    }

    let mut changed = 0;
    for (container_id, dates) in dates.iter().filter(|(_, dates)| !dates.is_empty()) {
        let mut sql = scope.sql("SELECT id, fields FROM blocks WHERE type IN ('card', 'view')");
        sql.push_bind(" AND container_id = ", Id(container_id));
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
        let mut sql = scope.sql("SELECT id, block FROM deleted_blocks WHERE container_id = ");
        sql.bind(Id(container_id));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The step keeps what a block may be given, and the page and the type of the
    /// original's board block besides: a type that becomes current is kept too.
    #[test]
    fn the_kept_types_are_the_current_ones_and_the_page_and_the_board() {
        let current = [
            "card",
            "view",
            "comment",
            "attachment",
            "board",
            "text",
            "h1",
            "",
        ]
        .map(BlockType::from)
        .into_iter()
        .filter(BlockType::is_current);
        for block_type in current {
            assert!(KEPT_TYPES.contains(&block_type.as_str()), "{block_type}");
        }
        let besides: Vec<&str> = KEPT_TYPES
            .into_iter()
            .filter(|kept| !BlockType::from(*kept).is_current())
            .collect();
        assert_eq!(besides, ["page", "board"]);
    }
}
