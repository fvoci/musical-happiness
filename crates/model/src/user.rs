//! Users, their preferences and their login sessions.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::serde_util::null_as_default;
use crate::settings::{ClientStateError, check_client_state_name, check_client_state_value};

/// A user account.
///
/// The JSON form carries only the public members. The email address, password hash and
/// the other authentication members are never serialized and are ignored when decoding.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct User {
    /// The user's identifier.
    pub id: String,
    /// The login name.
    pub username: String,
    /// The email address.
    #[serde(skip)]
    pub email: String,
    /// The nickname. Nothing stores one; not part of the JSON form.
    #[serde(skip)]
    pub nickname: String,
    /// The first name. Nothing stores one; not part of the JSON form.
    #[serde(skip)]
    pub first_name: String,
    /// The last name. Nothing stores one; not part of the JSON form.
    #[serde(skip)]
    pub last_name: String,
    /// The hash of the password, as a PHC string.
    #[serde(skip)]
    pub password_hash: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    pub delete_at: i64,
    /// Whether the user is an administrator of the instance. Decided by the server;
    /// nothing a request says about it is taken over.
    pub is_admin: bool,
    /// Whether the account is a bot. Nothing makes one; not part of the JSON form.
    #[serde(skip)]
    pub is_bot: bool,
    /// Whether the account is a guest. Nothing makes one; not part of the JSON form.
    #[serde(skip)]
    pub is_guest: bool,
    /// Special permissions of the user. Nothing stores any; not part of the JSON form.
    #[serde(skip)]
    pub permissions: Vec<String>,
    /// The user's system roles. Nothing stores any; not part of the JSON form.
    #[serde(skip)]
    pub roles: String,
}

impl fmt::Debug for User {
    /// Leaves out the password hash.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("username", &self.username)
            .field("email", &self.email)
            .field("nickname", &self.nickname)
            .field("first_name", &self.first_name)
            .field("last_name", &self.last_name)
            .field("create_at", &self.create_at)
            .field("update_at", &self.update_at)
            .field("delete_at", &self.delete_at)
            .field("is_admin", &self.is_admin)
            .field("is_bot", &self.is_bot)
            .field("is_guest", &self.is_guest)
            .field("permissions", &self.permissions)
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

/// A change to the bag of state a client keeps with the account: entries the server
/// stores without knowing what they mean.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct UserPreferencesPatch {
    /// Entries to set, by name.
    #[serde(deserialize_with = "null_as_default")]
    pub updated_fields: BTreeMap<String, String>,
    /// Names of entries to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_fields: Vec<String>,
}

impl UserPreferencesPatch {
    /// Checks that every name the change sets and every value it sets it to is one the
    /// bag takes. Names to remove are not checked: removing what cannot be there
    /// removes nothing.
    ///
    /// # Errors
    ///
    /// Returns what is wrong with the first entry, in the order of the names, that the
    /// bag does not take.
    pub fn validate(&self) -> Result<(), ClientStateError> {
        for (name, value) in &self.updated_fields {
            check_client_state_name(name)?;
            check_client_state_value(value)?;
        }
        Ok(())
    }
}

/// A login session, as the server keeps it. The token the client presents is not part
/// of it: only a digest of the token is stored, and that stays in the store.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Session {
    /// The session's identifier.
    pub id: String,
    /// The identifier of the signed-in user.
    pub user_id: String,
    /// When the session was made, in milliseconds since the Unix epoch.
    pub created_at: i64,
    /// When the session was last seen in use, in milliseconds since the Unix epoch. It
    /// is not written on every request, so it may be a little behind.
    pub last_used_at: i64,
    /// When the session ends whatever its use, in milliseconds since the Unix epoch.
    pub expires_at: i64,
    /// The `User-Agent` the session was made by, for telling a user's sessions apart.
    pub user_agent: String,
}
