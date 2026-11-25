//! Course exams (`course_exams`) and strictly personal exam scores (`exam_scores`).
//!
//! Individual scores and notes are never returned for any user other than the authenticated
//! caller. Aggregate statistics (`average_score`, `score_count`) are computed exclusively
//! over active enrolled course members whose accounts are live.
//!
//! Rows survive soft deletion of a course into the trash and are removed on permanent
//! purge ([`forget_gone_containers`]). Personal `exam_scores` rows are deleted when a user
//! is removed from a workspace ([`forget_user_in_workspace`]), ends an org-admin joining
//! ([`forget_user_in_workspace_unjoined`]), leaves an organization
//! ([`forget_user_in_organization`]), or has their account erased.

use std::collections::BTreeMap;

use fvoci_model::{
    ContainerKind, CourseExam, CourseExamChange, ExamDateChange, MyExamScore, ValidatedMyExamScore,
    ValidatedNewExam, is_id, new_id,
};

use crate::Store;
use crate::containers::inherited_members_source;
use crate::course_meetings::{
    push_active_course_members_subquery, require_active_course_member, require_live_course,
};
use crate::db::{Dialect, Id, Param, Row, Scope, now_millis};
use crate::error::StoreError;

/// Outcome of recording the caller's personal exam score.
#[derive(Debug, Clone, PartialEq)]
pub enum ExamScoreOutcome {
    /// The caller's score was saved.
    Saved(MyExamScore),
    /// The score exceeded the exam's configured `max_score`.
    ExceedsMaxScore {
        /// The exam's current maximum score.
        max_score: f64,
    },
}

impl Store {
    /// Lists all exams of live course `course_id`, ordered by `exam_date ASC NULLS LAST,
    /// created_at ASC, id ASC`.
    ///
    /// When `caller_user_id` is `Some(uid)` (an active enrolled course member), includes
    /// `my_score` strictly for `uid`, `score_count` across active enrolled course
    /// members, and `average_score` only when at least 2 other active members (`<> uid`)
    /// have recorded a score. When `caller_user_id` is `None` (unjoined reader), omits
    /// `my_score` and suppresses aggregates (`average_score: None`, `score_count: 0`).
    /// Never fetches or returns any other member's individual score or note.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization, and
    /// [`StoreError::Database`] if a query fails.
    pub async fn list_course_exams(
        &self,
        course_id: &str,
        caller_user_id: Option<&str>,
    ) -> Result<Vec<CourseExam>, StoreError> {
        self.with_conn(async |scope| {
            let org = scope.org()?;
            let mut sql = scope.sql(
                "SELECT e.id, e.container_id, e.title, e.exam_date, e.max_score, \
                 e.description, e.created_at, e.updated_at \
                 FROM course_exams AS e \
                 JOIN containers AS c ON c.id = e.container_id \
                 JOIN workspaces AS w ON w.id = c.workspace_id \
                 JOIN organizations AS o ON o.id = w.org_id \
                 WHERE e.container_id = ",
            );
            sql.bind(Id(course_id))
                .push_bind(" AND e.org_id = ", org.clone())
                .push_bind(" AND c.kind = ", ContainerKind::Course.as_str())
                .push_bind(" AND w.org_id = ", org.clone())
                .push(" AND w.deleted_at IS NULL")
                .push(" AND o.deleted_at IS NULL")
                .push(&scope.same_organization(&[("e", "c"), ("c", "w")]))
                .push(
                    " ORDER BY CASE WHEN e.exam_date IS NULL THEN 1 ELSE 0 END, \
                     e.exam_date ASC, e.created_at ASC, e.id ASC",
                );
            let rows = scope.fetch_all(sql).await?;
            let mut exams = Vec::with_capacity(rows.len());
            for row in &rows {
                exams.push(exam_from_row(row)?);
            }
            if exams.is_empty() {
                return Ok(exams);
            }
            let Some(uid) = caller_user_id else {
                return Ok(exams);
            };
            let mut stats = fetch_exam_stats(scope, course_id, &org, uid, None).await?;
            let mut my_scores = fetch_my_scores(scope, course_id, &org, uid, None).await?;
            for exam in &mut exams {
                if let Some((average, count)) = stats.remove(&exam.id) {
                    exam.average_score = average;
                    exam.score_count = count;
                }
                exam.my_score = my_scores.remove(&exam.id);
            }
            Ok(exams)
        })
        .await
    }

