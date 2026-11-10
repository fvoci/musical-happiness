//! The built-in template boards.
//!
//! Ports `server/app/templates.go`. The default templates are imported from an archive
//! embedded in the binary, once, when a database has none or an older version of them.

use std::collections::HashSet;

use bytes::Bytes;
use fvoci_model::{Block, Board, BoardType, GLOBAL_TEAM_ID, SYSTEM_USER_ID};

use super::App;
use super::import::ImportOptions;
use crate::error::AppError;

/// The current version of the built-in templates. Raising it re-imports them.
const DEFAULT_TEMPLATE_VERSION: i64 = 6;

/// The embedded default-templates archive, read from the original's asset at build time.
const DEFAULT_TEMPLATES_ARCHIVE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../server/assets/templates.boardarchive"
));

impl App {
    /// Imports the default templates if the database has none or an older version.
    /// Returns whether an import was done.
    ///
    /// A database written by the original, already holding the current templates, is
    /// accepted unchanged.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the templates cannot be read or removed, or an
    /// import error if the archive cannot be imported.
    pub async fn init_templates(&self) -> Result<bool, AppError> {
        let boards = self.store().get_template_boards(GLOBAL_TEAM_ID, "").await?;
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

        self.store().remove_default_templates(&boards).await?;

        let options = ImportOptions {
            team_id: GLOBAL_TEAM_ID.to_owned(),
            modified_by: SYSTEM_USER_ID.to_owned(),
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
            return Err(error.context(&format!(
                "cannot initialize global templates for team {GLOBAL_TEAM_ID}"
            )));
        }
        Ok(true)
    }

    /// Removes the built-in templates that a failed import left behind.
    async fn remove_partial_templates(&self) {
        match self.store().get_template_boards(GLOBAL_TEAM_ID, "").await {
            Ok(partial) => {
                if let Err(error) = self.store().remove_default_templates(&partial).await {
                    tracing::error!(%error, "cannot remove partially imported templates");
                }
            }
            Err(error) => tracing::error!(%error, "cannot list partially imported templates"),
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
        Ok(self.store().get_template_boards(team_id, user_id).await?)
    }
}

/// Why the templates must be imported, or `None` if they are current. The templates are
/// needed when there are none, or when a built-in one has an older version.
fn initialization_reason(boards: &[Board]) -> Option<&'static str> {
    if boards.is_empty() {
        return Some("no default templates found");
    }
    for board in boards {
        if board.created_by != SYSTEM_USER_ID {
            continue;
        }
        if board.template_version < DEFAULT_TEMPLATE_VERSION {
            return Some("template_version too old");
        }
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
