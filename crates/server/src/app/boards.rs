//! Boards and their members.

use fvoci_model::{
    AuditAction, Board, BoardCategoryWebsocketData, BoardMember, BoardPatch, BoardType,
    BoardsAndBlocks, DeleteBoardsAndBlocks, new_id,
};
use fvoci_store::{
    AuditEntry, DeletedBoard, DuplicateAs, Joined, MemberChange, PublicBoards, Standing,
};

use super::{App, BoardAccess, DEFAULT_CATEGORY_BOARDS, Former};
use crate::collab::{AccessScope, Deletion};
use crate::error::{AppError, Reason};
use crate::permissions::BoardScope;

/// Whether a patch of a board changes who may do what on it: its type (open to the
/// workspace or by invitation) or the role of who joins it.
pub(super) fn changes_access(patch: &BoardPatch) -> bool {
    patch.board_type.is_some() || patch.minimum_role.is_some()
}

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
    /// Returns a board: for a handle of one workspace ([`App::in_workspace`]), only a
    /// board of that workspace or a built-in template.
    ///
    /// # Errors
    ///
    /// Returns a not-found error if there is no such board, or the store's error.
    pub async fn board(&self, board_id: &str) -> Result<Board, AppError> {
        match self.store().get_board(board_id).await {
            // A board of another workspace than the handle's is not there.
            Ok(board) if self.reaches_board(&board) => Ok(board),
            Ok(_) => Err(AppError::not_found("board")),
            Err(error) if error.is_not_found() => Err(AppError::not_found("board")),
            Err(error) => Err(error.into()),
        }
    }

    /// Returns the boards of a workspace that a user is a member of and, if asked for, the
    /// workspace's open boards: of the kinds the handle reaches ([`App::reaching`]).
    /// Templates are left out.
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
            .get_boards_for_user_and_workspace(user_id, workspace_id, public_boards, self.kinds())
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
        self.require_makes(&board)?;
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
        let mut scope_before = None;
        if patch.board_type.is_some() {
            let board = match self.store().get_board(board_id).await {
                Ok(board) => board,
                Err(error) if error.is_not_found() => {
                    return Err(AppError::not_found("board"));
                }
                Err(error) => return Err(error.into()),
            };
            is_template = board.is_template;
            scope_before = Some(BoardScope::of(&board));
        }

        let updated = self.store().patch_board(board_id, patch, user_id).await?;
        // Who read an open wiki without joining it reads it no more once it is private:
        // they are told it is gone for them, as when it is deleted.
        let closed = scope_before.filter(|before| {
            before.read_by_the_workspace() && !BoardScope::of(&updated).read_by_the_workspace()
        });
        if changes_access(patch) {
            self.collab()
                .recheck_access(self, AccessScope::board(board_id))
                .await;
        }

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
            if let Some(scope) = closed {
                app.broadcast_closed(&board, scope).await;
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

    /// Tells who read the board as it was, `scope`, and no longer does that it is gone
    /// for them: the message of its deletion, to who reads it no more alone.
    async fn broadcast_closed(&self, board: &Board, scope: BoardScope) {
        let members = match self.effective_board_members(&board.id).await {
            Ok(members) => members,
            Err(error) => {
                tracing::error!(%error, "Unable to get the board members");
                return;
            }
        };
        self.ws()
            .broadcast_closed_to_others(&board.workspace_id, &board.id, &members, scope)
            .await;
    }

    /// Tells who works in the workspace and is no member of a template that they gained or
    /// lost access to it, after the template changed between open and private.
    async fn broadcast_workspace_users(&self, board: &Board, board_type: BoardType) {
        // A team's people keep their role across the change, as direct members do.
        let members = match self.effective_board_members(&board.id).await {
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
        let members = self.store().effective_board_members(board_id).await?;
        let (removed, by) = (board_id.to_owned(), user_id.to_owned());
        self.collab()
            .remove(
                self,
                Deletion::Boards(vec![removed.clone()]),
                async move |app| Ok(app.store().delete_board(&removed, &by).await?),
            )
            .await?;
        self.collab()
            .recheck_access(self, AccessScope::board(board_id))
            .await;

        let former = Former {
            members: &members,
            scope: BoardScope::of(&board),
        };
        self.ws()
            .broadcast_board_delete(&board.workspace_id, &board.id, former)
            .await;
        // The board's sharing went with it; so does whoever followed it by its link.
        self.ws().end_readers_of_board(&board.id);
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

    /// Restores a board from the trash, with its blocks, its file records, the
    /// memberships and places in the sidebar of the users who still exist and the roles
    /// of its surviving teams, rechecks its card rooms, tells the board's members and
    /// each user it is filed for, and returns the board.
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
        let restored = {
            let _structure = self.collab().changing_structure(self).await;
            self.store().undelete_board(board_id, modified_by).await
        };
        let board = match restored {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Err(AppError::not_found("board")),
            Err(error) => return Err(error.into()),
        };
        // Its people and its surviving teams have their roles on it again. Its rooms
        // were emptied when it was deleted: this is for an entry under way, which
        // reads the roles again.
        self.collab()
            .recheck_access(self, AccessScope::board(&board.id))
            .await;

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
            self.ws()
                .broadcast_category_board_change(
                    &board.workspace_id,
                    &place.user_id,
                    std::slice::from_ref(&filed),
                )
                .await;
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
                self.ws()
                    .broadcast_category_boards_reorder(
                        &board.workspace_id,
                        &place.user_id,
                        &place.category_id,
                        &order,
                    )
                    .await;
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
        self.write_board_bodies(board_id).await?;
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

    /// Returns the members of a board who are in its workspace now, as the board's member
    /// list shows them: who left the workspace is listed nowhere, whatever role is left.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the members cannot be read.
    pub async fn present_members_for_board(
        &self,
        board_id: &str,
    ) -> Result<Vec<BoardMember>, AppError> {
        Ok(self.store().get_present_members_for_board(board_id).await?)
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
        self.add_member_to_board_recorded(member, Word::Unasked)
            .await
    }

    /// Makes a user a member of a board as [`App::add_member_to_board`] does, on the
    /// word of who asked, whose permission was checked with `access`: only if they are
    /// still what it found when the membership is written (`Store::add_member_asked`).
    ///
    /// # Errors
    ///
    /// As [`App::add_member_to_board`]; `None` also if who asked is not what they were.
    pub async fn add_member_asked(
        &self,
        member: &BoardMember,
        user_id: &str,
        access: &BoardAccess,
    ) -> Result<Option<BoardMember>, AppError> {
        let word = Word::Asked(standing(user_id, access));
        self.add_member_to_board_recorded(member, word).await
    }

    /// Makes a user a member of a board, on behalf of `actor_id`, who manages the board
    /// without reading it, as [`App::add_member_to_board`] does and, if the membership
    /// is new, records in the organization's audit log in the same step who made whom a
    /// member: themselves, by which they come to read it, or somebody else, who might be
    /// another account of theirs.
    ///
    /// # Errors
    ///
    /// As [`App::add_member_to_board`]; a record that cannot be written is the store's
    /// error, and the membership is not made then.
    pub async fn add_member_as_manager(
        &self,
        member: &BoardMember,
        actor_id: &str,
    ) -> Result<Option<BoardMember>, AppError> {
        self.add_member_to_board_recorded(member, Word::Manager(actor_id))
            .await
    }

    /// Adds a member as [`App::add_member_to_board`] does and, if the membership is new
    /// and a manager is given, records the manager doing it in the audit log in the same
    /// step.
    async fn add_member_to_board_recorded(
        &self,
        member: &BoardMember,
        word: Word<'_>,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };

        // Added unless already there, in one step: of two requests that add the same
        // user at once, the second finds the membership the first made.
        let added = match word {
            Word::Manager(manager_id) => {
                let entry = audit_entry(&board, member, manager_id);
                self.store().add_member_recorded(member, &entry).await
            }
            Word::Asked(asked) => self.store().add_member_asked(member, asked).await,
            Word::Unasked => self.store().add_member(member).await,
        };
        let existing = match added {
            Ok(existing) => existing,
            // The board, or who asked as they were, is not there any more.
            Err(error) if error.is_not_found() && matches!(word, Word::Asked(_)) => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(existing) = existing {
            return Ok(Some(existing));
        }

        self.collab()
            .recheck_access(self, AccessScope::board(&board.id).of_user(&member.user_id))
            .await;
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
                self.collab()
                    .recheck_access(self, AccessScope::board(&board.id).of_user(user_id))
                    .await;
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
                self.collab()
                    .recheck_access(self, AccessScope::board(&board.id).of_user(user_id))
                    .await;
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
        self.update_board_member_recorded(member, Word::Unasked)
            .await
    }

    /// Replaces the roles of a member of a board as [`App::update_board_member`] does,
    /// on the word of who asked, as [`App::add_member_asked`] takes it.
    ///
    /// # Errors
    ///
    /// As [`App::update_board_member`]; `None` also if who asked is not what they were.
    pub async fn update_member_asked(
        &self,
        member: &BoardMember,
        user_id: &str,
        access: &BoardAccess,
    ) -> Result<Option<BoardMember>, AppError> {
        let word = Word::Asked(standing(user_id, access));
        self.update_board_member_recorded(member, word).await
    }

    /// Gives a member of a board roles on it, on behalf of `actor_id`, who manages the
    /// board without reading it, as [`App::update_board_member`] does and records in the
    /// organization's audit log in the same step who gave whom the roles, as
    /// [`App::add_member_as_manager`] does.
    ///
    /// # Errors
    ///
    /// As [`App::update_board_member`]; a record that cannot be written is the store's
    /// error, and the membership is not changed then.
    pub async fn update_member_as_manager(
        &self,
        member: &BoardMember,
        actor_id: &str,
    ) -> Result<Option<BoardMember>, AppError> {
        self.update_board_member_recorded(member, Word::Manager(actor_id))
            .await
    }

    async fn update_board_member_recorded(
        &self,
        member: &BoardMember,
        word: Word<'_>,
    ) -> Result<Option<BoardMember>, AppError> {
        let board = match self.store().get_board(&member.board_id).await {
            Ok(board) => board,
            Err(error) if error.is_not_found() => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        // A board keeps at least one admin. Whether this change would take the last one
        // away is decided where the change is written, so that two admins who step down
        // at once do not both pass.
        let changed = match word {
            Word::Manager(manager_id) => {
                let entry = audit_entry(&board, member, manager_id);
                self.store().change_member_recorded(member, &entry).await
            }
            Word::Asked(asked) => self.store().change_member_asked(member, asked).await,
            Word::Unasked => self.store().change_member(member).await,
        };
        let change = match changed {
            Ok(change) => change,
            // The board, or who asked as they were, is not there any more.
            Err(error) if error.is_not_found() && matches!(word, Word::Asked(_)) => {
                return Ok(None);
            }
            Err(error) => return Err(error.into()),
        };
        match change {
            MemberChange::Done => {}
            MemberChange::NotAMember => return Ok(None),
            MemberChange::LastAdmin => {
                return Err(AppError::conflict(LAST_ADMIN).with_reason(Reason::LastAdmin));
            }
        }

        self.collab()
            .recheck_access(self, AccessScope::board(&board.id).of_user(&member.user_id))
            .await;
        self.broadcast_member_change_in_background(board.workspace_id, member.clone());
        Ok(Some(member.clone()))
    }

    pub(super) fn broadcast_member_change_in_background(
        &self,
        workspace_id: String,
        member: BoardMember,
    ) {
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

        self.collab()
            .recheck_access(self, AccessScope::board(board_id).of_user(user_id))
            .await;
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

/// On whose word a board's membership changes, and what is checked of them where it is
/// written.
#[derive(Clone, Copy)]
enum Word<'a> {
    /// Nobody's that is to be checked again: the server's own (an import).
    Unasked,
    /// A user's whose permission was checked: they must still be what it found.
    Asked(Standing<'a>),
    /// A workspace's administrator's who manages the board without reading it: they must
    /// still administer the workspace, and are recorded.
    Manager(&'a str),
}

/// What `access` found of the user who asks.
fn standing<'a>(user_id: &'a str, access: &'a BoardAccess) -> Standing<'a> {
    Standing {
        user_id,
        workspace_role: access.actor.workspace,
        member: access.member.as_ref(),
        team_role: access.team_role,
    }
}

/// The record of a manager of the board, who does not read it, giving `member` its
/// roles: themselves, or somebody else.
fn audit_entry(board: &Board, member: &BoardMember, manager_id: &str) -> AuditEntry {
    if member.user_id == manager_id {
        AuditEntry::new(
            &board.workspace_id,
            manager_id,
            AuditAction::SelfAddedToBoard,
            &board.id,
        )
    } else {
        AuditEntry::new(
            &board.workspace_id,
            manager_id,
            AuditAction::AddedToBoard,
            &board.id,
        )
        .with_subject(&member.user_id)
    }
}
