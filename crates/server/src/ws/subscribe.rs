//! The wall of an organization that takes no password, as the connections meet it.
//!
//! A connection follows a workspace if its user is a member of it and its session opens
//! the workspace (`SessionTaken::opened_by`, asked with what the connection kept of
//! its session, `Listener::making`). That is asked when it
//! subscribes and again with each change it is sent (`delivery.rs`), from the cached
//! access the store's visibility revision keeps current: a change of an organization's
//! provider moves that revision on. When an organization begins to take only the
//! sessions of its provider, the connections of other sessions that follow its
//! workspaces stop following them at once ([`Hub::end_subscriptions_shut_out`]).
//! Blocks followed by a read token stay followed: whom the wall shuts out is told them
//! as a reader by the link alone is (`delivery.rs`).

use fvoci_model::LoginOrigin;

use super::Identity;
use crate::app::{Asker, SessionMade};

/// Whose a connection is, as its listener keeps it, and the token its session is
/// looked up by while it lasts.
pub(super) struct Who {
    pub(super) user_id: String,
    pub(super) session_id: String,
    pub(super) login: Option<LoginOrigin>,
    /// How the session was made and what it had confirmed, as the handshake found it.
    pub(super) making: SessionMade,
    pub(super) token: Option<String>,
}

impl Default for Who {
    /// Nobody's: a connection without a session, which opens nothing that asks for
    /// one, however it is taken to have been made.
    fn default() -> Self {
        Self {
            user_id: String::new(),
            session_id: String::new(),
            login: None,
            making: SessionMade::with_password(),
            token: None,
        }
    }
}

impl Who {
    /// Who asks, for what is asked of an organization for the connection.
    pub(super) fn asker(&self) -> Asker<'_> {
        Asker {
            user_id: &self.user_id,
            making: self.making.making(),
        }
    }

    /// Whose the connection of a handshake is; nobody's without a session.
    pub(super) fn of(identity: Option<Identity>) -> Self {
        identity.map_or_else(Self::default, |identity| Self {
            making: SessionMade::of(&identity.session),
            user_id: identity.session.user_id,
            session_id: identity.session.id,
            login: identity.session.login,
            token: Some(identity.token),
        })
    }
}
