//! The built-in template boards.
//!
//! Ports `server/app/templates.go`. The default templates are imported from an archive
//! embedded in the binary, once, when a database has none or an older version of them.

use std::collections::HashSet;

use bytes::Bytes;
use fvoci_model::{Block, Board, BoardType};
use serde_json::Value;

use super::App;
use super::import::ImportOptions;
use crate::error::AppError;

/// The current version of the built-in templates. Raising it re-imports them.
const DEFAULT_TEMPLATE_VERSION: i64 = 6;

/// The title, in the embedded archive, of the board the onboarding tour copies. It is
/// read once, when the archive is imported, to put [`ONBOARDING_PROPERTY`] on that board;
/// nothing looks a board up by its title afterwards.
const ARCHIVE_WELCOME_BOARD_TITLE: &str = "Welcome to Boards!";

/// The board property that marks the built-in template the onboarding tour copies.
///
/// A property alone proves nothing, since any user sets the properties of their own
/// boards; see [`is_onboarding_template`].
pub(crate) const ONBOARDING_PROPERTY: &str = "onboarding";

/// The embedded default-templates archive, read from the original's asset at build time.
const DEFAULT_TEMPLATES_ARCHIVE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../server/assets/templates.boardarchive"
));

impl App {
    /// Imports the default templates if the database has none, an older version, or
    /// none that is marked for the onboarding tour. Returns whether an import was done.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the templates cannot be read or removed, or an
    /// import error if the archive cannot be imported.
    pub async fn init_templates(&self) -> Result<bool, AppError> {
        let boards = self.store().get_builtin_templates().await?;
        tracing::debug!(count = boards.len(), "Fetched template boards");

        let Some(reason) = initialization_reason(&boards) else {
            tracing::debug!("Template import not needed, skipping");
            return Ok(false);
        };
        tracing::debug!(
            reason,
            size = DEFAULT_TEMPLATES_ARCHIVE.len(),
            "Importing new default templates"
        );

        self.store().remove_builtin_templates().await?;

        let options = ImportOptions {
            // A built-in template is offered to every team; it is filed under the root
            // team because a board has to be some team's.
            team_id: self.root_team().await?.id,
            // No user: what is stored on behalf of nobody is the system's own.
            modified_by: String::new(),
            board_modifier: Some(Box::new(fix_template_board)),
            block_modifier: Some(Box::new(fix_template_block)),
        };
        let imported = self
            .import_archive(Bytes::from_static(DEFAULT_TEMPLATES_ARCHIVE), &options)
            .await;
        if let Err(error) = imported {
            // The boards are stored before their files. Boards left behind by a failed
            // import carry the current version and would pass the check at the next
            // start, without their files; remove them so that the import is tried again.
            self.remove_partial_templates().await;
            return Err(error.context("cannot import the built-in templates"));
        }
        Ok(true)
    }

    /// Removes the built-in templates that a failed import left behind.
    async fn remove_partial_templates(&self) {
        if let Err(error) = self.store().remove_builtin_templates().await {
            tracing::error!(%error, "cannot remove partially imported templates");
        }
    }

    /// Returns the template boards of a team, for the templates route and onboarding.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn template_boards(
        &self,
        team_id: &str,
        user_id: &str,
    ) -> Result<Vec<Board>, AppError> {
        let mut boards = self.store().get_template_boards(team_id, user_id).await?;
        boards.extend(self.store().get_builtin_templates().await?);
        Ok(boards)
    }
}

/// Returns whether a board is the built-in template that the onboarding tour copies:
/// a template of the global team that the system created and marked.
///
/// The system's authorship is what cannot be forged. A board's creator is always the
/// session's user, on every route that makes a board; nobody signs in as the system.
pub(crate) fn is_onboarding_template(board: &Board) -> bool {
    board.is_template
        && board.is_builtin
        && board.properties.get(ONBOARDING_PROPERTY) == Some(&Value::Bool(true))
}

/// Why the templates must be imported, or `None` if they are current. The templates are
/// needed when there are none, when a built-in one has an older version, or when none of
/// them is marked for the onboarding tour (templates imported before the mark existed).
fn initialization_reason(boards: &[Board]) -> Option<&'static str> {
    if boards.is_empty() {
        return Some("no default templates found");
    }
    for board in boards {
        if board.template_version < DEFAULT_TEMPLATE_VERSION {
            return Some("template_version too old");
        }
    }
    if !boards.iter().any(is_onboarding_template) {
        return Some("no template is marked for the onboarding tour");
    }
    None
}

