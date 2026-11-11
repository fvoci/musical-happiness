//! The built-in template boards.
//!
//! Ports `server/app/templates.go`. The templates are the files of this crate's
//! `assets/templates/` (`docs/s6-design.md` 3.10 and 5.7), embedded in the binary; the
//! web app's in-memory backend reads the same files. They are stored once, when a
//! database has none or an older version of them.

use fvoci_model::{
    Block, BlockType, Board, BoardType, BoardsAndBlocks, renew_boards_and_blocks_ids,
};
use fvoci_store::{SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, Tenant};
use serde::Deserialize;
use serde_json::Value;

use super::App;
use crate::error::AppError;

/// The current version of the built-in templates. Raising it stores them again.
const DEFAULT_TEMPLATE_VERSION: i64 = 7;

/// The board property that marks the built-in template the onboarding tour copies.
///
/// A property alone proves nothing, since any user sets the properties of their own
/// boards; see [`is_onboarding_template`].
pub(crate) const ONBOARDING_PROPERTY: &str = "onboarding";

/// The board property that names a built-in template, as its file does (`key`). It
/// stays when the title changes; a copy carries it too, so it names a template only
/// together with `isBuiltin`.
const TEMPLATE_KEY_PROPERTY: &str = "templateKey";

/// The template files of `assets/templates/`, by name, in the order of their names.
const TEMPLATE_FILES: [(&str, &str); 13] = [
    (
        "01-meeting-agenda.json",
        include_str!("../../assets/templates/01-meeting-agenda.json"),
    ),
    (
        "02-sales-pipeline-crm.json",
        include_str!("../../assets/templates/02-sales-pipeline-crm.json"),
    ),
    (
        "03-personal-tasks.json",
        include_str!("../../assets/templates/03-personal-tasks.json"),
    ),
    (
        "04-project-tasks.json",
        include_str!("../../assets/templates/04-project-tasks.json"),
    ),
    (
        "05-company-goals-okrs.json",
        include_str!("../../assets/templates/05-company-goals-okrs.json"),
    ),
    (
        "06-personal-goals.json",
        include_str!("../../assets/templates/06-personal-goals.json"),
    ),
    (
        "07-sprint-planner.json",
        include_str!("../../assets/templates/07-sprint-planner.json"),
    ),
    (
        "08-user-research-sessions.json",
        include_str!("../../assets/templates/08-user-research-sessions.json"),
    ),
    (
        "09-competitive-analysis.json",
        include_str!("../../assets/templates/09-competitive-analysis.json"),
    ),
    (
        "10-content-calendar.json",
        include_str!("../../assets/templates/10-content-calendar.json"),
    ),
    (
        "11-team-retrospective.json",
        include_str!("../../assets/templates/11-team-retrospective.json"),
    ),
    (
        "12-roadmap.json",
        include_str!("../../assets/templates/12-roadmap.json"),
    ),
    (
        "13-welcome.json",
        include_str!("../../assets/templates/13-welcome.json"),
    ),
];

/// One template file of `assets/templates/`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    /// A name that stays when the title changes.
    key: String,
    /// Whether this is the board the onboarding tour copies; one template has it.
    onboarding: bool,
    board: TemplateBoard,
    views: Vec<Block>,
    cards: Vec<Block>,
    /// The files the bodies show: none, the built-in templates carry no pictures.
    files: Vec<Value>,
}

/// The board of a template file: what a template says of its board. The rest is the
/// server's to set.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TemplateBoard {
    id: String,
    title: String,
    description: String,
    icon: String,
    show_description: bool,
    card_properties: Vec<fvoci_model::JsonMap>,
}

