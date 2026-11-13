//! Operations on several boards and blocks at once, all or none.

use std::collections::HashSet;

use fvoci_model::{
    BlockType, Board, BoardMember, BoardType, BoardsAndBlocks, BoardsAndBlocksError,
    DeleteBoardsAndBlocks, FileInfo, PatchBoardsAndBlocks, renew_boards_and_blocks_ids,
};

use crate::Store;
use crate::blocks::{self, BlockFilter, DuplicateAs};
use crate::boards;
use crate::card_values;
use crate::db::{Dialect, Scope};
use crate::error::StoreError;
use crate::file_info;
use crate::file_refs::NewImages;
use crate::loose_objects;
use crate::members;
use crate::org_admin;
use crate::trash;

/// The key of the PostgreSQL advisory lock that the replacement of the built-in
/// templates holds. Any number that the migrator's lock does not use.
const BUILTIN_TEMPLATES_LOCK: i64 = 0x6676_6f63_6974_706c;

impl Store {
    /// Replaces the built-in templates with `templates`, stored on behalf of nobody, if
    /// `needed` says the stored ones must go. One transaction that holds the others off
    /// while it reads, decides, removes and stores: of two servers or requests that find
    /// the templates missing at once, the second finds them stored and keeps them.
    ///
    /// Returns `None` if the templates were kept, or the records of the files the
    /// removed templates had, whose records went with them and whose objects are doomed
    /// in the same transaction.
    ///
    /// # Errors
    ///
    /// Returns the store's error, or [`StoreError::InvalidBlock`] for a block that fails
    /// validation; nothing is removed or stored then.
    pub async fn replace_builtin_templates(
        &self,
        templates: &mut BoardsAndBlocks,
        needed: impl FnOnce(&[Board]) -> bool + Send,
    ) -> Result<Option<Vec<FileInfo>>, StoreError> {
        self.transaction(async |scope| {
            // On SQLite the transaction holds the database from its start.
            if scope.dialect() == Dialect::Postgres {
                let mut sql = scope.sql("SELECT pg_advisory_xact_lock(");
                sql.bind(BUILTIN_TEMPLATES_LOCK).push(")");
                scope.execute(sql).await?;
            }
            // Under READ COMMITTED, the default, the read below sees what the transaction
            // that held the lock before committed. A stricter isolation level would take
            // its snapshot before the lock is granted, and miss that.
            let stored = boards::get_builtin_templates(scope).await?;
            if !needed(&stored) {
                return Ok(None);
            }
            let mut files = Vec::new();
            for board in &stored {
                files.extend(file_info::file_infos_of_board(scope, &board.id).await?);
            }
            // The records go with the templates; their objects are doomed now.
            let doomed: Vec<_> = files.iter().map(|file| (file.path.clone(), None)).collect();
            loose_objects::doom(scope, &doomed).await?;
            boards::remove_builtin_templates(scope).await?;
            // Their images are pointed at the templates' files once those are stored.
            create_boards_and_blocks(scope, templates, "", NewImages::Unchecked).await?;
            Ok(Some(files))
        })
        .await
    }

