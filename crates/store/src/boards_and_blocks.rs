//! Operations on several boards and blocks at once, all or none.

use std::collections::HashSet;

use fvoci_model::{
    BlockType, BoardMember, BoardType, BoardsAndBlocks, BoardsAndBlocksError,
    DeleteBoardsAndBlocks, PatchBoardsAndBlocks, renew_boards_and_blocks_ids,
};

use crate::Store;
use crate::blocks::{self, BlockFilter, DuplicateAs};
use crate::boards;
use crate::db::Scope;
use crate::error::StoreError;

impl Store {
    /// Inserts or updates every board and then every block, as [`Store::insert_board`]
    /// and [`Store::insert_block`] do. `boards_and_blocks` is updated to what was stored.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] for the first block that fails validation;
    /// nothing is written in that case.
    pub async fn create_boards_and_blocks(
        &self,
        boards_and_blocks: &mut BoardsAndBlocks,
        user_id: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            create_boards_and_blocks(scope, boards_and_blocks, user_id).await
        })
        .await
    }

    /// Does what [`Store::create_boards_and_blocks`] does and makes the creator of each
    /// board an administrator and editor of it. Returns those memberships, in the order
    /// of the boards.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidBlock`] for the first block that fails validation;
    /// nothing is written in that case.
    pub async fn create_boards_and_blocks_with_admin(
        &self,
        boards_and_blocks: &mut BoardsAndBlocks,
        user_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            create_boards_and_blocks_with_admin(scope, boards_and_blocks, user_id).await
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

    /// Deletes the listed blocks, each with its direct children, and then the listed
    /// boards, each with all its blocks.
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
    /// The copy is a private board created by `user_id`, without a channel, in `to_team`
    /// or else in the team of the original. A copy of the same kind as the original has
    /// " copy" appended to its title, and a template made from a board is titled "New
    /// board template".
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such board, and
    /// the store's error if the copy cannot be written.
    pub async fn duplicate_board(
        &self,
        board_id: &str,
        user_id: &str,
        to_team: Option<&str>,
        duplicate_as: DuplicateAs,
    ) -> Result<(BoardsAndBlocks, Vec<BoardMember>), StoreError> {
        self.transaction(async |scope| {
            duplicate_board(scope, board_id, user_id, to_team, duplicate_as).await
        })
        .await
    }
}

pub(crate) async fn create_boards_and_blocks(
    scope: &mut Scope<'_>,
    boards_and_blocks: &mut BoardsAndBlocks,
    user_id: &str,
) -> Result<(), StoreError> {
    for board in &mut boards_and_blocks.boards {
        boards::insert_board(scope, board, user_id).await?;
    }
    for block in &mut boards_and_blocks.blocks {
        blocks::insert_block(scope, block, user_id).await?;
    }
    Ok(())
}

pub(crate) async fn create_boards_and_blocks_with_admin(
    scope: &mut Scope<'_>,
    boards_and_blocks: &mut BoardsAndBlocks,
    user_id: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    create_boards_and_blocks(scope, boards_and_blocks, user_id).await?;

    let mut members = Vec::with_capacity(boards_and_blocks.boards.len());
    for board in &boards_and_blocks.boards {
        // Preserved quirk: as in `insert_board_with_admin`, the administrator is the
        // creator the board names, which for a board that already existed is whatever
        // the caller put there (`server/services/store/sqlstore/boards_and_blocks.go:25-31`).
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

    // The blocks go first: deleting a board removes its blocks, and the blocks listed
    // here would then no longer be found.
    //
    // Preserved quirk: the same happens within the list. Deleting a block removes its
    // direct children, so a request that lists a block after its parent fails with "not
    // found" (`server/services/store/sqlstore/boards_and_blocks.go:110-123`).
    for block_id in &request.blocks {
        let block = blocks::get_block(scope, block_id).await?;
        if !board_ids.contains(block.board_id.as_str()) {
            return Err(StoreError::BlockNotInDeletedBoards {
                block_id: block_id.clone(),
            });
        }
        blocks::delete_block(scope, block_id, user_id).await?;
    }
    for board_id in &request.boards {
        boards::delete_board(scope, board_id, user_id).await?;
    }
    Ok(())
}

pub(crate) async fn duplicate_board(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
    to_team: Option<&str>,
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
    board.channel_id.clear();
    if let Some(team_id) = to_team {
        team_id.clone_into(&mut board.team_id);
    }

    let blocks: Vec<_> = blocks::get_blocks(scope, &BlockFilter::board(board_id))
        .await?
        .into_iter()
        .filter(|block| block.block_type != BlockType::Comment)
        .collect();

    // A board without blocks, or with comments only, is copied as a board without
    // blocks. The original refuses it, because it regenerates the identifiers with the
    // function for requests, which insists on at least one block
    // (`server/services/store/sqlstore/boards_and_blocks.go:177`).
    let mut copies = renew_boards_and_blocks_ids(BoardsAndBlocks {
        boards: vec![board],
        blocks,
    });
    let members = create_boards_and_blocks_with_admin(scope, &mut copies, user_id).await?;
    Ok((copies, members))
}
