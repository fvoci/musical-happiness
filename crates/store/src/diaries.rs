//! Diaries: making one in the workspace of a user's own space, the entry of a day, the
//! days that have one, and the trash of a diary's entries.
//!
//! An entry is a block ([`BlockType::Entry`]) that says its day among its fields. The
//! table `diary_days` has that day beside the block, a row per entry that is among the
//! blocks: [`refresh`] puts it down wherever a block is stored (`block_derived`), and
//! it goes with its block by its key. What asks for the entries of a day, or for the
//! days that are written, reads those rows and not the diary's entries; and the key of
//! the table is what holds a diary to one entry a day, whatever stores an entry.

use fvoci_model::{
    Block, BlockError, BlockType, Container, ContainerError, ContainerKind, ContainerMember,
    DIARIES_PER_SPACE, DeletedDiaryEntry, DiaryDate, DiaryDay, DiaryMaking, MonthDay,
    NewDiaryEntry, OrgKind, entry_date, entry_mood, new_id,
};

use crate::block_derived::Refreshing;
use crate::blocks::{self, Paging};
use crate::db::{Dialect, Id, Param, Scope, Sql, now_millis};
use crate::trash::{
    block_from_json, block_not_in_trash, from_json, push_paging, take_blocks_out_of_trash,
};
use crate::{Store, StoreError, containers, directory, org_admin, workspace_membership};

/// What [`Store::create_diary`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct DiaryMade {
    /// The diary: the one made, or the oldest the space had.
    pub diary: Container,
    /// The membership of its maker, if it was made now.
    pub made: Option<ContainerMember>,
}

/// What [`Store::create_diary_entry`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryMade {
    /// The entry of the day: the one made, or the one the day had.
    pub entry: Block,
    /// Whether it was made now.
    pub made: bool,
}

impl Store {
    /// Makes `diary` in its workspace with `user_id` as its admin, as `making` says.
    /// The workspace is held from before its diaries are counted, so two made at once
    /// are counted one after the other.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such workspace in use, and
    /// [`StoreError::InvalidContainer`] if the container may not be stored as a diary,
    /// the workspace is not of a user's own space
    /// ([`ContainerError::DiaryOffOwnSpace`]), or the space has as many diaries as it
    /// may ([`ContainerError::TooManyDiaries`]).
    pub async fn create_diary(
        &self,
        mut diary: Container,
        user_id: &str,
        making: DiaryMaking,
    ) -> Result<DiaryMade, StoreError> {
        self.transaction(async |scope| {
            // The organization's first, as whatever holds a workspace takes them.
            org_admin::hold_organization_row(scope).await?;
            if !workspace_membership::hold_workspace(scope, &diary.workspace_id).await? {
                return Err(StoreError::not_found("workspace"));
            }
            if making == DiaryMaking::IfNone
                && let Some(oldest) = oldest_diary(scope, &diary.workspace_id).await?
            {
                return Ok(DiaryMade {
                    diary: oldest,
                    made: None,
                });
            }
            diary.kind = ContainerKind::Diary;
            let member =
                containers::insert_container_with_admin(scope, &mut diary, user_id).await?;
            Ok(DiaryMade {
                diary,
                made: Some(member),
            })
        })
        .await
    }

