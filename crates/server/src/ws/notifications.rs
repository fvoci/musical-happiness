//! The channel of each person: every connection with a session is told when what its
//! user was told changed (`UPDATE_NOTIFICATIONS`), whatever workspace it follows, or
//! none.
//!
//! The message says nothing of what changed: the client reads the notifications again
//! through the routes, which ask of each what the session reads. What an organization
//! that takes only the sessions made through its provider told somebody is kept from
//! their other sessions by the list, and here too: their connections are not told of
//! it, not even that something happened.

use serde::Serialize;

use super::{Hub, ListenerId, Outgoing, text_of};
use crate::app::SessionMade;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNotificationsMessage {
    action: &'static str,
}

impl Outgoing for UpdateNotificationsMessage {
    fn action(&self) -> &'static str {
        self.action
    }
}

impl Hub {
    /// Tells the connections of `user_id` that their notifications changed: of the
    /// organization `org_id`, those whose session it does not keep them from
    /// (`Audience::notifications_kept_from`), or every one where the user changed
    /// them themself (`None`: they read some). A connection whose session cannot be
    /// asked now is not told.
    pub async fn notifications_changed(&self, user_id: &str, org_id: Option<&str>) {
        let connections: Vec<(ListenerId, SessionMade)> = {
            let listeners = self.listeners();
            listeners
                .by_user
                .get(user_id)
                .into_iter()
                .flatten()
                .filter_map(|id| {
                    listeners
                        .by_id
                        .get(id)
                        .map(|listener| (*id, listener.made()))
                })
                .collect()
        };
        if connections.is_empty() {
            return;
        }
        let message = UpdateNotificationsMessage {
            action: "UPDATE_NOTIFICATIONS",
        };
        let Some(text) = text_of(&message) else {
            return;
        };
        let mut told = Vec::with_capacity(connections.len());
        for (id, made) in connections {
            let Some(org_id) = org_id else {
                told.push(id);
                continue;
            };
            match self
                .audience
                .notifications_kept_from(user_id, made.making(), org_id)
                .await
            {
                Ok(false) => told.push(id),
                Ok(true) => {}
                Err(error) => {
                    tracing::warn!(%error, "websocket: whether a session is told of a notification could not be read");
                }
            }
        }
        self.queue_text(&told, &text, message.action());
    }
}
