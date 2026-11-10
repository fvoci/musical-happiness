//! Users and their preferences.
//!
//! A standalone server has one team and every user belongs to it, so the operations that
//! the original addresses to "the users of a team" take no team here. The original's
//! standalone store ignores the team, the guest and the visibility flags it is given
//! (`server/services/store/sqlstore/user.go:105`, `:217` and `:226`).

use fvoci_model::{User, UserPreferencesPatch};
use serde::{Deserialize, Serialize};

use crate::Store;
use crate::db::{Dialect, Id, Row, Scope, Sql, now_millis};
use crate::error::StoreError;
use crate::sessions;

/// The columns of a user, in the order [`user_from_row`] reads them.
const USER_COLUMNS: &str =
    "id, username, email, password_hash, create_at, update_at, delete_at, is_admin";

/// How a new user gets in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Admission {
    /// Whoever calls has checked that the user may register.
    Checked,
    /// As the first user only: the instance must not have one yet.
    FirstOnly,
}

/// What a user is to the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceRole {
    /// An administrator of the instance.
    Administrator,
    /// A user like any other.
    User,
}

/// What became of a change to who is an administrator of the instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminChange {
    /// The user is, or is no longer, an administrator: the user as stored now.
    Done(Box<User>),
    /// There is no such user, or the user is deactivated; nothing changed.
    NoSuchUser,
    /// The user is the only administrator and the change would have ended that;
    /// nothing changed.
    LastAdmin,
}

/// The category under which this server files its user preferences.
///
/// The original files them under its own product name. The rename is deliberate: a
/// database written by the original shows no user preferences here, and a database
/// written here shows none there.
const PREFERENCES_CATEGORY: &str = "fvoci";

/// The most users a search returns.
const SEARCH_LIMIT: i64 = 10;

/// One stored preference of a user.
///
/// The JSON form has the member names of the original's preference record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preference {
    /// The identifier of the user the preference belongs to.
    pub user_id: String,
    /// The category the preference is filed under.
    pub category: String,
    /// The name of the preference.
    pub name: String,
    /// The value of the preference.
    pub value: String,
}