    /// Makes the entry of the day `date` of the diary as `new` says, unless the day has
    /// one: then that entry is answered as it is, and nothing of `new` is stored. The
    /// diary is held while the day is asked for, so of two entries made of one day at
    /// once one is made and the other request is answered with it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such diary, and
    /// [`StoreError::InvalidBlock`] if the diary has as many entries as `limit`
    /// ([`BlockError::TooManyEntries`]) or the entry may not be stored.
    pub async fn create_diary_entry(
        &self,
        diary_id: &str,
        date: DiaryDate,
        new: NewDiaryEntry,
        limit: u32,
        user_id: &str,
    ) -> Result<EntryMade, StoreError> {
        self.transaction(async |scope| {
            if containers::lock_container_of_kind(scope, diary_id).await?
                != Some(ContainerKind::Diary)
            {
                return Err(StoreError::not_found(format_args!("diary ID={diary_id}")));
            }
            if let Some(entry_id) = entry_of_day(scope, diary_id, &date.to_string()).await? {
                let entry = blocks::get_block(scope, &entry_id).await?;
                return Ok(EntryMade { entry, made: false });
            }
            let mut entry = Block {
                id: new_id(),
                container_id: diary_id.to_owned(),
                block_type: BlockType::Entry,
                title: date.to_string(),
                fields: new.into_fields(date),
                ..Block::default()
            };
            check_room(scope, &entry, limit).await?;
            blocks::insert_block(scope, &mut entry, user_id).await?;
            Ok(EntryMade { entry, made: true })
        })
        .await
    }

    /// The entries of the diary whose day is `at` in any year, the latest year first.
    /// The rows of the days say which they are: no other entry of the diary is read.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_diary_entries_on(
        &self,
        diary_id: &str,
        at: MonthDay,
    ) -> Result<Vec<Block>, StoreError> {
        self.with_conn(async |scope| {
            let sql = entries_on(scope, diary_id, at);
            blocks::fetch_blocks(scope, sql).await
        })
        .await
    }

    /// Checks that `coming` more diaries may be made in the workspace, as each is
    /// checked when it is made: for what makes several at once and would be left with
    /// some of them, asked before the first.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidContainer`] if the workspace is not of a user's own
    /// space ([`ContainerError::DiaryOffOwnSpace`]) or would have more diaries than a
    /// space may ([`ContainerError::TooManyDiaries`]).
    pub async fn check_room_for_diaries(
        &self,
        workspace_id: &str,
        coming: u32,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| check_space(scope, workspace_id, coming).await)
            .await
    }

    /// The days of the diary that have an entry, from `from` to `to` (each included;
    /// `None` for no bound), the earliest first: the day, its entry and its mood, read
    /// from the rows of the days and from no entry.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_diary_days(
        &self,
        diary_id: &str,
        from: Option<DiaryDate>,
        to: Option<DiaryDate>,
    ) -> Result<Vec<DiaryDay>, StoreError> {
        self.with_conn(async |scope| {
            let sql = days_between(scope, diary_id, from, to);
            let mut days = Vec::new();
            for row in &scope.fetch_all(sql).await? {
                let corrupt = |column: &'static str| StoreError::Corrupt {
                    column,
                    reason: "a row of diary_days holds what no entry has".to_owned(),
                };
                days.push(DiaryDay {
                    date: row.text(0)?.parse().map_err(|_| corrupt("day"))?,
                    entry_id: row.id(1)?,
                    mood: match row.opt_text(2)? {
                        Some(mood) => Some(mood.parse().map_err(|_| corrupt("mood"))?),
                        None => None,
                    },
                });
            }
            Ok(days)
        })
        .await
    }

    /// The entries of the diary in the trash, the last deleted first and then by
    /// identifier: one page of them if `paging` says so (a page size of zero is every
    /// one).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_deleted_diary_entries(
        &self,
        diary_id: &str,
        paging: Option<Paging>,
    ) -> Result<Vec<DeletedDiaryEntry>, StoreError> {
        self.with_conn(async |scope| {
            // The type is in the stored block.
            let is_entry = match scope.dialect() {
                Dialect::Sqlite => "json_extract(block, '$.type') = 'entry'",
                Dialect::Postgres => "block->>'type' = 'entry'",
            };
            let mut sql = scope.sql(
                "SELECT block, deleted_by, deleted_at FROM deleted_blocks WHERE container_id = ",
            );
            sql.bind(Id(diary_id))
                .push(&format!(" AND {is_entry} ORDER BY deleted_at DESC, id"));
            push_paging(&mut sql, paging);
            let mut entries = Vec::new();
            for row in &scope.fetch_all(sql).await? {
                let block = block_from_json(row, 0)?;
                entries.push(DeletedDiaryEntry {
                    date: entry_date(&block)
                        .map(|date| date.to_string())
                        .unwrap_or_default(),
                    mood: entry_mood(&block),
                    id: block.id,
                    deleted_at: row.int(2)?,
                    deleted_by: row.id(1)?,
                });
            }
            Ok(entries)
        })
        .await
    }

    /// Brings an entry of the diary back from the trash to its day, held to `limit`,
    /// and marks it as modified by `user_id` now. The diary is held while its days are
    /// asked for, so the day is still free when the entry is back on it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the trash of the diary has no such entry,
    /// [`StoreError::NotRestorable`] with [`BlockError::DayTaken`] if the day has an
    /// entry since, and [`StoreError::InvalidBlock`] with
    /// [`BlockError::TooManyEntries`] if the diary has as many entries as `limit`.
    pub async fn undelete_diary_entry(
        &self,
        diary_id: &str,
        entry_id: &str,
        limit: u32,
        user_id: &str,
    ) -> Result<Block, StoreError> {
        self.transaction(async |scope| {
            // The diary first, then the trash read under its lock.
            if containers::lock_container_of_kind(scope, diary_id).await?
                != Some(ContainerKind::Diary)
            {
                return Err(block_not_in_trash());
            }
            let mut sql = scope.sql("SELECT deletion_id, block FROM deleted_blocks WHERE id = ");
            sql.bind(Id(entry_id))
                .push_bind(" AND container_id = ", Id(diary_id));
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(block_not_in_trash)?;
            let entry = block_from_json(&row, 1)?;
            if entry.block_type != BlockType::Entry {
                return Err(block_not_in_trash());
            }
            if let Some(date) = entry_date(&entry)
                && entry_of_day(scope, diary_id, &date.to_string())
                    .await?
                    .is_some()
            {
                return Err(StoreError::NotRestorable {
                    id: entry.id,
                    source: BlockError::DayTaken {
                        date: date.to_string(),
                    },
                });
            }
            check_room(scope, &entry, limit).await?;
            let restored =
                take_blocks_out_of_trash(scope, &row.id(0)?, user_id, now_millis(), |_| {}).await?;
            restored
                .into_iter()
                .find(|block| block.id == entry_id)
                .ok_or_else(block_not_in_trash)
        })
        .await
    }
}

