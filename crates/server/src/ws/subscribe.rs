//! The commands that subscribe a connection, and the wall of an organization that
//! takes no password.
//!
//! A connection follows a workspace if its user is a member of it and its session opens
//! the workspace's organization (`permissions::session_opens`), and follows blocks by
//! the read token of their board, as anybody does, unless it comes with a session that
//! does not open the board's organization. When an organization begins to take only
//! the sessions of its provider, the connections of the other sessions of its people
//! are closed ([`Hub::close_sessions_closed_out`]): what a connection follows is asked
//! when it subscribes, so a change of the answer ends the connections it was given to.

use std::collections::BTreeSet;

use super::{
    Command, Hub, Identity, ListenerId, subscribe_to_blocks, subscribe_to_workspace,
    unsubscribe_from_blocks,
};
use crate::metrics::Disconnect;
use crate::permissions::session_opens;

/// Whose a connection is, as its listener keeps it, and the token its session is
/// looked up by while it lasts.
#[derive(Debug, Default)]
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

/// What a connection is asked by when it subscribes, and how many walls had been
/// raised then.
struct Asker {
    user_id: String,
    login_provider: Option<String>,
    walls_raised: u64,
}

impl Hub {
    fn asker(&self, id: ListenerId) -> Option<Asker> {
        let listeners = self.listeners();
        let listener = listeners.by_id.get(&id)?;
        Some(Asker {
            user_id: listener.user_id.clone(),
            login_provider: listener.login_provider.clone(),
            walls_raised: listeners.walls_raised,
        })
    }

    /// Subscribes the connection to the workspace, if its user is a member of it and
    /// its session opens the workspace's organization. Refused, it stays as it is; the
    /// connection is told nothing, as of any command.
    ///
    /// The answer is asked without the table held. Had a wall been raised meanwhile,
    /// the answer may be of before it: then it is asked again, so that no connection
    /// is subscribed past the closing of those the wall shuts out.
    pub(super) async fn follow_workspace(&self, id: ListenerId, workspace_id: &str) {
        loop {
            let Some(asker) = self.asker(id) else {
                return;
            };
            let admitted = self
                .audience
                .workspace_access(&asker.user_id, workspace_id)
                .await
                .is_some_and(|access| {
                    session_opens(
                        access.required_provider.as_deref(),
                        asker.login_provider.as_deref(),
                    )
                });
            if !admitted {
                tracing::error!(workspace_id, user_id = %asker.user_id, "websocket: no workspace access");
                return;
            }
            let mut listeners = self.listeners();
            if listeners.walls_raised == asker.walls_raised {
                subscribe_to_workspace(&mut listeners, id, workspace_id);
                return;
            }
        }
    }

    /// Carries out `SUBSCRIBE_BLOCKS` and `UNSUBSCRIBE_BLOCKS`: by the read token of the
    /// board the blocks are of. A connection with a session that does not open the
    /// board's organization is refused: the hub tells a board's member more of a block
    /// than a reader (who wrote it), and that session is no member's there.
    pub(super) async fn follow_blocks(&self, id: ListenerId, command: &Command) {
        loop {
            let Some(asker) = self.asker(id) else {
                return;
            };
            let Some(board_id) = self.board_of_read_token(command).await else {
                tracing::error!(action = %command.action, "websocket: rejected an invalid read token");
                return;
            };
            if !self.reads_as_anybody(&asker, &board_id, command).await {
                tracing::error!(action = %command.action, user_id = %asker.user_id, "websocket: the session does not open the board's organization");
                return;
            }
            let mut listeners = self.listeners();
            if listeners.walls_raised != asker.walls_raised {
                continue;
            }
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
            return;
        }
    }

    /// Whether the connection may follow the shared board by its read token: one
    /// without a session, and one whose session opens the board's organization. What
    /// cannot be read is a "no".
    async fn reads_as_anybody(&self, asker: &Asker, board_id: &str, command: &Command) -> bool {
        if asker.user_id.is_empty() {
            return true;
        }
        let org_id = match self
            .inner
            .auth
            .org_of_read_token(board_id, &command.read_token)
            .await
        {
            Ok(Some(org_id)) => org_id,
            Ok(None) => return false,
            Err(error) => {
                tracing::error!(%error, "websocket: reading the organization of a shared board failed");
                return false;
            }
        };
        match self
            .audience
            .required_provider_for(&asker.user_id, &org_id)
            .await
        {
            Ok(required) => session_opens(required.as_deref(), asker.login_provider.as_deref()),
            Err(error) => {
                tracing::error!(%error, "websocket: reading the provider of an organization failed");
                false
            }
        }
    }

    /// Closes the connections that the organization `org_id` shuts out as it begins to
    /// take only sessions made through the provider `provider_id`: those of its people
    /// (`Store::required_provider_for`) whose session was not made through it,
    /// whatever they follow. For after that is stored. A connection made since, or
    /// subscribing meanwhile, is asked again where it subscribes.
    ///
    /// A user of whom it cannot be read whether they are of the organization is taken
    /// for one: the page connects again, and is asked then.
    pub async fn close_sessions_closed_out(&self, org_id: &str, provider_id: &str) {
        let made_otherwise =
            |login_provider: &Option<String>| login_provider.as_deref() != Some(provider_id);
        let users: BTreeSet<String> = {
            let mut listeners = self.listeners();
            listeners.walls_raised += 1;
            listeners
                .by_id
                .values()
                .filter(|listener| {
                    !listener.user_id.is_empty() && made_otherwise(&listener.login_provider)
                })
                .map(|listener| listener.user_id.clone())
                .collect()
        };
        let mut shut_out = BTreeSet::new();
        for user_id in users {
            match self.audience.required_provider_for(&user_id, org_id).await {
                Ok(None) => {}
                Ok(Some(_)) => {
                    shut_out.insert(user_id);
                }
                Err(error) => {
                    tracing::warn!(%error, org_id, "websocket: reading whether a user is of an organization failed");
                    shut_out.insert(user_id);
                }
            }
        }
        self.close_where(Disconnect::SsoRequired, |listener| {
            shut_out.contains(&listener.user_id) && made_otherwise(&listener.login_provider)
        });
    }
}