    /// Creates a new exam on live course `course_id`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if `course_id` is not a live course, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn create_course_exam(
        &self,
        course_id: &str,
        new: &ValidatedNewExam,
    ) -> Result<CourseExam, StoreError> {
        self.transaction(async |scope| {
            let org = scope.org()?;
            require_live_course(scope, course_id, org.clone()).await?;
            let id = new_id();
            let now = now_millis();
            let mut sql = scope.sql(
                "INSERT INTO course_exams \
                 (id, org_id, container_id, title, exam_date, max_score, description, \
                  created_at, updated_at) VALUES (",
            );
            sql.changing_no_access()
                .bind(Id(&id))
                .push_bind(", ", org)
                .push_bind(", ", Id(course_id))
                .push_bind(", ", &new.title)
                .push_bind(", ", Param::OptText(new.exam_date.clone()))
                .push_bind(", ", new.max_score)
                .push_bind(", ", &new.description)
                .push_bind(", ", now)
                .push_bind(", ", now)
                .push(")");
            scope.execute(sql).await?;
            Ok(CourseExam {
                id,
                container_id: course_id.to_owned(),
                title: new.title.clone(),
                exam_date: new.exam_date.clone(),
                max_score: new.max_score,
                description: new.description.clone(),
                created_at: now,
                updated_at: now,
                my_score: None,
                average_score: None,
                score_count: 0,
            })
        })
        .await
    }

    /// Updates an exam of live course `course_id` and returns the updated view for
    /// `caller_user_id` under the same `my_score`, `score_count`, and k-anonymity
    /// (`>= 2` other active scorers) rules as [`Store::list_course_exams`].
    ///
    /// Changing `max_score` intentionally does not inspect other members' stored scores
    /// so it cannot be used as an oracle to probe private scores.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if the course or exam does not exist, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn patch_course_exam(
        &self,
        course_id: &str,
        exam_id: &str,
        caller_user_id: Option<&str>,
        change: &CourseExamChange,
    ) -> Result<CourseExam, StoreError> {
        self.transaction(async |scope| {
            let org = scope.org()?;
            require_live_course(scope, course_id, org.clone()).await?;
            let existing = fetch_exam_for_update(scope, course_id, exam_id, &org).await?;
            let now = now_millis();
            let title = change.title.as_deref().unwrap_or(&existing.title);
            let exam_date = match &change.exam_date {
                ExamDateChange::Keep => existing.exam_date.clone(),
                ExamDateChange::Clear => None,
                ExamDateChange::Set(date) => Some(date.clone()),
            };
            let max_score = change.max_score.unwrap_or(existing.max_score);
            let description = change
                .description
                .as_deref()
                .unwrap_or(&existing.description);
            let mut sql = scope.sql("UPDATE course_exams SET title = ");
            sql.changing_no_access()
                .bind(title)
                .push_bind(", exam_date = ", Param::OptText(exam_date.clone()))
                .push_bind(", max_score = ", max_score)
                .push_bind(", description = ", description)
                .push_bind(", updated_at = ", now)
                .push_bind(" WHERE id = ", Id(exam_id))
                .push_bind(" AND org_id = ", org.clone())
                .push_bind(" AND container_id = ", Id(course_id));
            scope.execute(sql).await?;

            let (average_score, score_count, my_score) = match caller_user_id {
                Some(uid) => {
                    let mut stats =
                        fetch_exam_stats(scope, course_id, &org, uid, Some(exam_id)).await?;
                    let (avg, cnt) = stats.remove(exam_id).unwrap_or((None, 0));
                    let mine = fetch_my_scores(scope, course_id, &org, uid, Some(exam_id))
                        .await?
                        .remove(exam_id);
                    (avg, cnt, mine)
                }
                None => (None, 0, None),
            };
            Ok(CourseExam {
                id: existing.id,
                container_id: existing.container_id,
                title: title.to_owned(),
                exam_date,
                max_score,
                description: description.to_owned(),
                created_at: existing.created_at,
                updated_at: now,
                my_score,
                average_score,
                score_count,
            })
        })
        .await
    }

    /// Permanently deletes an exam of live course `course_id` and all associated
    /// `exam_scores` rows.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if the course or exam does not exist, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn delete_course_exam(
        &self,
        course_id: &str,
        exam_id: &str,
    ) -> Result<(), StoreError> {
        if !is_id(exam_id) {
            return Err(StoreError::not_found("exam"));
        }
        self.transaction(async |scope| {
            let org = scope.org()?;
            require_live_course(scope, course_id, org.clone()).await?;
            let mut sql = scope.sql("DELETE FROM course_exams WHERE id = ");
            sql.changing_no_access()
                .bind(Id(exam_id))
                .push_bind(" AND org_id = ", org.clone())
                .push_bind(" AND container_id = ", Id(course_id));
            if scope.execute(sql).await? == 0 {
                return Err(StoreError::not_found("exam"));
            }
            let mut sql = scope.sql("DELETE FROM exam_scores WHERE exam_id = ");
            sql.changing_no_access()
                .bind(Id(exam_id))
                .push_bind(" AND org_id = ", org);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Creates or updates the calling learner's own personal score on `exam_id` of live
    /// course `course_id`.
    ///
    /// Enforces `0 <= score <= exam.max_score` and active course enrollment atomically
    /// inside the transaction while locking the exam, workspace membership, and user rows
    /// on PostgreSQL.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if `user_id` is not an identifier,
    /// [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if the course or exam does not exist or `user_id` is not
    /// an active enrolled member of `course_id`, and [`StoreError::Database`] if a
    /// statement fails.
    pub async fn upsert_my_exam_score(
        &self,
        course_id: &str,
        exam_id: &str,
        user_id: &str,
        input: &ValidatedMyExamScore,
    ) -> Result<ExamScoreOutcome, StoreError> {
        if !is_id(user_id) {
            return Err(StoreError::InvalidId(user_id.to_owned()));
        }
        self.transaction(async |scope| {
            let org = scope.org()?;
            let (workspace_id, max_score) =
                require_live_exam(scope, course_id, exam_id, &org).await?;
            require_active_workspace_user(scope, &workspace_id, user_id).await?;
            require_active_course_member(scope, course_id, user_id, org.clone()).await?;
            if !input.score.is_finite() || input.score < 0.0 || input.score > max_score {
                return Ok(ExamScoreOutcome::ExceedsMaxScore { max_score });
            }
            let score = if input.score == 0.0 { 0.0 } else { input.score };
            let now = now_millis();
            let mut sql = scope.sql(
                "INSERT INTO exam_scores \
                 (exam_id, org_id, container_id, user_id, score, note, updated_at) VALUES (",
            );
            sql.changing_no_access()
                .bind(Id(exam_id))
                .push_bind(", ", org)
                .push_bind(", ", Id(course_id))
                .push_bind(", ", Id(user_id))
                .push_bind(", ", score)
                .push_bind(", ", &input.note)
                .push_bind(", ", now)
                .push(
                    ") ON CONFLICT (exam_id, org_id, user_id) \
                     DO UPDATE SET score = excluded.score, note = excluded.note, \
                     updated_at = excluded.updated_at",
                );
            scope.execute(sql).await?;
            Ok(ExamScoreOutcome::Saved(MyExamScore {
                score,
                note: input.note.clone(),
                updated_at: now,
            }))
        })
        .await
    }

    /// Deletes the calling learner's own personal score on `exam_id` of live course
    /// `course_id`. Succeeds idempotently if the exam exists and the caller has no score.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoOrganization`] if the handle names no organization,
    /// [`StoreError::NotFound`] if the course or exam does not exist, and
    /// [`StoreError::Database`] if a statement fails.
    pub async fn delete_my_exam_score(
        &self,
        course_id: &str,
        exam_id: &str,
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let org = scope.org()?;
            require_live_exam(scope, course_id, exam_id, &org).await?;
            let mut sql = scope.sql("DELETE FROM exam_scores WHERE exam_id = ");
            sql.changing_no_access()
                .bind(Id(exam_id))
                .push_bind(" AND org_id = ", org)
                .push_bind(" AND user_id = ", Id(user_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

async fn require_live_exam(
    scope: &mut Scope<'_>,
    course_id: &str,
    exam_id: &str,
    org: &Param,
) -> Result<(String, f64), StoreError> {
    let mut sql = scope.sql(
        "SELECT c.workspace_id, e.max_score \
         FROM course_exams AS e \
         JOIN containers AS c ON c.id = e.container_id \
         JOIN workspaces AS w ON w.id = c.workspace_id \
         JOIN organizations AS o ON o.id = w.org_id \
         WHERE e.id = ",
    );
    sql.bind(Id(exam_id))
        .push_bind(" AND e.org_id = ", org.clone())
        .push_bind(" AND e.container_id = ", Id(course_id))
        .push_bind(" AND c.kind = ", ContainerKind::Course.as_str())
        .push_bind(" AND w.org_id = ", org.clone())
        .push(" AND w.deleted_at IS NULL")
        .push(" AND o.deleted_at IS NULL")
        .push(&scope.same_organization(&[("e", "c"), ("c", "w")]));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR KEY SHARE OF e");
    }
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Err(StoreError::not_found("exam"));
    };
    Ok((row.id(0)?, row.float(1)?))
}

async fn require_active_workspace_user(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut member = scope.sql("SELECT 1 FROM workspace_members WHERE workspace_id = ");
    member
        .bind(Id(workspace_id))
        .push_bind(" AND user_id = ", Id(user_id));
    if scope.dialect() == Dialect::Postgres {
        member.push(" FOR SHARE");
    }
    if scope.fetch_optional(member).await?.is_none() {
        return Err(StoreError::not_found("exam"));
    }
    let mut user = scope.sql("SELECT 1 FROM users WHERE id = ");
    user.bind(Id(user_id))
        .push(" AND deleted_at IS NULL")
        .push_bind(" AND erased_at = ", 0_i64);
    if scope.dialect() == Dialect::Postgres {
        user.push(" FOR KEY SHARE");
    }
    if scope.fetch_optional(user).await?.is_none() {
        return Err(StoreError::not_found("exam"));
    }
    Ok(())
}

async fn fetch_exam_for_update(
    scope: &mut Scope<'_>,
    course_id: &str,
    exam_id: &str,
    org: &Param,
) -> Result<CourseExam, StoreError> {
    let mut sql = scope.sql(
        "SELECT id, container_id, title, exam_date, max_score, description, created_at, \
         updated_at FROM course_exams WHERE id = ",
    );
    sql.bind(Id(exam_id))
        .push_bind(" AND org_id = ", org.clone())
        .push_bind(" AND container_id = ", Id(course_id));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    let Some(row) = scope.fetch_optional(sql).await? else {
        return Err(StoreError::not_found("exam"));
    };
    exam_from_row(&row)
}

/// Minimum number of other active course members who must have submitted a score before
/// `averageScore` is disclosed, preventing single-scorer exposure and two-scorer
/// subtraction (`2 * avg - myScore`).
const MIN_OTHER_SCORERS_FOR_AVERAGE: i64 = 2;

async fn fetch_exam_stats(
    scope: &mut Scope<'_>,
    course_id: &str,
    org: &Param,
    caller_user_id: &str,
    exam_id: Option<&str>,
) -> Result<BTreeMap<String, (Option<f64>, u32)>, StoreError> {
    let mut sql = scope.sql(
        "SELECT es.exam_id, AVG(es.score), COUNT(*), \
         COUNT(CASE WHEN es.user_id <> ",
    );
    sql.bind(Id(caller_user_id)).push(
        " THEN 1 END) \
         FROM exam_scores AS es \
         JOIN ",
    );
    push_active_course_members_subquery(scope, &mut sql, course_id, org.clone());
    sql.push(" AS am ON am.user_id = es.user_id WHERE es.container_id = ")
        .bind(Id(course_id))
        .push_bind(" AND es.org_id = ", org.clone());
    if let Some(eid) = exam_id {
        sql.push_bind(" AND es.exam_id = ", Id(eid));
    }
    sql.push(" GROUP BY es.exam_id");
    let mut stats = BTreeMap::new();
    for row in scope.fetch_all(sql).await? {
        let count = u32::try_from(row.int(2)?.max(0)).unwrap_or(u32::MAX);
        let others = row.int(3)?.max(0);
        if count > 0 {
            let avg = if others >= MIN_OTHER_SCORERS_FOR_AVERAGE {
                row.opt_float(1)?.filter(|v| v.is_finite())
            } else {
                None
            };
            stats.insert(row.id(0)?, (avg, count));
        }
    }
    Ok(stats)
}

async fn fetch_my_scores(
    scope: &mut Scope<'_>,
    course_id: &str,
    org: &Param,
    user_id: &str,
    exam_id: Option<&str>,
) -> Result<BTreeMap<String, MyExamScore>, StoreError> {
    let mut sql = scope.sql(
        "SELECT exam_id, score, note, updated_at \
         FROM exam_scores WHERE container_id = ",
    );
    sql.bind(Id(course_id))
        .push_bind(" AND org_id = ", org.clone())
        .push_bind(" AND user_id = ", Id(user_id));
    if let Some(eid) = exam_id {
        sql.push_bind(" AND exam_id = ", Id(eid));
    }
    let mut scores = BTreeMap::new();
    for row in scope.fetch_all(sql).await? {
        scores.insert(
            row.id(0)?,
            MyExamScore {
                score: row.float(1)?,
                note: row.text(2)?,
                updated_at: row.int(3)?,
            },
        );
    }
    Ok(scores)
}

fn exam_from_row(row: &Row) -> Result<CourseExam, StoreError> {
    Ok(CourseExam {
        id: row.id(0)?,
        container_id: row.id(1)?,
        title: row.text(2)?,
        exam_date: row.opt_text(3)?,
        max_score: row.float(4)?,
        description: row.text(5)?,
        created_at: row.int(6)?,
        updated_at: row.int(7)?,
        my_score: None,
        average_score: None,
        score_count: 0,
    })
}

/// Removes `course_exams` and `exam_scores` rows of courses that are gone for good:
/// neither live in `containers` nor awaiting restore in `deleted_containers`.
pub(crate) async fn forget_gone_containers(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    let del_exams = [
        "DELETE FROM course_exams WHERE NOT EXISTS \
         (SELECT 1 FROM containers AS c WHERE c.id = course_exams.container_id",
        scope.same_organization(&[("c", "course_exams")]).as_str(),
        ") AND NOT EXISTS \
         (SELECT 1 FROM deleted_containers AS d WHERE d.id = course_exams.container_id",
        scope.same_organization(&[("d", "course_exams")]).as_str(),
        ")",
    ]
    .concat();
    let mut sql = scope.sql(&del_exams);
    sql.changing_no_access();
    scope.execute(sql).await?;

    let del_scores = [
        "DELETE FROM exam_scores WHERE NOT EXISTS \
         (SELECT 1 FROM course_exams AS e WHERE e.id = exam_scores.exam_id",
        scope.same_organization(&[("e", "exam_scores")]).as_str(),
        ")",
    ]
    .concat();
    let mut sql = scope.sql(&del_scores);
    sql.changing_no_access();
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes every `exam_scores` row of `user_id` for courses belonging to `workspace_id`
/// (both live and in the trash).
pub(crate) async fn forget_user_in_workspace(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM exam_scores WHERE user_id = ");
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

/// Deletes `exam_scores` rows of `user_id` in `workspace_id` for courses where `user_id`
/// no longer holds any direct or inherited membership (called when an org admin ends
/// their joining).
pub(crate) async fn forget_user_in_workspace_unjoined(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    let inherited = inherited_members_source(scope);
    let mut sql = scope.sql("DELETE FROM exam_scores WHERE user_id = ");
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
             WHERE cm.container_id = exam_scores.container_id \
               AND cm.user_id = exam_scores.user_id",
        )
        .push(&scope.same_organization(&[("cm", "exam_scores")]))
        .push(") AND NOT EXISTS (SELECT 1 FROM ")
        .push(&inherited)
        .push(
            " AS im \
             WHERE im.container_id = exam_scores.container_id \
               AND im.user_id = exam_scores.user_id",
        )
        .push_bind(" AND im.org_id = ", org.clone())
        .push(&scope.same_organization(&[("im", "exam_scores")]))
        .push(")");
    if scope.dialect() == Dialect::Postgres {
        sql.push_bind(" AND org_id = ", org);
    }
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes every `exam_scores` row of `user_id` belonging to the scope's organization,
/// without touching scores stored in other organizations or the user's personal space.
pub(crate) async fn forget_user_in_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    let mut sql = scope.sql("DELETE FROM exam_scores WHERE user_id = ");
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
