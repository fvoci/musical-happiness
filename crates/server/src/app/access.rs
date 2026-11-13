//! What is known of a user and a board: the facts [`permissions::grant`] is asked with.
//!
//! This and `Store::resolve_workspace` (by `App::actor`, and `Store::org_role_of_user` by
//! `App::org_actor` for the organization alone) are where the facts of a permission are
//! read.
//! Every check of a route that names a board asks [`App::board_access`], and decides by
//! the [`Grant`] it gives; nothing else reads a membership to decide what a user may do.

use fvoci_model::{Board, BoardMember, BoardRole, Upload, UploadPart, UploadPurpose};
use fvoci_store::DeletedBoard;

use super::App;
use crate::error::AppError;
use crate::permissions::{self, Actor, BoardFacts, BoardScope, Grant};

/// A board, and what one user is and may do there.
#[derive(Debug, Clone)]
pub struct BoardAccess {
    /// The board as it is stored.
    pub board: Board,
    /// What the user is in the board's workspace and organization.
    pub actor: Actor,
    /// The user's membership of the board as stored; `None` for who is no member.
    pub member: Option<BoardMember>,
    /// The inherited role observed with the direct membership.
    pub team_role: Option<BoardRole>,
    /// What the user may do on the board.
    pub grant: Grant,
}

/// A board in the trash, and what one user may do with it: what the role they had
/// when it was deleted would let them do, as what they are in its workspace now.
#[derive(Debug, Clone)]
pub struct DeletedBoardAccess {
    /// The board as the trash keeps it.
    pub deleted: DeletedBoard,
    /// What the user may do with the board.
    pub grant: Grant,
}

impl App {
    /// The board and what the user may do on it; `None` if there is no such board, and
    /// for a handle of one workspace ([`App::in_workspace`]) if the board is of another:
    /// this is where a request's workspace and the workspace of what it names are
    /// compared. A user without an identifier may do nothing.
    ///
    /// The same fact reads are made whether the board is there or not, so that a
    /// board that is not there for the user takes as long to refuse as one that does
    /// not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read: that is no "no", which would
    /// tell every member that their board does not exist.
    pub async fn board_access(
        &self,
        user_id: &str,
        board_id: &str,
    ) -> Result<Option<BoardAccess>, AppError> {
        let (board, member, team_role) = tokio::join!(
            self.store().get_board(board_id),
            self.store().get_member_for_board(board_id, user_id),
            self.store().board_team_role(board_id, user_id),
        );
        let board = match board {
            Ok(board) => Some(board).filter(|board| self.reaches_board(board)),
            Err(error) if error.is_not_found() => None,
            Err(error) => {
                return Err(AppError::from(error).context("reading a board to check access"));
            }
        };
        let member = match member {
            Ok(member) => Some(member),
            Err(error) if error.is_not_found() => None,
            Err(error) => {
                return Err(AppError::from(error).context("reading the membership of a board"));
            }
        };
        let team_role = team_role
            .map_err(|error| AppError::from(error).context("reading the team roles on a board"))?;
        let workspace_id = board.as_ref().map_or("", |board| &board.workspace_id);
        let actor = self.actor(user_id, workspace_id).await?;
        let Some(board) = board else {
            return Ok(None);
        };
        let grant = if user_id.is_empty() {
            Grant::NONE
        } else {
            permissions::grant(
                actor,
                BoardFacts {
                    team_role,
                    scope: BoardScope::of(&board),
                    role: member.as_ref().and_then(permissions::role_of),
                },
            )
        };
        Ok(Some(BoardAccess {
            board,
            actor,
            member,
            team_role,
            grant,
        }))
    }