    /// Inserts or updates every board and then every block, as [`Store::insert_board`]
    /// and [`Store::insert_block`] do. `boards_and_blocks` is updated to what was stored.
    /// The images the blocks show newly are asked as `new_images` says.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] for the first block that fails validation;
    /// nothing is written in that case.
    pub async fn create_boards_and_blocks(
        &self,
        boards_and_blocks: &mut BoardsAndBlocks,
        user_id: &str,
        new_images: NewImages,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            create_boards_and_blocks(scope, boards_and_blocks, user_id, new_images).await
        })
        .await
    }

    /// Does what [`Store::create_boards_and_blocks`] does and makes the creator of each
    /// board an administrator and editor of it. Returns those memberships, in the order
    /// of the boards.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] for the first block that fails validation,
    /// and [`StoreError::NotFound`] for the workspace if the creator of a board does not
    /// work in its workspace; nothing is written in either case.
    pub async fn create_boards_and_blocks_with_admin(
        &self,
        boards_and_blocks: &mut BoardsAndBlocks,
        user_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            create_boards_and_blocks_with_admin(
                scope,
                boards_and_blocks,
                user_id,
                NewImages::Recorded,
            )
            .await
        })
        .await
    }

    /// Applies each board patch and each block patch of `patches` to the board or block
    /// at the same position. Returns the patched boards and blocks, in the order of the
    /// request.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if a board or block does not exist,
    /// [`StoreError::InvalidBlock`] if a patched block fails validation, and
    /// [`StoreError::InvalidBoardsAndBlocks`] if there are more identifiers than
    /// patches. Nothing is changed in any of these cases.
    pub async fn patch_boards_and_blocks(
        &self,
        patches: &PatchBoardsAndBlocks,
        user_id: &str,
    ) -> Result<BoardsAndBlocks, StoreError> {
        self.transaction(async |scope| patch_boards_and_blocks(scope, patches, user_id).await)
            .await
    }

    /// Deletes the listed boards, each with all its blocks, into the trash. The listed
    /// blocks must be blocks of those boards, and go with them.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if a board or block does not exist, and
    /// [`StoreError::BlockNotInDeletedBoards`] if a block belongs to none of the listed
    /// boards. Nothing is deleted in either case.
    pub async fn delete_boards_and_blocks(
        &self,
        request: &DeleteBoardsAndBlocks,
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| delete_boards_and_blocks(scope, request, user_id).await)
            .await
    }

    /// Copies a board and its blocks, except comments, under new identifiers. Returns the
    /// copies and the membership that makes `user_id` an administrator of the new board.
    ///
    /// The copy is a private board created by `user_id`, in `to_workspace`
    /// or else in the workspace of the original. A copy of the same kind as the original has
    /// " copy" appended to its title, and a template made from a board is titled "New
    /// board template".
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board, or if `user_id` does
    /// not work in the copy's workspace, and the store's error if the copy cannot be
    /// written.
    pub async fn duplicate_board(
        &self,
        board_id: &str,
        user_id: &str,
        to_workspace: Option<&str>,
        duplicate_as: DuplicateAs,
    ) -> Result<(BoardsAndBlocks, Vec<BoardMember>), StoreError> {
        self.transaction(async |scope| {
            duplicate_board(scope, board_id, user_id, to_workspace, duplicate_as).await
        })
        .await
    }
}

pub(crate) async fn create_boards_and_blocks(
    scope: &mut Scope<'_>,
    boards_and_blocks: &mut BoardsAndBlocks,
    user_id: &str,
    new_images: NewImages,
) -> Result<(), StoreError> {
    for board in &mut boards_and_blocks.boards {
        boards::insert_board(scope, board, user_id).await?;
    }
    // Parents first, wherever in the list they are: a copied board hands its blocks
    // over oldest first, and a block may be older than the block it is on.
    blocks::insert_blocks(scope, &mut boards_and_blocks.blocks, user_id, new_images).await
}

pub(crate) async fn create_boards_and_blocks_with_admin(
    scope: &mut Scope<'_>,
    boards_and_blocks: &mut BoardsAndBlocks,
    user_id: &str,
    new_images: NewImages,
) -> Result<Vec<BoardMember>, StoreError> {
    create_boards_and_blocks(scope, boards_and_blocks, user_id, new_images).await?;

    let mut members = Vec::with_capacity(boards_and_blocks.boards.len());
    for board in &boards_and_blocks.boards {
        // As in `insert_board_with_admin`, the administrator is the board's creator:
        // `user_id` for a new board, the stored creator for one that existed.
        members::hold_creator(scope, &board.workspace_id, &board.created_by).await?;
        let member = BoardMember {
            board_id: board.id.clone(),
            user_id: board.created_by.clone(),
            scheme_admin: true,
            scheme_editor: true,
            ..BoardMember::default()
        };
        boards::save_member(scope, &member).await?;
        members.push(member);
    }
    Ok(members)
}

