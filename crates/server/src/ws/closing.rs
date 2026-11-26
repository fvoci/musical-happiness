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

    /// Has the connections of the session look it up under `token`, which it goes on
    /// under: they stay open, so that none comes back under a cookie that is the
    /// session's no more, before the answer that carries the new one has arrived. For
    /// after the store put the session under the token, as the account's password was
    /// set or changed through it. A handshake that read the session before that and
    /// becomes a connection after this is ended by its own look at the session
    /// (`Hub::serve`), and comes back.
    pub fn session_goes_on_under(&self, session_id: &str, token: &str) {
        self.each_of_session(session_id, |listener| {
            if let Some(held) = &listener.token {
                held.replace(token);
            }
        });
    }

    /// As [`Self::session_goes_on_under`], for a session that gave the account's
    /// password: its connections are marked as ones whose session did, and are asked
    /// from here on as the session is now. For after the store recorded it.
    pub fn session_gave_password(&self, session_id: &str, token: &str) {
        self.each_of_session(session_id, |listener| {
            if let Some(making) = &listener.making {
                making.gave_password();
            }
            if let Some(held) = &listener.token {
                held.replace(token);
            }
        });
    }

    fn each_of_session(&self, session_id: &str, mut each: impl FnMut(&Listener)) {
        let listeners = self.listeners();
        for listener in listeners.by_id.values() {
            if listener.session_id == session_id {
                each(listener);
            }
        }
    }

    /// Closes the connections of the sessions made through a provider of the
    /// organization under `issuer`, and with them their seats in the rooms. For when
    /// the store ended those sessions: the provider was removed or took another issuer.
    pub fn close_sessions_of_origin(&self, org_id: &str, issuer: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener
                .login
                .as_ref()
                .is_some_and(|login| login.org_id == org_id && login.issuer == issuer)
        });
    }

    /// Closes the connections of the user's sessions made for the identity `subject` of
    /// `issuer`, in any organization, and their seats in the rooms: also those of the
    /// issuer that know no subject ([`LoginOrigin::is_of_identity`]). For when the user
    /// unlinked that identity and the store ended those sessions.
    ///
    /// [`LoginOrigin::is_of_identity`]: fvoci_model::LoginOrigin::is_of_identity
    pub fn close_sessions_of_identity(&self, user_id: &str, issuer: &str, subject: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id
                && listener
                    .login
                    .as_ref()
                    .is_some_and(|login| login.is_of_identity(issuer, subject))
        });
    }

    /// Closes the connections of the user that authenticated with another session than
    /// the given one. For when a user changes their password or sets the first: the
    /// session it was done through stays, and so do its connections
    /// ([`Self::session_goes_on_under`]).
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
