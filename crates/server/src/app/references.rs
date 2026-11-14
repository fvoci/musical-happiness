//! References: the card or the page a body names, as each reader sees it.
//!
//! A body holds the identifier of what it names and nothing else, and whoever wrote it
//! was asked nothing about it. So everything is decided here, for the one who reads and
//! as things are now: what a reference names is told to whoever reads the board or the
//! wiki it is on ([`Grant::reads`](crate::permissions::Grant::reads), asked of
//! [`App::board_access`] as every route to a board asks), in the workspace of the
//! request. To anybody else it is what an identifier of nothing is: left out, with the
//! same reads made on the way.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use fvoci_model::{Block, Board, LinkedBlock, is_id};
use fvoci_store::{Paging, PublicBoards, StoreError};

use super::App;
use super::org_admin::PageAsked;
use crate::error::AppError;

/// The most references one request resolves: each is a question of access of its own.
pub const MAX_REFERENCE_IDS: usize = 100;

/// The most cards and pages a search for what a reference may name finds.
const FOUND_AT_MOST: usize = 20;

/// What is looked for among the cards and pages of the boards a user reads.
#[derive(Debug, Clone, Copy)]
enum Sought<'a> {
    /// One page of those that name the block.
    Naming {
        /// The block that is named.
        block_id: &'a str,
        /// The page of them.
        paging: Paging,
    },
    /// Those whose title has the words of the term, the ones changed last first.
    Titled {
        /// The words, as spaces divide them.
        term: &'a str,
    },
}

impl App {
    /// What the references to `ids` show the user: the cards and pages among them that
    /// are on a board or a wiki of this handle's workspace ([`App::in_workspace`]) the
    /// user reads, each once, in the order they were first asked for.
    ///
    /// Whatever else an identifier names is left out and nothing says why: no block,
    /// one in the trash, one of another workspace or organization, one of a board the
    /// user does not read, or a block that is neither a card nor a page. Access is
    /// asked once for every identifier, whatever it names: of the board its block is
    /// on, or, where it names no block, of the identifier itself, which is no board. A
    /// board that is not there takes the reads of one that is ([`App::board_access`]),
    /// so the work done is that of as many identifiers as were asked and says nothing
    /// of what they name, nor of which of them are on one board.
    ///
    /// # Errors
    ///
    /// Returns 400 for more than [`MAX_REFERENCE_IDS`] identifiers and for an entry
    /// that has not the form of one (naming its place in the list), and the store's
    /// error if a read fails.
    pub async fn resolve_references(
        &self,
        user_id: &str,
        ids: &[String],
    ) -> Result<Vec<LinkedBlock>, AppError> {
        if ids.len() > MAX_REFERENCE_IDS {
            return Err(AppError::bad_request(format_args!(
                "at most {MAX_REFERENCE_IDS} references are resolved at once"
            )));
        }
        if let Some(index) = ids.iter().position(|id| !is_id(id)) {
            return Err(
                AppError::bad_request("a reference names a block by its identifier")
                    .with_pointer(format!("/{index}")),
            );
        }
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
        let mut told = Vec::new();
        for id in asked {
            let block = named.get(id);
            let board_id = block.map_or(id, |block| block.board_id.as_str());
            let board = self.board_read(user_id, board_id).await?;
            if let (Some(block), Some(board)) = (block, board)
                && let Some(linked) = LinkedBlock::of(block, &board)
            {
                told.push(linked);
            }
        }
        Ok(told)
    }

    /// One page of the cards and pages whose body names the block, or which have a
    /// comment whose body does, on the boards and wikis of the workspace the user
    /// reads: the oldest first. What names it from anywhere else takes no place in a
    /// page and is not counted: the answer does not say whether there is any.
    ///
    /// `None` unless the block is a card or a page the user is told of
    /// ([`App::resolve_references`]): what names something is told to who reads that.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a read fails.
    pub async fn references_to(
        &self,
        user_id: &str,
        workspace_id: &str,
        block_id: &str,
        asked: PageAsked,
    ) -> Result<Option<Vec<LinkedBlock>>, AppError> {
        let told = self
            .resolve_references(user_id, &[block_id.to_owned()])
            .await?;
        if told.is_empty() {
            return Ok(None);
        }
        let Some(paging) = asked.paging() else {
            return Ok(Some(Vec::new()));
        };
        let mut boards = self.boards_held(user_id, workspace_id).await?;
        // A template of the workspace is read by who works there, and what is on it
        // names as anything does.
        boards.extend(
            self.store()
                .get_template_boards(workspace_id, user_id)
                .await?,
        );
        let sought = Sought::Naming { block_id, paging };
        Ok(Some(self.on_boards_read(user_id, boards, sought).await?))
    }

