//! The authors an imported archive names: kept where they are members of the
//! organization imported into, the importer everywhere else.

use std::collections::{HashMap, HashSet};

use fvoci_model::BoardsAndBlocks;

use crate::Store;
use crate::boards_and_blocks::create_boards_and_blocks;
use crate::db::{Id, OptId, Scope};
use crate::error::StoreError;
use crate::file_refs::NewImages;

/// The most identifiers one statement names.
const CHUNK: usize = 500;

impl Store {
    /// Stores an archive's boards and blocks for `importer` as
    /// [`Store::create_boards_and_blocks`] does with [`NewImages::Unchecked`], and keeps
    /// each author the archive names (`created_by`, `modified_by`) who is a member of the
    /// handle's organization. Any other author, or none, is the importer: an archive
    /// cannot make somebody of another organization an author in the workspace, whom
    /// its readers would then be named. `boards_and_blocks` is updated to what was
    /// stored.
    ///
    /// # Errors
    ///
    /// As [`Store::create_boards_and_blocks`], and [`StoreError::NoOrganization`] if the
    /// handle names no organization; nothing is written in either case.
    pub async fn create_imported_boards_and_blocks(
        &self,
        boards_and_blocks: &mut BoardsAndBlocks,
        importer: &str,
    ) -> Result<(), StoreError> {
        self.transaction(async |scope| {
            let named = Named::of(boards_and_blocks);
            let members = members_among(scope, &named.authors()).await?;
            create_boards_and_blocks(scope, boards_and_blocks, importer, NewImages::Unchecked)
                .await?;
            named
                .keep(scope, boards_and_blocks, &members, importer)
                .await
        })
        .await
    }
}

/// Who made and who last changed a board or block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Authors {
    created_by: String,
    modified_by: String,
}

/// The authors the archive named for each board and block, by identifier, before the
/// store wrote the importer there.
struct Named {
    boards: HashMap<String, Authors>,
    blocks: HashMap<String, Authors>,
}

impl Named {
    fn of(boards_and_blocks: &BoardsAndBlocks) -> Self {
        let authors = |created_by: &str, modified_by: &str| Authors {
            created_by: created_by.to_owned(),
            modified_by: modified_by.to_owned(),
        };
        Self {
            boards: boards_and_blocks
                .boards
                .iter()
                .map(|board| {
                    let named = authors(&board.created_by, &board.modified_by);
                    (board.id.clone(), named)
                })
                .collect(),
            blocks: boards_and_blocks
                .blocks
                .iter()
                .map(|block| {
                    let named = authors(&block.created_by, &block.modified_by);
                    (block.id.clone(), named)
                })
                .collect(),
        }
    }

    /// Every user named, each once.
    fn authors(&self) -> Vec<String> {
        let named: HashSet<&String> = self
            .boards
            .values()
            .chain(self.blocks.values())
            .flat_map(|authors| [&authors.created_by, &authors.modified_by])
            .filter(|id| !id.is_empty())
            .collect();
        named.into_iter().cloned().collect()
    }

    /// Writes back, over the stored importer, each author named who is one of
    /// `members`, in the store and in `boards_and_blocks`.
    async fn keep(
        &self,
        scope: &mut Scope<'_>,
        boards_and_blocks: &mut BoardsAndBlocks,
        members: &HashSet<String>,
        importer: &str,
    ) -> Result<(), StoreError> {
        let kept = |named: Option<&Authors>| {
            let named = named?;
            let keep = |id: &String| {
                if members.contains(id) {
                    id.clone()
                } else {
                    importer.to_owned()
                }
            };
            let authors = Authors {
                created_by: keep(&named.created_by),
                modified_by: keep(&named.modified_by),
            };
            (authors.created_by != importer || authors.modified_by != importer).then_some(authors)
        };
        let mut boards: HashMap<Authors, Vec<String>> = HashMap::new();
        for board in &mut boards_and_blocks.boards {
            if let Some(authors) = kept(self.boards.get(&board.id)) {
                board.created_by.clone_from(&authors.created_by);
                board.modified_by.clone_from(&authors.modified_by);
                boards.entry(authors).or_default().push(board.id.clone());
            }
        }
        let mut blocks: HashMap<Authors, Vec<String>> = HashMap::new();
        for block in &mut boards_and_blocks.blocks {
            if let Some(authors) = kept(self.blocks.get(&block.id)) {
                block.created_by.clone_from(&authors.created_by);
                block.modified_by.clone_from(&authors.modified_by);
                blocks.entry(authors).or_default().push(block.id.clone());
            }
        }
        write_authors(scope, "boards", &boards).await?;
        write_authors(scope, "blocks", &blocks).await
    }
}

/// The users of `ids` who are members of the handle's organization.
async fn members_among(
    scope: &mut Scope<'_>,
    ids: &[String],
) -> Result<HashSet<String>, StoreError> {
    let mut members = HashSet::new();
    for chunk in ids.chunks(CHUNK) {
        let mut sql = scope.sql("SELECT user_id FROM organization_members WHERE org_id = ");
        sql.bind(scope.org()?)
            .push(" AND user_id IN (")
            .bind_list(chunk.iter().map(|id| Id(id)))
            .push(")");
        for row in scope.fetch_all(sql).await? {
            members.insert(row.id(0)?);
        }
    }
    Ok(members)
}

/// Sets the authors of the rows of `table` (`boards` or `blocks`) with each identifier
/// listed under them.
async fn write_authors(
    scope: &mut Scope<'_>,
    table: &str,
    authored: &HashMap<Authors, Vec<String>>,
) -> Result<(), StoreError> {
    for (authors, ids) in authored {
        for chunk in ids.chunks(CHUNK) {
            let mut sql = scope.sql(&format!("UPDATE {table} SET created_by = "));
            sql.bind(OptId(&authors.created_by))
                .push_bind(", modified_by = ", OptId(&authors.modified_by))
                .push(" WHERE id IN (")
                .bind_list(chunk.iter().map(|id| Id(id)))
                .push(")");
            scope.execute(sql).await?;
        }
    }
    Ok(())
}
