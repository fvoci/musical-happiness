//! A team is a group of the members of one workspace, excluding its guests.

use serde::{Deserialize, Serialize};

/// A named group of people within a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    /// The group's identifier.
    pub id: String,
    /// The workspace it belongs to.
    pub workspace_id: String,
    /// Its name, unique within the workspace in any letter case.
    pub name: String,
    /// Its people, ordered by user identifier.
    pub members: Vec<TeamMember>,
}

/// A person in a team. Neither email addresses nor credentials are exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    /// The person's identifier.
    pub user_id: String,
    /// Their login name.
    pub username: String,
    /// Their displayed name, empty when none is set.
    pub display_name: String,
}

/// The name when creating or renaming a team.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, utoipa::ToSchema)]
pub struct TeamName {
    /// A nonempty name; surrounding whitespace is removed.
    pub name: String,
}

/// Why a team name cannot be stored on both databases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TeamNameError {
    /// The name is empty after surrounding whitespace is removed.
    #[error("a team needs a name")]
    Empty,
    /// PostgreSQL text cannot hold the null character.
    #[error("a team name cannot contain a null character")]
    Null,
}

/// Trims a team name and checks that it can be stored on either database.
///
/// # Errors
/// Returns [`TeamNameError`] for an empty name or a null character.
pub fn team_name(text: &str) -> Result<&str, TeamNameError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(TeamNameError::Empty);
    }
    if text.contains('\0') {
        return Err(TeamNameError::Null);
    }
    Ok(text)
}

/// The form of a team name that names differing only in letter case share. A name is
/// unique in its workspace by this form, and teams are listed in the order of its code
/// points; the server makes it, so both databases hold, compare and order the same text.
///
/// Upper case and then lower case fold what lower case alone keeps apart: `Straße` and
/// `STRASSE`, a final sigma and another, the long s and `s`.
#[must_use]
pub fn team_name_key(name: &str) -> String {
    name.to_uppercase().to_lowercase()
}