/// The query of [`Store::get_diary_entries_on`].
fn entries_on(scope: &Scope<'_>, diary_id: &str, at: MonthDay) -> Sql {
    let mut sql = blocks::select_blocks(scope);
    sql.push_bind(" WHERE container_id = ", Id(diary_id))
        .push(" AND id IN (SELECT d.entry_id FROM diary_days AS d WHERE d.container_id = ")
        .bind(Id(diary_id))
        .push_bind(" AND d.month_day = ", at.to_string())
        // The title of an entry is its day, and days compare as their texts do.
        .push(") ORDER BY title DESC");
    sql
}

/// The query of [`Store::get_diary_days`].
fn days_between(
    scope: &Scope<'_>,
    diary_id: &str,
    from: Option<DiaryDate>,
    to: Option<DiaryDate>,
) -> Sql {
    let mut sql = scope.sql("SELECT day, entry_id, mood FROM diary_days WHERE container_id = ");
    sql.bind(Id(diary_id));
    if let Some(from) = from {
        sql.push_bind(" AND day >= ", from.to_string());
    }
    if let Some(to) = to {
        sql.push_bind(" AND day <= ", to.to_string());
    }
    sql.push(" ORDER BY day");
    sql
}

#[cfg(any(test, feature = "test-util"))]
impl Store {
    /// How the database answers [`Store::get_diary_entries_on`] and
    /// [`Store::get_diary_days`] for these: the lines of the plan of each, the very
    /// statements the two run. On PostgreSQL the statements are run, and the plan says
    /// what they read. For the tests that a day is found without reading the diary.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn plans_of_diary_reads(
        &self,
        diary_id: &str,
        at: MonthDay,
        from: DiaryDate,
        to: DiaryDate,
    ) -> Result<[Vec<String>; 2], StoreError> {
        self.with_conn(async |scope| {
            let mut plans: [Vec<String>; 2] = Default::default();
            let queries = [
                entries_on(scope, diary_id, at),
                days_between(scope, diary_id, Some(from), Some(to)),
            ];
            for (plan, query) in plans.iter_mut().zip(queries) {
                let line = match scope.dialect() {
                    Dialect::Sqlite => 3,
                    Dialect::Postgres => 0,
                };
                for row in &scope.fetch_all(query.explained()).await? {
                    plan.push(row.text(line)?);
                }
            }
            Ok(plans)
        })
        .await
    }
}

