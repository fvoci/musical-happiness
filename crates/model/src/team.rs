//! Teams.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::JsonMap;
use crate::serde_util::null_as_default;

/// A team. A standalone server has exactly one, with identifier `0`.
#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Team {
    /// The team's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The team's title.
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// The token new users must present to register. Empty, and left out of the JSON
    /// form, in what is answered to anyone but an administrator of the instance.
    #[serde(
        deserialize_with = "null_as_default",
        skip_serializing_if = "String::is_empty"
    )]
    pub signup_token: String,
    /// Free-form team settings.
    #[serde(deserialize_with = "null_as_default")]
    pub settings: JsonMap,
    /// The identifier of the user who last modified the team.
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
}

impl fmt::Debug for Team {
    /// Leaves out the signup token.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Team")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("settings", &self.settings)
            .field("modified_by", &self.modified_by)
            .field("update_at", &self.update_at)
            .finish_non_exhaustive()
    }
}
