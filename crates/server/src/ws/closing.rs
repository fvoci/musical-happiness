//! Ending what connections hear: closing them when what let them in is over, and
//! stopping them following a workspace their user is no member of any more, or one in
//! the trash.

use crate::metrics::Disconnect;

use super::{Hub, Listener, ListenerId, Listeners, remove_from};

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
        self.close_where(Disconnect::ShareEnded, |listener| {
            listener.shared_boards.iter().any(|board| board == board_id)
        });
    }

    /// Closes the connections of a session. For when the session ends: at logout, and
    /// when it has gone unused for too long or reached its end.
    pub fn close_session(&self, session_id: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.session_id == session_id
        });
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

    /// Closes the connections of the user's sessions made through a provider of
    /// `issuer`, in any organization, and their seats in the rooms. For when the user
    /// unlinked their identity of that issuer and the store ended those sessions.
    pub fn close_sessions_of_issuer(&self, user_id: &str, issuer: &str) {
        self.close_where(Disconnect::SessionEnded, |listener| {
            listener.user_id == user_id
                && listener
                    .login
                    .as_ref()
                    .is_some_and(|login| login.issuer == issuer)
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

    /// Closes the connections `closes` picks, for `reason`.
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
            }
            listeners.remove(id);
        }
    }

    /// Stops the user's connections following the workspaces they are no member of
    /// now. For when what they were in a workspace was taken back (an organization's
    /// administrator whose joining ended): following one again asks whether they are in
    /// it.
    pub async fn follow_only_own_workspaces(&self, user_id: &str) {
        let followed: Vec<(ListenerId, String)> = {
            let listeners = self.listeners();
            listeners
                .by_id
                .iter()
                .filter(|(_, listener)| listener.user_id == user_id)
                .flat_map(|(id, listener)| {
                    listener
                        .workspaces
                        .iter()
                        .map(move |workspace| (*id, workspace.clone()))
                })
                .collect()
        };
        for (id, workspace_id) in followed {
            let still_in_it = self
                .audience
                .workspace_access(user_id, &workspace_id)
                .await
                .is_some();
            if still_in_it {
                continue;
            }
            let mut listeners = self.listeners();
            if let Some(listener) = listeners.by_id.get_mut(&id) {
                listener
                    .workspaces
                    .retain(|workspace| *workspace != workspace_id);
            }
            remove_from(&mut listeners.by_workspace, &workspace_id, id);
        }
    }

    /// Stops every connection following the workspace. For when it goes to the trash:
    /// it is there for nobody, and who follows it once it is restored asks again.
    pub fn stop_following_workspace(&self, workspace_id: &str) {
        self.listeners().stop_following_workspace(workspace_id);
    }
}

impl Listeners {
    /// Takes the workspace out of what every listener follows, and out of the index.
    pub(super) fn stop_following_workspace(&mut self, workspace_id: &str) {
        let Some(ids) = self.by_workspace.remove(workspace_id) else {
            return;
        };
        for id in ids {
            if let Some(listener) = self.by_id.get_mut(&id) {
                listener
                    .workspaces
                    .retain(|workspace| workspace != workspace_id);
            }
        }
    }
}
