//! Users, their preferences and their login sessions.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::JsonMap;
use crate::serde_util::{is_zero, null_as_default};

/// A user account.
///
/// The JSON form carries only the public members. The email address, password hash and
/// the other authentication members are never serialized and are ignored when decoding.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct User {
    /// The user's identifier.
    pub id: String,
    /// The login name.
    pub username: String,
    /// The email address.
    #[serde(skip)]
    pub email: String,
    /// The nickname.
    pub nickname: String,
    /// The first name.
    #[serde(rename = "firstname")]
    pub first_name: String,
    /// The last name.
    #[serde(rename = "lastname")]
    pub last_name: String,
    /// The bcrypt hash of the password.
    #[serde(skip)]
    pub password: String,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(rename = "create_at", skip_serializing_if = "is_zero")]
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(rename = "update_at", skip_serializing_if = "is_zero")]
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    #[serde(rename = "delete_at")]
    pub delete_at: i64,
    /// Whether the account is a bot.
    pub is_bot: bool,
    /// Whether the account is a guest.
    pub is_guest: bool,
    /// Special permissions of the user.
    #[serde(
        deserialize_with = "null_as_default",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub permissions: Vec<String>,
    /// The user's system roles.
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
            .field("is_bot", &self.is_bot)
            .field("is_guest", &self.is_guest)
            .field("permissions", &self.permissions)
            .field("roles", &self.roles)
            .finish_non_exhaustive()
    }
}

/// A partial update of a user's preferences.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserPreferencesPatch {
    /// Preferences to set, by name.
    #[serde(deserialize_with = "null_as_default")]
    pub updated_fields: BTreeMap<String, String>,
    /// Names of preferences to remove.
    #[serde(deserialize_with = "null_as_default")]
    pub deleted_fields: Vec<String>,
}

/// A login session.
#[derive(Clone, PartialEq, Default)]
pub struct Session {
    /// The session's identifier.
    pub id: String,
    /// The secret token the client presents.
    pub token: String,
    /// The identifier of the logged-in user.
    pub user_id: String,
    /// The authentication service that created the session.
    pub auth_service: String,
    /// Free-form session properties.
    pub props: JsonMap,
    /// Creation time in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// Last refresh time in milliseconds since the Unix epoch.
    pub update_at: i64,
}

impl fmt::Debug for Session {
    /// Leaves out the token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("props", &self.props)
            .field("create_at", &self.create_at)
            .field("update_at", &self.update_at)
            .finish_non_exhaustive()
    }
}
