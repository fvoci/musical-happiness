//! Login sessions.
//!
//! A session is found by the digest of its token; the token itself is never stored, so
//! that whoever reads the database (or a backup of it) cannot sign in with what is
//! there. A session ends when it has not been used for longer than the idle limit, and
//! at its `expires_at` whatever its use. All times are milliseconds since the Unix
//! epoch; the callers pass the current time and the limits.

use fvoci_model::Session;

use crate::Store;
use crate::db::{Id, Row, Scope, Sql};
use crate::error::StoreError;

const SESSION_COLUMNS: &str = "id, user_id, created_at, last_used_at, expires_at, user_agent";

impl Store {
    /// Stores a new session under the digest of its token. Its `last_used_at` is stored
    /// as its `created_at`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the session's or the user's identifier is
    /// not one, and [`StoreError::Database`] if a session with the same identifier or
    /// the same token digest exists.
    pub async fn create_session(
        &self,
        session: &Session,
        token_digest: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "INSERT INTO sessions (token_hash, {SESSION_COLUMNS}) VALUES ("
            ));
            sql.bind(token_digest)
                .push_bind(", ", Id(&session.id))
                .push_bind(", ", Id(&session.user_id))
                .push_bind(", ", session.created_at)
                .push_bind(", ", session.created_at)
                .push_bind(", ", session.expires_at)
                .push_bind(", ", &session.user_agent)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Returns the session with the given token digest, if it is still valid at `now`:
    /// not past its `expires_at`, used after `idle_since`, and of a user who is not
    /// deactivated.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoRows`] if there is no such session or it has ended.
    pub async fn get_session(
        &self,
        token_digest: &str,
        now: i64,
        idle_since: i64,
    ) -> Result<Session, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {SESSION_COLUMNS} FROM sessions WHERE token_hash = "
            ));
            sql.bind(token_digest)
                .push_bind(" AND expires_at > ", now)
                .push_bind(" AND last_used_at > ", idle_since)
                .push(
                    " AND EXISTS (SELECT 1 FROM users WHERE users.id = sessions.user_id \
                     AND users.delete_at = 0)",
                );
            let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
            session_from_row(&row)
        })
        .await
    }

    /// Records that a session was in use at `now`. A session that is gone is left gone.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn touch_session(&self, session_id: &str, now: i64) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE sessions SET last_used_at = ");
            sql.bind(now).push_bind(" WHERE id = ", Id(session_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Deletes the session with the given identifier. Deleting one that does not exist
    /// succeeds.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn delete_session(&self, session_id: &str) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("DELETE FROM sessions WHERE id = ");
            sql.bind(Id(session_id));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Deletes every session that has ended at `now`: past its `expires_at`, or not
    /// used after `idle_since`. Returns how many went.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn clean_up_sessions(&self, now: i64, idle_since: i64) -> Result<u64, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("DELETE FROM sessions WHERE expires_at <= ");
            sql.bind(now).push_bind(" OR last_used_at <= ", idle_since);
            scope.execute(sql).await
        })
        .await
    }
}

fn session_from_row(row: &Row) -> Result<Session, StoreError> {
    Ok(Session {
        id: row.id(0)?,
        user_id: row.id(1)?,
        created_at: row.int(2)?,
        last_used_at: row.int(3)?,
        expires_at: row.int(4)?,
        user_agent: row.text(5)?,
    })
}

/// Starts the deletion of a user's sessions; the caller may narrow it further.
fn delete_sessions_where_user(scope: &Scope<'_>, user_id: &str) -> Sql {
    let mut sql = scope.sql("DELETE FROM sessions WHERE user_id = ");
    sql.bind(Id(user_id));
    sql
}

/// Deletes every session of a user.
pub(crate) async fn delete_sessions_of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let sql = delete_sessions_where_user(scope, user_id);
    scope.execute(sql).await?;
    Ok(())
}

/// Puts a session under another token digest: its old token no longer finds it.
pub(crate) async fn replace_token_digest(
    scope: &mut Scope<'_>,
    session_id: &str,
    token_digest: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE sessions SET token_hash = ");
    sql.bind(token_digest)
        .push_bind(" WHERE id = ", Id(session_id));
    scope.execute(sql).await?;
    Ok(())
}

/// Deletes every session of a user but one.
pub(crate) async fn delete_other_sessions_of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
    kept_session_id: &str,
) -> Result<(), StoreError> {
    let mut sql = delete_sessions_where_user(scope, user_id);
    sql.push_bind(" AND id <> ", Id(kept_session_id));
    scope.execute(sql).await?;
    Ok(())
}