    /// The cards and pages a reference of the user may name, by their titles: those on
    /// the boards and wikis of the workspace the user reads whose title has every word
    /// of `term`, at most 20, the one changed last first; for
    /// an empty term the ones changed last. Bodies are not looked into, and what is on
    /// a template is not found.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a read fails.
    pub async fn search_references(
        &self,
        user_id: &str,
        workspace_id: &str,
        term: &str,
    ) -> Result<Vec<LinkedBlock>, AppError> {
        let boards = self.boards_held(user_id, workspace_id).await?;
        self.on_boards_read(user_id, boards, Sought::Titled { term })
            .await
    }

    /// The boards and wikis of the workspace the user holds a role on, templates left
    /// out: where to look for what the user reads. Whether the user does read one is
    /// not said here ([`App::on_boards_read`] asks).
    async fn boards_held(&self, user_id: &str, workspace_id: &str) -> Result<Vec<Board>, AppError> {
        self.boards_for_user_and_workspace(user_id, workspace_id, PublicBoards::Exclude)
            .await
    }

    /// What is sought on those of `boards` the user reads, as a reference shows it.
    ///
    /// The store looks on all of `boards`; the board of each block it finds is then
    /// asked of [`App::board_access`], and if the user reads every one, that is the
    /// answer. A board the user does not read is taken out and the store is asked
    /// again, so that what was found there takes no place in the answer. `boards` is
    /// where the store says the user holds a role, which is no answer of its own: a
    /// board that is not among them is never looked on, so what the user has nothing
    /// to do with costs nothing and shows in nothing.
    async fn on_boards_read(
        &self,
        user_id: &str,
        boards: Vec<Board>,
        sought: Sought<'_>,
    ) -> Result<Vec<LinkedBlock>, AppError> {
        let mut boards: BTreeMap<String, Board> = boards
            .into_iter()
            .map(|board| (board.id.clone(), board))
            .collect();
        let mut read: HashSet<String> = HashSet::new();
        loop {
            let board_ids: Vec<&str> = boards.keys().map(String::as_str).collect();
            let found = self.sought_on(&board_ids, sought).await?;
            let unasked: BTreeSet<&str> = found
                .iter()
                .map(|block| block.board_id.as_str())
                .filter(|board_id| !read.contains(*board_id))
                .collect();
            let unasked: Vec<&str> = unasked.into_iter().collect();
            let answers = self.boards_read(user_id, &unasked).await?;
            let refused: Vec<String> = unasked
                .iter()
                .filter(|board_id| !answers.contains_key(**board_id))
                .map(|board_id| (*board_id).to_owned())
                .collect();
            if refused.is_empty() {
                return Ok(found
                    .iter()
                    .filter_map(|block| LinkedBlock::of(block, boards.get(&block.board_id)?))
                    .collect());
            }
            read.extend(answers.into_keys());
            for board_id in refused {
                boards.remove(&board_id);
            }
        }
    }

    /// What is sought, on the boards of `board_ids`, whoever reads them.
    async fn sought_on(
        &self,
        board_ids: &[&str],
        sought: Sought<'_>,
    ) -> Result<Vec<Block>, StoreError> {
        match sought {
            Sought::Naming { block_id, paging } => {
                self.store()
                    .blocks_naming(block_id, board_ids, paging)
                    .await
            }
            Sought::Titled { term } => {
                self.store()
                    .titled_blocks(board_ids, term, FOUND_AT_MOST)
                    .await
            }
        }
    }

    /// The boards and wikis among `board_ids` that the user reads ([`App::board_read`]),
    /// by their identifiers, each asked one after another: a request holds no more of
    /// the store's connections than any other does.
    async fn boards_read(
        &self,
        user_id: &str,
        board_ids: &[&str],
    ) -> Result<HashMap<String, Board>, AppError> {
        let mut read = HashMap::new();
        for board_id in board_ids {
            if let Some(board) = self.board_read(user_id, board_id).await? {
                read.insert(board.id.clone(), board);
            }
        }
        Ok(read)
    }

    /// The board or wiki, if it is of this handle's workspace and the user reads it:
    /// what [`App::board_access`] grants, as every route to a board asks.
    ///
    /// A template the server ships with is every workspace's to read and of none: what
    /// is on it is not of this workspace, and no reference of this workspace shows it.
    async fn board_read(&self, user_id: &str, board_id: &str) -> Result<Option<Board>, AppError> {
        let access = self.board_access(user_id, board_id).await?;
        Ok(access
            .filter(|access| {
                access.grant.reads() && self.reaches_workspace(&access.board.workspace_id)
            })
            .map(|access| access.board))
    }
}
