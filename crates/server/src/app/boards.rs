//! Boards and their members.

use fvoci_model::{
    Board, BoardCategoryWebsocketData, BoardMember, BoardPatch, BoardType, BoardsAndBlocks,
    DeleteBoardsAndBlocks, new_id,
};
use fvoci_store::{
    AuditAction, AuditEntry, DeletedBoard, DuplicateAs, Joined, MemberChange, PublicBoards,
};

use super::{App, DEFAULT_CATEGORY_BOARDS};
use crate::error::{AppError, Reason};

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
        match self.store().get_board(board_id).await {
            Ok(board) => Ok(board),
            Err(error) if error.is_not_found() => Err(AppError::not_found("board")),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the boards of a workspace that a user is a member of and, if asked for, the
    /// workspace's open boards. Templates are left out.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn boards_for_user_and_workspace(
        &self,
        user_id: &str,
        workspace_id: &str,
        public_boards: PublicBoards,
    ) -> Result<Vec<Board>, AppError> {
        Ok(self
            .store()
            .get_boards_for_user_and_workspace(user_id, workspace_id, public_boards)
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
            // The original does not classify this error and answers 500
            // (`server/app/boards.go:234-236`).
            return Err(AppError::bad_request("new board cannot have an ID").with_field("id"));
        }
        board.id = new_id();

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
                .broadcast_board_change(&created.workspace_id, &created)
                .await;
            if let Some(creator) = creator {
                app.ws()
                    .broadcast_member_change(&created.workspace_id, &created.id, &creator)
                    .await;
            }
        });

        if !board.is_template {
            self.add_boards_to_default_category(
                user_id,
                &board.workspace_id,
                std::slice::from_ref(&board.id),
            )
            .await?;
        }
        Ok(board)
    }

    /// Files boards under a user's default category in a workspace, creating that category
    /// if the user does not have it yet.
    pub(super) async fn add_boards_to_default_category(
        &self,
        user_id: &str,
        workspace_id: &str,
        board_ids: &[String],
    ) -> Result<(), AppError> {
        let categories = self.user_category_boards(user_id, workspace_id).await?;
        let Some(default) = categories
            .iter()
            .find(|entry| entry.category.name == DEFAULT_CATEGORY_BOARDS)
        else {
            return Err(AppError::internal(format_args!(
                "no default category found for user userID: {user_id}"
            )));
        };
        self.add_update_user_category_board(workspace_id, user_id, &default.category.id, board_ids)
            .await
    }

    /// Applies a patch to a board on behalf of a user and tells the board's members.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, or the store's error.
    pub async fn patch_board(
        &self,
        patch: &BoardPatch,
        board_id: &str,
        user_id: &str,
    ) -> Result<Board, AppError> {
        let mut is_template = false;
        if patch.board_type.is_some() {
            let board = match self.store().get_board(board_id).await {
                Ok(board) => board,
                Err(error) if error.is_not_found() => {
                    return Err(AppError::not_found("board"));
                }
                Err(error) => return Err(error.into()),
            };
            is_template = board.is_template;
        }

        let updated = self.store().patch_board(board_id, patch, user_id).await?;

        let app = self.clone();
        let board = updated.clone();
        let new_type = patch.board_type.filter(|_| is_template);
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&board.workspace_id, &board)
                .await;
            if let Some(board_type) = new_type {
                app.broadcast_workspace_users(&board, board_type).await;
            }
        });

        Ok(updated)
    }

    /// Puts the card properties of a board into the order of their identifiers in
    /// `order`, which names each of them once, tells the board's listeners with the
    /// board's change as for any change of it, and returns the order.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, a bad request if `order`
    /// does not name each property once, and the store's error.
    pub async fn reorder_card_properties(
        &self,
        board_id: &str,
        order: &[String],
        user_id: &str,
    ) -> Result<Vec<String>, AppError> {
        let board = match self
            .store()
            .reorder_card_properties(board_id, order, user_id)
            .await
        {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&board.workspace_id, &board)
                .await;
        });
        Ok(order.to_vec())
    }

    /// Tells who works in the workspace and is no member of a template that they gained or
    /// lost access to it, after the template changed between open and private.
    async fn broadcast_workspace_users(&self, board: &Board, board_type: BoardType) {
        let members = match self.members_for_board(&board.id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, "Unable to get the board members");
                Vec::new()
            }
        };
        // Who works in the board's workspace, whom an open template is for: nobody else
        // gains or loses it, and nobody else is named.
        let users = match self
            .store()
            .get_workspace_worker_ids(&board.workspace_id)
            .await
        {
            Ok(users) => users,
            Err(error) => {
                tracing::error!(%error, "Unable to get the workspace users");
                Vec::new()
            }
        };
        let others = users
            .iter()
            .filter(|user_id| !members.iter().any(|member| member.user_id == **user_id));
        for user_id in others {
            match board_type {
                BoardType::Private => {
                    self.ws()
                        .broadcast_member_delete(&board.workspace_id, &board.id, user_id)
                        .await;
                }
                BoardType::Open => {
                    let viewer = BoardMember {
                        user_id: user_id.clone(),
                        board_id: board.id.clone(),
                        scheme_viewer: true,
                        synthetic: true,
                        ..BoardMember::default()
                    };
                    self.ws()
                        .broadcast_member_change(&board.workspace_id, &board.id, &viewer)
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
        // The memberships go with the board; who is told is who was a member.
        let members = self.store().get_members_for_board(board_id).await?;
        self.store().delete_board(board_id, user_id).await?;

        self.ws()
            .broadcast_board_delete(&board.workspace_id, &board.id, &members);
        // The board's sharing went with it; so does whoever followed it by its link.
        self.ws().close_readers_of_board(&board.id);
        Ok(())
    }

    /// Returns a board that is in the trash with the memberships it had, or `None` if
    /// no board of that identifier is there.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the trash cannot be read.
    pub async fn deleted_board(&self, board_id: &str) -> Result<Option<DeletedBoard>, AppError> {
        match self.store().get_deleted_board(board_id).await {
            Ok(deleted) => Ok(Some(deleted)),
            Err(error) if error.is_not_found() => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Restores a board from the trash, with its blocks, its file records and the
    /// memberships and places in the sidebar of the users who still exist, tells the
    /// board's members and each user it is filed for, and returns the board.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if the board is not in the trash, and the store's
    /// error if it cannot be restored.
    pub async fn undelete_board(
        &self,
        board_id: &str,
        modified_by: &str,
    ) -> Result<Board, AppError> {
        let board = match self.store().undelete_board(board_id, modified_by).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };

        let app = self.clone();
        let restored = board.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_board_change(&restored.workspace_id, &restored)
                .await;
            app.tell_where_restored_board_is(&restored).await;
        });
        Ok(board)
    }

    /// Tells each user who has a restored board filed where it is: the board's entry, and
    /// the order of the boards of that category, which moved to make room for it. Read
    /// after the restore; a failed read is logged, the restore stands.
    async fn tell_where_restored_board_is(&self, board: &Board) {
        let places = match self.store().get_board_places(&board.id).await {
            Ok(places) => places,
            Err(error) => {
                tracing::error!(%error, board_id = board.id, "the places of a restored board could not be read");
                return;
            }
        };
        for place in places {
            let filed = BoardCategoryWebsocketData {
                board_id: board.id.clone(),
                category_id: place.category_id.clone(),
                hidden: place.hidden,
            };
            self.ws().broadcast_category_board_change(
                &board.workspace_id,
                &place.user_id,
                std::slice::from_ref(&filed),
            );
            let categories = self
                .store()
                .get_user_category_boards(&place.user_id, &board.workspace_id)
                .await;
            let order: Option<Vec<String>> = categories.ok().and_then(|categories| {
                categories
                    .into_iter()
                    .find(|entry| entry.category.id == place.category_id)
                    .map(|entry| {
                        entry
                            .board_metadata
                            .into_iter()
                            .map(|metadata| metadata.board_id)
                            .collect()
                    })
            });
            if let Some(order) = order {
                self.ws().broadcast_category_boards_reorder(
                    &board.workspace_id,
                    &place.user_id,
                    &place.category_id,
                    &order,
                );
            }
        }
    }

    /// Copies a board and its blocks, except comments, into `to_workspace` for a user, who
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
        to_workspace: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<(BoardsAndBlocks, Vec<BoardMember>), AppError> {
        let (mut copies, members) = self
            .store()
            .duplicate_board(
                board_id,
                user_id,
                Some(to_workspace).filter(|workspace| !workspace.is_empty()),
                duplicate_as,
            )
            .await?;

        // The copied blocks get files of their own.
        if let Err(error) = self
            .copy_and_update_card_files(board_id, user_id, &mut copies.blocks)
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
            return Err(error.context(format_args!(
                "could not patch file IDs while duplicating board {board_id}"
            )));
        }

        if duplicate_as == DuplicateAs::Copy {
            for board in &copies.boards {
                self.set_board_category_from_source(board_id, &board.id, user_id, to_workspace)
                    .await?;
            }
        }

        let app = self.clone();
        let broadcast = copies.clone();
        let new_members = members.clone();
        self.spawn_background(async move {
            let mut workspace_id = "";
            for board in &broadcast.boards {
                workspace_id = &board.workspace_id;
                app.ws().broadcast_board_change(workspace_id, board).await;
            }
            for block in &broadcast.blocks {
                app.ws().broadcast_block_change(workspace_id, block).await;
            }
            for member in &new_members {
                app.ws()
                    .broadcast_member_change(workspace_id, &member.board_id, member)
                    .await;
            }
        });

        Ok((copies, members))
    }

    /// Files a copy of a board under the category its source is filed under for the
    /// user in the workspace, or else under the default category.
    async fn set_board_category_from_source(
        &self,
        source_board_id: &str,
        destination_board_id: &str,
        user_id: &str,
        workspace_id: &str,
    ) -> Result<(), AppError> {
        let destination = [destination_board_id.to_owned()];
        let categories = self.user_category_boards(user_id, workspace_id).await?;
        let source_category = categories.iter().find(|entry| {
            entry
                .board_metadata
                .iter()
                .any(|metadata| metadata.board_id == source_board_id)
        });
        match source_category {
            Some(entry) => {
                self.add_update_user_category_board(
                    workspace_id,
                    user_id,
                    &entry.category.id,
                    &destination,
                )
                .await
            }
            None => {
                self.add_boards_to_default_category(user_id, workspace_id, &destination)
                    .await
            }
        }
    }

    /// Returns the members of a board.
    ///
    /// The original also reports the workspace's administrators as admins of the board; a
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
        self.add_member_to_board_recorded(member, None).await
    }

    /// Makes a user, who manages the board without reading it, a member of it as
    /// [`App::add_member_to_board`] does and, if the membership is new, records in the
    /// organization's audit log in the same step that they made themselves one.
    ///
    /// # Errors
    ///
    /// As [`App::add_member_to_board`]; a record that cannot be written is the store's
    /// error, and the membership is not made then.
    pub async fn add_self_to_board_recorded(
        &self,
        member: &BoardMember,
    ) -> Result<Option<BoardMember>, AppError> {
        self.add_member_to_board_recorded(member, Some(AuditAction::SelfAddedToBoard))
            .await
    }

    /// Adds a member as [`App::add_member_to_board`] does and, if the membership is new
    /// and `action` is given, records the user doing it in the audit log in the same
    /// step.
    async fn add_member_to_board_recorded(
        &self,
        member: &BoardMember,
        action: Option<AuditAction>,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        // Added unless already there, in one step: of two requests that add the same
        // user at once, the second finds the membership the first made.
        let existing = match action {
            Some(action) => {
                let entry = audit_entry(&board, member, action);
                self.store().add_member_recorded(member, &entry).await?
            }
            None => self.store().add_member(member).await?,
        };
        if let Some(existing) = existing {
            return Ok(Some(existing));
        }

        // The membership is stored: it is announced whatever becomes of the filing.
        self.broadcast_member_change_in_background(board.workspace_id.clone(), member.clone());
        if !board.is_template {
            self.add_boards_to_default_category(
                &member.user_id,
                &board.workspace_id,
                std::slice::from_ref(&board.id),
            )
            .await?;
        }
        Ok(Some(member.clone()))
    }

    /// Makes a user a member of a board with the board's minimum role, as joining does,
    /// and returns the membership. A member below the minimum role is raised to it; a
    /// member with that role or a higher one stays as they are. The board's members are
    /// told about a membership that is new or changed.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, and an error if the board
    /// cannot be filed under the user's default category; in that case the membership
    /// exists nonetheless.
    pub async fn join_board(&self, board_id: &str, user_id: &str) -> Result<BoardMember, AppError> {
        let board = self.board(board_id).await?;
        let joined = match self.store().join_board(board_id, user_id).await {
            Ok(joined) => joined,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        match joined {
            Joined::New(member) => {
                // The membership is stored: it is announced whatever becomes of the
                // filing.
                self.broadcast_member_change_in_background(
                    board.workspace_id.clone(),
                    member.clone(),
                );
                if !board.is_template {
                    self.add_boards_to_default_category(
                        user_id,
                        &board.workspace_id,
                        std::slice::from_ref(&board.id),
                    )
                    .await?;
                }
                Ok(member)
            }
            Joined::Raised(member) => {
                self.broadcast_member_change_in_background(board.workspace_id, member.clone());
                Ok(member)
            }
            Joined::Already(member) => Ok(member),
        }
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
        self.update_board_member_recorded(member, None).await
    }

    /// Gives a user, who manages the board without reading it, roles on it as
    /// [`App::update_board_member`] does and records in the organization's audit log in
    /// the same step that they gave them to themselves.
    ///
    /// # Errors
    ///
    /// As [`App::update_board_member`]; a record that cannot be written is the store's
    /// error, and the membership is not changed then.
    pub async fn update_self_on_board_recorded(
        &self,
        member: &BoardMember,
    ) -> Result<Option<BoardMember>, AppError> {
        self.update_board_member_recorded(member, Some(AuditAction::SelfAddedToBoard))
            .await
    }

    async fn update_board_member_recorded(
        &self,
        member: &BoardMember,
        action: Option<AuditAction>,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        // A board keeps at least one admin. Whether this change would take the last one
        // away is decided where the change is written, so that two admins who step down
        // at once do not both pass.
        let change = match action {
            Some(action) => {
                let entry = audit_entry(&board, member, action);
                self.store().change_member_recorded(member, &entry).await?
            }
            None => self.store().change_member(member).await?,
        };
        match change {
            MemberChange::Done => {}
            MemberChange::NotAMember => return Ok(None),
            MemberChange::LastAdmin => {
                return Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin));
            }
        }

        self.broadcast_member_change_in_background(board.workspace_id, member.clone());
        Ok(Some(member.clone()))
    }

    fn broadcast_member_change_in_background(&self, workspace_id: String, member: BoardMember) {
        let app = self.clone();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_change(&workspace_id, &member.board_id, &member)
                .await;
        });
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
        // A board keeps at least one admin; decided where the membership is removed.
        match self.store().remove_member(board_id, user_id).await? {
            MemberChange::Done => {}
            MemberChange::NotAMember => return Ok(()),
            MemberChange::LastAdmin => {
                return Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin));
            }
        }

        // The original announces a membership through a channel in place of the removed
        // one, if the user has one; a standalone server has no such memberships.
        let app = self.clone();
        let user_id = user_id.to_owned();
        self.spawn_background(async move {
            app.ws()
                .broadcast_member_delete(&board.workspace_id, &board.id, &user_id)
                .await;
        });
        Ok(())
    }
}

/// The record of a user doing `action` to their own membership of the board.
fn audit_entry(board: &Board, member: &BoardMember, action: AuditAction) -> AuditEntry {
    AuditEntry {
        workspace_id: board.workspace_id.clone(),
        actor_id: member.user_id.clone(),
        action,
        target_id: board.id.clone(),
        create_at: 0,
    }
}
