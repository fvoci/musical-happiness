//! Login sessions.
//!
//! A session lives as long as it keeps being refreshed: reads, the active-user count and
//! the cleanup all compare its last refresh time with a limit. The limits are given in
//! seconds, as the server's configuration has them, while the stored times are
//! milliseconds since the Unix epoch.

use fvoci_model::Session;

use crate::Store;
use crate::db::{Id, Param, Scope, now_millis};
use crate::error::StoreError;

/// Returns the time `seconds` before now, in milliseconds since the Unix epoch.
///
/// A negative number of seconds gives a time in the future, as in the original.
fn millis_before_now(seconds: i64) -> i64 {
    now_millis().saturating_sub(seconds.saturating_mul(1000))
}

impl Store {
    /// Returns the session with the given token, if it was refreshed less than
    /// `expire_time_seconds` seconds ago.
    ///
    /// The returned session has `create_at` and `update_at` zero, whatever is stored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NoRows`] if there is no such session or it has expired.
    pub async fn get_session(
        &self,
        token: &str,
        expire_time_seconds: i64,
    ) -> Result<Session, StoreError> {
        self.with_conn(async |scope| get_session(scope, token, expire_time_seconds).await)
            .await
    }

    /// Stores a new session with the current time as creation and refresh time.
    ///
    /// The times of `session` are not used.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a session with the same identifier or the
    /// same token exists.
    pub async fn create_session(&self, session: &Session) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let now = now_millis();
            let mut sql = scope.sql("INSERT INTO sessions (id, token, user_id, auth_service, props, create_at, update_at) \
                 VALUES (");
            sql.bind(Id(&session.id))
                .push_bind(", ", &session.token)
                .push_bind(", ", Id(&session.user_id))
                .push_bind(", ", &session.auth_service)
                .push_bind(", ", Param::json(&session.props)?)
                .push_bind(", ", now)
                .push_bind(", ", now)
                .push(")");
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Marks the session that has `session`'s token as refreshed now. Succeeds without
    /// effect if there is no such session.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn refresh_session(&self, session: &Session) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE sessions SET ");
            sql.push_bind("update_at = ", now_millis())
                .push_bind(" WHERE token = ", &session.token);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Deletes the session with the given identifier. Succeeds if there is none.
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

    /// Deletes every session last refreshed more than `expire_time_seconds` seconds ago.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn clean_up_sessions(&self, expire_time_seconds: i64) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: the limit is in seconds although every stored time is in
            // milliseconds, and a session exactly at the limit is neither deleted here
            // nor returned by `get_session`
            // (`server/services/store/sqlstore/session.go:105-111`).
            let mut sql = scope.sql("DELETE FROM sessions WHERE update_at < ");
            sql.bind(millis_before_now(expire_time_seconds));
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }
}

/// Deletes every session of a user.
pub(crate) async fn delete_sessions_of_user(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM sessions WHERE user_id = ");
    sql.bind(Id(user_id));
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn get_session(
    scope: &mut Scope<'_>,
    token: &str,
    expire_time_seconds: i64,
) -> Result<Session, StoreError> {
    // Preserved quirk: the times are not selected, so they read as zero
    // (`server/services/store/sqlstore/session.go:30-34`). The original's authentication
    // code compares that zero with its refresh limit and so refreshes the session on
    // every request.
    let mut sql =
        scope.sql("SELECT id, token, user_id, auth_service, props FROM sessions WHERE token = ");
    sql.bind(token)
        .push_bind(" AND update_at > ", millis_before_now(expire_time_seconds));
    // Preserved quirk: an unknown or expired token is reported with the database layer's
    // "no rows" text, which clients see in the "unauthorized" response
    // (`server/services/store/sqlstore/session.go:40-43`).
    let row = scope.fetch_optional(sql).await?.ok_or(StoreError::NoRows)?;
    Ok(Session {
        id: row.id(0)?,
        token: row.text(1)?,
        user_id: row.id(2)?,
        auth_service: row.text(3)?,
        props: row.json_map(4, "props")?,
        create_at: 0,
        update_at: 0,
    })
}
