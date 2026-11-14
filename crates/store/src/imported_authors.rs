//! The authors an imported archive names: kept where they are members of the
//! organization and of the workspace imported into, the importer everywhere else.

use std::collections::{HashMap, HashSet};

use fvoci_model::BoardsAndBlocks;

use crate::Store;
use crate::boards_and_blocks::create_boards_and_blocks;
use crate::db::{Id, OptId, Param, Scope};
use crate::error::StoreError;
use crate::file_refs::NewImages;

/// The most identifiers one statement names.
const CHUNK: usize = 500;

impl Store {
    /// Stores an archive's boards and blocks for `importer` as
    /// [`Store::create_boards_and_blocks`] does with [`NewImages::Unchecked`], and keeps
    /// each author the archive names (`created_by`, `modified_by`) who is a member of
    /// the handle's organization and of the board's workspace: somebody the importer, who
    /// works there, already sees. Any other author, or none, is the importer: an archive
    /// cannot make somebody of another organization, or of a workspace the importer does
    /// not know, an author, whose name the workspace's readers would then be told.
    /// `boards_and_blocks` is updated to what was stored.
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
            let org = scope.org()?;
            let named = Named::of(boards_and_blocks);
            let mut members = Members::new();
            for (workspace_id, authors) in named.authors_by_workspace() {
                let found = members_among(scope, &org, &workspace_id, &authors).await?;
                members.insert(workspace_id, found);
            }
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
/// store wrote the importer there, with the workspace each is in.
struct Named {
    boards: HashMap<String, Authors>,
    blocks: HashMap<String, Authors>,
    /// The workspace of each board and block, by identifier.
    workspaces: HashMap<String, String>,
}

/// The users who may stay authors, by the workspace they may stay authors in.
type Members = HashMap<String, HashSet<String>>;

impl Named {
    fn of(boards_and_blocks: &BoardsAndBlocks) -> Self {
        let authors = |created_by: &str, modified_by: &str| Authors {
            created_by: created_by.to_owned(),
            modified_by: modified_by.to_owned(),
        };
        let mut workspaces: HashMap<String, String> = boards_and_blocks
            .boards
            .iter()
            .map(|board| (board.id.clone(), board.workspace_id.clone()))
            .collect();
        for block in &boards_and_blocks.blocks {
            if let Some(workspace_id) = workspaces.get(&block.board_id).cloned() {
                workspaces.insert(block.id.clone(), workspace_id);
            }
        }
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
            workspaces,
        }
    }

    /// Every user named, each once, by the workspace they are named in.
    fn authors_by_workspace(&self) -> HashMap<String, Vec<String>> {
        let mut named: HashMap<&String, HashSet<&String>> = HashMap::new();
        for (id, authors) in self.boards.iter().chain(&self.blocks) {
            let Some(workspace_id) = self.workspaces.get(id) else {
                continue;
            };
            let users = named.entry(workspace_id).or_default();
            for user in [&authors.created_by, &authors.modified_by] {
                if !user.is_empty() {
                    users.insert(user);
                }
            }
        }
        named
            .into_iter()
            .map(|(workspace_id, users)| {
                let users = users.into_iter().cloned().collect();
                (workspace_id.clone(), users)
            })
            .collect()
    }

    /// Writes back, over the stored importer, each author named who is one of
    /// `members`, in the store and in `boards_and_blocks`.
    async fn keep(
        &self,
        scope: &mut Scope<'_>,
        boards_and_blocks: &mut BoardsAndBlocks,
        members: &Members,
        importer: &str,
    ) -> Result<(), StoreError> {
        let kept = |id: &String, named: Option<&Authors>| {
            let named = named?;
            let workspace_id = self.workspaces.get(id)?;
            let keep = |user: &String| {
                if members
                    .get(workspace_id)
                    .is_some_and(|users| users.contains(user))
                {
                    user.clone()
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
            if let Some(authors) = kept(&board.id, self.boards.get(&board.id)) {
                board.created_by.clone_from(&authors.created_by);
                board.modified_by.clone_from(&authors.modified_by);
                boards.entry(authors).or_default().push(board.id.clone());
            }
        }
        let mut blocks: HashMap<Authors, Vec<String>> = HashMap::new();
        for block in &mut boards_and_blocks.blocks {
            if let Some(authors) = kept(&block.id, self.blocks.get(&block.id)) {
                block.created_by.clone_from(&authors.created_by);
                block.modified_by.clone_from(&authors.modified_by);
                blocks.entry(authors).or_default().push(block.id.clone());
            }
        }
        write_authors(scope, Authored::Boards, &boards).await?;
        write_authors(scope, Authored::Blocks, &blocks).await
    }
}

/// The users of `ids` who are members of the organization `org` and of its workspace
/// `workspace_id`.
async fn members_among(
    scope: &mut Scope<'_>,
    org: &Param,
    workspace_id: &str,
    ids: &[String],
) -> Result<HashSet<String>, StoreError> {
    let mut members = HashSet::new();
    for chunk in ids.chunks(CHUNK) {
        let mut sql = scope.sql(
            "SELECT wm.user_id FROM workspace_members AS wm \
             JOIN workspaces AS t ON t.id = wm.workspace_id \
             JOIN organization_members AS om ON om.user_id = wm.user_id AND om.org_id = t.org_id \
             WHERE t.org_id = ",
        );
        sql.bind(org.clone())
            .push_bind(" AND wm.workspace_id = ", Id(workspace_id))
            .push(" AND wm.user_id IN (")
            .bind_list(chunk.iter().map(|id| Id(id)))
            .push(")");
        for row in scope.fetch_all(sql).await? {
            members.insert(row.id(0)?);
        }
    }
    Ok(members)
}

/// A table whose rows have authors.
#[derive(Debug, Clone, Copy)]
enum Authored {
    Boards,
    Blocks,
}

impl Authored {
    fn table(self) -> &'static str {
        match self {
            Self::Boards => "boards",
            Self::Blocks => "blocks",
        }
    }
}

/// Sets the authors of the rows of `table` with each identifier listed under them.
async fn write_authors(
    scope: &mut Scope<'_>,
    table: Authored,
    authored: &HashMap<Authors, Vec<String>>,
) -> Result<(), StoreError> {
    for (authors, ids) in authored {
        for chunk in ids.chunks(CHUNK) {
            let mut sql = scope.sql(&format!("UPDATE {} SET created_by = ", table.table()));
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
