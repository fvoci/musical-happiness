//! The channel of each person: every connection with a session is told when what its
//! user was told changed (`UPDATE_NOTIFICATIONS`), whatever workspace it follows, or
//! none.
//!
//! The message says nothing of what changed: the client reads the notifications again
//! through the routes, which ask of each what the session reads. What an organization
//! that takes only the sessions made through its provider told somebody is kept from
//! their other sessions by the list, and here too: their connections are not told of
//! it, not even that something happened, nor that they read it.

use std::collections::HashMap;

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
    /// Tells the connections of `user_id` that their notifications of the
    /// organizations `org_ids` changed: each connection whose session one of them
    /// does not keep them from (`Audience::notifications_kept_from`). Each way a
    /// session is made is asked once; a connection whose session cannot be asked now
    /// is closed, so that it reads everything again once it is back.
    pub async fn notifications_changed(&self, user_id: &str, org_ids: &[&str]) {
        if org_ids.is_empty() {
            return;
        }
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
        let mut opens: HashMap<SessionMade, bool> = HashMap::new();
        let mut told = Vec::with_capacity(connections.len());
        let mut unread = Vec::new();
        for (id, made) in connections {
            let opened = if let Some(opened) = opens.get(&made) {
                Some(*opened)
            } else {
                let opened = self.opens_any(user_id, &made, org_ids).await;
                if let Some(opened) = opened {
                    opens.insert(made, opened);
                }
                opened
            };
            match opened {
                Some(true) => told.push(id),
                Some(false) => {}
                None => unread.push(id),
            }
        }
        self.queue_text(&told, &text, message.action());
        self.close_unread(&unread);
    }

    /// Whether one of `org_ids` lets a session made as `made` hear of what it told
    /// `user_id`; `None` where that cannot be read now.
    async fn opens_any(&self, user_id: &str, made: &SessionMade, org_ids: &[&str]) -> Option<bool> {
        for org_id in org_ids {
            match self
                .audience
                .notifications_kept_from(user_id, made.making(), org_id)
                .await
            {
                Ok(false) => return Some(true),
                Ok(true) => {}
                Err(error) => {
                    tracing::warn!(%error, "websocket: whether a session is told of a notification could not be read");
                    return None;
                }
            }
        }
        Some(false)
    }
}
