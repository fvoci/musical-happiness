//! Authorization at dispatch, backed by the store's committed visibility revision.

use fvoci_model::{Block, BoardMember};

use super::{Hub, ListenerId, Outgoing, UpdateBlockMessage};
use crate::metrics::Disconnect;
use crate::permissions::{role_of, session_opens};

#[derive(Clone, Copy)]
pub(super) enum Access<'a> {
    Workspace(&'a str),
    Members {
        workspace: &'a str,
        board: &'a str,
        also: Option<&'a str>,
    },
    Public(&'a str),
    All,
}

impl Hub {
    pub(super) async fn send(
        &self,
        ids: &[ListenerId],
        message: &impl Outgoing,
        access: Access<'_>,
    ) {
        if ids.is_empty() {
            return;
        }
        loop {
            // Not cacheable for a while after an abandoned write: the checks below
            // then read the store anew.
            let revision = self.audience.version().await;
            let authorized = self.authorized(ids, access).await;
            #[cfg(feature = "test-util")]
            self.pause_dispatch().await;
            if self
                .audience
                .if_current(revision, || self.queue(&authorized, message))
            {
                return;
            }
        }
    }

    pub(super) async fn send_public_block(
        &self,
        ids: &[ListenerId],
        workspace_id: &str,
        block: &Block,
    ) {
        if ids.is_empty() {
            return;
        }
        loop {
            // Not cacheable for a while after an abandoned write: the checks below
            // then read the store anew.
            let revision = self.audience.version().await;
            let authorized = self.authorized(ids, Access::Public(&block.board_id)).await;
            if authorized.is_empty() {
                return;
            }
            let people = match self.audience.person_properties(&block.board_id).await {
                Ok(people) => people,
                Err(error) => {
                    tracing::error!(%error, board_id = %block.board_id, "websocket: cannot redact public block");
                    self.close_unread(&authorized);
                    return;
                }
            };
            let mut public = block.clone();
            public.hide_people(&people);
            #[cfg(feature = "test-util")]
            self.pause_dispatch().await;
            if self.audience.if_current(revision, || {
                self.queue(
                    &authorized,
                    &UpdateBlockMessage {
                        action: "UPDATE_BLOCK",
                        workspace_id,
                        block: &public,
                    },
                );
            }) {
                return;
            }
        }
    }

    /// Closes connections that a change could not be checked for, as retryable
    /// (`StoreUnavailable`): a page connects again and reads anew, rather than miss
    /// the change unknowingly. Their access did not end.
    pub(super) fn close_unread(&self, ids: &[ListenerId]) {
        for id in ids {
            self.close_listener(*id, Disconnect::StoreUnavailable);
        }
    }

    async fn authorized(&self, ids: &[ListenerId], access: Access<'_>) -> Vec<ListenerId> {
        let members = match access {
            Access::Members { board, .. } => match self.audience.members(board).await {
                Ok(members) => members,
                Err(error) => {
                    tracing::error!(%error, board_id = board, "websocket: cannot read dispatch membership");
                    self.close_unread(ids);
                    return Vec::new();
                }
            },
            _ => Vec::new(),
        };
        let mut authorized = Vec::new();
        for id in ids {
            if self.authorizes(*id, access, &members).await {
                authorized.push(*id);
            }
        }
        authorized
    }

    async fn authorizes(
        &self,
        id: ListenerId,
        access: Access<'_>,
        members: &[BoardMember],
    ) -> bool {
        let identity = {
            let listeners = self.listeners();
            let Some(listener) = listeners.by_id.get(&id) else {
                return false;
            };
            match access {
                Access::All => return true,
                Access::Public(board) => listener
                    .shared_boards
                    .iter()
                    .find(|permit| permit.board == board)
                    .map(|permit| (String::new(), None, Some(permit.clone()), None)),
                Access::Workspace(workspace) | Access::Members { workspace, .. } => {
                    listener.workspace_roles.get(workspace).map(|role| {
                        let login = listener.login_provider.clone();
                        (listener.user_id.clone(), Some(*role), None, login)
                    })
                }
            }
        };
        let Some((user, subscribed_role, permit, login_provider)) = identity else {
            return false;
        };
        if let Some(permit) = permit {
            return match self
                .audience
                .shared_board(&permit.board, &permit.token)
                .await
            {
                Ok(true) => true,
                Ok(false) => {
                    self.close_listener(id, Disconnect::ShareEnded);
                    false
                }
                // The link may still open: not "ended", but not to miss the change.
                Err(error) => {
                    tracing::error!(%error, board_id = %permit.board, "websocket: reading a board's sharing failed");
                    self.close_unread(&[id]);
                    false
                }
            };
        }
        let workspace = match access {
            Access::Workspace(workspace) | Access::Members { workspace, .. } => workspace,
            Access::All | Access::Public(_) => return false,
        };
        let current = match self.audience.workspace_access(&user, workspace).await {
            Ok(Some(current)) => current,
            Ok(None) => {
                self.close_listener(id, Disconnect::AccessEnded);
                return false;
            }
            // Access that cannot be read has not ended, but the change is not to be
            // missed either.
            Err(error) => {
                tracing::error!(%error, workspace_id = workspace, "websocket: reading a workspace's membership failed");
                self.close_unread(&[id]);
                return false;
            }
        };
        if Some(current.workspace_role) != subscribed_role {
            self.close_listener(id, Disconnect::AccessEnded);
            return false;
        }
        if !session_opens(
            current.required_provider.as_deref(),
            login_provider.as_deref(),
        ) {
            self.close_listener(id, Disconnect::SsoRequired);
            return false;
        }
        match access {
            Access::Members { also, .. } => {
                also == Some(user.as_str())
                    || members
                        .iter()
                        .any(|member| member.user_id == user && role_of(member).is_some())
            }
            _ => true,
        }
    }
}