/// The built-in templates as boards and blocks with new identifiers, ready to be
/// stored for the system in `team_id`.
///
/// # Errors
///
/// Returns a server error if a template file is not of the shape of [`TemplateFile`]:
/// a mistake in the embedded files, which the tests catch.
fn builtin_templates(team_id: &str) -> Result<BoardsAndBlocks, AppError> {
    let mut boards_and_blocks = BoardsAndBlocks::default();
    for (name, text) in TEMPLATE_FILES {
        let template: TemplateFile = serde_json::from_str(text).map_err(|error| {
            AppError::from(
                anyhow::Error::new(error).context(format!("the built-in template {name}")),
            )
        })?;
        if !template.files.is_empty() {
            return Err(AppError::from(anyhow::anyhow!(
                "the built-in template {name} shows files; the templates carry none"
            )));
        }
        let TemplateBoard {
            id,
            title,
            description,
            icon,
            show_description,
            card_properties,
        } = template.board;
        let mut board = Board {
            id,
            team_id: team_id.to_owned(),
            title,
            description,
            icon,
            show_description,
            card_properties,
            is_template: true,
            template_version: DEFAULT_TEMPLATE_VERSION,
            board_type: BoardType::Open,
            ..Board::default()
        };
        board.properties.insert(
            TEMPLATE_KEY_PROPERTY.to_owned(),
            Value::String(template.key),
        );
        if template.onboarding {
            board
                .properties
                .insert(ONBOARDING_PROPERTY.to_owned(), Value::Bool(true));
        }
        for (mut block, block_type) in template
            .views
            .into_iter()
            .map(|view| (view, BlockType::View))
            .chain(
                template
                    .cards
                    .into_iter()
                    .map(|card| (card, BlockType::Card)),
            )
        {
            if block.block_type != block_type {
                return Err(AppError::from(anyhow::anyhow!(
                    "the built-in template {name} has a {} among its {}s",
                    block.block_type.as_str(),
                    block_type.as_str()
                )));
            }
            block.board_id.clone_from(&board.id);
            block.parent_id.clone_from(&board.id);
            boards_and_blocks.blocks.push(block);
        }
        boards_and_blocks.boards.push(board);
    }
    Ok(renew_boards_and_blocks_ids(boards_and_blocks))
}

impl App {
    /// Stores the built-in templates if the database has none, an older version, or
    /// none that is marked for the onboarding tour. Returns whether they were stored.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the templates cannot be read, removed or stored.
    pub async fn init_templates(&self) -> Result<bool, AppError> {
        // The built-in templates are the system organization's, in its workspace:
        // offered to every organization, and nobody's to change.
        let system = self.scoped(Tenant::org(SYSTEM_ORG_ID)?);
        if initialization_reason(&system.store().get_builtin_templates().await?).is_none() {
            tracing::debug!("Template import not needed, skipping");
            return Ok(false);
        }

        let mut templates = builtin_templates(SYSTEM_WORKSPACE_ID)?;
        // One transaction that decides again: of two servers, or onboarding requests,
        // that found the templates missing, one stores them. What is stored on behalf of
        // nobody is the system's own.
        let replaced = system
            .store()
            .replace_builtin_templates(&mut templates, |stored| {
                initialization_reason(stored)
                    .inspect(|reason| tracing::debug!(reason, "Storing the built-in templates"))
                    .is_some()
            })
            .await
            .map_err(|error| {
                AppError::from(error).context("cannot store the built-in templates")
            })?;
        let Some(files) = replaced else {
            return Ok(false);
        };
        // The records of the old templates' files went with them.
        for file in &files {
            if let Err(error) = self.files().delete(&file.path).await {
                tracing::warn!(%error, path = file.path, "a file of a removed built-in template could not be removed");
            }
        }
        system.announce_created(&templates, &[]).await;
        Ok(true)
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
    fn every_template_file_is_read_with_one_marked_for_the_tour() {
        let templates = builtin_templates("t-root").expect("the embedded template files are read");
        assert_eq!(
            templates.boards.len(),
            TEMPLATE_FILES.len(),
            "a board per file"
        );
        let marked = templates
            .boards
            .iter()
            .filter(|board| board.properties.get(ONBOARDING_PROPERTY) == Some(&Value::Bool(true)))
            .count();
        assert_eq!(marked, 1, "one template is marked for the onboarding tour");
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
