//! The commands that subscribe a connection, and the wall of an organization that
//! takes no password.
//!
//! A connection follows a workspace if its user is a member of it and its session opens
//! the workspace's organization (`permissions::session_opens`). That is asked when it
//! subscribes, so when an organization begins to take only the sessions of its
//! provider, the connections of other sessions that follow its workspaces are closed
//! ([`Hub::close_sessions_closed_out`]). Blocks are followed by the read token of their
//! board, by anybody, as a reader; a follower who is a member of the board is told what
//! a member is told only while its session opens the organization, which is asked with
//! each change ([`Hub::session_opens_workspace`]).

use super::{
    Command, Hub, Identity, Listener, ListenerId, Listeners, subscribe_to_blocks,
    subscribe_to_workspace, unsubscribe_from_blocks,
};
use crate::metrics::Disconnect;
use crate::permissions::session_opens;

/// How often a subscription is asked again because a wall was raised while it was
/// asked, before it is refused: the page subscribes anew when it connects again.
const ASKED_AT_MOST: usize = 3;

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

/// Whether the wall of an organization that now takes only sessions made through
/// `provider_id` shuts the connection out: one that follows a workspace of the
/// organization with a session made otherwise.
pub(super) fn shuts_out(listener: &Listener, workspace_ids: &[String], provider_id: &str) -> bool {
    listener.login_provider.as_deref() != Some(provider_id)
        && listener
            .workspaces
            .iter()
            .any(|workspace| workspace_ids.contains(workspace))
}

/// Subscribes the connection to the workspace, unless a wall was raised since
/// `walls_raised` was read: the answer that admitted it may be of before the wall.
/// Says whether it subscribed.
pub(super) fn subscribe_unless_walled(
    listeners: &mut Listeners,
    id: ListenerId,
    workspace_id: &str,
    walls_raised: u64,
) -> bool {
    if listeners.walls_raised != walls_raised {
        return false;
    }
    subscribe_to_workspace(listeners, id, workspace_id);
    true
}

impl Hub {
    /// Whether the connection's session opens the organization of the workspace its
    /// user is a member of; false for who is no member, and where that cannot be read.
    pub(super) async fn session_opens_workspace(
        &self,
        id: ListenerId,
        user_id: &str,
        workspace_id: &str,
    ) -> bool {
        let Some(login_provider) = self
            .listeners()
            .by_id
            .get(&id)
            .map(|listener| listener.login_provider.clone())
        else {
            return false;
        };
        self.audience
            .workspace_access(user_id, workspace_id)
            .await
            .is_some_and(|access| {
                session_opens(
                    access.required_provider.as_deref(),
                    login_provider.as_deref(),
                )
            })
    }

    /// Subscribes the connection to the workspace, if its user is a member of it and
    /// its session opens the workspace's organization. Refused, it stays as it is; the
    /// connection is told nothing, as of any command.
    ///
    /// The answer is asked without the table held. Had a wall been raised meanwhile,
    /// the answer may be of before it: then it is asked again, so that no connection
    /// is subscribed past the closing of those the wall shuts out.
    pub(super) async fn follow_workspace(&self, id: ListenerId, user_id: &str, workspace_id: &str) {
        for _ in 0..ASKED_AT_MOST {
            let walls_raised = self.listeners().walls_raised;
            if !self
                .session_opens_workspace(id, user_id, workspace_id)
                .await
            {
                tracing::error!(workspace_id, user_id, "websocket: no workspace access");
                return;
            }
            if subscribe_unless_walled(&mut self.listeners(), id, workspace_id, walls_raised) {
                return;
            }
        }
        tracing::warn!(
            workspace_id,
            user_id,
            "websocket: a subscription was given up"
        );
    }

    /// Carries out `SUBSCRIBE_BLOCKS` and `UNSUBSCRIBE_BLOCKS`: by the read token of the
    /// board the blocks are of, for a connection with or without a session.
    pub(super) async fn follow_blocks(&self, id: ListenerId, command: &Command) {
        let Some(board_id) = self.board_of_read_token(command).await else {
            tracing::error!(action = %command.action, "websocket: rejected an invalid read token");
            return;
        };
        let mut listeners = self.listeners();
        if command.action == "SUBSCRIBE_BLOCKS" {
            subscribe_to_blocks(&mut listeners, id, &command.block_ids);
            if let Some(listener) = listeners.by_id.get_mut(&id)
                && !listener.shared_boards.contains(&board_id)
            {
                listener.shared_boards.push(board_id);
            }
        } else {
            unsubscribe_from_blocks(&mut listeners, id, &command.block_ids);
        }
    }

    /// Closes the connections that an organization shuts out as it begins to take only
    /// sessions made through the provider `provider_id`: those that follow one of its
    /// workspaces, `workspace_ids`, with a session made otherwise (`shuts_out`). For
    /// after that is stored. A connection that subscribes meanwhile or after is asked
    /// where it subscribes: the wall is counted first, so an answer of before it is
    /// asked again.
    pub fn close_sessions_closed_out(&self, workspace_ids: &[String], provider_id: &str) {
        self.listeners().walls_raised += 1;
        self.close_where(Disconnect::SsoRequired, |listener| {
            shuts_out(listener, workspace_ids, provider_id)
        });
    }

    /// As [`Self::close_sessions_closed_out`] where the organization's workspaces are
    /// not known: closes every connection that follows a workspace, whichever, with a
    /// session that was not made through the provider. One that follows none has
    /// nothing of the organization to be told: blocks followed by a read token are
    /// asked about at each change.
    pub fn close_sessions_made_otherwise(&self, provider_id: &str) {
        self.listeners().walls_raised += 1;
        self.close_where(Disconnect::SsoRequired, |listener| {
            !listener.workspaces.is_empty()
                && listener.login_provider.as_deref() != Some(provider_id)
        });
    }
}