/// Turns an archive board into a default template: non-template boards become open
/// templates of the current version, and already-template boards are dropped (and
/// recorded so their blocks are dropped too).
fn fix_template_board(board: &mut Board, skipped: &mut HashSet<String>) -> bool {
    if board.is_template {
        skipped.insert(board.id.clone());
        return false;
    }
    board.title = board.title.replace("(NEW)", "");
    board.is_template = true;
    board.template_version = DEFAULT_TEMPLATE_VERSION;
    board.board_type = BoardType::Open;
    if board.title == ARCHIVE_WELCOME_BOARD_TITLE {
        board
            .properties
            .insert(ONBOARDING_PROPERTY.to_owned(), Value::Bool(true));
    }
    true
}

/// Drops a block whose board or parent was dropped.
fn fix_template_block(block: &mut Block, skipped: &mut HashSet<String>) -> bool {
    if skipped.contains(&block.board_id) || skipped.contains(&block.parent_id) {
        skipped.insert(block.id.clone());
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A built-in template as the import stores it, without the onboarding mark.
    fn unmarked() -> Board {
        Board {
            id: "b-built-in".to_owned(),
            team_id: "t-root".to_owned(),
            is_template: true,
            is_builtin: true,
            template_version: DEFAULT_TEMPLATE_VERSION,
            board_type: BoardType::Open,
            ..Board::default()
        }
    }

    /// The built-in template that carries the onboarding mark.
    fn marked() -> Board {
        let mut board = unmarked();
        board
            .properties
            .insert(ONBOARDING_PROPERTY.to_owned(), Value::Bool(true));
        board
    }

    #[test]
    fn only_a_marked_template_of_the_system_is_the_onboarding_template() {
        assert!(is_onboarding_template(&marked()), "the built-in one");
        assert!(!is_onboarding_template(&unmarked()), "without the mark");

        let by_a_user = Board {
            created_by: "u-somebody".to_owned(),
            is_builtin: false,
            ..marked()
        };
        assert!(
            !is_onboarding_template(&by_a_user),
            "a user's board with the mark and the title"
        );
        let no_template = Board {
            is_template: false,
            ..marked()
        };
        assert!(!is_onboarding_template(&no_template), "not a template");
        let mut marked_otherwise = unmarked();
        marked_otherwise.properties.insert(
            ONBOARDING_PROPERTY.to_owned(),
            Value::String("true".to_owned()),
        );
        assert!(
            !is_onboarding_template(&marked_otherwise),
            "the mark is the boolean true"
        );
    }

    #[test]
    fn the_archive_board_with_the_welcome_title_gets_the_mark() {
        let mut skipped = HashSet::new();
        let mut welcome = Board {
            title: ARCHIVE_WELCOME_BOARD_TITLE.to_owned(),
            ..Board::default()
        };
        assert!(fix_template_board(&mut welcome, &mut skipped), "kept");
        assert_eq!(
            welcome.properties.get(ONBOARDING_PROPERTY),
            Some(&Value::Bool(true)),
            "the welcome board is marked"
        );

        let mut other = Board {
            title: "Roadmap".to_owned(),
            ..Board::default()
        };
        assert!(fix_template_board(&mut other, &mut skipped), "kept");
        assert_eq!(
            other.properties.get(ONBOARDING_PROPERTY),
            None,
            "another template is not"
        );
    }

    #[test]
    fn templates_are_imported_when_missing_old_or_unmarked() {
        assert_eq!(
            initialization_reason(&[]),
            Some("no default templates found"),
            "none"
        );
        let old = Board {
            template_version: DEFAULT_TEMPLATE_VERSION - 1,
            ..marked()
        };
        assert_eq!(
            initialization_reason(&[old]),
            Some("template_version too old"),
            "an old one"
        );
        assert_eq!(
            initialization_reason(&[unmarked()]),
            Some("no template is marked for the onboarding tour"),
            "current ones without the mark"
        );
        assert_eq!(
            initialization_reason(&[unmarked(), marked()]),
            None,
            "current ones with the marked board among them"
        );
    }
}
