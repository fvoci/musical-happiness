//! Login sessions.
//!
//! A session is found by the digest of its token; the token itself is never stored, so
//! that whoever reads the database (or a backup of it) cannot sign in with what is
//! there. A session ends when it has not been used for longer than the idle limit, and
//! at its `expires_at` whatever its use. All times are milliseconds since the Unix
//! epoch; the callers pass the current time and the limits.

use fvoci_model::{LoginOrigin, Session};

use crate::Store;
use crate::db::{Dialect, Id, OptId, Param, Row, Scope, Sql};
use crate::error::StoreError;

const SESSION_COLUMNS: &str = "id, user_id, created_at, last_used_at, expires_at, user_agent, login_provider_id, \
     login_org_id, login_issuer";

impl Store {
    /// Stores a new session under the digest of its token, provided the password hash of
    /// its user is still `seen_password_hash`: the hash the password was checked
    /// against. Returns whether the session was stored. Its `last_used_at` is stored as
    /// its `created_at`.
    ///
    /// Checking a password takes a while. If the password is set anew in that time, the
    /// reset ends the user's sessions, and a session opened with the old password must
    /// not appear after it. The check and the insertion are one step, and a reset that
    /// is under way is waited for.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the session's identifier is not one, and
    /// [`StoreError::Database`] if a session with the same identifier or the same token
    /// digest exists. A user that does not exist, also one named by text that is no
    /// identifier, has no password hash to be the seen one: the answer is `false`.
    pub async fn create_session(
        &self,
        session: &Session,
        token_digest: &str,
        seen_password_hash: &str,
    ) -> Result<bool, StoreError> {
        self.transaction(async |scope| {
            if !hold_unchanged_user(scope, &session.user_id, seen_password_hash).await? {
                return Ok(false);
            }
            insert_session(scope, session, token_digest).await?;
            Ok(true)
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

    /// Counts the sessions that are live at `now`, as [`Store::get_session`] would find
    /// them: not past their `expires_at`, used after `idle_since`, and of a user who is
    /// not deactivated.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn count_live_sessions(&self, now: i64, idle_since: i64) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("SELECT COUNT(*) FROM sessions WHERE expires_at > ");
            sql.bind(now)
                .push_bind(" AND last_used_at > ", idle_since)
                .push(
                    " AND EXISTS (SELECT 1 FROM users WHERE users.id = sessions.user_id \
                     AND users.delete_at = 0)",
                );
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }

    /// Records that a session was in use at `now`. A session that is gone is left gone,
    /// and the record only ever moves forward: a use that is older than what is recorded
    /// changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn touch_session(&self, session_id: &str, now: i64) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE sessions SET last_used_at = ");
            sql.bind(now)
                .push_bind(" WHERE id = ", Id(session_id))
                .push_bind(" AND last_used_at < ", now);
            scope.execute(sql).await?;
            Ok(())
        })
        .await
    }

    /// Sets when a session was last in use, to any time: for tests that need a session
    /// of an age.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    #[cfg(feature = "test-util")]
    pub async fn set_session_last_used(
        &self,
        session_id: &str,
        last_used_at: i64,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE sessions SET last_used_at = ");
            sql.bind(last_used_at)
                .push_bind(" WHERE id = ", Id(session_id));
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
    /// used after `idle_since`. Returns the identifiers of the sessions that went, for
    /// whoever holds something open on their behalf.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn clean_up_sessions(
        &self,
        now: i64,
        idle_since: i64,
    ) -> Result<Vec<String>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = scope.sql("SELECT id FROM sessions WHERE expires_at <= ");
            sql.bind(now).push_bind(" OR last_used_at <= ", idle_since);
            let ended: Vec<String> = scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.id(0))
                .collect::<Result<_, _>>()?;
            let mut sql = scope.sql("DELETE FROM sessions WHERE expires_at <= ");
            sql.bind(now).push_bind(" OR last_used_at <= ", idle_since);
            scope.execute(sql).await?;
            Ok(ended)
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
        login: login_origin(row)?,
    })
}

/// The origin of a session row: all three columns or none of them (the schema holds
/// them so).
fn login_origin(row: &Row) -> Result<Option<LoginOrigin>, StoreError> {
    let provider_id = row.id(6)?;
    if provider_id.is_empty() {
        return Ok(None);
    }
    Ok(Some(LoginOrigin {
        provider_id,
        org_id: row.id(7)?,
        issuer: row.text(8)?,
    }))
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

/// Ends the sessions made through a provider of the organization `org_id` under
/// `issuer`, whoever's: for when that provider is removed or takes another issuer.
pub(crate) async fn end_sessions_of_origin(
    scope: &mut Scope<'_>,
    org_id: &str,
    issuer: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("DELETE FROM sessions WHERE login_org_id = ");
    sql.bind(Id(org_id))
        .push_bind(" AND login_issuer = ", issuer);
    scope.execute(sql).await?;
    Ok(())
}

/// Ends the sessions of a user made through a provider of `issuer`, in whichever
/// organization: for when the user unlinks their identity of that issuer.
pub(crate) async fn end_sessions_of_issuer(
    scope: &mut Scope<'_>,
    user_id: &str,
    issuer: &str,
) -> Result<(), StoreError> {
    let mut sql = delete_sessions_where_user(scope, user_id);
    sql.push_bind(" AND login_issuer = ", issuer);
    scope.execute(sql).await?;
    Ok(())
}

/// Holds the row of a user who is not deleted and whose password hash is still
/// `seen_password_hash`, till the end of the transaction; false for anybody else. A
/// reset of the password waits for this, or this for it. On SQLite a transaction that
/// writes holds the whole database.
pub(crate) async fn hold_unchanged_user(
    scope: &mut Scope<'_>,
    user_id: &str,
    seen_password_hash: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT password_hash FROM users WHERE delete_at = 0 AND id = ");
    sql.bind(Id(user_id));
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR SHARE");
    }
    Ok(match scope.fetch_optional(sql).await? {
        Some(row) => row.text(0)? == seen_password_hash,
        None => false,
    })
}

/// Stores a session under the digest of its token.
pub(crate) async fn insert_session(
    scope: &mut Scope<'_>,
    session: &Session,
    token_digest: &str,
) -> Result<(), StoreError> {
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
        .push_bind(", ", OptId(session.login_provider_id().unwrap_or_default()))
        .push_bind(
            ", ",
            OptId(
                session
                    .login
                    .as_ref()
                    .map_or("", |login| login.org_id.as_str()),
            ),
        )
        .push_bind(
            ", ",
            Param::OptText(session.login.as_ref().map(|login| login.issuer.clone())),
        )
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
