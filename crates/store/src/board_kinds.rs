//! The kind of a board (`boards.kind`) as queries read it and ask for it.
//!
//! A list of boards is of the kinds its caller names: there is no list that reads
//! boards and wikis without saying so.

use fvoci_model::{BoardKind, BoardKinds};

use crate::db::Sql;
use crate::error::StoreError;

/// Pushes ` AND <column> = <kind>` for kinds that are one kind, and nothing for both.
/// `column` names the kind column of the boards read, with its alias.
pub(crate) fn push_kinds(sql: &mut Sql, column: &str, kinds: BoardKinds) {
    if let Some(kind) = kinds.only() {
        sql.push(" AND ")
            .push(column)
            .push_bind(" = ", kind.as_str());
    }
}

/// The kind a stored text names.
pub(crate) fn kind_from_text(text: &str) -> Result<BoardKind, StoreError> {
    BoardKind::ALL
        .into_iter()
        .find(|kind| kind.as_str() == text)
        .ok_or_else(|| StoreError::Corrupt {
            column: "kind",
            reason: format!("unknown board kind {text:?}"),
        })
}
