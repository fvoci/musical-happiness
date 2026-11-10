//! Login, registration and password-change requests.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The minimum length of a password, in bytes.
pub const MINIMUM_PASSWORD_LENGTH: usize = 8;

/// A request parameter of an authentication request is missing or malformed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct AuthParamError(String);

impl AuthParamError {
    /// Creates an error with the given message, which is sent to the client as is.
    pub fn new(message: &str) -> Self {
        Self(message.to_owned())
    }
}

/// A login request.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginRequest {
    /// The type of login; must be `normal`.
    #[serde(rename = "type")]
    pub login_type: String,
    /// The user name to log in with.
    pub username: String,
    /// The email address to log in with, used when no user has the given name.
    pub email: String,
    /// The password.
    pub password: String,
    /// A multi-factor token; ignored.
    pub mfa_token: String,
}

impl fmt::Debug for LoginRequest {
    /// Leaves out the password and the multi-factor token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginRequest")
            .field("login_type", &self.login_type)
            .field("username", &self.username)
            .field("email", &self.email)
            .finish_non_exhaustive()
    }
}

/// The response to a successful login.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginResponse {
    /// The session token to present on later requests.
    pub token: String,
}

impl fmt::Debug for LoginResponse {
    /// Leaves out the token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginResponse").finish_non_exhaustive()
    }
}

/// A request to register a new user.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RegisterRequest {
    /// The login name.
    pub username: String,
    /// The email address.
    pub email: String,
    /// The password.
    pub password: String,
    /// The team's signup token; may be empty while no user exists yet.
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
            return Err(AuthParamError::new("username is required"));
        }
        if self.email.trim().is_empty() {
            return Err(AuthParamError::new("email is required"));
        }
        if !is_email_valid(&self.email) {
            return Err(AuthParamError::new("invalid email format"));
        }
        if self.password.is_empty() {
            return Err(AuthParamError::new("password is required"));
        }
        check_password_length(&self.password)
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
    pub old_password: String,
    /// The password to change to.
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
            return Err(AuthParamError::new("old password is required"));
        }
        if self.new_password.is_empty() {
            return Err(AuthParamError::new("new password is required"));
        }
        check_password_length(&self.new_password)
    }
}

impl fmt::Debug for ChangePasswordRequest {
    /// Leaves out both passwords.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChangePasswordRequest")
            .finish_non_exhaustive()
    }
}

fn check_password_length(password: &str) -> Result<(), AuthParamError> {
    if password.len() < MINIMUM_PASSWORD_LENGTH {
        return Err(AuthParamError(format!(
            "password must be at least {MINIMUM_PASSWORD_LENGTH} characters"
        )));
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
