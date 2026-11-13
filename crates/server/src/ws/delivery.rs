//! Authorization at dispatch, backed by the store's committed visibility revision.

use fvoci_model::{Block, BoardMember};

use super::ending::{Ended, Of};
use super::{CHECKED_AT_MOST, Hub, ListenerId, Outgoing, UpdateBlockMessage, text_of};
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
        // Made once, before any revision is held: queueing under it only hands it on.
        let Some(text) = text_of(message) else {
            return;
        };
        let action = message.action();
        // What every connection is told depends on no one's access.
        if matches!(access, Access::All) {
            self.queue_text(ids, &text, action);
            return;
        }
        let mut authorized = Vec::new();
        for _ in 0..CHECKED_AT_MOST {
            // Not cacheable for a while after an abandoned write: the checks below
            // then read the store anew.
            let revision = self.audience.version().await;
            authorized = self.authorized(ids, access).await;
            #[cfg(feature = "test-util")]
            self.pause_dispatch().await;
            if self.audience.if_current(revision, || {
                self.queue_text(&authorized, &text, action);
            }) {
                return;
            }
        }
        // Who was to be told at the last check: not who could never see it.
        self.give_up(&authorized, action);
    }

    /// Tells who follows the block's board by its read token of the block: who sees
    /// the board as its member does ([`Self::split_by_sight`]) as a member is told it,
    /// whoever reads it by the link alone without the users it names.
    pub(super) async fn send_public_block(
        &self,
        ids: &[ListenerId],
        workspace_id: &str,
        block: &Block,
    ) {
        if ids.is_empty() {
            return;
        }
        let Some(whole) = text_of(&UpdateBlockMessage {
            action: "UPDATE_BLOCK",
            workspace_id,
            block,
        }) else {
            return;
        };
        let mut authorized = Vec::new();
        for _ in 0..CHECKED_AT_MOST {
            // Not cacheable for a while after an abandoned write: the checks below
            // then read the store anew.
            let revision = self.audience.version().await;
            authorized = self.authorized(ids, Access::Public(&block.board_id)).await;
            if authorized.is_empty() {
                return;
            }
            let (members, mut readers) = self
                .split_by_sight(&authorized, workspace_id, &block.board_id)
                .await;
            let hidden = if readers.is_empty() {
                None
            } else {
                match self.audience.person_properties(&block.board_id).await {
                    Ok(people) => {
                        let mut public = block.clone();
                        public.hide_people(&people);
                        text_of(&UpdateBlockMessage {
                            action: "UPDATE_BLOCK",
                            workspace_id,
                            block: &public,
                        })
                    }
                    Err(error) => {
                        tracing::error!(%error, board_id = %block.board_id, "websocket: cannot redact public block");
                        self.close_unread(&readers);
                        readers.clear();
                        None
                    }
                }
            };
            #[cfg(feature = "test-util")]
            self.pause_dispatch().await;
            if self.audience.if_current(revision, || {
                self.queue_text(&members, &whole, "UPDATE_BLOCK");
                if let Some(hidden) = &hidden {
                    self.queue_text(&readers, hidden, "UPDATE_BLOCK");
                }
            }) {
                return;
            }
        }
        self.give_up(&authorized, "UPDATE_BLOCK");
    }

    /// Splits who follows the board by its read token into who sees it as its member
    /// does, and who reads it by the link alone. A member is a connection of a user who
    /// holds a role on the board, is in its workspace, and whose session opens the
    /// workspace's organization; anybody else, a connection without a session, and one
    /// whose standing cannot be read now, reads by the link.
    async fn split_by_sight(
        &self,
        ids: &[ListenerId],
        workspace_id: &str,
        board_id: &str,
    ) -> (Vec<ListenerId>, Vec<ListenerId>) {
        let identities: Vec<(ListenerId, String, Option<String>)> = {
            let listeners = self.listeners();
            ids.iter()
                .filter_map(|id| {
                    let listener = listeners.by_id.get(id)?;
                    Some((
                        *id,
                        listener.user_id.clone(),
                        listener.login_provider.clone(),
                    ))
                })
                .collect()
        };
        let (mut members, mut readers) = (Vec::new(), Vec::new());
        if identities.iter().all(|(_, user, _)| user.is_empty()) {
            readers.extend(identities.into_iter().map(|(id, _, _)| id));
            return (members, readers);
        }
        let board_members = self.audience.members(board_id).await.unwrap_or_default();
        for (id, user, login_provider) in identities {
            let on_board = !user.is_empty()
                && board_members
                    .iter()
                    .any(|member| member.user_id == user && role_of(member).is_some());
            let sees = on_board
                && match self.audience.workspace_access(&user, workspace_id).await {
                    Ok(Some(access)) => session_opens(
                        access.required_provider.as_deref(),
                        login_provider.as_deref(),
                    ),
                    Ok(None) | Err(_) => false,
                };
            if sees {
                members.push(id);
            } else {
                readers.push(id);
            }
        }
        (members, readers)
    }

    /// Closes the recipients of a change whose check was overtaken by permission
    /// writes every time, as retryable: they read anew rather than miss it.
    fn give_up(&self, ids: &[ListenerId], action: &str) {
        tracing::warn!(
            action,
            recipients = ids.len(),
            "websocket: a change was given up, checked again too often"
        );
        for id in ids {
            self.close_listener(*id, Disconnect::ChecksOvertaken);
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
                    self.end_board(id, &permit.board);
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
        // What it followed when this was read: a subscription made since is not ended
        // for what this finds.
        let checked = subscribed_role.map_or(Of::Any, Of::Role);
        let current = match self.audience.workspace_access(&user, workspace).await {
            Ok(Some(current)) => current,
            Ok(None) => {
                self.end_workspace(id, workspace, Ended::Access, checked);
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
            self.end_workspace(id, workspace, Ended::Access, checked);
            return false;
        }
        if !session_opens(
            current.required_provider.as_deref(),
            login_provider.as_deref(),
        ) {
            self.end_workspace(id, workspace, Ended::Sso, checked);
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
