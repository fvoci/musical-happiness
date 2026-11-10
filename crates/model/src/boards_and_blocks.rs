//! Requests that operate on several boards and blocks at once.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::block::{Block, BlockPatch};
use crate::block_ids::generate_block_ids;
use crate::board::{Board, BoardPatch};
use crate::id::{IdKind, new_id};
use crate::serde_util::null_as_default;

/// Reasons a boards-and-blocks request is rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoardsAndBlocksError {
    /// The request lists no board.
    #[error("at least one board is required")]
    NoBoards,
    /// The request lists no block.
    #[error("at least one block is required")]
    NoBlocks,
    /// The number of board identifiers and board patches differ.
    #[error("board ids and patches need to match")]
    BoardIdsAndPatchesMismatch,
    /// The number of block identifiers and block patches differ.
    #[error("block ids and patches need to match")]
    BlockIdsAndPatchesMismatch,
    /// A block names a board that is not part of the request.
    #[error("block {block_id} doesn't belong to any board")]
    BlockWithoutBoard {
        /// The identifier of the offending block.
        block_id: String,
    },
}

/// Boards and the blocks that belong to them, created together.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardsAndBlocks {
    /// The boards.
    #[serde(deserialize_with = "null_as_default")]
    pub boards: Vec<Board>,
    /// The blocks; each must belong to one of `boards`.
    #[serde(deserialize_with = "null_as_default")]
    pub blocks: Vec<Block>,
}

impl BoardsAndBlocks {
    /// Checks that there is at least one board and one block, and that every block belongs
    /// to one of the boards.
    ///
    /// # Errors
    ///
    /// Returns the first [`BoardsAndBlocksError`] found, checking boards, then blocks, then
    /// each block's board in order.
    pub fn validate(&self) -> Result<(), BoardsAndBlocksError> {
        if self.boards.is_empty() {
            return Err(BoardsAndBlocksError::NoBoards);
        }
        if self.blocks.is_empty() {
            return Err(BoardsAndBlocksError::NoBlocks);
        }
        let board_ids: HashSet<&str> = self.boards.iter().map(|board| board.id.as_str()).collect();
        match self
            .blocks
            .iter()
            .find(|block| !board_ids.contains(block.board_id.as_str()))
        {
            Some(block) => Err(BoardsAndBlocksError::BlockWithoutBoard {
                block_id: block.id.clone(),
            }),
            None => Ok(()),
        }
    }
}

/// The identifiers of boards and blocks to delete together.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeleteBoardsAndBlocks {
    /// The identifiers of the boards.
    #[serde(deserialize_with = "null_as_default")]
    pub boards: Vec<String>,
    /// The identifiers of the blocks.
    #[serde(deserialize_with = "null_as_default")]
    pub blocks: Vec<String>,
}

impl DeleteBoardsAndBlocks {
    /// Checks that at least one board is listed. Blocks are optional.
    ///
    /// # Errors
    ///
    /// Returns [`BoardsAndBlocksError::NoBoards`] if no board is listed.
    pub fn validate(&self) -> Result<(), BoardsAndBlocksError> {
        if self.boards.is_empty() {
            return Err(BoardsAndBlocksError::NoBoards);
        }
        Ok(())
    }
}

impl From<&BoardsAndBlocks> for DeleteBoardsAndBlocks {
    fn from(boards_and_blocks: &BoardsAndBlocks) -> Self {
        Self {
            boards: boards_and_blocks
                .boards
                .iter()
                .map(|board| board.id.clone())
                .collect(),
            blocks: boards_and_blocks
                .blocks
                .iter()
                .map(|block| block.id.clone())
                .collect(),
        }
    }
}

/// Patches for several boards and blocks, applied together.
///
/// Identifiers and patches are matched by position.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PatchBoardsAndBlocks {
    /// The identifiers of the boards to patch.
    #[serde(rename = "boardIDs", deserialize_with = "null_as_default")]
    pub board_ids: Vec<String>,
    /// The board patches, one per board identifier.
    #[serde(rename = "boardPatches", deserialize_with = "null_as_default")]
    pub board_patches: Vec<BoardPatch>,
    /// The identifiers of the blocks to patch.
    #[serde(rename = "blockIDs", deserialize_with = "null_as_default")]
    pub block_ids: Vec<String>,
    /// The block patches, one per block identifier.
    #[serde(rename = "blockPatches", deserialize_with = "null_as_default")]
    pub block_patches: Vec<BlockPatch>,
}

impl PatchBoardsAndBlocks {
    /// Checks that at least one board is patched and that identifiers and patches pair up.
    ///
    /// # Errors
    ///
    /// Returns a [`BoardsAndBlocksError`] if there is no board identifier, or the board or
    /// block identifiers and patches differ in number.
    pub fn validate(&self) -> Result<(), BoardsAndBlocksError> {
        if self.board_ids.is_empty() {
            return Err(BoardsAndBlocksError::NoBoards);
        }
        if self.board_ids.len() != self.board_patches.len() {
            return Err(BoardsAndBlocksError::BoardIdsAndPatchesMismatch);
        }
        if self.block_ids.len() != self.block_patches.len() {
            return Err(BoardsAndBlocksError::BlockIdsAndPatchesMismatch);
        }
        Ok(())
    }
}

/// Gives every board and block a new identifier, keeping blocks attached to their boards
/// and to each other.
///
/// Blocks come back grouped by board, in the order of `boards`.
///
/// # Errors
///
/// Returns the [`BoardsAndBlocksError`] from [`BoardsAndBlocks::validate`] if the input is
/// not valid.
pub fn generate_boards_and_blocks_ids(
    boards_and_blocks: BoardsAndBlocks,
) -> Result<BoardsAndBlocks, BoardsAndBlocksError> {
    boards_and_blocks.validate()?;
    let BoardsAndBlocks { mut boards, blocks } = boards_and_blocks;

    let mut blocks_by_board: HashMap<String, Vec<Block>> = HashMap::new();
    for block in blocks {
        blocks_by_board
            .entry(block.board_id.clone())
            .or_default()
            .push(block);
    }

    let mut blocks = Vec::new();
    for board in &mut boards {
        let new_board_id = new_id(IdKind::Board);
        // Validation does not reject two boards sharing an identifier. Here the first one
        // takes the blocks; the original lists those blocks twice, attached to the last
        // such board.
        for mut block in blocks_by_board.remove(&board.id).unwrap_or_default() {
            block.board_id.clone_from(&new_board_id);
            blocks.push(block);
        }
        board.id = new_board_id;
    }
    generate_block_ids(&mut blocks);

    Ok(BoardsAndBlocks { boards, blocks })
}