/// The entry the diary has of `day`, if it has one.
async fn entry_of_day(
    scope: &mut Scope<'_>,
    diary_id: &str,
    day: &str,
) -> Result<Option<String>, StoreError> {
    let mut sql = scope.sql("SELECT entry_id FROM diary_days WHERE container_id = ");
    sql.bind(Id(diary_id)).push_bind(" AND day = ", day);
    match scope.fetch_optional(sql).await? {
        Some(row) => Ok(Some(row.id(0)?)),
        None => Ok(None),
    }
}

/// Checks that the diary of `entry`, which is held, has room for one more entry.
async fn check_room(scope: &mut Scope<'_>, entry: &Block, limit: u32) -> Result<(), StoreError> {
    let mut sql =
        scope.sql("SELECT CAST(COUNT(*) AS BIGINT) FROM diary_days WHERE container_id = ");
    sql.bind(Id(&entry.container_id));
    let entries = match scope.fetch_optional(sql).await? {
        Some(row) => row.int(0)?,
        None => 0,
    };
    if entries >= i64::from(limit) {
        return Err(StoreError::InvalidBlock {
            id: entry.id.clone(),
            source: BlockError::TooManyEntries { limit },
        });
    }
    Ok(())
}

/// The oldest diary of the workspace, if it has one.
async fn oldest_diary(
    scope: &mut Scope<'_>,
    workspace_id: &str,
) -> Result<Option<Container>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {} FROM containers AS b WHERE b.workspace_id = ",
        containers::container_select_list("b.")
    ));
    sql.bind(Id(workspace_id))
        .push_bind(" AND b.kind = ", ContainerKind::Diary.as_str())
        .push(containers::OLDEST_CONTAINER_FIRST)
        .push(" LIMIT 1");
    Ok(containers::fetch_containers(scope, sql).await?.pop())
}

/// Checks that a diary that is about to be among the containers may be: its workspace
/// is of a user's own space, and has fewer diaries than a space may have
/// ([`DIARIES_PER_SPACE`]). Asked wherever a container's row is put down, a new one and
/// one that comes back from the trash, with the workspace held from before its diaries
/// are counted. Nothing is asked of a container of another kind.
pub(crate) async fn check_place(
    scope: &mut Scope<'_>,
    container: &Container,
) -> Result<(), StoreError> {
    if container.kind != ContainerKind::Diary {
        return Ok(());
    }
    check_space(scope, &container.workspace_id, 1).await
}

/// Checks that `coming` more diaries may be in the workspace: it is of a user's own
/// space, and with them it has no more diaries than a space may have. The workspace is
/// held from before its diaries are counted.
async fn check_space(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    coming: u32,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "SELECT o.kind FROM workspaces AS w JOIN organizations AS o ON o.id = w.org_id \
         WHERE w.id = ",
    );
    sql.bind(Id(workspace_id));
    if scope.dialect() == Dialect::Postgres {
        // One diary is put down in a workspace at a time: the count below is of what
        // is there when this one is.
        sql.push(" FOR NO KEY UPDATE OF w");
    }
    let of_own_space = match scope.fetch_optional(sql).await? {
        Some(row) => directory::org_kind(&row, 0)? == OrgKind::Personal,
        None => false,
    };
    if !of_own_space {
        return Err(ContainerError::DiaryOffOwnSpace.into());
    }
    let mut sql =
        scope.sql("SELECT CAST(COUNT(*) AS BIGINT) FROM containers WHERE workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND kind = ", ContainerKind::Diary.as_str());
    let diaries = match scope.fetch_optional(sql).await? {
        Some(row) => row.int(0)?,
        None => 0,
    };
    if diaries.saturating_add(i64::from(coming)) > i64::from(DIARIES_PER_SPACE) {
        return Err(ContainerError::TooManyDiaries {
            limit: DIARIES_PER_SPACE,
        }
        .into());
    }
    Ok(())
}

