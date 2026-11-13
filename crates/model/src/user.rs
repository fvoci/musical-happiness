//! Users, their preferences and their login sessions.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::refusal::{RefusalReason, Refusals};
use crate::serde_util::null_as_default;
use crate::settings::{
    CLIENT_STATE_MAX_NAME_LENGTH, ClientStateError, check_client_state_name,
    check_client_state_value,
};

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
    /// The name the user chose to be shown by; empty for none, and then the login name
    /// is shown. At most 64 UTF-16 code units, not unique.
    pub display_name: String,
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
            .field("display_name", &self.display_name)
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
    /// Returns every entry, in the order of the names, that the bag does not take: each
    /// at `/updatedFields/<name>`, with what is wrong with its name or its value.
    pub fn validate(&self) -> Result<(), Refusals> {
        let mut refused = Refusals::new();
        for (name, value) in &self.updated_fields {
            let checked =
                check_client_state_name(name).and_then(|()| check_client_state_value(value));
            if let Err(error) = checked {
                let reason = match error {
                    ClientStateError::InvalidName if name.is_empty() => RefusalReason::Required,
                    ClientStateError::InvalidName if name.len() > CLIENT_STATE_MAX_NAME_LENGTH => {
                        RefusalReason::TooLong
                    }
                    ClientStateError::InvalidName => RefusalReason::InvalidCharacters,
                    ClientStateError::ValueTooLong => RefusalReason::TooLong,
                };
                refused.refuse_within("updatedFields", name, reason, error.to_string());
            }
        }
        refused.into_result()
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
    /// The `OpenID Connect` provider the session was made through; `None` for a session
    /// made with a password.
    pub login: Option<LoginOrigin>,
}

impl Session {
    /// The identifier of the provider the session was made through, if any.
    #[must_use]
    pub fn login_provider_id(&self) -> Option<&str> {
        self.login.as_ref().map(|login| login.provider_id.as_str())
    }
}

/// Where a session made through a provider came from. Kept as it was when the session
/// was made: removing the provider or changing its issuer ends the sessions of the
/// organization and issuer, and unlinking an identity those made for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginOrigin {
    /// The provider row the session was made through; a replaced row gets a new one.
    pub provider_id: String,
    /// The organization whose provider it was.
    pub org_id: String,
    /// The provider's issuer at the time.
    pub issuer: String,
    /// The subject the provider gave: with the issuer, the identity the session was
    /// made for. `None` for a session made before that was kept, which is taken for
    /// one of every identity of its issuer.
    pub subject: Option<String>,
}

impl LoginOrigin {
    /// Whether the session was made for the identity `subject` of `issuer`, or may have
    /// been: one that knows its issuer only answers for every subject of it.
    #[must_use]
    pub fn is_of_identity(&self, issuer: &str, subject: &str) -> bool {
        self.issuer == issuer
            && self
                .subject
                .as_deref()
                .is_none_or(|made_for| made_for == subject)
    }
}
