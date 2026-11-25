//! Personal unit progress (`unit_progress`) and course-level completion summaries.
//!
//! Every personal read and write is scoped to the calling user (`user_id`) and the
//! handle's organization. Progress rows survive soft deletion of a unit or course into
//! the trash so restoration brings progress back, and are cleaned up on permanent purge
//! ([`forget_gone_blocks`]), workspace removal ([`forget_user_in_workspace`]),
//! organization departure ([`forget_user_in_organization`]), and account erasure.

use fvoci_model::{
    BlockType, CompletionChange, ContainerKind, CourseProgressSummary, LastPositionChange,
    MemberCourseProgress, UnitProgress, UnitProgressChange, is_id,
};

use crate::Store;
use crate::containers::inherited_members_source;
use crate::db::{Dialect, Id, Param, Row, Scope, now_millis};
use crate::error::StoreError;

impl Store {
    /// Returns the calling learner's personal progress rows for live units of `course_id`,
    /// ordered by `unit_id`.
    ///
    /// Units currently in the trash are excluded while their progress rows remain stored
    /// until permanent purge.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn get_my_course_progress(
        &self,
        course_id: &str,
        user_id: &str,
    ) -> Result<Vec<UnitProgress>, StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            let mut sql = scope.sql(
                "SELECT up.unit_id, up.completed_at, up.planned_date, up.last_position, \
                 up.updated_at \
                 FROM unit_progress AS up \
                 JOIN blocks AS b ON b.id = up.unit_id \
                 JOIN containers AS c ON c.id = b.container_id \
                 JOIN workspaces AS w ON w.id = c.workspace_id \
                 JOIN organizations AS o ON o.id = w.org_id \
                 WHERE up.container_id = ",
            );
            sql.bind(Id(course_id))
                .push_bind(" AND up.user_id = ", Id(user_id))
                .push(" AND b.container_id = up.container_id")
                .push_bind(" AND b.type = ", BlockType::Unit.as_str())
                .push_bind(" AND c.kind = ", ContainerKind::Course.as_str())
                .push_bind(" AND w.org_id = ", org)
                .push(" AND w.deleted_at IS NULL")
                .push(" AND o.deleted_at IS NULL")
                .push(&scope.same_organization(&[("up", "b"), ("b", "c"), ("c", "w")]))
                .push(" ORDER BY up.unit_id");
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(progress_from_row)
                .collect()
        })
        .await
    }

    /// Creates or updates the calling learner's personal progress row on a live unit of
    /// `course_id`.
    ///
    /// Completion is idempotent: marking an already completed unit complete preserves its
    /// existing `completed_at` timestamp. `planned_date` is preserved across updates.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if `user_id` is not an identifier,
    /// [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if `unit_id` is not a live [`BlockType::Unit`] belonging
    /// to live course `course_id` in the handle's organization (including chapters,
    /// trashed units, and non-course containers), and [`StoreError::Database`] if a
    /// statement fails.
    pub async fn upsert_unit_progress(
        &self,
        course_id: &str,
        unit_id: &str,
        user_id: &str,
        change: &UnitProgressChange,
    ) -> Result<UnitProgress, StoreError> {
        if !is_id(user_id) {
            return Err(StoreError::InvalidId(user_id.to_owned()));
        }
        self.transaction(async |scope| {
            let org = scope.org()?;
            require_live_unit(scope, course_id, unit_id, user_id, org.clone()).await?;
            upsert_row(scope, course_id, unit_id, user_id, org, change).await
        })
        .await
    }

    /// Returns course-wide progress counts: the number of live units (`BlockType::Unit`)
    /// and each active course member's completed live-unit count, ordered by `user_id`.
    ///
    /// Excludes chapters, trashed units, and accounts that are deleted, pending deletion,
    /// or no longer active members of the course's workspace and organization. Never reads
    /// or returns per-unit identifiers, timestamps, planned dates, or last positions.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization, and
    /// [`StoreError::Database`] if the query fails.
    pub async fn get_course_progress_summary(
        &self,
        course_id: &str,
    ) -> Result<CourseProgressSummary, StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            let rows = fetch_summary_rows(scope, course_id, org.clone()).await?;
            if let Some(first) = rows.first() {
                let total_units = count_u32(first.int(2)?);
                let members = rows
                    .iter()
                    .map(|row| {
                        Ok(MemberCourseProgress {
                            user_id: row.id(0)?,
                            completed_units: count_u32(row.int(1)?).min(total_units),
                        })
                    })
                    .collect::<Result<Vec<_>, StoreError>>()?;
                return Ok(CourseProgressSummary {
                    total_units,
                    members,
                });
            }
            let total_units = count_live_units(scope, course_id, org).await?;
            Ok(CourseProgressSummary {
                total_units,
                members: Vec::new(),
            })
        })
        .await
    }
}