/// Holds the workspace of a diary that is in the trash, before the row of the trash is
/// held to bring it back: a workspace is held before the trash, and [`check_place`]
/// holds this one. Nothing is held for what is no diary, or is not in the trash.
pub(crate) async fn hold_place_of_deleted(
    scope: &mut Scope<'_>,
    container_id: &str,
) -> Result<(), StoreError> {
    if scope.dialect() != Dialect::Postgres {
        return Ok(());
    }
    let mut sql = scope.sql("SELECT container FROM deleted_containers WHERE id = ");
    sql.bind(Id(container_id));
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Ok(());
    };
    let container: Container = from_json(&row, 0, "container")?;
    if container.kind == ContainerKind::Diary {
        workspace_membership::hold_workspace(scope, &container.workspace_id).await?;
    }
    Ok(())
}

/// Brings the row of the day of `block` up to it, as it was just stored, if it is an
/// entry: a new row for an entry that had none (a new entry, one back from the trash,
/// one that came with an archive), and its mood for one that had. Nothing is done for a
/// block of another type.
///
/// The block's container is held by whoever stores a block, so the day that is found
/// free here is free when the row is put down; the key of the table holds it besides.
///
/// # Errors
///
/// Returns [`StoreError::InvalidBlock`] with [`BlockError::DayTaken`] if another entry
/// of the diary has the day, and with [`BlockError::EntryDateChanges`] if the entry is
/// stored with another day than it had.
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    _: Refreshing,
) -> Result<(), StoreError> {
    if block.block_type != BlockType::Entry {
        return Ok(());
    }
    let invalid = |source| StoreError::InvalidBlock {
        id: block.id.clone(),
        source,
    };
    let date = entry_date(block).ok_or_else(|| {
        invalid(BlockError::InvalidEntryField {
            field: fvoci_model::ENTRY_DATE_FIELD,
            expected: "a day, as `2026-11-21`",
        })
    })?;
    let day = date.to_string();
    let mood = entry_mood(block).map(|mood| mood.as_str().to_owned());
    let mut sql = scope.sql("SELECT day, mood FROM diary_days WHERE entry_id = ");
    sql.bind(Id(&block.id));
    if let Some(row) = scope.fetch_optional(sql).await? {
        if row.text(0)? != day {
            return Err(invalid(BlockError::EntryDateChanges));
        }
        // Most writes of an entry are of its body, and leave its row as it is.
        if row.opt_text(1)? != mood {
            let mut sql = scope.sql("UPDATE diary_days SET mood = ");
            sql.bind(Param::OptText(mood))
                .push_bind(" WHERE entry_id = ", Id(&block.id));
            scope.execute(sql).await?;
        }
        return Ok(());
    }
    if entry_of_day(scope, &block.container_id, &day)
        .await?
        .is_some()
    {
        return Err(invalid(BlockError::DayTaken { date: day }));
    }
    let mut sql =
        scope.sql("INSERT INTO diary_days (container_id, day, month_day, entry_id, mood) VALUES (");
    sql.bind(Id(&block.container_id))
        .push_bind(", ", day)
        .push_bind(", ", date.month_day().to_string())
        .push_bind(", ", Id(&block.id))
        .push_bind(", ", Param::OptText(mood))
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
