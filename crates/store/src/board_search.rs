//! Boards found by a search term, among those a user can reach.

use fvoci_model::{Board, BoardKinds, BoardSearchField};

use crate::Store;
use crate::boards::{self, PublicBoards};
use crate::db::{Dialect, Scope};
use crate::error::StoreError;

impl Store {
    /// Returns the boards of `kinds`, of any workspace, that match a search term and that
    /// a user is a member of or, with [`PublicBoards::Include`], that are open, oldest
    /// first. Templates are left out.
    ///
    /// An empty `term` matches every board. Searching by title matches boards whose
    /// title contains every space-separated word of the term, ignoring case; searching
    /// by property name matches boards that have a property of that name.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn search_boards_for_user(
        &self,
        term: &str,
        search_field: BoardSearchField,
        user_id: &str,
        public_boards: PublicBoards,
        kinds: BoardKinds,
    ) -> Result<Vec<Board>, StoreError> {
        self.with_conn(async |scope| {
            search_boards_for_user(scope, term, search_field, user_id, public_boards, kinds).await
        })
        .await
    }
}

/// Whether the title contains every space-separated word of `term`, ignoring case.
///
/// Compared here rather than in SQL, so that both databases fold every letter the same
/// way (SQLite's `lower()` folds ASCII letters only) and `%` and `_` are characters
/// like any other. Preserved quirk: the words are split at single spaces, so two spaces
/// in a row yield an empty word, which every title contains
/// (`server/services/store/sqlstore/board.go:691-695`).
fn title_has_words(title: &str, term: &str) -> bool {
    let title = title.to_lowercase();
    term.trim()
        .split(' ')
        .all(|word| title.contains(&word.to_lowercase()))
}

/// Keeps the boards whose title has the words of `term`; an empty term keeps all.
pub(crate) fn with_title_words(mut boards: Vec<Board>, term: &str) -> Vec<Board> {
    if !term.is_empty() {
        boards.retain(|board| title_has_words(&board.title, term));
    }
    boards
}

async fn search_boards_for_user(
    scope: &mut Scope<'_>,
    term: &str,
    search_field: BoardSearchField,
    user_id: &str,
    public_boards: PublicBoards,
    kinds: BoardKinds,
) -> Result<Vec<Board>, StoreError> {
    // No board's property has U+0000 in its name: it is refused when a board is stored,
    // and PostgreSQL refuses it as a parameter.
    if search_field == BoardSearchField::PropertyName && term.contains('\0') {
        return Ok(Vec::new());
    }
    let mut sql = boards::select_boards_of_user(scope, user_id, public_boards, kinds);
    if !term.is_empty() && search_field == BoardSearchField::PropertyName {
        // A board property of that name, whatever its value, `null` too; a name is no
        // path, so `a.b` is a name with a dot.
        match scope.dialect() {
            Dialect::Postgres => {
                sql.push_bind(" AND b.properties -> ", term)
                    .push(" IS NOT NULL");
            }
            Dialect::Sqlite => {
                sql.push_bind(
                    " AND EXISTS (SELECT 1 FROM json_each(b.properties) AS p WHERE p.key = ",
                    term,
                )
                .push(")");
            }
        }
    }
    sql.push(boards::OLDEST_BOARD_FIRST);
    let boards = boards::fetch_boards(scope, sql).await?;
    Ok(match search_field {
        BoardSearchField::Title => with_title_words(boards, term),
        BoardSearchField::PropertyName => boards,
    })
}
