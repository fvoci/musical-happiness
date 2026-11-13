//! The authority behind a board visit, and the people its reader may identify.

use fvoci_model::{BlockType, WorkspaceRole};

use super::{App, Asker};
use crate::error::AppError;

/// A visit checked behind its organization's wall. A token reader has no named role.
#[derive(Debug)]
pub(crate) struct PresenceAccess {
    pub(crate) app: App,
    pub(crate) workspace_id: String,
    pub(crate) role: Option<WorkspaceRole>,
}

impl App {
    /// Checks a visit with the same board authority as HTTP. A supplied read token
    /// always means an anonymous visit, even on a signed-in connection.
    pub(crate) async fn presence_access(
        &self,
        asker: Asker<'_>,
        workspace_id: &str,
        board_id: &str,
        card_id: &str,
        read_token: &str,
    ) -> Result<Option<PresenceAccess>, AppError> {
        let (app, role) = if read_token.is_empty() {
            let Some(entered) = self.enter_workspace_as(asker, workspace_id).await? else {
                return Ok(None);
            };
            (entered.app.in_workspace(workspace_id), Some(entered.role))
        } else {
            let Some(app) = self.enter_shared_board(board_id, read_token).await? else {
                return Ok(None);
            };
            (app, None)
        };
        let Some(access) = app.board_access(asker.user_id, board_id).await? else {
            return Ok(None);
        };
        if access.board.delete_at != 0
            || (role.is_some() && !access.grant.reads())
            || (!workspace_id.is_empty() && workspace_id != access.board.workspace_id)
        {
            return Ok(None);
        }
        if !card_id.is_empty() {
            let card = match app.block(card_id).await {
                Ok(card) => card,
                Err(error) if error.kind() == crate::error::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            if card.board_id != board_id
                || card.block_type != BlockType::Card
                || card.delete_at != 0
            {
                return Ok(None);
            }
        }
        Ok(Some(PresenceAccess {
            app,
            workspace_id: access.board.workspace_id,
            role,
        }))
    }
}
