//! Boards and their members.

use fvoci_model::{
    Board, BoardMember, BoardPatch, BoardType, BoardsAndBlocks, DeleteBoardsAndBlocks, IdKind,
    new_id,
};
use fvoci_store::{DuplicateAs, HistoryOrder, HistoryQuery, PublicBoards};

use super::{App, DEFAULT_CATEGORY_BOARDS};
use crate::error::AppError;

/// The message of the refusal to remove or demote a board's only admin.
const LAST_ADMIN: &str = "cannot leave a board with no admins";

/// Whether the user who creates a board becomes a member of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreatorMembership {
    /// The creator becomes an admin and editor of the board.
    Admin,
    /// The board starts without members.
    None,
}

impl App {
    /// Returns a board.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, or the store's error.
    pub async fn board(&self, board_id: &str) -> Result<Board, AppError> {
        Ok(self.store().get_board(board_id).await?)
    }

    /// Returns the boards of a team that a user is a member of and, if asked for, the
    /// team's open boards. Templates are left out.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn boards_for_user_and_team(
        &self,
        user_id: &str,
        team_id: &str,
        public_boards: PublicBoards,
    ) -> Result<Vec<Board>, AppError> {
        Ok(self
            .store()
            .get_boards_for_user_and_team(user_id, team_id, public_boards)
            .await?)
    }

    /// Creates a board without members and files it under the default category of
    /// `user_id`, unless it is a template. The board gets a new identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the board already has an identifier, if it cannot be stored,
    /// or if it cannot be filed under the user's default category; in the last case the
    /// board exists nonetheless.
    pub async fn create_board(&self, board: Board, user_id: &str) -> Result<Board, AppError> {
        self.create_board_as(board, user_id, CreatorMembership::None)
            .await
    }

    /// Creates a board as [`App::create_board`] does and makes `user_id` an admin and
    /// editor of it.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::create_board`].
    pub async fn create_board_with_admin(
        &self,
        board: Board,
        user_id: &str,
    ) -> Result<Board, AppError> {
        self.create_board_as(board, user_id, CreatorMembership::Admin)
            .await
    }

    async fn create_board_as(
        &self,
        mut board: Board,
        user_id: &str,
        membership: CreatorMembership,
    ) -> Result<Board, AppError> {
        if !board.id.is_empty() {
            // Preserved quirk: the original does not classify this error, so a client
            // that names an identifier gets a 500 (`server/app/boards.go:234-236`).
            return Err(AppError::internal("new board cannot have an ID"));
        }
        board.id = new_id(IdKind::Board);

        let creator = match membership {
            CreatorMembership::Admin => Some(
                self.store()
                    .insert_board_with_admin(&mut board, user_id)
                    .await?,
            ),
            CreatorMembership::None => {
                self.store().insert_board(&mut board, user_id).await?;
                None
            }
        };

        let app = self.clone();
        let created = board.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&created.team_id, &created)
                .await;
            if created.channel_id.is_empty() {
                if let Some(creator) = creator {
                    app.ws()
                        .broadcast_member_change(&created.team_id, &created.id, &creator)
                        .await;
                }
            } else {
                // A board that is linked to a channel announces all its members.
                app.broadcast_members(&created).await;
            }
        });

        if !board.is_template {
            self.add_boards_to_default_category(
                user_id,
                &board.team_id,
                std::slice::from_ref(&board.id),
            )
            .await?;
        }
        Ok(board)
    }

    /// Tells the members of a board about every membership of it.
    async fn broadcast_members(&self, board: &Board) {
        match self.members_for_board(&board.id).await {
            Ok(members) => {
                for member in &members {
                    self.ws()
                        .broadcast_member_change(&board.team_id, &member.board_id, member)
                        .await;
                }
            }
            Err(error) => tracing::error!(%error, "Unable to get the board members"),
        }
    }

    /// Files boards under a user's default category in a team, creating that category
    /// if the user does not have it yet.
    pub(super) async fn add_boards_to_default_category(
        &self,
        user_id: &str,
        team_id: &str,
        board_ids: &[String],
    ) -> Result<(), AppError> {
        let categories = self.user_category_boards(user_id, team_id).await?;
        let Some(default) = categories
            .iter()
            .find(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
        else {
            return Err(AppError::internal(format_args!(
                "no default category found for user userID: {user_id}"
            )));
        };
        self.add_update_user_category_board(team_id, user_id, &default.category.id, board_ids)
            .await
    }

    /// Applies a patch to a board on behalf of a user and tells the board's members.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, a permission error if the
    /// patch concerns a channel and names no user, or the store's error.
    pub async fn patch_board(
        &self,
        patch: &BoardPatch,
        board_id: &str,
        user_id: &str,
    ) -> Result<Board, AppError> {
        let mut is_template = false;
        if patch.board_type.is_some() || patch.channel_id.is_some() {
            let board = match self.store().get_board(board_id).await {
                Ok(board) => board,
                Err(error) if error.is_not_found() => {
                    return Err(AppError::not_found(format_args!("board ID={board_id}")));
                }
                Err(error) => return Err(error.into()),
            };
            is_template = board.is_template;

            // Linking, unlinking and retyping a board that has a channel need the
            // permission to post to that channel, which on a standalone server everybody
            // has who is somebody.
            let channel_id = match patch.channel_id.as_deref() {
                Some(new_channel) if !new_channel.is_empty() => new_channel,
                _ => board.channel_id.as_str(),
            };
            if !channel_id.is_empty() && user_id.is_empty() {
                return Err(AppError::forbidden("access denied to channel"));
            }
        }

        let updated = self.store().patch_board(board_id, patch, user_id).await?;

        // The original posts a message to the channel when a board is linked to or
        // unlinked from it, which a standalone server cannot do, and announces the
        // channel's members as members of the board, of which there are none.

        let app = self.clone();
        let board = updated.clone();
        let new_type = patch.board_type.filter(|_| is_template);
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&board.team_id, &board)
                .await;
            if let Some(board_type) = new_type {
                app.broadcast_team_users(&board, board_type).await;
            }
        });

        Ok(updated)
    }

    /// Tells the users of a team who are not members of a template that they gained or
    /// lost access to it, after the template changed between open and private.
    async fn broadcast_team_users(&self, board: &Board, board_type: BoardType) {
        let members = match self.members_for_board(&board.id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, "Unable to get the board members");
                Vec::new()
            }
        };
        let users = match self.store().get_users_by_team().await {
            Ok(users) => users,
            Err(error) => {
                tracing::error!(%error, "Unable to get the team users");
                Vec::new()
            }
        };
        let others = users
            .iter()
            .filter(|user| !members.iter().any(|member| member.user_id == user.id));
        for user in others {
            match board_type {
                BoardType::Private => {
                    self.ws()
                        .broadcast_member_delete(&board.team_id, &board.id, &user.id)
                        .await;
                }
                BoardType::Open => {
                    let viewer = BoardMember {
                        user_id: user.id.clone(),
                        board_id: board.id.clone(),
                        scheme_viewer: true,
                        synthetic: true,
                        ..BoardMember::default()
                    };
                    self.ws()
                        .broadcast_member_change(&board.team_id, &board.id, &viewer)
                        .await;
                }
            }
        }
    }

    /// Deletes a board with its blocks and tells the board's members. Deleting a board
    /// that does not exist succeeds.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board cannot be read or deleted.
    pub async fn delete_board(&self, board_id: &str, user_id: &str) -> Result<(), AppError> {
        let board = match self.store().get_board(board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        self.store().delete_board(board_id, user_id).await?;

        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_delete(&board.team_id, &board.id)
                .await;
        });
        Ok(())
    }

    /// Restores a deleted board with its blocks and tells the board's members.
    /// Undeleting a board that never existed succeeds.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board's history cannot be read or the board
    /// cannot be restored.
    pub async fn undelete_board(&self, board_id: &str, modified_by: &str) -> Result<(), AppError> {
        let newest = HistoryQuery {
            limit: Some(1),
            order: HistoryOrder::Descending,
            ..HistoryQuery::default()
        };
        if self
            .store()
            .get_board_history(board_id, &newest)
            .await?
            .is_empty()
        {
            return Ok(());
        }

        self.store().undelete_board(board_id, modified_by).await?;
        let board = self.store().get_board(board_id).await?;

        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&board.team_id, &board)
                .await;
        });
        Ok(())
    }

    /// Copies a board and its blocks, except comments, into `to_team` for a user, who
    /// becomes the copy's admin. Returns the copies and that membership.
    ///
    /// A copy that is not a template is filed under the category the user has filed the
    /// source under, or else under the default category.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, an error if the board has no
    /// blocks to copy or its files cannot be copied, or the store's error.
    pub async fn duplicate_board(
        &self,
        board_id: &str,
        user_id: &str,
        to_team: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<(BoardsAndBlocks, Vec<BoardMember>), AppError> {
        let (copies, members) = self
            .store()
            .duplicate_board(
                board_id,
                user_id,
                Some(to_team).filter(|team| !team.is_empty()),
                duplicate_as,
            )
            .await?;

        // The copied blocks get files of their own.
        if let Err(error) = self
            .copy_and_update_card_files(board_id, user_id, &copies.blocks, duplicate_as)
            .await
        {
            let delete = DeleteBoardsAndBlocks::from(&copies);
            if let Err(delete_error) = self
                .store()
                .delete_boards_and_blocks(&delete, user_id)
                .await
            {
                tracing::error!(
                    board_id,
                    error = %delete_error,
                    "Cannot delete board after duplication error when updating block's file info"
                );
            }
            return Err(error.context(&format!(
                "could not patch file IDs while duplicating board {board_id}"
            )));
        }

        if duplicate_as == DuplicateAs::Copy {
            for board in &copies.boards {
                self.set_board_category_from_source(board_id, &board.id, user_id, to_team)
                    .await?;
            }
        }

        let app = self.clone();
        let broadcast = copies.clone();
        let new_members = members.clone();
        self.spawn_background(async move {
            let mut team_id = "";
            for board in &broadcast.boards {
                team_id = &board.team_id;
                app.ws().broadcast_board_change(team_id, board).await;
            }
            for block in &broadcast.blocks {
                app.ws().broadcast_block_change(team_id, block).await;
            }
            for member in &new_members {
                app.ws()
                    .broadcast_member_change(team_id, &member.board_id, member)
                    .await;
            }
        });

        Ok((copies, members))
    }

    /// Files a copy of a board under the category its source is filed under for the
    /// user in the team, or else under the default category.
    async fn set_board_category_from_source(
        &self,
        source_board_id: &str,
        destination_board_id: &str,
        user_id: &str,
        team_id: &str,
    ) -> Result<(), AppError> {
        let destination = [destination_board_id.to_owned()];
        let categories = self.user_category_boards(user_id, team_id).await?;
        let source_category = categories.iter().find(|entry| {
            entry
                .board_metadata
                .iter()
                .any(|metadata| metadata.board_id == source_board_id)
        });
        match source_category {
            Some(entry) => {
                self.add_update_user_category_board(
                    team_id,
                    user_id,
                    &entry.category.id,
                    &destination,
                )
                .await
            }
            None => {
                self.add_boards_to_default_category(user_id, team_id, &destination)
                    .await
            }
        }
    }

    /// Returns the members of a board.
    ///
    /// The original also reports the team's administrators as admins of the board; a
    /// standalone server has none.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn members_for_board(&self, board_id: &str) -> Result<Vec<BoardMember>, AppError> {
        Ok(self.store().get_members_for_board(board_id).await?)
    }

    /// Makes a user a member of a board with the roles of `member`, files the board
    /// under the user's default category unless it is a template, and tells the board's
    /// members.
    ///
    /// A user who is a member already stays as they are, and that membership is
    /// returned. `None` is returned if the board does not exist.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the membership cannot be read or written, or an
    /// error if the board cannot be filed under the user's default category; in that
    /// case the membership exists nonetheless.
    pub async fn add_member_to_board(
        &self,
        member: &BoardMember,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        let existing = self
            .find_member_for_board(&member.board_id, &member.user_id)
            .await?;
        if let Some(existing) = existing.filter(|existing| !existing.synthetic) {
            return Ok(Some(existing));
        }

        self.store().save_member(member).await?;

        if !board.is_template {
            self.add_boards_to_default_category(
                &member.user_id,
                &board.team_id,
                std::slice::from_ref(&board.id),
            )
            .await?;
        }

        self.broadcast_member_change_in_background(board.team_id, member.clone());
        Ok(Some(member.clone()))
    }

    /// Replaces the roles of a member of a board and tells the board's members.
    ///
    /// `None` is returned, and nothing is changed, if the board does not exist or the
    /// user is not a member of it.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the change would leave the board without an admin, or
    /// the store's error.
    pub async fn update_board_member(
        &self,
        member: &BoardMember,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let Some(old_member) = self
            .find_member_for_board(&member.board_id, &member.user_id)
            .await?
        else {
            return Ok(None);
        };

        // A board keeps at least one admin.
        if old_member.scheme_admin
            && !member.scheme_admin
            && self
                .is_last_admin(&member.user_id, &member.board_id)
                .await?
        {
            return Err(AppError::bad_request(LAST_ADMIN));
        }

        self.store().save_member(member).await?;

        self.broadcast_member_change_in_background(board.team_id, member.clone());
        Ok(Some(member.clone()))
    }

    fn broadcast_member_change_in_background(&self, team_id: String, member: BoardMember) {
        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_change(&team_id, &member.board_id, &member)
                .await;
        });
    }

    /// Returns whether no other member than `user_id` is an admin of the board.
    async fn is_last_admin(&self, user_id: &str, board_id: &str) -> Result<bool, AppError> {
        let members = self.store().get_members_for_board(board_id).await?;
        Ok(!members
            .iter()
            .any(|member| member.scheme_admin && member.user_id != user_id))
    }

    /// Removes a user from a board and tells the board's members and the user. Nothing
    /// happens if the board does not exist or the user is not a member of it.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the user is the board's only admin, or the store's
    /// error.
    pub async fn delete_board_member(&self, board_id: &str, user_id: &str) -> Result<(), AppError> {
        let board = match self.store().get_board(board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let Some(old_member) = self.find_member_for_board(board_id, user_id).await? else {
            return Ok(());
        };

        // A board keeps at least one admin.
        if old_member.scheme_admin && self.is_last_admin(user_id, board_id).await? {
            return Err(AppError::bad_request(LAST_ADMIN));
        }

        self.store().delete_member(board_id, user_id).await?;

        // The original announces a membership through a channel in place of the removed
        // one, if the user has one; a standalone server has no such memberships.
        let app = self.clone();
        let user_id = user_id.to_owned();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_delete(&board.team_id, &board.id, &user_id)
                .await;
        });
        Ok(())
    }
}
