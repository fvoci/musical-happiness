//! The kind of an organization: one that people work in together, or the space of one
//! account.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What an organization is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum OrgKind {
    /// People work in it together: it has members and administrators, workspaces,
    /// invitations, guests, teams and a provider of sign-ins.
    Team,
    /// The space of one account, which every account has one of: the account is its
    /// only member and it has one workspace. Nobody is invited into it and nobody
    /// administers it; what is in it is the account's alone, but for what the account
    /// shares by a public link.
    Personal,
}

impl OrgKind {
    /// Returns the wire and database form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Personal => "personal",
        }
    }
}

impl FromStr for OrgKind {
    type Err = UnknownOrgKind;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "team" => Ok(Self::Team),
            "personal" => Ok(Self::Personal),
            other => Err(UnknownOrgKind(other.to_owned())),
        }
    }
}

/// Text that names no kind of organization.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is no kind of organization")]
pub struct UnknownOrgKind(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_reads_back_from_its_text_and_nothing_else_does() {
        for kind in [OrgKind::Team, OrgKind::Personal] {
            assert_eq!(kind.as_str().parse(), Ok(kind), "{kind:?}");
            assert_eq!(
                serde_json::to_value(kind).expect("a kind is JSON"),
                kind.as_str(),
                "the wire form is the database's"
            );
        }
        assert_eq!(
            "Personal".parse::<OrgKind>(),
            Err(UnknownOrgKind("Personal".to_owned())),
            "in this letter case only"
        );
    }
}
