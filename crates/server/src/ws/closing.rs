//! Ending what connections hear: closing them when what let them in is over, and
//! stopping them following a workspace their user is no member of any more, or one in
//! the trash.

use std::collections::HashMap;

use crate::metrics::Disconnect;

use super::{Hub, Listener, ListenerId, Listeners};

/// Counts of what was taken back, kept so that a subscription checked meanwhile is
/// checked again when, and only when, something that concerns it was: another user's
/// logout does not overtake it.
#[derive(Debug, Default)]
pub(super) struct Revocations {
    /// What may end anybody's access: an organization beginning to take no password.
    everyone: u64,
    /// What ended a user's access somewhere, or closed a connection of theirs.
    users: HashMap<String, u64>,
    /// A workspace that went to the trash.
    workspaces: HashMap<String, u64>,
    /// A board's read token that stopped opening it.
    links: u64,
}

/// What a check saw of [`Revocations`]: equal again as it registers, nothing that
/// concerns it was taken back meanwhile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Seen(u64, u64, u64);

impl Revocations {
    /// What concerns `user_id` following `workspace_id`.
    pub(super) fn of_workspace(&self, user_id: &str, workspace_id: &str) -> Seen {
        Seen(
            self.everyone,
            self.users.get(user_id).copied().unwrap_or_default(),
            self.workspaces
                .get(workspace_id)
                .copied()
                .unwrap_or_default(),
        )
    }

    /// What concerns a connection of `user_id` (empty without a session) following
    /// blocks by a read token.
    pub(super) fn of_links(&self, user_id: &str) -> Seen {
        Seen(
            self.everyone,
            self.users.get(user_id).copied().unwrap_or_default(),
            self.links,
        )
    }

    pub(super) fn everyone(&mut self) {
        self.everyone = self.everyone.wrapping_add(1);
    }

    fn user(&mut self, user_id: &str) {
        if !user_id.is_empty() {
            let count = self.users.entry(user_id.to_owned()).or_default();
            *count = count.wrapping_add(1);
        }
    }

    fn workspace(&mut self, workspace_id: &str) {
        let count = self.workspaces.entry(workspace_id.to_owned()).or_default();
        *count = count.wrapping_add(1);
    }

    fn links(&mut self) {
        self.links = self.links.wrapping_add(1);
    }
}

impl Hub {
    /// Closes every connection of the user. For when the user's sessions are ended: a
    /// connection asks for its session once, when it is made, and whoever ends a session
    /// closes its connections.
    pub fn close_user(&self, user_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id
        });
    }

    /// Closes the connections that follow blocks of a board by its read token. For when
    /// that token stops opening the board: sharing is turned off, the board is shared
    /// anew under another token, or the board is deleted. A reader with the new link
    /// connects again.
    pub fn close_readers_of_board(&self, board_id: &str) {
        self.listeners().revoked.links();
        self.close_where(Disconnect::ShareEnded, |listener| {
            listener
                .shared_boards
                .iter()
                .any(|permit| permit.board == board_id)
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
        let mut listeners = self.listeners();
        let ids: Vec<ListenerId> = listeners
            .by_id
            .iter()
            .filter(|(_, listener)| closes(listener))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(listener) = listeners.by_id.get(&id) {
                listener.close(reason);
                let user_id = listener.user_id.clone();
                listeners.revoked.user(&user_id);
            }
            listeners.remove(id);
        }
    }

    /// Stops the user's connections following the workspaces they are no member of
    /// now. For when what they were in a workspace was taken back (an organization's
    /// administrator whose joining ended): following one again asks whether they are in
    /// it.
    pub async fn follow_only_own_workspaces(&self, user_id: &str) {
        let followed: Vec<(ListenerId, String, Option<fvoci_model::WorkspaceRole>)> = {
            let mut listeners = self.listeners();
            listeners.revoked.user(user_id);
            listeners
                .by_id
                .iter()
                .filter(|(_, listener)| listener.user_id == user_id)
                .flat_map(|(id, listener)| {
                    listener
                        .workspace_roles
                        .iter()
                        .map(move |(workspace, role)| (*id, workspace.clone(), Some(*role)))
                        .chain(
                            listener
                                .documents
                                .workspaces()
                                .into_iter()
                                .map(move |(workspace, role)| (*id, workspace, role)),
                        )
                })
                .collect()
        };
        // A connection closed for one workspace is not asked about its others.
        let mut closed = std::collections::HashSet::new();
        for (id, workspace_id, subscribed_role) in followed {
            // A document still checking access has no admitted role to lose. Its
            // role is published under the store's visibility generation, so a
            // concurrent role change makes that admission read again.
            if subscribed_role.is_none() || closed.contains(&id) {
                continue;
            }
            let role = match self
                .audience
                .current_workspace_access(user_id, &workspace_id)
                .await
            {
                Ok(access) => access.map(|access| access.workspace_role),
                // Not known to have ended: closed as retryable, so the page asks
                // again as it connects.
                Err(error) => {
                    tracing::error!(%error, %workspace_id, "websocket: reading a workspace's membership failed");
                    self.close_unread(&[id]);
                    closed.insert(id);
                    continue;
                }
            };
            if role.is_some() && role == subscribed_role {
                continue;
            }
            closed.insert(id);
            let mut listeners = self.listeners();
            if let Some(listener) = listeners.by_id.get(&id) {
                listener.close(Disconnect::AccessEnded);
            }
            listeners.remove(id);
        }
    }

    /// Ends a user's connections that subscribed to the workspace whose role changed.
    pub fn workspace_role_changed(&self, workspace_id: &str, user_id: &str) {
        // Also a subscription of theirs being checked, which follows nothing yet.
        self.listeners().revoked.user(user_id);
        self.close_where(Disconnect::AccessEnded, |listener| {
            listener.user_id == user_id
                && (listener.workspaces.iter().any(|id| id == workspace_id)
                    || listener.documents.contains_workspace(workspace_id))
        });
    }

    /// Stops every connection following the workspace. For when it goes to the trash:
    /// it is there for nobody, and who follows it once it is restored asks again. A
    /// connection that follows it among others keeps the others; one in a document of
    /// it is closed, as its room is no more.
    pub fn stop_following_workspace(&self, workspace_id: &str) {
        self.listeners().stop_following_workspace(workspace_id);
    }
}

impl Listeners {
    /// Takes the workspace out of what every listener follows, and out of the index;
    /// closes the listeners in a document of it.
    pub(super) fn stop_following_workspace(&mut self, workspace_id: &str) {
        self.revoked.workspace(workspace_id);
        let in_documents: Vec<_> = self
            .by_id
            .iter()
            .filter(|(_, listener)| listener.documents.contains_workspace(workspace_id))
            .map(|(id, _)| *id)
            .collect();
        for id in in_documents {
            if let Some(listener) = self.by_id.get(&id) {
                listener.close(Disconnect::AccessEnded);
            }
            self.remove(id);
        }
        for listener in self.by_id.values_mut() {
            listener.workspaces.retain(|id| id != workspace_id);
            listener.workspace_roles.remove(workspace_id);
        }
        self.by_workspace.remove(workspace_id);
    }
}
