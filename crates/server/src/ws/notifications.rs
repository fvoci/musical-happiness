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

/// Whether the sessions made one way hear of what some organizations told their user.
#[derive(Debug, Clone, Copy)]
enum Opens {
    /// One of the organizations lets them hear.
    Yes,
    /// None does.
    No,
    /// None of those that could be read does, and one could not be read now.
    Unknown,
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
        let mut asked: HashMap<SessionMade, Opens> = HashMap::new();
        let mut told = Vec::with_capacity(connections.len());
        let mut unread = Vec::new();
        for (id, made) in connections {
            let opens = if let Some(known) = asked.get(&made) {
                *known
            } else {
                let opens = self.opens_any(user_id, &made, org_ids).await;
                asked.insert(made, opens);
                opens
            };
            match opens {
                Opens::Yes => told.push(id),
                Opens::No => {}
                Opens::Unknown => unread.push(id),
            }
        }
        self.queue_text(&told, &text, message.action());
        self.close_unread(&unread);
    }

    /// Whether one of `org_ids` lets a session made as `made` hear of what it told
    /// `user_id`. Those that cannot be read now are passed over: one of the others may
    /// open it.
    async fn opens_any(&self, user_id: &str, made: &SessionMade, org_ids: &[&str]) -> Opens {
        let mut opens = Opens::No;
        for org_id in org_ids {
            match self
                .audience
                .notifications_kept_from(user_id, made.making(), org_id)
                .await
            {
                Ok(false) => return Opens::Yes,
                Ok(true) => {}
                Err(error) => {
                    tracing::warn!(%error, "websocket: whether a session is told of a notification could not be read");
                    opens = Opens::Unknown;
                }
            }
        }
        opens
    }
}