impl Store {
    /// Returns the number of users that are not deleted.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_registered_user_count(&self) -> Result<i64, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT COUNT(*) FROM users WHERE delete_at = 0");
            match scope.fetch_optional(sql).await? {
                Some(row) => row.int(0),
                None => Ok(0),
            }
        })
        .await
    }

    /// Returns the user with the given identifier, unless deleted.
    ///
    /// Only the members a standalone server stores are filled in: the nickname, the
    /// names, the roles and the permissions are empty, and the user is neither a bot
    /// nor a guest.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such user.
    pub async fn get_user_by_id(&self, user_id: &str) -> Result<User, StoreError> {
        self.with_conn(async |scope| get_user_by_id(scope, user_id).await)
            .await
    }

    /// Returns the users with the given identifiers, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if none of the users exists and
    /// [`StoreError::NotAllFound`] unless exactly one user is found per identifier.
    pub async fn get_users_list(&self, user_ids: &[String]) -> Result<Vec<User>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = select_users(scope);
            sql.push(" AND id IN (")
                .bind_list(user_ids.iter().map(|id| Id(id)))
                .push(")");
            let users = fetch_users(scope, sql).await?;
            if users.is_empty() {
                return Err(user_not_found());
            }
            // Preserved quirk: the counts are compared, so naming a user twice fails even
            // though that user exists (`server/services/store/sqlstore/user.go:111`).
            if users.len() != user_ids.len() {
                return Err(StoreError::NotAllFound {
                    entity: "user",
                    ids: user_ids.to_vec(),
                });
            }
            Ok(users)
        })
        .await
    }

    /// Returns the user with the given email address, unless deleted.
    ///
    /// The address is compared without regard to case, as the unique index on it is:
    /// all letters on PostgreSQL, the ASCII letters on SQLite.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such user.
    pub async fn get_user_by_email(&self, email: &str) -> Result<User, StoreError> {
        self.with_conn(async |scope| find_user(scope, Lookup::Email(email)).await)
            .await
    }

    /// Returns the user with the given name, unless deleted.
    ///
    /// The name is compared without regard to case, as the unique index on it is: all
    /// letters on PostgreSQL, the ASCII letters on SQLite.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such user.
    pub async fn get_user_by_username(&self, username: &str) -> Result<User, StoreError> {
        self.with_conn(async |scope| find_user(scope, Lookup::Username(username)).await)
            .await
    }

    /// Stores a new user. The first user of the instance becomes an administrator, and
    /// nobody after that does by registering.
    ///
    /// `user` is updated to what was stored: the current time as creation and
    /// modification time, not deleted, and whether an administrator, whatever the
    /// caller put there. Of its other members, only the identifier, the name, the
    /// email address and the password hash are stored.
    ///
    /// Who is first is decided with the insertion, in one transaction: of two users who
    /// register at once into an empty instance, one is the administrator.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Conflict`] if another user, deleted or not, has the name or
    /// the email address in any letter case, and [`StoreError::InvalidId`] if the
    /// identifier is not one.
    pub async fn create_user(&self, user: &mut User) -> Result<(), StoreError> {
        self.transaction(async |scope| create_user(scope, user, Admission::Checked).await)
            .await
    }

    /// Stores a new user as [`Store::create_user`] does, but only as the first user of
    /// the instance: for a registration that presents no signup token.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFirstUser`] if the instance has had its first user, and
    /// otherwise the errors of [`Store::create_user`]. Nothing is stored in either case.
    pub async fn create_first_user(&self, user: &mut User) -> Result<(), StoreError> {
        self.transaction(async |scope| create_user(scope, user, Admission::FirstOnly).await)
            .await
    }

    /// Whether the instance has had its first user.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn is_bootstrapped(&self) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql("SELECT bootstrapped FROM instance WHERE id = 1");
            match scope.fetch_optional(sql).await? {
                Some(row) => row.bool(0),
                None => Ok(false),
            }
        })
        .await
    }

    /// Makes a user an administrator of the instance, or ends that, unless it would
    /// leave the instance without an administrator. Deactivated users do not count as
    /// administrators.
    ///
    /// The check and the write are one step: of two administrators who give up the role
    /// at once, one is refused.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn set_user_admin(
        &self,
        user_id: &str,
        role: InstanceRole,
    ) -> Result<AdminChange, StoreError> {
        let is_admin = role == InstanceRole::Administrator;
        self.transaction(async |scope| {
            // One change to who is an administrator at a time: the instance's row is
            // held. On SQLite a transaction that writes holds the whole database.
            let mut sql = scope.sql("SELECT id FROM instance WHERE id = 1");
            if scope.dialect() == Dialect::Postgres {
                sql.push(" FOR UPDATE");
            }
            scope.fetch_optional(sql).await?;

            let mut sql = select_users(scope);
            sql.push_bind(" AND id = ", Id(user_id));
            let Some(current) = fetch_users(scope, sql).await?.into_iter().next() else {
                return Ok(AdminChange::NoSuchUser);
            };
            if current.is_admin && !is_admin {
                let mut sql = scope.sql("SELECT 1 FROM users WHERE delete_at = 0 AND is_admin = ");
                sql.bind(true)
                    .push_bind(" AND id <> ", Id(user_id))
                    .push(" LIMIT 1");
                if scope.fetch_optional(sql).await?.is_none() {
                    return Ok(AdminChange::LastAdmin);
                }
            }
            if current.is_admin != is_admin {
                let mut sql = scope.sql("UPDATE users SET is_admin = ");
                sql.bind(is_admin)
                    .push_bind(", update_at = ", now_millis())
                    .push_bind(" WHERE id = ", Id(user_id));
                scope.execute(sql).await?;
            }
            let user = get_user_by_id(scope, user_id).await?;
            Ok(AdminChange::Done(Box::new(user)))
        })
        .await
    }

    /// Sets the password hash of the user with the given name, in any letter case, and
    /// deletes their sessions, so that nobody stays signed in with the old password. For
    /// a password an administrator sets. Returns the identifier of that user.
    ///
    /// Either both happen or neither does.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if no user has that name.
    pub async fn reset_user_password(
        &self,
        username: &str,
        password_hash: &str,
    ) -> Result<String, StoreError> {
        self.transaction(async |scope| {
            let lookup = Lookup::Username(username);
            update_password(scope, lookup, password_hash).await?;
            let mut sql = scope.sql("SELECT id FROM users WHERE ");
            lookup.push_condition(&mut sql);
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(|| StoreError::UserNotFound(username.to_owned()))?;
            let user_id = row.id(0)?;
            sessions::delete_sessions_of_user(scope, &user_id).await?;
            Ok(user_id)
        })
        .await
    }

    /// Sets the password hash of the user with the given identifier and deletes their
    /// sessions, except the one named: the session the password was changed through
    /// stays, every other device has to sign in again with the new password.
    ///
    /// Either both happen or neither does.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if there is no such user.
    pub async fn change_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        kept_session_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            update_password(scope, Lookup::Id(user_id), password_hash).await?;
            sessions::delete_other_sessions_of_user(scope, user_id, kept_session_id).await
        })
        .await
    }

    /// Replaces a user's password hash by one made from the same password with other
    /// parameters, provided the stored hash is still `old_hash`. Sessions stay: the
    /// password did not change. Returns whether the hash was replaced; it is not if the
    /// password was changed in the meantime.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn replace_password_hash(
        &self,
        user_id: &str,
        old_hash: &str,
        new_hash: &str,
    ) -> Result<bool, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql("UPDATE users SET ");
            sql.push_bind("password_hash = ", new_hash)
                .push_bind(" WHERE id = ", Id(user_id))
                .push_bind(" AND password_hash = ", old_hash);
            Ok(scope.execute(sql).await? > 0)
        })
        .await
    }

    /// Returns every user that is not deleted, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_users_by_team(&self) -> Result<Vec<User>, StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: asked for the users of a team, the original returns all
            // users whatever the team (`server/services/store/sqlstore/user.go:217-218`).
            let sql = select_users(scope);
            fetch_users(scope, sql).await
        })
        .await
    }

    /// Returns up to ten users whose name contains `search_query`, in no particular
    /// order.
    ///
    /// The match is the database's `LIKE`: SQLite ignores the case of ASCII letters and
    /// PostgreSQL does not, and `%` and `_` in the query are wildcards.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_users_by_team(&self, search_query: &str) -> Result<Vec<User>, StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: one `LIKE` for both databases, so the case sensitivity is
            // whatever the database's is, and the query's own wildcards are not escaped
            // (`server/services/store/sqlstore/user.go:227`).
            let mut sql = select_users(scope);
            sql.push_bind(" AND username LIKE ", format!("%{search_query}%"))
                .push_bind(" LIMIT ", SEARCH_LIMIT);
            fetch_users(scope, sql).await
        })
        .await
    }

    /// Sets and removes preferences of a user and returns the resulting preferences.
    ///
    /// The result lists the preferences the patch did not set, followed by the ones it
    /// set in the order of their names. A name that is both set and removed ends up
    /// removed. Either all changes are stored or none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn patch_user_preferences(
        &self,
        user_id: &str,
        patch: &UserPreferencesPatch,
    ) -> Result<Vec<Preference>, StoreError> {
        self.transaction(async |scope| patch_user_preferences(scope, user_id, patch).await)
            .await
    }

    /// Returns the preferences of a user, in no particular order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_user_preferences(&self, user_id: &str) -> Result<Vec<Preference>, StoreError> {
        self.with_conn(async |scope| get_user_preferences(scope, user_id).await)
            .await
    }
}

