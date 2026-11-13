//! Asking the rooms again at once when who may be in them changed: what a connection
//! follows of a workspace ends without the connection (`ws/ending.rs`), and so the
//! rooms of that workspace put out by themselves who may no longer be in them, and
//! keep the connection's rooms of other workspaces.

use std::sync::Arc;

use super::{Member, Room};
use crate::app::App;
use crate::collab::Collab;

/// The facts changed in this part of the application's organization, for everybody
/// or for one user.
#[derive(Debug, Clone)]
pub(crate) struct AccessScope {
    place: Place,
    /// The one user whose facts changed; everybody's when `None`.
    user_id: Option<String>,
}

#[derive(Debug, Clone)]
enum Place {
    Workspace(String),
    Organization,
}

impl AccessScope {
    /// Every board of a workspace.
    pub(crate) fn workspace(workspace_id: &str) -> Self {
        Self::of(Place::Workspace(workspace_id.to_owned()))
    }

    /// Every workspace of the organization.
    pub(crate) fn organization() -> Self {
        Self::of(Place::Organization)
    }

    /// The same place, where only `user_id`'s facts changed.
    pub(crate) fn of_user(self, user_id: &str) -> Self {
        Self {
            user_id: Some(user_id.to_owned()),
            ..self
        }
    }

    fn of(place: Place) -> Self {
        Self {
            place,
            user_id: None,
        }
    }

    /// Whether the member is one whose facts changed.
    fn concerns(&self, member: &Member) -> bool {
        let in_place = match &self.place {
            Place::Workspace(id) => member.workspace_id == *id,
            Place::Organization => true,
        };
        in_place
            && self
                .user_id
                .as_deref()
                .is_none_or(|changed| changed == member.connection.user_id)
    }
}

impl Collab {
    /// Asks the rooms of `app`'s organization that hold a member `scope` concerns
    /// again whether each member may be in them, and how ([`Collab::recheck`]): who
    /// may no longer read the card is put out of that room, told why, and keeps the
    /// connection. For after the change is stored, before the request that stored it
    /// answers. A connection coming in meanwhile is asked at the store's visibility
    /// revision, which the change moved on.
    pub(crate) async fn recheck_access(&self, app: &App, scope: AccessScope) {
        let tenant = app.store().tenant();
        let of_organization: Vec<Arc<Room>> = self
            .inner
            .rooms
            .by_key
            .lock()
            .expect("the rooms are never held across a panic")
            .values()
            .filter(|room| room.app.store().tenant() == tenant)
            .cloned()
            .collect();
        for room in of_organization {
            let concerned = room
                .turn
                .lock()
                .await
                .members
                .iter()
                .any(|member| scope.concerns(member));
            if concerned {
                self.recheck(&room).await;
            }
        }
    }
}