pub(crate) async fn patch_boards_and_blocks(
    scope: &mut Scope<'_>,
    patches: &PatchBoardsAndBlocks,
    user_id: &str,
) -> Result<BoardsAndBlocks, StoreError> {
    // The original indexes the patches by the position of the identifier and crashes when
    // there are fewer patches than identifiers; that is an error here.
    // The boards that are patched and the boards of the blocks that are patched are
    // locked in one order, before any of them is written. A block that does not exist
    // is reported where it is patched, below.
    let mut block_boards = Vec::with_capacity(patches.block_ids.len());
    for block_id in &patches.block_ids {
        if let Some(block) = blocks::find_block(scope, block_id).await? {
            block_boards.push(block.board_id);
        }
    }
    let to_lock = patches.board_ids.iter().chain(&block_boards);
    boards::lock_boards(scope, to_lock.map(String::as_str)).await?;

    let mut result = BoardsAndBlocks::default();
    for (index, board_id) in patches.board_ids.iter().enumerate() {
        let patch = patches
            .board_patches
            .get(index)
            .ok_or(BoardsAndBlocksError::BoardIdsAndPatchesMismatch)?;
        let board = boards::patch_board(scope, board_id, patch, user_id).await?;
        result.boards.push(board);
    }
    for (index, block_id) in patches.block_ids.iter().enumerate() {
        let patch = patches
            .block_patches
            .get(index)
            .ok_or(BoardsAndBlocksError::BlockIdsAndPatchesMismatch)?;
        blocks::patch_block(scope, block_id, patch, user_id).await?;
        result
            .blocks
            .push(blocks::get_block(scope, block_id).await?);
    }
    Ok(result)
}

pub(crate) async fn delete_boards_and_blocks(
    scope: &mut Scope<'_>,
    request: &DeleteBoardsAndBlocks,
    user_id: &str,
) -> Result<(), StoreError> {
    let board_ids: HashSet<&str> = request.boards.iter().map(String::as_str).collect();
    // The organization's before the boards': with a joining that ends, whose roles on
    // them are given back in the trash too (`joinings::give_back_roles`).
    org_admin::lock_organization(scope).await?;
    boards::lock_boards(scope, board_ids.iter().copied()).await?;

    // Every listed block must be a block of one of the listed boards. That is all
    // there is to do about the blocks: a board takes all its blocks into the trash, as
    // one deletion, and they come back with it.
    for block_id in &request.blocks {
        let block = blocks::get_block(scope, block_id).await?;
        if !board_ids.contains(block.board_id.as_str()) {
            return Err(StoreError::BlockNotInDeletedBoards {
                block_id: block_id.clone(),
            });
        }
    }
    for board_id in &request.boards {
        trash::delete_board(scope, board_id, user_id).await?;
    }
    Ok(())
}

pub(crate) async fn duplicate_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
    to_workspace: Option<&str>,
    duplicate_as: DuplicateAs,
) -> Result<(BoardsAndBlocks, Vec<BoardMember>), StoreError> {
    let mut board = boards::get_board(scope, board_id).await?;

    let as_template = duplicate_as == DuplicateAs::Template;
    if as_template == board.is_template {
        // board -> board or template -> template
        board.title.push_str(" copy");
    } else if as_template {
        // template from board
        "New board template".clone_into(&mut board.title);
    }

    // The copy is private to the user who made it.
    board.board_type = BoardType::Private;
    board.is_template = as_template;
    user_id.clone_into(&mut board.created_by);
    if let Some(workspace_id) = to_workspace {
        workspace_id.clone_into(&mut board.workspace_id);
    }

    let mut blocks: Vec<_> = blocks::get_blocks(scope, &BlockFilter::board(board_id))
        .await?
        .into_iter()
        .filter(|block| block.block_type != BlockType::Comment)
        .collect();
    card_values::clear_copied_values(&mut blocks, &board.card_properties);

    // A board without blocks, or with comments only, is copied as a board without
    // blocks. The original refuses it, because it regenerates the identifiers with the
    // function for requests, which insists on at least one block
    // (`server/services/store/sqlstore/boards_and_blocks.go:177`).
    let mut copies = renew_boards_and_blocks_ids(BoardsAndBlocks {
        boards: vec![board],
        blocks,
    });
    // The copies show the original board's files until theirs are copied.
    let members =
        create_boards_and_blocks_with_admin(scope, &mut copies, user_id, NewImages::Unchecked)
            .await?;
    Ok((copies, members))
}
