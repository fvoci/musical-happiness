//! Closing connections: when the session they were made with ends, when they do not
//! keep up, and when what they may be told cannot be read. What ends of a workspace or
//! of a shared board ends only what follows it (`ending.rs`).

use crate::metrics::Disconnect;

use super::{Hub, Listener, ListenerId};

impl Hub {
    /// Closes every connection of the user. For when the user's sessions are ended: a
    /// connection asks for its session once, when it is made, and whoever ends a session
    /// closes its connections.
    pub fn close_user(&self, user_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id
        });
    }

    /// Closes the connections of a session. For when the session ends: at logout, and
    /// when it has gone unused for too long or reached its end.
    pub fn close_session(&self, session_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.session_id == session_id
        });
    }

    /// Closes the connections of the user that authenticated with another session than
    /// the given one. For when a user changes their password: the session it was
    /// changed through stays, and so do its connections.
    pub fn close_other_sessions_of_user(&self, user_id: &str, kept_session_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id && listener.session_id != kept_session_id
        });
    }

    /// Ends one connection without invalidating other subscriptions' admission checks.
    pub(super) fn close_listener(&self, id: ListenerId, reason: Disconnect) {
        let mut listeners = self.listeners();
        if let Some(listener) = listeners.by_id.get(&id) {
            listener.close(reason);
        }
        listeners.remove(id);
    }

    /// Closes the connections `closes` picks, for `reason`: a subscription of their
    /// users being checked is checked again.
    pub(super) fn close_where(&self, reason: Disconnect, closes: impl Fn(&Listener) -> bool) {
        let mut guard = self.listeners();
        let listeners = &mut *guard;
        let ids: Vec<ListenerId> = listeners
            .by_id
            .iter()
            .filter(|(_, listener)| closes(listener))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(listener) = listeners.by_id.get(&id) {
                listener.close(reason);
                listeners.revoked.user(&listener.user_id);
            }
            listeners.remove(id);
        }
    }
}
