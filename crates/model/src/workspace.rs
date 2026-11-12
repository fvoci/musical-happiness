//! Workspaces: where the members of an organization work, as the routes under a workspace
//! answer one.

use serde::{Deserialize, Serialize};

use crate::JsonMap;
use crate::serde_util::null_as_default;

/// A workspace of an organization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    /// The workspace's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The workspace's title.
    #[serde(deserialize_with = "null_as_default")]
    pub title: String,
    /// Free-form workspace settings.
    #[serde(deserialize_with = "null_as_default")]
    #[schema(value_type = Object)]
    pub settings: JsonMap,
    /// The identifier of the user who last modified the workspace.
    #[serde(deserialize_with = "null_as_default")]
    pub modified_by: String,
    /// Last modification time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub update_at: i64,
    /// Whether the admins of the workspace's boards may invite guests to them.
    pub guest_invitations: bool,
}

impl Default for Workspace {
    /// A workspace with nothing set, in which guests may be invited.
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            settings: JsonMap::new(),
            modified_by: String::new(),
            update_at: 0,
            guest_invitations: true,
        }
    }
}