/// Verifies `unit_id` is a live `BlockType::Unit` on live course `course_id` in live
/// organization `org`, and that `user_id` is an active member of its workspace and not
/// deleted or erased.
///
/// On PostgreSQL locks `blocks` (`FOR KEY SHARE`), `workspace_members` (`FOR SHARE`),
/// and `users` (`FOR KEY SHARE`) in that global order so concurrent unit deletion,
/// workspace removal, org departure, or account erasure serialize cleanly without
/// blocking learners updating progress side by side.
pub(crate) async fn require_live_unit(
    scope: &mut Scope<'_>,
    course_id: &str,
    unit_id: &str,
    user_id: &str,
    org: Param,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(
        "SELECT c.workspace_id \
         FROM blocks AS b \
         JOIN containers AS c ON c.id = b.container_id \
         JOIN workspaces AS w ON w.id = c.workspace_id \
         JOIN organizations AS o ON o.id = w.org_id \
         WHERE b.id = ",
    );
    sql.bind(Id(unit_id))
        .push_bind(" AND b.container_id = ", Id(course_id))
        .push_bind(" AND b.type = ", BlockType::Unit.as_str())
        .push_bind(" AND c.kind = ", ContainerKind::Course.as_str())
        .push_bind(" AND w.org_id = ", org.clone())
        .push(" AND w.deleted_at IS NULL")
        .push(" AND o.deleted_at IS NULL")
        .push(&scope.same_organization(&[("b", "c"), ("c", "w")]));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR KEY SHARE OF b");
    }
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Err(StoreError::not_found("unit"));
    };
    let workspace_id = row.id(0)?;

    let mut member = scope.sql("SELECT 1 FROM workspace_members WHERE workspace_id = ");
    member
        .bind(Id(&workspace_id))
        .push_bind(" AND user_id = ", Id(user_id));
    if scope.dialect() == Dialect::Postgres {
        member.push(" FOR SHARE");
    }
    if scope.fetch_optional(member).await?.is_none() {
        return Err(StoreError::not_found("unit"));
    }

    let mut user = scope.sql("SELECT 1 FROM users WHERE id = ");
    user.bind(Id(user_id))
        .push(" AND deleted_at IS NULL")
        .push_bind(" AND erased_at = ", 0_i64);
    if scope.dialect() == Dialect::Postgres {
        user.push(" FOR KEY SHARE");
    }
    if scope.fetch_optional(user).await?.is_none() {
        return Err(StoreError::not_found("unit"));
    }
    crate::course_meetings::require_active_course_member(scope, course_id, user_id, org)
        .await
        .map_err(|error| {
            if error.is_not_found() {
                StoreError::not_found("unit")
            } else {
                error
            }
        })
}

