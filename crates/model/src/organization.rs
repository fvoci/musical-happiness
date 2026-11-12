//! Organizations and workspaces: who belongs to which, and as what.
//!
//! An organization is who everything belongs to; a workspace is where its members
//! work, and what a user who is no member of it does not know to be there
//! ([`Workspace`](crate::Workspace)).

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What a member of an organization is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OrgRole {
    /// Administers the organization: its workspaces and its members.
    Admin,
    /// Belongs to it.
    Member,
}

impl OrgRole {
    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }
}

impl FromStr for OrgRole {
    type Err = UnknownRole;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            other => Err(UnknownRole(other.to_owned())),
        }
    }
}

/// What a member of a workspace is in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceRole {
    /// Administers the workspace.
    Admin,
    /// Works in it: sees its open boards and makes boards.
    Member,
    /// Was invited to boards of it, one by one, and sees nothing else of it.
    Guest,
}

impl WorkspaceRole {
    /// Returns the wire and database form.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
            Self::Guest => "guest",
        }
    }
}

impl FromStr for WorkspaceRole {
    type Err = UnknownRole;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            "guest" => Ok(Self::Guest),
            other => Err(UnknownRole(other.to_owned())),
        }
    }
}

/// Text that names no role.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is no role")]
pub struct UnknownRole(pub String);

/// A workspace as one of its members finds it among theirs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserWorkspace {
    /// The workspace's identifier.
    pub id: String,
    /// The identifier of the organization the workspace belongs to.
    pub org_id: String,
    /// The workspace's name; empty if it was given none.
    pub name: String,
    /// What the user is in the workspace.
    pub role: WorkspaceRole,
    /// Whether the workspace is closed to the session that asks: its organization
    /// takes only sessions made through its provider, and this one was not. What is
    /// under the workspace then answers 401 with the reason `sso_required`; signing in
    /// through the provider of `orgId` opens it.
    pub sso_required: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_reads_back_from_its_text_and_nothing_else_does() {
        for role in [OrgRole::Admin, OrgRole::Member] {
            assert_eq!(role.as_str().parse(), Ok(role), "{role:?}");
        }
        for role in [
            WorkspaceRole::Admin,
            WorkspaceRole::Member,
            WorkspaceRole::Guest,
        ] {
            assert_eq!(role.as_str().parse(), Ok(role), "{role:?}");
        }
        assert_eq!(
            "guest".parse::<OrgRole>(),
            Err(UnknownRole("guest".to_owned())),
            "an organization has no guests"
        );
        assert_eq!(
            "Admin".parse::<WorkspaceRole>(),
            Err(UnknownRole("Admin".to_owned())),
            "in this letter case only"
        );
    }
}
