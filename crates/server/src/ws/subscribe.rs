//! The wall of an organization that takes no password, as the connections meet it.
//!
//! A connection follows a workspace if its user is a member of it and its session opens
//! the workspace (`SessionTaken::opened_by`). That is asked when it
//! subscribes and again with each change it is sent (`delivery.rs`), from the cached
//! access the store's visibility revision keeps current: a change of an organization's
//! provider moves that revision on. When an organization begins to take only the
//! sessions of its provider, the connections of other sessions that follow its
//! workspaces stop following them at once ([`Hub::end_subscriptions_shut_out`]).
//! Blocks followed by a read token stay followed: whom the wall shuts out is told them
//! as a reader by the link alone is (`delivery.rs`).

use fvoci_model::LoginOrigin;

use super::Identity;
use crate::app::Asker;

/// Whose a connection is, as its listener keeps it, and the token its session is
/// looked up by while it lasts.
#[derive(Default)]
pub(super) struct Who {
    pub(super) user_id: String,
    pub(super) session_id: String,
    pub(super) login: Option<LoginOrigin>,
    pub(super) token: Option<String>,
}

impl Who {
    /// Who asks, for what is asked of an organization for the connection.
    pub(super) fn asker(&self) -> Asker<'_> {
        Asker {
            user_id: &self.user_id,
            made_through: self.login.as_ref().map(|login| login.provider_id.as_str()),
        }
    }

    /// Whose the connection of a handshake is; nobody's without a session.
    pub(super) fn of(identity: Option<Identity>) -> Self {
        identity.map_or_else(Self::default, |identity| Self {
            user_id: identity.session.user_id,
            session_id: identity.session.id,
            login: identity.session.login,
            token: Some(identity.token),
        })
    }
}
