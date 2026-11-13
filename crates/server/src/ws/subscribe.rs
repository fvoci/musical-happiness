//! The wall of an organization that takes no password, as the connections meet it.
//!
//! A connection follows a workspace if its user is a member of it and its session opens
//! the workspace's organization (`permissions::session_opens`). That is asked when it
//! subscribes and again with each change it is sent (`delivery.rs`), from the cached
//! access the store's visibility revision keeps current: a change of an organization's
//! provider moves that revision on. When an organization begins to take only the
//! sessions of its provider, the connections of other sessions that follow its
//! workspaces stop following them at once ([`Hub::end_subscriptions_shut_out`]).
//! Blocks followed by a read token are told no user's identifier whoever follows them,
//! so the wall asks nothing of them.

use super::{Identity, Listener};
use crate::app::Asker;
use crate::permissions::session_opens;

/// Whose a connection is, as its listener keeps it, and the token its session is
/// looked up by while it lasts.
#[derive(Default)]
pub(super) struct Who {
    pub(super) user_id: String,
    pub(super) session_id: String,
    pub(super) login_provider: Option<String>,
    pub(super) token: Option<String>,
}

impl Who {
    /// Who asks, for what is asked of an organization for the connection.
    pub(super) fn asker(&self) -> Asker<'_> {
        Asker {
            user_id: &self.user_id,
            made_through: self.login_provider.as_deref(),
        }
    }

    /// Whose the connection of a handshake is; nobody's without a session.
    pub(super) fn of(identity: Option<Identity>) -> Self {
        identity.map_or_else(Self::default, |identity| Self {
            user_id: identity.session.user_id,
            session_id: identity.session.id,
            login_provider: identity.session.login_provider_id,
            token: Some(identity.token),
        })
    }
}

impl Listener {
    /// Whether the connection's session opens an organization that takes only
    /// sessions made through `required`, if any.
    pub(super) fn session_opens(&self, required: Option<&str>) -> bool {
        session_opens(required, self.login_provider.as_deref())
    }
}
