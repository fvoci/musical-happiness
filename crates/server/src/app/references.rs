//! References: the card or the page a body names, as each reader sees it.
//!
//! A body holds the identifier of what it names and nothing else, and whoever wrote it
//! was asked nothing about it. So everything is decided here, for the one who reads and
//! as things are now: what a reference names is told to whoever reads the board or the
//! wiki it is on ([`Grant::reads`](crate::permissions::Grant::reads), asked of
//! [`App::board_access`] as every route to a board asks), in the workspace of the
//! request. To anybody else it is what an identifier of nothing is: left out, with the
//! same reads made on the way.

use std::collections::{BTreeSet, HashMap};

use fvoci_model::{Block, Board, LinkedBlock};

use super::App;
use crate::error::AppError;

impl App {
    /// What the references to `ids` show the user: the cards and pages among them that
    /// are on a board or a wiki of this handle's workspace ([`App::in_workspace`]) the
    /// user reads, each once, in the order they were first asked for.
    ///
    /// Whatever else an identifier names is left out and nothing says why: no block,
    /// one in the trash, one of another workspace or organization, one of a board the
    /// user does not read, or a block that is neither a card nor a page. One question
    /// of access is asked for each board the blocks are on, and one for each identifier
    /// that names no block (of the identifier, which is no board): a board that is not
    /// there takes the reads of one that is ([`App::board_access`]), so what is left
    /// out takes what it would take if it were told.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a read fails.
    pub async fn resolve_references(
        &self,
        user_id: &str,
        ids: &[String],
    ) -> Result<Vec<LinkedBlock>, AppError> {
        let mut asked: Vec<&str> = Vec::with_capacity(ids.len());
        for id in ids {
            if !asked.contains(&id.as_str()) {
                asked.push(id);
            }
        }
        let blocks = self.store().named_blocks(&asked).await?;
        let named: HashMap<&str, &Block> = blocks
            .iter()
            .map(|block| (block.id.as_str(), block))
            .collect();
        let boards: BTreeSet<&str> = asked
            .iter()
            .map(|id| named.get(id).map_or(*id, |block| block.board_id.as_str()))
            .collect();
        let read = self.boards_read(user_id, boards).await?;
        Ok(asked
            .iter()
            .filter_map(|id| {
                let block = named.get(id)?;
                LinkedBlock::of(block, read.get(block.board_id.as_str())?)
            })
            .collect())
    }

    /// The boards and wikis among `board_ids` that are of this handle's workspace and
    /// that the user reads, by their identifiers. Each is asked of
    /// [`App::board_access`], one after another: a request holds no more of the store's
    /// connections than any other does.
    ///
    /// A template the server ships with is every workspace's to read and of none: what
    /// is on it is not of this workspace, and no reference of this workspace shows it.
    async fn boards_read(
        &self,
        user_id: &str,
        board_ids: impl IntoIterator<Item = &str>,
    ) -> Result<HashMap<String, Board>, AppError> {
        let mut read = HashMap::new();
        for board_id in board_ids {
            let access = self.board_access(user_id, board_id).await?;
            if let Some(access) = access
                && access.grant.reads()
                && self.reaches_workspace(&access.board.workspace_id)
            {
                read.insert(access.board.id.clone(), access.board);
            }
        }
        Ok(read)
    }
}