/// Atomically inserts or updates one learner's `unit_progress` row and returns it.
async fn upsert_row(
    scope: &mut Scope<'_>,
    course_id: &str,
    unit_id: &str,
    user_id: &str,
    org: Param,
    change: &UnitProgressChange,
) -> Result<UnitProgress, StoreError> {
    let now = now_millis();
    let inserted_completed_at = match change.completion {
        CompletionChange::Complete => Some(now),
        CompletionChange::Keep | CompletionChange::Incomplete => None,
    };
    let inserted_last_position = match &change.last_position {
        LastPositionChange::Set(position) => Some(position.clone()),
        LastPositionChange::Keep | LastPositionChange::Clear => None,
    };
    let conflict_target = match scope.dialect() {
        Dialect::Postgres => "(unit_id, org_id, user_id)",
        Dialect::Sqlite => "(unit_id, user_id)",
    };
    let completed_set = match change.completion {
        CompletionChange::Complete => {
            "completed_at = COALESCE(unit_progress.completed_at, excluded.completed_at)"
        }
        CompletionChange::Incomplete => "completed_at = NULL",
        CompletionChange::Keep => "completed_at = unit_progress.completed_at",
    };
    let position_set = match change.last_position {
        LastPositionChange::Set(_) => "last_position = excluded.last_position",
        LastPositionChange::Clear => "last_position = NULL",
        LastPositionChange::Keep => "last_position = unit_progress.last_position",
    };
    let mut sql = scope.sql(
        "INSERT INTO unit_progress \
         (unit_id, container_id, user_id, completed_at, planned_date, last_position, \
          updated_at, org_id) VALUES (",
    );
    sql.changing_no_access()
        .bind(Id(unit_id))
        .push_bind(", ", Id(course_id))
        .push_bind(", ", Id(user_id))
        .push_bind(", ", Param::OptInt(inserted_completed_at))
        .push_bind(", ", Param::OptText(None))
        .push_bind(", ", Param::OptText(inserted_last_position))
        .push_bind(", ", now)
        .push_bind(", ", org)
        .push(") ON CONFLICT ")
        .push(conflict_target)
        .push(" DO UPDATE SET container_id = excluded.container_id, ")
        .push(completed_set)
        .push(", ")
        .push(position_set)
        .push(
            ", updated_at = excluded.updated_at \
             RETURNING unit_id, completed_at, planned_date, last_position, updated_at",
        );
    let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
    progress_from_row(&row)
}