/// Starts the query for the users that are not deleted; conditions follow with `AND`.
fn select_users(scope: &Scope<'_>) -> Sql {
    scope.sql(&format!(
        "SELECT {USER_COLUMNS} FROM users WHERE delete_at = 0"
    ))
}

async fn fetch_users(scope: &mut Scope<'_>, sql: Sql) -> Result<Vec<User>, StoreError> {
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(user_from_row)
        .collect()
}

fn user_from_row(row: &Row) -> Result<User, StoreError> {
    Ok(User {
        id: row.id(0)?,
        username: row.text(1)?,
        email: row.text(2)?,
        password_hash: row.text(3)?,
        create_at: row.int(4)?,
        update_at: row.int(5)?,
        delete_at: row.int(6)?,
        is_admin: row.bool(7)?,
        ..User::default()
    })
}

/// The error for a user that a read did not find.
///
/// Preserved quirk: it does not say which user is missing; clients see `{user} not found`
/// (`server/services/store/sqlstore/user.go:54` and `:95`).
fn user_not_found() -> StoreError {
    StoreError::not_found("user")
}

/// What a user is looked up by.
#[derive(Debug, Clone, Copy)]
enum Lookup<'a> {
    Id(&'a str),
    Username(&'a str),
    Email(&'a str),
}

impl Lookup<'_> {
    /// Appends the condition that selects the user.
    fn push_condition(self, sql: &mut Sql) {
        match self {
            Self::Id(id) => sql.push_bind("id = ", Id(id)),
            Self::Username(name) => sql.push_bind("lower(username) = lower(", name).push(")"),
            Self::Email(email) => sql.push_bind("lower(email) = lower(", email).push(")"),
        };
    }
}

/// Returns the user that is not deleted and that `lookup` selects.
async fn find_user(scope: &mut Scope<'_>, lookup: Lookup<'_>) -> Result<User, StoreError> {
    let mut sql = select_users(scope);
    sql.push(" AND ");
    lookup.push_condition(&mut sql);
    fetch_users(scope, sql)
        .await?
        .into_iter()
        .next()
        .ok_or_else(user_not_found)
}

/// Whether there is a user row with the identifier, deactivated or not: whether a row
/// that refers to the user can be stored.
pub(crate) async fn user_row_exists(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<bool, StoreError> {
    let mut sql = scope.sql("SELECT 1 FROM users WHERE id = ");
    sql.bind(Id(user_id));
    Ok(scope.fetch_optional(sql).await?.is_some())
}

pub(crate) async fn get_user_by_id(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<User, StoreError> {
    find_user(scope, Lookup::Id(user_id)).await
}

async fn create_user(
    scope: &mut Scope<'_>,
    user: &mut User,
    admission: Admission,
) -> Result<(), StoreError> {
    // Whoever turns the flag on is the first user. A second transaction that tries at
    // the same moment waits for this one and then finds the flag on.
    let mut sql = scope.sql("UPDATE instance SET bootstrapped = ");
    sql.bind(true)
        .push_bind(" WHERE id = 1 AND bootstrapped = ", false);
    let is_first = scope.execute(sql).await? > 0;
    if !is_first && admission == Admission::FirstOnly {
        return Err(StoreError::NotFirstUser);
    }

    let now = now_millis();
    user.create_at = now;
    user.update_at = now;
    user.delete_at = 0;
    user.is_admin = is_first;

    let mut sql = scope.sql(&format!("INSERT INTO users ({USER_COLUMNS}) VALUES ("));
    sql.bind(Id(&user.id))
        .push_bind(", ", &user.username)
        .push_bind(", ", &user.email)
        .push_bind(", ", &user.password_hash)
        .push_bind(", ", user.create_at)
        .push_bind(", ", user.update_at)
        .push_bind(", ", user.delete_at)
        .push_bind(", ", user.is_admin)
        .push(")");
    match scope.execute(sql).await {
        Ok(_) => Ok(()),
        Err(StoreError::Database(error)) => Err(unique_violation(error)),
        Err(error) => Err(error),
    }
}

/// Reads the violation of one of the two unique indexes of the users as a conflict.
fn unique_violation(error: sqlx::Error) -> StoreError {
    let Some(database_error) = error.as_database_error() else {
        return StoreError::Database(error);
    };
    if !database_error.is_unique_violation() {
        return StoreError::Database(error);
    }
    // PostgreSQL names the index; SQLite names the indexed expression.
    let message = database_error.message();
    let of_index = database_error.constraint().unwrap_or(message);
    if of_index.contains("email") {
        StoreError::Conflict("the email address")
    } else if of_index.contains("username") {
        StoreError::Conflict("the user name")
    } else {
        StoreError::Database(error)
    }
}

/// Sets the password hash of the user that `lookup` selects, deleted or not.
async fn update_password(
    scope: &mut Scope<'_>,
    lookup: Lookup<'_>,
    password_hash: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("UPDATE users SET ");
    sql.push_bind("password_hash = ", password_hash)
        .push_bind(", update_at = ", now_millis())
        .push(" WHERE ");
    lookup.push_condition(&mut sql);
    if scope.execute(sql).await? == 0 {
        let (Lookup::Id(user) | Lookup::Username(user) | Lookup::Email(user)) = lookup;
        return Err(StoreError::UserNotFound(user.to_owned()));
    }
    Ok(())
}

pub(crate) async fn get_user_preferences(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<Vec<Preference>, StoreError> {
    let mut sql =
        scope.sql("SELECT user_id, category, name, value FROM preferences WHERE user_id = ");
    sql.bind(Id(user_id))
        .push_bind(" AND category = ", PREFERENCES_CATEGORY);
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(Preference {
                user_id: row.id(0)?,
                category: row.text(1)?,
                name: row.text(2)?,
                value: row.text(3)?,
            })
        })
        .collect()
}

pub(crate) async fn patch_user_preferences(
    scope: &mut Scope<'_>,
    user_id: &str,
    patch: &UserPreferencesPatch,
) -> Result<Vec<Preference>, StoreError> {
    let mut preferences = get_user_preferences(scope, user_id).await?;

    for (name, value) in &patch.updated_fields {
        let mut sql =
            scope.sql("INSERT INTO preferences (user_id, category, name, value) VALUES (");
        sql.bind(Id(user_id))
            .push_bind(", ", PREFERENCES_CATEGORY)
            .push_bind(", ", name)
            .push_bind(", ", value)
            .push(") ON CONFLICT (user_id, category, name) DO UPDATE SET value = EXCLUDED.value");
        scope.execute(sql).await?;

        preferences.retain(|existing| existing.name != *name);
        preferences.push(Preference {
            user_id: user_id.to_owned(),
            category: PREFERENCES_CATEGORY.to_owned(),
            name: name.clone(),
            value: value.clone(),
        });
    }

    for name in &patch.deleted_fields {
        let mut sql = scope.sql("DELETE FROM preferences WHERE user_id = ");
        sql.bind(Id(user_id))
            .push_bind(" AND category = ", PREFERENCES_CATEGORY)
            .push_bind(" AND name = ", name);
        scope.execute(sql).await?;

        preferences.retain(|existing| existing.name != *name);
    }

    Ok(preferences)
}
