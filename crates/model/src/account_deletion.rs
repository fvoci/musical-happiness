//! Deleting an account: what its person asks, and what they are shown before and
//! while it waits.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::refusal::{RefusalReason, Refusals};
use crate::serde_util::null_as_default;

/// How long an account waits between the asking and the erasing, in days: until then
/// its person may take the asking back.
pub const ACCOUNT_DELETION_GRACE_DAYS: i64 = 30;

/// A request to delete the account of the session.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct DeleteAccountRequest {
    /// The account's login name, typed out: that the person means this account.
    #[serde(deserialize_with = "null_as_default")]
    pub username: String,
    /// The account's password; left out, or empty, by an account that has none.
    #[serde(deserialize_with = "null_as_default")]
    pub password: String,
}

impl DeleteAccountRequest {
    /// Checks that the account is named. Whether the name is the account's, and the
    /// password, are the account's to judge.
    ///
    /// # Errors
    ///
    /// Returns `username` if it is empty.
    pub fn validate(&self) -> Result<(), Refusals> {
        let mut refused = Refusals::new();
        if self.username.is_empty() {
            refused.refuse("username", RefusalReason::Required, "username is required");
        }
        refused.into_result()
    }
}

impl fmt::Debug for DeleteAccountRequest {
    /// Leaves out the password.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeleteAccountRequest")
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// Until when an account that was asked to be deleted waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaving {
    /// When the deletion was asked, in milliseconds since the Unix epoch.
    pub requested_at: i64,
    /// When the account is erased at the earliest.
    pub delete_after: i64,
}

/// How an account shows that the request to delete it is its person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeletionProof {
    /// By its password, sent with the request.
    Password,
    /// By a session made through a provider just now: the account has no password.
    SignIn,
}

/// An organization named to its member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationName {
    /// The organization's identifier.
    pub id: String,
    /// Its name; empty if it was given none.
    pub name: String,
}

/// An organization that goes with the account of its only member, and how much is in
/// it. The numbers are left out for an organization the session that asks does not
/// open (one that takes only sessions made through its provider).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LeavingOrganization {
    /// The organization's identifier.
    pub id: String,
    /// Its name; empty if it was given none.
    pub name: String,
    /// How many boards it has in use, templates not counted.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub boards: Option<i64>,
    /// How many wikis it has in use.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub wikis: Option<i64>,
    /// How many files its boards and wikis in use hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub files: Option<i64>,
}

impl LeavingOrganization {
    /// The organization named, with nothing told of what is in it.
    #[must_use]
    pub fn closed(organization: OrganizationName) -> Self {
        Self {
            id: organization.id,
            name: organization.name,
            boards: None,
            wikis: None,
            files: None,
        }
    }
}

/// What deleting the account of the session comes to: what stands in its way, what
/// goes with it, and, once it was asked, until when it waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeletion {
    /// When the deletion was asked, in milliseconds since the Unix epoch; left out
    /// while it is not.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub requested_at: Option<i64>,
    /// When the account is erased at the earliest, in milliseconds since the Unix
    /// epoch; left out while the deletion is not asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(nullable = false)]
    pub delete_after: Option<i64>,
    /// How many days an account waits between the asking and the erasing.
    pub grace_days: i64,
    /// How the request shows that it is the person's.
    pub proof: DeletionProof,
    /// Whether the account is the server's only administrator, who stays: the server
    /// keeps one.
    pub last_instance_admin: bool,
    /// The organizations the account is the only administrator of that stay when it
    /// goes: those that have other people in them, and the server's own first
    /// organization, which goes with no account. Somebody else is made an
    /// administrator of each before the account can be deleted.
    pub sole_admin_of: Vec<OrganizationName>,
    /// The organizations that have nobody but the account, which go with it, and
    /// everything in them. The server's own first organization is never one.
    pub organizations: Vec<LeavingOrganization>,
}

/// The answer to a request to delete the account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeletionAsked {
    /// When the account is erased at the earliest, in milliseconds since the Unix
    /// epoch.
    pub delete_after: i64,
}