/// Fetches `(user_id, completed_units, total_units)` for every active course member in
/// a single read-only `SELECT` statement.
async fn fetch_summary_rows(
    scope: &mut Scope<'_>,
    course_id: &str,
    org: Param,
) -> Result<Vec<Row>, StoreError> {
    let inherited = inherited_members_source(scope);
    let unit_type = BlockType::Unit.as_str();
    let course_kind = ContainerKind::Course.as_str();
    let mut sql = scope.sql(
        "SELECT am.user_id, COALESCE(cc.completed_units, 0), \
         (SELECT COUNT(*) FROM blocks AS ub \
          JOIN containers AS uc ON uc.id = ub.container_id \
          JOIN workspaces AS uw ON uw.id = uc.workspace_id \
          JOIN organizations AS uo ON uo.id = uw.org_id \
          WHERE ub.container_id = ",
    );
    sql.bind(Id(course_id))
        .push_bind(" AND ub.type = ", unit_type)
        .push_bind(" AND uc.kind = ", course_kind)
        .push_bind(" AND uw.org_id = ", org.clone())
        .push(" AND uw.deleted_at IS NULL")
        .push(" AND uo.deleted_at IS NULL")
        .push(&scope.same_organization(&[("ub", "uc"), ("uc", "uw")]))
        .push(
            ") FROM (\
             SELECT cm.user_id \
             FROM container_members AS cm \
             JOIN containers AS c ON c.id = cm.container_id \
             JOIN workspace_members AS wm \
               ON wm.workspace_id = c.workspace_id AND wm.user_id = cm.user_id \
             JOIN workspaces AS w ON w.id = c.workspace_id \
             JOIN organizations AS o ON o.id = w.org_id \
             JOIN users AS u ON u.id = cm.user_id \
             WHERE cm.container_id = ",
        )
        .bind(Id(course_id))
        .push_bind(" AND c.kind = ", course_kind)
        .push_bind(" AND w.org_id = ", org.clone())
        .push(" AND w.deleted_at IS NULL")
        .push(" AND o.deleted_at IS NULL")
        .push(" AND u.deleted_at IS NULL")
        .push_bind(" AND u.erased_at = ", 0_i64)
        .push(&scope.same_organization(&[("cm", "c"), ("wm", "c"), ("w", "c")]))
        .push(" UNION SELECT im.user_id FROM ")
        .push(&inherited)
        .push(
            " AS im \
             JOIN containers AS c ON c.id = im.container_id \
             JOIN users AS u ON u.id = im.user_id \
             WHERE im.container_id = ",
        )
        .bind(Id(course_id))
        .push_bind(" AND c.kind = ", course_kind)
        .push_bind(" AND im.org_id = ", org.clone())
        .push(" AND u.deleted_at IS NULL")
        .push_bind(" AND u.erased_at = ", 0_i64)
        .push(&scope.same_organization(&[("im", "c")]))
        .push(
            ") AS am \
             LEFT JOIN (\
             SELECT up.user_id, COUNT(*) AS completed_units \
             FROM unit_progress AS up \
             JOIN blocks AS b ON b.id = up.unit_id \
             JOIN containers AS c ON c.id = b.container_id \
             JOIN workspaces AS w ON w.id = c.workspace_id \
             JOIN organizations AS o ON o.id = w.org_id \
             WHERE up.container_id = ",
        )
        .bind(Id(course_id))
        .push(" AND up.completed_at IS NOT NULL AND b.container_id = up.container_id")
        .push_bind(" AND b.type = ", unit_type)
        .push_bind(" AND c.kind = ", course_kind)
        .push_bind(" AND w.org_id = ", org)
        .push(" AND w.deleted_at IS NULL")
        .push(" AND o.deleted_at IS NULL")
        .push(&scope.same_organization(&[("up", "b"), ("b", "c"), ("c", "w")]))
        .push(" GROUP BY up.user_id) AS cc ON cc.user_id = am.user_id ORDER BY am.user_id");
    scope.fetch_all(sql).await
}

/// Counts live units (`BlockType::Unit`) in `course_id`, used when a course has zero
/// active members.
async fn count_live_units(
    scope: &mut Scope<'_>,
    course_id: &str,
    org: Param,
) -> Result<u32, StoreError> {
    let mut sql = scope.sql(
        "SELECT COUNT(*) \
         FROM blocks AS b \
         JOIN containers AS c ON c.id = b.container_id \
         JOIN workspaces AS w ON w.id = c.workspace_id \
         JOIN organizations AS o ON o.id = w.org_id \
         WHERE b.container_id = ",
    );
    sql.bind(Id(course_id))
        .push_bind(" AND b.type = ", BlockType::Unit.as_str())
        .push_bind(" AND c.kind = ", ContainerKind::Course.as_str())
        .push_bind(" AND w.org_id = ", org)
        .push(" AND w.deleted_at IS NULL")
        .push(" AND o.deleted_at IS NULL")
        .push(&scope.same_organization(&[("b", "c"), ("c", "w")]));
    let count = scope
        .fetch_optional(sql)
        .await?
        .map_or(Ok(0), |row| row.int(0))?;
    Ok(count_u32(count))
}

pub(crate) fn progress_from_row(row: &Row) -> Result<UnitProgress, StoreError> {
    Ok(UnitProgress {
        unit_id: row.id(0)?,
        completed_at: row.opt_int(1)?,
        planned_date: row.opt_text(2)?,
        last_position: row.opt_text(3)?,
        updated_at: row.int(4)?,
        suggested_next_interval_days: None,
    })
}

