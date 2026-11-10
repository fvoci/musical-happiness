//! Login, registration and password-change requests.

use std::fmt;

use crate::serde_util::null_as_default;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The minimum length of a password, in characters (Unicode scalar values).
pub const MINIMUM_PASSWORD_LENGTH: usize = 8;

/// The maximum length of a password, in bytes. Far more than anyone types; it keeps a
/// request from making the server hash megabytes.
pub const MAXIMUM_PASSWORD_BYTES: usize = 1024;

/// A request parameter of an authentication request is missing or malformed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct AuthParamError {
    field: &'static str,
    message: String,
}

impl AuthParamError {
    /// Creates an error about the request member `field`, with a message that is sent to
    /// the client as is.
    pub fn new(field: &'static str, message: impl Into<String>) -> Self {
        Self {
            field,
            message: message.into(),
        }
    }

    /// The name of the request member the error is about, as it is spelled on the wire.
    pub fn field(&self) -> &'static str {
        self.field
    }

    /// The same error about another request member: for a rule that several requests
    /// apply to members of different names.
    #[must_use]
    pub fn about(mut self, field: &'static str) -> Self {
        self.field = field;
        self
    }
}

/// A login request.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginRequest {
    /// The user name to log in with.
    #[serde(deserialize_with = "null_as_default")]
    pub username: String,
    /// The email address to log in with, used when no user has the given name.
    #[serde(deserialize_with = "null_as_default")]
    pub email: String,
    /// The password.
    #[serde(deserialize_with = "null_as_default")]
    pub password: String,
}

impl fmt::Debug for LoginRequest {
    /// Leaves out the password.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("username", &self.username)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// A request to register a new user.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RegisterRequest {
    /// The login name.
    #[serde(deserialize_with = "null_as_default")]
    pub username: String,
    /// The email address.
    #[serde(deserialize_with = "null_as_default")]
    pub email: String,
    /// The password.
    #[serde(deserialize_with = "null_as_default")]
    pub password: String,
    /// The team's signup token; may be empty while no user exists yet.
    #[serde(deserialize_with = "null_as_default")]
    pub token: String,
}

impl RegisterRequest {
    /// Checks the request's parameters.
    ///
    /// # Errors
    ///
    /// Returns an [`AuthParamError`] if the user name or email is blank, the email is not
    /// well formed, or the password is empty or shorter than
    /// [`MINIMUM_PASSWORD_LENGTH`].
    pub fn validate(&self) -> Result<(), AuthParamError> {
        if self.username.trim().is_empty() {
            return Err(AuthParamError::new("username", "username is required"));
        }
        if self.email.trim().is_empty() {
            return Err(AuthParamError::new("email", "email is required"));
        }
        if !is_email_valid(&self.email) {
            return Err(AuthParamError::new("email", "invalid email format"));
        }
        check_new_password(&self.password)
    }
}

impl fmt::Debug for RegisterRequest {
    /// Leaves out the password and the signup token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisterRequest")
            .field("username", &self.username)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// A request to change a user's password.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    /// The current password.
    #[serde(deserialize_with = "null_as_default")]
    pub old_password: String,
    /// The password to change to.
    #[serde(deserialize_with = "null_as_default")]
    pub new_password: String,
}

impl ChangePasswordRequest {
    /// Checks the request's parameters.
    ///
    /// # Errors
    ///
    /// Returns an [`AuthParamError`] if either password is empty or the new one is shorter
    /// than [`MINIMUM_PASSWORD_LENGTH`].
    pub fn validate(&self) -> Result<(), AuthParamError> {
        if self.old_password.is_empty() {
            return Err(AuthParamError::new(
                "oldPassword",
                "old password is required",
            ));
        }
        if self.new_password.is_empty() {
            return Err(AuthParamError::new(
                "newPassword",
                "new password is required",
            ));
        }
        check_new_password(&self.new_password).map_err(|error| error.about("newPassword"))
    }
}

impl fmt::Debug for ChangePasswordRequest {
    /// Leaves out both passwords.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePasswordRequest")
            .finish_non_exhaustive()
    }
}

/// Checks a password that is about to be set: on registration, on a password change,
/// and when an administrator sets one.
///
/// # Errors
///
/// Returns an [`AuthParamError`] if the password is empty, has fewer than
/// [`MINIMUM_PASSWORD_LENGTH`] characters, or more than [`MAXIMUM_PASSWORD_BYTES`]
/// bytes.
pub fn check_new_password(password: &str) -> Result<(), AuthParamError> {
    if password.is_empty() {
        return Err(AuthParamError::new("password", "password is required"));
    }
    if password.chars().count() < MINIMUM_PASSWORD_LENGTH {
        return Err(AuthParamError::new(
            "password",
            format!("password must be at least {MINIMUM_PASSWORD_LENGTH} characters"),
        ));
    }
    if password.len() > MAXIMUM_PASSWORD_BYTES {
        return Err(AuthParamError::new(
            "password",
            format!("password must be at most {MAXIMUM_PASSWORD_BYTES} bytes"),
        ));
    }
    Ok(())
}

/// Returns `true` if `email` has the shape of an email address.
///
/// The address must be 3 to 254 bytes long: a local part of ASCII letters, digits and
/// ``.!#$%&'*+/=?^_`{|}~-``, an `@`, and a domain of dot-separated labels. Each label is
/// 1 to 63 ASCII letters, digits and hyphens, and neither starts nor ends with a hyphen.
pub fn is_email_valid(email: &str) -> bool {
    if !(3..=254).contains(&email.len()) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+/=?^_`{|}~-".contains(&b));
    local_ok && domain.split('.').all(is_domain_label)
}

fn is_domain_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return false;
    };
    bytes.len() <= 63
        && first.is_ascii_alphanumeric()
        && last.is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}
