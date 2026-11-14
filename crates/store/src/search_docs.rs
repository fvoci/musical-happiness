//! What the search server is to hold of the blocks: a document for each card that is
//! on a board, which is no template. One read says so, for the worker that sends the
//! documents and for the search that checks each of the search server's answers
//! against it: what is not read here is not found.

use fvoci_model::{BlockType, SearchKind};

use crate::Store;
use crate::db::{Id, Row, Scope};
use crate::error::StoreError;

/// Bounded parameters however many identifiers are asked for.
const BATCH: usize = 300;

/// A card as the search server is to hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDocument {
    /// The identifier of the card.
    pub id: String,
    /// The organization of its workspace.
    pub org_id: String,
    /// The workspace of its board.
    pub workspace_id: String,
    /// The board it is on.
    pub container_id: String,
    /// What it is.
    pub kind: SearchKind,
    /// Its title.
    pub title: String,
    /// The plain text of its body; empty without one.
    pub text: String,
}

/// The columns of a document, of the blocks `b`, their boards `bo` and the boards'
/// workspaces `w`.
const DOCUMENT_COLUMNS: &str =
    "b.id, w.org_id, bo.workspace_id, b.board_id, b.title, COALESCE(b.body_text, '')";

/// The blocks that have a document, with their boards and workspaces: the cards of
/// the boards that are no templates. A card in the trash is no block, and has none.
fn documents_from(scope: &Scope<'_>) -> String {
    [
        " FROM blocks AS b JOIN boards AS bo ON bo.id = b.board_id",
        &scope.same_organization(&[("bo", "b")]),
        " JOIN workspaces AS w ON w.id = bo.workspace_id",
        &scope.same_organization(&[("w", "bo")]),
        " WHERE bo.is_template = false AND bo.is_builtin = false AND b.type = ",
    ]
    .concat()
}

impl Store {
    /// The documents of those of `ids` that have one, among the rows this handle
    /// reaches, by identifier. What is no identifier has none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_documents(
        &self,
        ids: &[String],
    ) -> Result<Vec<SearchDocument>, StoreError> {
        let mut ids: Vec<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|id| fvoci_model::is_id(id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        self.with_conn(async |scope| {
            let mut documents = Vec::with_capacity(ids.len());
            for batch in ids.chunks(BATCH) {
                let mut sql = scope.sql(&format!("SELECT {DOCUMENT_COLUMNS}"));
                sql.push(&documents_from(scope))
                    .bind(BlockType::Card.as_str())
                    .push(" AND b.id IN (")
                    .bind_list(batch.iter().map(|id| Id(id)))
                    .push(") ORDER BY b.id");
                for row in scope.fetch_all(sql).await? {
                    documents.push(document_from_row(&row)?);
                }
            }
            Ok(documents)
        })
        .await
    }

    /// Up to `limit` documents of the blocks of a board, by identifier, those after
    /// the identifier `after` (empty for the first): for sending a board whole, a page
    /// at a time. A board that is a template, or is not there, has none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_documents_of_container(
        &self,
        board_id: &str,
        after: &str,
        limit: i64,
    ) -> Result<Vec<SearchDocument>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!("SELECT {DOCUMENT_COLUMNS}"));
            sql.push(&documents_from(scope))
                .bind(BlockType::Card.as_str())
                .push_bind(" AND b.board_id = ", Id(board_id));
            if !after.is_empty() {
                sql.push_bind(" AND b.id > ", Id(after));
            }
            sql.push_bind(" ORDER BY b.id LIMIT ", limit);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(document_from_row)
                .collect()
        })
        .await
    }
}

fn document_from_row(row: &Row) -> Result<SearchDocument, StoreError> {
    Ok(SearchDocument {
        id: row.id(0)?,
        org_id: row.id(1)?,
        workspace_id: row.id(2)?,
        container_id: row.id(3)?,
        kind: SearchKind::Card,
        title: row.text(4)?,
        text: row.text(5)?,
    })
}
