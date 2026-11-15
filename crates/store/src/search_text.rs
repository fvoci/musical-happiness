//! The search of the cards and pages of a workspace by the letters of their title and
//! body (`blocks.body_text`), for a server without a search server: every word asked
//! for is in the title or in the body, as it is written there, ASCII letters in either
//! case. It reads the blocks that have a document ([`crate::search_docs`]), the same
//! ones a search server holds, and answers their identifiers alone, the best first:
//! the caller checks each against what the user reads, as it checks what a search
//! server answers.

use std::time::Instant;

use fvoci_model::SearchKind;

use crate::Store;
use crate::db::{Dialect, Id, Scope, Sql};
use crate::error::StoreError;
use crate::search_docs::select_documents;

/// What a search by the letters asks for.
#[derive(Debug, Clone, Copy)]
pub struct TextSearch<'a> {
    /// The workspace searched.
    pub workspace_id: &'a str,
    /// The boards and wikis of the workspace searched; `None` for all of them.
    pub containers: Option<&'a [String]>,
    /// The one kind asked for, if one is.
    pub kind: Option<SearchKind>,
    /// The words, none empty: each is in the title or the body of what is found.
    pub words: &'a [String],
}

/// What came of a search by the letters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFound {
    /// The identifiers of the cards and the pages found, the best first.
    Ids(Vec<String>),
    /// The database did not finish in the time the search had, and was stopped.
    OutOfTime,
}

/// The text of a word as a pattern of `LIKE` that finds it anywhere, with the escape
/// character `\`: its ASCII letters in lower case, and `%`, `_` and `\` taken as
/// themselves.
fn pattern(word: &str) -> String {
    let mut pattern = String::with_capacity(word.len() + 2);
    pattern.push('%');
    for c in word.chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c.to_ascii_lowercase());
    }
    pattern.push('%');
    pattern
}

/// A column of `b` as it is matched against a [`pattern`]: with its ASCII letters in
/// lower case and no other letter changed. SQLite's `LIKE` already reads ASCII letters
/// alone in either case; PostgreSQL's `lower` does the same under the collation `C`.
fn folded(dialect: Dialect, column: &str) -> String {
    match dialect {
        Dialect::Sqlite => column.to_owned(),
        Dialect::Postgres => format!("lower({column} COLLATE \"C\")"),
    }
}

/// Adds the condition that every word is in the title or in the body.
fn push_every_word(sql: &mut Sql, dialect: Dialect, words: &[String]) {
    let title = folded(dialect, "b.title");
    let body = folded(dialect, "COALESCE(b.body_text, '')");
    for (index, word) in words.iter().enumerate() {
        let pattern = pattern(word);
        if index > 0 {
            sql.push(" AND ");
        }
        sql.push_bind(&format!("({title} LIKE "), pattern.clone())
            .push_bind(&format!(" ESCAPE '\\' OR {body} LIKE "), pattern)
            .push(" ESCAPE '\\')");
    }
}

impl Store {
    /// Up to `limit` cards and pages that `search` finds, past the `offset` best: the
    /// ones whose title holds every word first, then the most recently changed, and
    /// then by identifier, so that a page follows the one before. Stopped at `by`.
    /// Words of which one holds a NUL character find nothing: no text stored holds one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails otherwise than by the time.
    pub async fn search_text(
        &self,
        search: &TextSearch<'_>,
        offset: i64,
        limit: i64,
        by: Instant,
    ) -> Result<TextFound, StoreError> {
        if search.words.is_empty() || search.words.iter().any(|word| word.contains('\0')) {
            return Ok(TextFound::Ids(Vec::new()));
        }
        self.with_conn(async |scope| {
            let sql = statement(scope, search, offset, limit);
            let Some(rows) = scope.fetch_all_by(sql, by).await? else {
                return Ok(TextFound::OutOfTime);
            };
            rows.iter()
                .map(|row| row.id(0))
                .collect::<Result<Vec<String>, StoreError>>()
                .map(TextFound::Ids)
        })
        .await
    }
}

/// The statement of [`Store::search_text`].
fn statement(scope: &Scope<'_>, search: &TextSearch<'_>, offset: i64, limit: i64) -> Sql {
    let dialect = scope.dialect();
    let mut sql = select_documents(scope, "b.id");
    sql.push_bind(" AND bo.workspace_id = ", Id(search.workspace_id));
    if let Some(containers) = search.containers {
        sql.push(" AND b.board_id IN (")
            .bind_list(containers.iter().map(|id| Id(id)))
            .push(")");
    }
    if let Some(kind) = search.kind {
        sql.push_bind(" AND b.type = ", kind.block_type().as_str().to_owned());
    }
    sql.push(" AND ");
    push_every_word(&mut sql, dialect, search.words);
    sql.push(" ORDER BY CASE WHEN ");
    push_title_holds_every_word(&mut sql, dialect, search.words);
    sql.push(" THEN 0 ELSE 1 END, b.update_at DESC, b.id")
        .push_bind(" LIMIT ", limit)
        .push_bind(" OFFSET ", offset);
    sql
}

/// Adds the condition that the title holds every word.
fn push_title_holds_every_word(sql: &mut Sql, dialect: Dialect, words: &[String]) {
    let title = folded(dialect, "b.title");
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            sql.push(" AND ");
        }
        sql.push_bind(&format!("{title} LIKE "), pattern(word))
            .push(" ESCAPE '\\'");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_takes_what_like_reads_as_itself() {
        assert_eq!(pattern("환불"), "%환불%");
        assert_eq!(pattern("100%"), "%100\\%%");
        assert_eq!(pattern("a_b"), "%a\\_b%");
        assert_eq!(pattern(r"C:\x"), r"%c:\\x%");
        assert_eq!(pattern("ÉTÉ API"), "%ÉtÉ api%");
    }
}