    /// The user's upload that is still going and the parts it has received, as
    /// [`App::upload`] gives them, if it is of this handle's workspace: the workspace an
    /// archive is imported into, or the one of the board a file is for, in the trash too.
    /// An upload for a board that is gone altogether is of no workspace, and its owner
    /// ends it under any. For a handle of every workspace, any of the user's uploads.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for an upload of another workspace and whatever
    /// [`App::upload`] returns.
    pub async fn workspace_upload(
        &self,
        upload_id: &str,
        user_id: &str,
    ) -> Result<(Upload, Vec<UploadPart>), AppError> {
        let (upload, parts) = self.upload(upload_id, user_id).await?;
        if self.workspace.is_none() {
            return Ok((upload, parts));
        }
        let reached = match upload.purpose {
            UploadPurpose::Import => self.reaches_workspace(&upload.workspace_id),
            UploadPurpose::File => match self.store().get_board(&upload.board_id).await {
                Ok(board) => self.reaches_board(&board),
                Err(error) if error.is_not_found() => self
                    .deleted_board(&upload.board_id)
                    .await?
                    .is_none_or(|deleted| self.reaches_board(&deleted.board)),
                Err(error) => return Err(error.into()),
            },
        };
        if reached {
            Ok((upload, parts))
        } else {
            Err(AppError::not_found("upload"))
        }
    }

    /// What the user may do on the board; nothing if there is no such board.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read.
    pub async fn board_grant(&self, user_id: &str, board_id: &str) -> Result<Grant, AppError> {
        Ok(self
            .board_access(user_id, board_id)
            .await?
            .map_or(Grant::NONE, |access| access.grant))
    }

    /// Whether the user holds a role on the board as its member, directly: what a
    /// guest needs to read a template (a template is a board, and a guest reads the
    /// boards they were invited to).
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn invited_to_board(&self, user_id: &str, board_id: &str) -> Result<bool, AppError> {
        match self.store().get_member_for_board(board_id, user_id).await {
            Ok(member) => Ok(permissions::role_of(&member).is_some()),
            Err(error) if error.is_not_found() => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    /// A board in the trash and what the user may do with it; `None` if no board of
    /// that identifier is there, or for a handle of one workspace if it is of another.
    /// The role is the one the trash kept of the user, and counts as what the user is in
    /// the board's workspace now: who has left the
    /// workspace since brings nothing back. A board in the trash is open to nobody
    /// (`BoardScope::in_the_trash`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read.
    pub async fn deleted_board_access(
        &self,
        user_id: &str,
        board_id: &str,
    ) -> Result<Option<DeletedBoardAccess>, AppError> {
        let Some(deleted) = self
            .deleted_board(board_id)
            .await?
            .filter(|deleted| self.reaches_board(&deleted.board))
        else {
            return Ok(None);
        };
        let actor = self.actor(user_id, &deleted.board.workspace_id).await?;
        let role = deleted
            .members
            .iter()
            .find(|member| member.user_id == user_id)
            .and_then(permissions::role_of);
        let grant = if user_id.is_empty() {
            Grant::NONE
        } else {
            permissions::grant(
                actor,
                BoardFacts {
                    team_role: None,
                    scope: BoardScope::of(&deleted.board).in_the_trash(),
                    role,
                },
            )
        };
        Ok(Some(DeletedBoardAccess { deleted, grant }))
    }

    /// What the user is in the organization alone, of no workspace of it, as
    /// `Store::org_role_of_user` answers: nothing for who is no member of it, and for an
    /// organization that is not in use or is the system's.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn org_actor(&self, user_id: &str, org_id: &str) -> Result<Actor, AppError> {
        Ok(Actor {
            org: self.store().org_role_of_user(user_id, org_id).await?,
            workspace: None,
        })
    }

    /// What the user is in the workspace and its organization, as
    /// `Store::resolve_workspace` answers: nothing for who is no member of it, and for
    /// a workspace that is not there or not in use.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read.
    pub async fn actor(&self, user_id: &str, workspace_id: &str) -> Result<Actor, AppError> {
        let access = self
            .store()
            .resolve_workspace(user_id, workspace_id)
            .await?;
        Ok(access.map_or(Actor::default(), |access| Actor {
            org: access.org_role,
            workspace: Some(access.workspace_role),
        }))
    }
}

/// The board as who only manages it sees it ([`Grant::manages_only`]): its name, kind
/// and times, without its description and properties, which are its content.
#[must_use]
pub fn as_managed(mut board: Board) -> Board {
    board.description.clear();
    board.properties.clear();
    board.card_properties.clear();
    board
}
