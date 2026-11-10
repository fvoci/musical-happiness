//! Users and their preferences.
//!
//! A standalone server has one team and every user belongs to it, so the operations that
//! the original addresses to "the users of a team" take no team here. The original's
//! standalone store ignores the team, the guest and the visibility flags it is given
//! (`server/services/store/sqlstore/user.go:105`, `:217` and `:226`).

use fvoci_model::{User, UserPreferencesPatch};
use serde::{Deserialize, Serialize};

use crate::Store;
use crate::db::{Row, Scope, Sql, now_millis};
use crate::error::StoreError;

/// The columns of a user, in the order [`user_from_row`] reads them.
const USER_COLUMNS: &str = "id, username, email, password, mfa_secret, auth_service, auth_data, \
                            create_at, update_at, delete_at";

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
            let sql = scope.sql(&format!(
                "SELECT COUNT(*) FROM {} WHERE delete_at = 0",
                scope.table("users")
            ));
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
            sql.push(" AND id IN (").bind_list(user_ids).push(")");
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

    /// Returns a user with the given email address, unless deleted.
    ///
    /// The address is compared exactly, including letter case, on both databases.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such user.
    pub async fn get_user_by_email(&self, email: &str) -> Result<User, StoreError> {
        self.with_conn(async |scope| find_user(scope, "email", email).await)
            .await
    }

    /// Returns a user with the given name, unless deleted.
    ///
    /// The name is compared exactly, including letter case, on both databases.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such user.
    pub async fn get_user_by_username(&self, username: &str) -> Result<User, StoreError> {
        self.with_conn(async |scope| find_user(scope, "username", username).await)
            .await
    }

    /// Stores a new user.
    ///
    /// `user` is updated to what was stored: the current time as creation and
    /// modification time, and not deleted. Of its members, only the identifier, the
    /// name, the email address and the authentication members are stored. Nothing but
    /// the identifier has to be unique.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a user with the same identifier exists.
    pub async fn create_user(&self, user: &mut User) -> Result<(), StoreError> {
        self.with_conn(async |scope| create_user(scope, user).await)
            .await
    }

    /// Changes the name and the email address of the user with `user`'s identifier.
    ///
    /// `user` gets the current time as modification time. No other member is written,
    /// and a deleted user is updated like any other.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if there is no such user.
    pub async fn update_user(&self, user: &mut User) -> Result<(), StoreError> {
        self.with_conn(async |scope| {
            // Preserved quirk: only the name and the email address are written, so this
            // can neither delete nor restore a user, and it does not skip deleted users
            // (`server/services/store/sqlstore/user.go:144-148`).
            user.update_at = now_millis();
            let mut sql = scope.sql(&format!("UPDATE {} SET ", scope.table("users")));
            sql.push_bind("username = ", &user.username)
                .push_bind(", email = ", &user.email)
                .push_bind(", update_at = ", user.update_at)
                .push_bind(" WHERE id = ", &user.id);
            require_user(scope.execute(sql).await?, &user.id)
        })
        .await
    }

    /// Sets the password hash of every user with the given name.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if no user has that name.
    pub async fn update_user_password(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| update_password(scope, "username", username, password).await)
            .await
    }

    /// Sets the password hash of the user with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::UserNotFound`] if there is no such user.
    pub async fn update_user_password_by_id(
        &self,
        user_id: &str,
        password: &str,
    ) -> Result<(), StoreError> {
        self.with_conn(async |scope| update_password(scope, "id", user_id, password).await)
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
        "SELECT {USER_COLUMNS} FROM {} WHERE delete_at = 0",
        scope.table("users")
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
        id: row.text(0)?,
        username: row.text(1)?,
        email: row.text(2)?,
        password: row.text(3)?,
        mfa_secret: row.text(4)?,
        auth_service: row.text(5)?,
        auth_data: row.text(6)?,
        create_at: row.int(7)?,
        update_at: row.int(8)?,
        delete_at: row.int(9)?,
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

/// Returns the first user that is not deleted and whose `column` equals `value`.
async fn find_user(scope: &mut Scope<'_>, column: &str, value: &str) -> Result<User, StoreError> {
    let mut sql = select_users(scope);
    sql.push(" AND ").push(column).push_bind(" = ", value);
    fetch_users(scope, sql)
        .await?
        .into_iter()
        .next()
        .ok_or_else(user_not_found)
}

pub(crate) async fn get_user_by_id(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<User, StoreError> {
    find_user(scope, "id", user_id).await
}

pub(crate) async fn create_user(scope: &mut Scope<'_>, user: &mut User) -> Result<(), StoreError> {
    let now = now_millis();
    user.create_at = now;
    user.update_at = now;
    user.delete_at = 0;

    let mut sql = scope.sql(&format!(
        "INSERT INTO {} ({USER_COLUMNS}) VALUES (",
        scope.table("users")
    ));
    sql.bind(&user.id)
        .push_bind(", ", &user.username)
        .push_bind(", ", &user.email)
        .push_bind(", ", &user.password)
        .push_bind(", ", &user.mfa_secret)
        .push_bind(", ", &user.auth_service)
        .push_bind(", ", &user.auth_data)
        .push_bind(", ", user.create_at)
        .push_bind(", ", user.update_at)
        .push_bind(", ", user.delete_at)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}

/// Sets the password hash of the users whose `column` equals `value`.
async fn update_password(
    scope: &mut Scope<'_>,
    column: &str,
    value: &str,
    password: &str,
) -> Result<(), StoreError> {
    // Preserved quirk: deleted users are not skipped, and since user names are not
    // unique in the database, every user with the name gets the password
    // (`server/services/store/sqlstore/user.go:170-173` and `:195-198`).
    let mut sql = scope.sql(&format!("UPDATE {} SET ", scope.table("users")));
    sql.push_bind("password = ", password)
        .push_bind(", update_at = ", now_millis())
        .push(" WHERE ")
        .push(column)
        .push_bind(" = ", value);
    require_user(scope.execute(sql).await?, value)
}

/// Turns an update that changed no row into the error for a missing user.
fn require_user(rows_affected: u64, user: &str) -> Result<(), StoreError> {
    if rows_affected == 0 {
        return Err(StoreError::UserNotFound(user.to_owned()));
    }
    Ok(())
}

pub(crate) async fn get_user_preferences(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<Vec<Preference>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT userid, category, name, value FROM {} WHERE userid = ",
        scope.table("preferences")
    ));
    sql.bind(user_id)
        .push_bind(" AND category = ", PREFERENCES_CATEGORY);
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(Preference {
                user_id: row.text(0)?,
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
    let table = scope.table("preferences");
    let mut preferences = get_user_preferences(scope, user_id).await?;

    for (name, value) in &patch.updated_fields {
        let mut sql = scope.sql(&format!(
            "INSERT INTO {table} (userid, category, name, value) VALUES ("
        ));
        sql.bind(user_id)
            .push_bind(", ", PREFERENCES_CATEGORY)
            .push_bind(", ", name)
            .push_bind(", ", value)
            .push(") ON CONFLICT (userid, category, name) DO UPDATE SET value = EXCLUDED.value");
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
        let mut sql = scope.sql(&format!("DELETE FROM {table} WHERE userid = "));
        sql.bind(user_id)
            .push_bind(" AND category = ", PREFERENCES_CATEGORY)
            .push_bind(" AND name = ", name);
        scope.execute(sql).await?;

        preferences.retain(|existing| existing.name != *name);
    }

    Ok(preferences)
}
