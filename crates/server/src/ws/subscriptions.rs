//! Validating and registering subscriptions under the same revocation generation.

use fvoci_model::WorkspaceRole;

use super::{
    Command, Hub, ListenerId, ReadPermit, remove_from, subscribe_to_blocks, subscribe_to_workspace,
    unsubscribe_from_blocks,
};
use crate::metrics::Disconnect;

impl Hub {
    /// Handles one command. Returns `false` if the connection must be closed.
    pub(super) async fn handle(&self, id: ListenerId, command: Command) -> bool {
        match command.action.as_str() {
            "SUBSCRIBE_BLOCKS" => {
                self.subscribe_blocks(id, &command).await;
                return true;
            }
            "UNSUBSCRIBE_BLOCKS" => {
                if self.board_of_read_token(&command).await.is_some() {
                    unsubscribe_from_blocks(&mut self.listeners(), id, &command.block_ids);
                }
                return true;
            }
            _ => {}
        }
        let user_id = self
            .listeners()
            .by_id
            .get(&id)
            .map(|listener| listener.user_id.clone())
            .unwrap_or_default();
        if user_id.is_empty() {
            tracing::error!(action = %command.action, "websocket: rejected an unauthenticated message");
            return true;
        }
        match command.action.as_str() {
            "SUBSCRIBE_WORKSPACE" => {
                self.subscribe_workspace(id, &user_id, &command.workspace_id)
                    .await;
            }
            "UNSUBSCRIBE_WORKSPACE" => {
                let mut listeners = self.listeners();
                if let Some(listener) = listeners.by_id.get_mut(&id) {
                    listener
                        .workspaces
                        .retain(|workspace| *workspace != command.workspace_id);
                    listener.workspace_roles.remove(&command.workspace_id);
                }
                remove_from(&mut listeners.by_workspace, &command.workspace_id, id);
            }
            other => tracing::error!(action = other, "websocket: invalid action"),
        }
        true
    }

    async fn subscribe_blocks(&self, id: ListenerId, command: &Command) {
        let mut validated = false;
        loop {
            let revision = self.listeners().revision;
            let Some(board_id) = self.board_of_read_token(command).await else {
                if validated {
                    self.close_listener(id, Disconnect::ShareEnded);
                }
                return;
            };
            validated = true;
            #[cfg(feature = "test-util")]
            self.pause_subscription().await;
            let mut listeners = self.listeners();
            // Unrelated revocations only trigger another check; a still-valid
            // subscription must not be closed because another user's access changed.
            if listeners.revision != revision {
                continue;
            }
            subscribe_to_blocks(&mut listeners, id, &command.block_ids);
            if let Some(listener) = listeners.by_id.get_mut(&id)
                && !listener
                    .shared_boards
                    .iter()
                    .any(|permit| permit.board == board_id)
            {
                listener.shared_boards.push(ReadPermit {
                    board: board_id,
                    token: command.read_token.clone(),
                });
            }
            return;
        }
    }

    async fn subscribe_workspace(&self, id: ListenerId, user_id: &str, workspace: &str) {
        let mut validated_role = None;
        loop {
            let revision = self.listeners().revision;
            let role = match self.opened_role(id, user_id, workspace).await {
                Ok(role) => role,
                Err(ended) => {
                    if validated_role.is_some() {
                        self.close_listener(id, ended);
                    }
                    return;
                }
            };
            if validated_role.is_some_and(|previous| previous != role) {
                self.close_listener(id, Disconnect::AccessEnded);
                return;
            }
            validated_role = Some(role);
            #[cfg(feature = "test-util")]
            self.pause_subscription().await;
            let mut listeners = self.listeners();
            if listeners.revision != revision {
                continue;
            }
            subscribe_to_workspace(&mut listeners, id, workspace);
            if let Some(listener) = listeners.by_id.get_mut(&id) {
                listener.workspace_roles.insert(workspace.to_owned(), role);
            }
            return;
        }
    }

    /// What the connection's user is in the workspace, if they are a member of it and
    /// the connection's session opens the workspace's organization. Otherwise why a
    /// subscription checked before would end: `AccessEnded` for who is no member (and
    /// a connection that is gone), `SsoRequired` for a session the organization does
    /// not take.
    pub(super) async fn opened_role(
        &self,
        id: ListenerId,
        user_id: &str,
        workspace: &str,
    ) -> Result<WorkspaceRole, Disconnect> {
        let access = self
            .audience
            .workspace_access(user_id, workspace)
            .await
            .ok_or(Disconnect::AccessEnded)?;
        let listeners = self.listeners();
        let listener = listeners.by_id.get(&id).ok_or(Disconnect::AccessEnded)?;
        if listener.session_opens(access.required_provider.as_deref()) {
            Ok(access.workspace_role)
        } else {
            Err(Disconnect::SsoRequired)
        }
    }

    /// A block subscription needs a workspace, blocks that all belong to one board, and that
    /// board's read token. Returns the board, or `None` if the subscription is refused.
    async fn board_of_read_token(&self, command: &Command) -> Option<String> {
        if command.workspace_id.is_empty() {
            return None;
        }
        let mut board_id = String::new();
        for block_id in &command.block_ids {
            let of_block = self
                .audience
                .board_of_shared_block(&command.workspace_id, block_id, &command.read_token)
                .await
                .ok()??;
            if board_id.is_empty() {
                board_id = of_block;
            } else if board_id != of_block {
                return None;
            }
        }
        match self
            .inner
            .auth
            .is_valid_read_token(&board_id, &command.read_token)
            .await
        {
            Ok(true) => Some(board_id),
            Ok(false) => None,
            Err(error) => {
                tracing::error!(%error, workspace_id = %command.workspace_id, "websocket: checking a read token failed");
                None
            }
        }
    }
}