fn count_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// Forgets personal progress rows of units that are gone for good: neither live in
/// `blocks` nor awaiting restore in `deleted_blocks` of the row's organization.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let statement = [
        "DELETE FROM unit_progress WHERE NOT EXISTS \
         (SELECT 1 FROM blocks AS b WHERE b.id = unit_progress.unit_id",
        scope.same_organization(&[("b", "unit_progress")]).as_str(),
        ") AND NOT EXISTS \
         (SELECT 1 FROM deleted_blocks AS d WHERE d.id = unit_progress.unit_id",
        scope.same_organization(&[("d", "unit_progress")]).as_str(),
        ")",
    ]
    .concat();
    let mut sql = scope.sql(&statement);
    sql.changing_no_access();
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes every `unit_progress` row of `user_id` for courses belonging to
/// `workspace_id` (both live and in the trash).
pub(crate) async fn forget_user_in_workspace(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM unit_progress WHERE user_id = ");
    sql.changing_no_access()
        .bind(Id(user_id))
        .push_bind(
            " AND container_id IN (\
             SELECT id FROM containers WHERE workspace_id = ",
            Id(workspace_id),
        )
        .push_bind(
            " UNION SELECT id FROM deleted_containers WHERE workspace_id = ",
            Id(workspace_id),
        )
        .push(")");
    if scope.dialect() == Dialect::Postgres {
        sql.push_bind(" AND org_id = ", scope.org()?);
    }
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes `unit_progress` rows of `user_id` in `workspace_id` for courses where
/// `user_id` no longer holds any direct or inherited membership (called when an
/// organization admin who already belonged to the workspace ends their joining).
pub(crate) async fn forget_user_in_workspace_unjoined(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    let inherited = inherited_members_source(scope);
    let mut sql = scope.sql("DELETE FROM unit_progress WHERE user_id = ");
    sql.changing_no_access()
        .bind(Id(user_id))
        .push_bind(
            " AND container_id IN (\
             SELECT id FROM containers WHERE workspace_id = ",
            Id(workspace_id),
        )
        .push_bind(
            " UNION SELECT id FROM deleted_containers WHERE workspace_id = ",
            Id(workspace_id),
        )
        .push(
            ") AND NOT EXISTS (\
             SELECT 1 FROM container_members AS cm \
             WHERE cm.container_id = unit_progress.container_id \
               AND cm.user_id = unit_progress.user_id",
        )
        .push(&scope.same_organization(&[("cm", "unit_progress")]))
        .push(") AND NOT EXISTS (SELECT 1 FROM ")
        .push(&inherited)
        .push(
            " AS im \
             WHERE im.container_id = unit_progress.container_id \
               AND im.user_id = unit_progress.user_id",
        )
        .push_bind(" AND im.org_id = ", org.clone())
        .push(&scope.same_organization(&[("im", "unit_progress")]))
        .push(")");
    if scope.dialect() == Dialect::Postgres {
        sql.push_bind(" AND org_id = ", org);
    }
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes every `unit_progress` row of `user_id` belonging to the scope's organization,
/// without touching progress stored in other organizations or the user's personal space.
pub(crate) async fn forget_user_in_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    let mut sql = scope.sql("DELETE FROM unit_progress WHERE user_id = ");
    sql.changing_no_access().bind(Id(user_id));
    match scope.dialect() {
        Dialect::Postgres => {
            sql.push_bind(" AND org_id = ", org);
        }
        Dialect::Sqlite => {
            sql.push_bind(" AND (org_id = ", org.clone())
                .push_bind(
                    " OR (org_id = '' AND container_id IN (\
                     SELECT c.id FROM containers AS c \
                     JOIN workspaces AS w ON w.id = c.workspace_id WHERE w.org_id = ",
                    org.clone(),
                )
                .push_bind(
                    " UNION SELECT d.id FROM deleted_containers AS d \
                     JOIN workspaces AS w ON w.id = d.workspace_id WHERE w.org_id = ",
                    org,
                )
                .push(")))");
        }
    }
    scope.execute(sql).await?;
    Ok(())
}
