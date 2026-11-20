//! The built-in template boards.
//!
//! The templates are the files of this crate's `assets/templates/`, embedded in the
//! binary: each template once in every language the server writes in, with the pictures
//! and documents its bodies show (`assets/templates/files/`). They are stored once,
//! when a database has none or an older version of them.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use fvoci_model::{
    Block, BlockType, Container, ContainerType, ContainersAndBlocks, FileInfo, WorkspaceRole,
    renew_containers_and_blocks_ids,
};
use fvoci_store::{CopyChanges, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, Tenant};
use serde::Deserialize;
use serde_json::Value;

use super::App;
use super::files::{FilePlace, file_info};
use super::language::Language;
use super::loose_objects::Written;
use crate::error::AppError;

/// The current version of the built-in templates. Raising it stores them again.
const DEFAULT_TEMPLATE_VERSION: i64 = 9;

/// The longest the storing of one file of the templates may take. The files are a few
/// kilobytes each, and a server does not serve, nor a request for a first board that
/// finds no template answer, before the storing has ended one way or the other: a file
/// storage that does not answer holds either for this long, not for as long as an
/// upload may take.
const TEMPLATE_FILE_TIMEOUT: Duration = Duration::from_secs(15);

/// The board property that marks the built-in template a new account's first board is
/// copied from: the welcome board, once in each language.
///
/// A property alone proves nothing, since any user sets the properties of their own
/// boards; see [`is_onboarding_template`].
pub(crate) const ONBOARDING_PROPERTY: &str = "onboarding";

/// The board property that names a built-in template, as its file does (`key`). It
/// stays when the title changes. Any user sets the properties of their own boards, so it
/// names a template only together with `isBuiltin`.
const TEMPLATE_KEY_PROPERTY: &str = "templateKey";

/// The board property that says which language a built-in template is written in: a
/// tag of [`Language`], as its file gives it (`language`).
const TEMPLATE_LANGUAGE_PROPERTY: &str = "templateLanguage";

/// The board property that says which day a built-in template was written for, as its
/// file gives it (`baseDate`, `YYYY-MM-DD`): the days of the template are so many days
/// before or after it, and a copy has them as many days before or after the day it is
/// made on.
const TEMPLATE_BASE_DATE_PROPERTY: &str = "templateBaseDate";

/// The properties that say of a board what only a built-in template is: a copy, which
/// is somebody's own board, is made without them.
const TEMPLATE_MARKS: [&str; 4] = [
    ONBOARDING_PROPERTY,
    TEMPLATE_KEY_PROPERTY,
    TEMPLATE_LANGUAGE_PROPERTY,
    TEMPLATE_BASE_DATE_PROPERTY,
];

/// The files of `assets/templates/` under `$folder` that are named, each with what it
/// holds: as text (`text`) or as bytes (`bytes`).
macro_rules! embedded {
    (@text $path:expr) => {
        include_str!($path)
    };
    (@bytes $path:expr) => {
        include_bytes!($path).as_slice()
    };
    ($held:tt, $folder:literal, [$($name:literal),* $(,)?]) => {
        [$((
            $name,
            embedded!(@$held concat!("../../assets/templates/", $folder, $name)),
        )),*]
    };
}

/// The template files of `assets/templates/`, by name, in the order of their names:
/// the order the templates are stored and offered in.
const TEMPLATE_FILES: [(&str, &str); 14] = embedded!(
    text,
    "",
    [
        "01-welcome.en.json",
        "01-welcome.ko.json",
        "02-project-schedule.en.json",
        "02-project-schedule.ko.json",
        "03-issue-tracker.en.json",
        "03-issue-tracker.ko.json",
        "04-meeting-notes.en.json",
        "04-meeting-notes.ko.json",
        "05-content-calendar.en.json",
        "05-content-calendar.ko.json",
        "06-customer-contacts.en.json",
        "06-customer-contacts.ko.json",
        "07-study-notes.en.json",
        "07-study-notes.ko.json",
    ]
);

/// The pictures and documents of `assets/templates/files/` that the templates' bodies
/// show, by name.
const TEMPLATE_ASSETS: [(&str, &[u8]); 7] = embedded!(
    bytes,
    "files/",
    [
        "agenda.en.docx",
        "agenda.ko.docx",
        "board.png",
        "chart.png",
        "timeline.png",
        "viewer-sample.en.docx",
        "viewer-sample.ko.docx",
    ]
);

/// One template file of `assets/templates/`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateFile {
    /// A name that stays when the title changes, the same in every language.
    key: String,
    /// The tag of the language the template is written in.
    language: String,
    /// The day the template is written for: what its days are counted from.
    #[serde(rename = "baseDate")]
    base_date: String,
    /// Whether this is the board a new account starts on; one template has it.
    onboarding: bool,
    board: TemplateBoard,
    views: Vec<Block>,
    cards: Vec<Block>,
    /// The files the bodies show.
    files: Vec<TemplateAsset>,
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

/// A file that the bodies of a template show.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateAsset {
    /// Its name in `assets/templates/files/`, which is what the bodies name it by.
    asset: String,
    /// The name it is shown under.
    name: String,
}

/// The built-in templates as boards and blocks with new identifiers, ready to be
/// stored for the system, and the files their bodies show.
struct BuiltinTemplates {
    boards_and_blocks: ContainersAndBlocks,
    files: Vec<StoredAsset>,
}

/// A file of a built-in template: its record, at a place of its board, and its bytes.
struct StoredAsset {
    info: FileInfo,
    bytes: &'static [u8],
}

/// Why the built-in templates were not stored.
#[derive(Debug)]
pub(super) enum NotStored {
    /// The file storage did not take a file of theirs. It may another time.
    Files(AppError),
    /// Anything else: the database, or a mistake in the embedded files.
    Other(AppError),
}

impl From<NotStored> for AppError {
    fn from(not_stored: NotStored) -> Self {
        match not_stored {
            NotStored::Files(error) | NotStored::Other(error) => error,
        }
    }
}

/// A mistake in the embedded template `name`, which the tests catch.
fn mistake(name: &str, what: impl std::fmt::Display) -> AppError {
    AppError::from(anyhow::anyhow!("the built-in template {name} {what}"))
}

/// The built-in templates with new identifiers, in `workspace_id`, their bodies naming
/// the places their files are to be written at.
///
/// # Errors
///
/// Returns a server error if a template file is not of the shape of [`TemplateFile`],
/// is written in no language of [`Language`], or shows a file otherwise than it says
/// in `files`: mistakes in the embedded files, which the tests catch.
fn builtin_templates(workspace_id: &str) -> Result<BuiltinTemplates, AppError> {
    let mut templates = BuiltinTemplates {
        boards_and_blocks: ContainersAndBlocks::default(),
        files: Vec::new(),
    };
    for (name, text) in TEMPLATE_FILES {
        let template: TemplateFile = serde_json::from_str(text).map_err(|error| {
            AppError::from(
                anyhow::Error::new(error).context(format!("the built-in template {name}")),
            )
        })?;
        let assets = template.files;
        let board = template_board(name, workspace_id, template.board, &template.key)?;
        let mut board = in_language(board, &template.language)
            .ok_or_else(|| mistake(name, "is written in no language of the server"))?;
        if template.onboarding {
            board
                .properties
                .insert(ONBOARDING_PROPERTY.to_owned(), Value::Bool(true));
        }
        if fvoci_model::days_between(&template.base_date, &template.base_date).is_none() {
            return Err(mistake(name, "is written for no day of the calendar"));
        }
        board.properties.insert(
            TEMPLATE_BASE_DATE_PROPERTY.to_owned(),
            Value::String(template.base_date),
        );
        let blocks = template_blocks(name, &board.id, template.views, template.cards)?;

        // Each file by itself: a template has the identifiers of its file in every
        // language, and what is stored has its own.
        let mut renewed = renew_containers_and_blocks_ids(ContainersAndBlocks {
            containers: vec![board],
            blocks,
        });
        let [board] = renewed.containers.as_slice() else {
            return Err(mistake(
                name,
                "is not one board once its identifiers are new",
            ));
        };
        let placed = place_assets(name, &board.id, &assets, renewed.blocks.iter_mut())?;
        templates.files.extend(placed);
        templates
            .boards_and_blocks
            .containers
            .append(&mut renewed.containers);
        templates
            .boards_and_blocks
            .blocks
            .append(&mut renewed.blocks);
    }
    Ok(templates)
}

/// The board of the template `name`, as a built-in template of `workspace_id` that
/// carries its `key`.
fn template_board(
    name: &str,
    workspace_id: &str,
    board: TemplateBoard,
    key: &str,
) -> Result<Container, AppError> {
    if key.is_empty() {
        return Err(mistake(name, "has no key"));
    }
    let TemplateBoard {
        id,
        title,
        description,
        icon,
        show_description,
        card_properties,
    } = board;
    let mut board = Container {
        id,
        workspace_id: workspace_id.to_owned(),
        title,
        description,
        icon,
        show_description,
        card_properties,
        is_template: true,
        template_version: DEFAULT_TEMPLATE_VERSION,
        container_type: ContainerType::Open,
        ..Container::default()
    };
    board.properties.insert(
        TEMPLATE_KEY_PROPERTY.to_owned(),
        Value::String(key.to_owned()),
    );
    Ok(board)
}

/// The board with the language it is written in: `tag`, which has to be exactly a tag
/// of [`Language`]. `None` for any other tag.
fn in_language(mut board: Container, tag: &str) -> Option<Container> {
    let language = Language::written_in(tag).filter(|language| language.tag() == tag)?;
    board.properties.insert(
        TEMPLATE_LANGUAGE_PROPERTY.to_owned(),
        Value::String(language.tag().to_owned()),
    );
    Some(board)
}

/// The views and then the cards of the template `name`, as blocks directly on the
/// board `board_id`.
///
/// # Errors
///
/// Returns a server error if a view is no view or a card no card.
fn template_blocks(
    name: &str,
    board_id: &str,
    views: Vec<Block>,
    cards: Vec<Block>,
) -> Result<Vec<Block>, AppError> {
    let mut blocks = Vec::with_capacity(views.len() + cards.len());
    let views = views.into_iter().map(|view| (view, BlockType::View));
    let cards = cards.into_iter().map(|card| (card, BlockType::Card));
    for (mut block, block_type) in views.chain(cards) {
        if block.block_type != block_type {
            return Err(mistake(
                name,
                format_args!(
                    "has a {} among its {}s",
                    block.block_type.as_str(),
                    block_type.as_str()
                ),
            ));
        }
        board_id.clone_into(&mut block.container_id);
        // Directly on the board: no parent.
        block.parent_id.clear();
        blocks.push(block);
    }
    Ok(blocks)
}

/// Gives each of the `assets` of the template `name` a place among the files of its
/// board, and points the bodies of the board's `blocks` at those places.
///
/// # Errors
///
/// Returns a server error for an asset that is not embedded or that no body shows, and
/// for a body that shows a file the template does not name.
fn place_assets<'b>(
    name: &str,
    board_id: &str,
    assets: &[TemplateAsset],
    blocks: impl Iterator<Item = &'b mut Block>,
) -> Result<Vec<StoredAsset>, AppError> {
    let mut stored = Vec::with_capacity(assets.len());
    let mut stored_names: HashMap<&str, String> = HashMap::with_capacity(assets.len());
    for asset in assets {
        let bytes = TEMPLATE_ASSETS
            .iter()
            .find_map(|(embedded, bytes)| (*embedded == asset.asset).then_some(*bytes))
            .ok_or_else(|| {
                mistake(
                    name,
                    format_args!("names the file {}, which is not embedded", asset.asset),
                )
            })?;
        let place = FilePlace::new(SYSTEM_ORG_ID, board_id, &asset.name);
        let size = u64::try_from(bytes.len()).map_err(|_| {
            mistake(
                name,
                format_args!("names the file {}, which is too large", asset.asset),
            )
        })?;
        let info = file_info(board_id, place, size, "");
        if stored_names
            .insert(&asset.asset, info.stored_name.clone())
            .is_some()
        {
            return Err(mistake(
                name,
                format_args!("names the file {} twice", asset.asset),
            ));
        }
        stored.push(StoredAsset { info, bytes });
    }

    let mut shown: HashSet<&str> = HashSet::with_capacity(assets.len());
    for block in blocks {
        let Some(body) = block.body.take() else {
            continue;
        };
        for file in body.file_ids() {
            let Some((asset, _)) = stored_names.get_key_value(file) else {
                return Err(mistake(
                    name,
                    format_args!("shows the file {file}, which it does not name"),
                ));
            };
            shown.insert(asset);
        }
        block.body = body.with_files_renamed(|asset| stored_names.get(asset).cloned());
    }
    if let Some(unused) = assets
        .iter()
        .find(|asset| !shown.contains(asset.asset.as_str()))
    {
        return Err(mistake(
            name,
            format_args!("names the file {}, which no body shows", unused.asset),
        ));
    }
    Ok(stored)
}

impl App {
    /// Stores the built-in templates if the database has none, an older version, or
    /// no welcome board in some language. Returns whether they were stored.
    ///
    /// Their files are written first, at marked places, and recorded in the transaction
    /// that stores the templates: a server that stops in between leaves marks that the
    /// housekeeping removes with their objects, and no template whose file is missing.
    ///
    /// # Errors
    ///
    /// Returns a server error if a file cannot be written, and the store's error if the
    /// templates cannot be read, removed or stored.
    pub async fn init_templates(&self) -> Result<bool, AppError> {
        Ok(self.try_init_templates().await?)
    }

    /// [`App::init_templates`], saying of a failure whether it was the file storage's.
    /// Templates that are stored, or found current, are owed no more.
    pub(super) async fn try_init_templates(&self) -> Result<bool, NotStored> {
        let stored = self.store_templates_if_needed().await?;
        self.inner.template_stock.settle();
        Ok(stored)
    }

    async fn store_templates_if_needed(&self) -> Result<bool, NotStored> {
        let other = |error: AppError| NotStored::Other(error);
        // The built-in templates are the system organization's, in its workspace:
        // offered to every organization, and nobody's to change.
        let system = self.scoped(Tenant::org(SYSTEM_ORG_ID).map_err(|error| other(error.into()))?);
        let stored = system
            .store()
            .get_builtin_templates()
            .await
            .map_err(|error| other(error.into()))?;
        if initialization_reason(&stored).is_none() {
            tracing::debug!("Template import not needed, skipping");
            return Ok(false);
        }

        let BuiltinTemplates {
            boards_and_blocks: mut templates,
            files,
        } = builtin_templates(SYSTEM_WORKSPACE_ID).map_err(other)?;
        if let Err(error) = system.write_template_files(&files).await {
            // The write that failed may yet appear, its answer lost.
            system
                .give_up_template_files(&files, Written::Unsettled)
                .await;
            return Err(error);
        }
        let records: Vec<FileInfo> = files.iter().map(|file| file.info.clone()).collect();
        // One transaction that decides again: of two servers, or requests for a first
        // board, that found the templates missing, one stores them. What is stored on
        // behalf of nobody is the system's own.
        let replaced = system
            .store()
            .replace_builtin_templates(&mut templates, &records, |stored| {
                initialization_reason(stored)
                    .inspect(|reason| tracing::debug!(reason, "Storing the built-in templates"))
                    .is_some()
            })
            .await;
        match replaced {
            // The records of the old templates' files went with them, and their
            // objects are doomed: the housekeeping removes them.
            Ok(Some(_)) => {
                system.announce_created(&templates, &[]).await;
                Ok(true)
            }
            // Every write is over: what was written is removed now.
            Ok(None) => {
                system
                    .give_up_template_files(&files, Written::Settled)
                    .await;
                Ok(false)
            }
            Err(error) => {
                system
                    .give_up_template_files(&files, Written::Settled)
                    .await;
                Err(other(
                    AppError::from(error).context("cannot store the built-in templates"),
                ))
            }
        }
    }

    /// Writes the files of the templates, each at its place, marked as being written.
    async fn write_template_files(&self, files: &[StoredAsset]) -> Result<(), NotStored> {
        for file in files {
            let path = &file.info.path;
            self.mark_writing(path).await.map_err(NotStored::Other)?;
            let mut bytes = file.bytes;
            let write = self.files().write(path, &mut bytes);
            match tokio::time::timeout(TEMPLATE_FILE_TIMEOUT, write).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    return Err(NotStored::Files(AppError::internal(format_args!(
                        "unable to store a file of the built-in templates at {path}: {error}"
                    ))));
                }
                Err(_) => {
                    return Err(NotStored::Files(AppError::internal(format_args!(
                        "storing a file of the built-in templates at {path} took longer than {} seconds",
                        TEMPLATE_FILE_TIMEOUT.as_secs()
                    ))));
                }
            }
        }
        Ok(())
    }

    /// Gives up the files of templates that are not stored after all, `written` saying
    /// whether a write of theirs may yet appear. One that was never marked, or is
    /// recorded (the templates were stored, with the answer lost), is left alone.
    async fn give_up_template_files(&self, files: &[StoredAsset], written: Written) {
        for file in files {
            self.give_up_writing(&file.info.path, written).await;
        }
    }

    /// Returns the template boards of a workspace, for the templates route: for an
    /// administrator of the workspace (`role`) every one of it, for anybody else those
    /// that are open or that the user is a member of; and the built-in ones that are
    /// written in the language the server writes in for the user
    /// (`App::language_for`, by `accept_language`, the request's `Accept-Language`).
    ///
    /// # Errors
    ///
    /// Returns the store's error if a query fails.
    pub async fn template_boards(
        &self,
        workspace_id: &str,
        user_id: &str,
        role: WorkspaceRole,
        accept_language: Option<&str>,
    ) -> Result<Vec<Container>, AppError> {
        let language = self.language_for(user_id, accept_language).await?;
        // What is owed is not waited for: the list has what the database holds.
        self.restock_templates_soon();
        let mut boards = if role == WorkspaceRole::Admin {
            self.store()
                .get_templates_of_workspace(workspace_id)
                .await?
        } else {
            self.store()
                .get_template_boards(workspace_id, user_id)
                .await?
        };
        let builtin = self.store().get_builtin_templates().await?;
        boards.extend(
            builtin
                .into_iter()
                .filter(|board| written_in(board) == Some(language)),
        );
        Ok(boards)
    }
}

/// Returns whether a board is a built-in template that a new account's first board is
/// copied from: a template of the system organization that the system created and
/// marked.
///
/// The system's authorship is what cannot be forged. A board's creator is always the
/// session's user, on every route that makes a board; nobody signs in as the system.
pub(crate) fn is_onboarding_template(board: &Container) -> bool {
    board.is_template
        && board.is_builtin
        && board.properties.get(ONBOARDING_PROPERTY) == Some(&Value::Bool(true))
}

/// What a copy of `source` made on the day `today` (`YYYY-MM-DD`) does not take from it
/// as it stands. Of a built-in template, the marks that say it is one, and its days:
/// they are moved by as many days as `today` is after the day the template was written
/// for, and stay where `today` is unknown. Of any other board, nothing.
pub(super) fn changes_of_copy(source: &Container, today: Option<&str>) -> CopyChanges<'static> {
    if !(source.is_template && source.is_builtin) {
        return CopyChanges::default();
    }
    let written_for = source
        .properties
        .get(TEMPLATE_BASE_DATE_PROPERTY)
        .and_then(Value::as_str);
    let days_later = written_for
        .zip(today)
        .and_then(|(written_for, today)| fvoci_model::days_between(written_for, today))
        .unwrap_or_default();
    CopyChanges {
        without_properties: &TEMPLATE_MARKS,
        days_later,
    }
}

/// The language a built-in template says it is written in.
pub(crate) fn written_in(board: &Container) -> Option<Language> {
    board
        .properties
        .get(TEMPLATE_LANGUAGE_PROPERTY)
        .and_then(Value::as_str)
        .and_then(Language::written_in)
}

/// Why the templates must be imported, or `None` if they are current. The templates are
/// needed when there are none, when a built-in one has an older version, or when a
/// language has no welcome board among them.
fn initialization_reason(boards: &[Container]) -> Option<&'static str> {
    if boards.is_empty() {
        return Some("no default templates found");
    }
    for board in boards {
        if board.template_version < DEFAULT_TEMPLATE_VERSION {
            return Some("template_version too old");
        }
    }
    let welcomes = |language: &Language| {
        boards
            .iter()
            .any(|board| is_onboarding_template(board) && written_in(board) == Some(*language))
    };
    if !Language::ALL.iter().all(welcomes) {
        return Some("a language has no welcome board");
    }
    None
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;

    /// A built-in template as the import stores it, in English, without the mark of
    /// the welcome board.
    fn unmarked() -> Container {
        let mut board = Container {
            id: "b-built-in".to_owned(),
            workspace_id: "t-root".to_owned(),
            is_template: true,
            is_builtin: true,
            template_version: DEFAULT_TEMPLATE_VERSION,
            container_type: ContainerType::Open,
            ..Container::default()
        };
        board.properties.insert(
            TEMPLATE_LANGUAGE_PROPERTY.to_owned(),
            Value::String("en".to_owned()),
        );
        board
    }

    /// The built-in welcome board of a language.
    fn marked(language: Language) -> Container {
        let mut board = unmarked();
        board
            .properties
            .insert(ONBOARDING_PROPERTY.to_owned(), Value::Bool(true));
        board.properties.insert(
            TEMPLATE_LANGUAGE_PROPERTY.to_owned(),
            Value::String(language.tag().to_owned()),
        );
        board
    }

    /// The template files as JSON, by key and then by the tag of their language.
    fn files_by_key() -> BTreeMap<String, BTreeMap<String, Value>> {
        let mut files: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
        for (name, text) in TEMPLATE_FILES {
            let file: Value = serde_json::from_str(text).expect(name);
            let key = file["key"].as_str().expect(name).to_owned();
            let language = file["language"].as_str().expect(name).to_owned();
            assert!(
                files
                    .entry(key)
                    .or_default()
                    .insert(language, file)
                    .is_none(),
                "{name} is written twice in one language"
            );
        }
        files
    }

    /// What a template has that is the same in every language: its property
    /// definitions without their names, its views without their titles, and its cards
    /// without their titles, their texts and the names of their files.
    fn skeleton(file: &Value) -> Value {
        let text_kinds = ["text", "email", "phone", "url"];
        let properties = file["board"]["cardProperties"]
            .as_array()
            .expect("the properties");
        let written: BTreeSet<&str> = properties
            .iter()
            .filter(|property| text_kinds.contains(&property["type"].as_str().expect("a type")))
            .map(|property| property["id"].as_str().expect("an identifier"))
            .collect();
        let definitions: Vec<Value> = properties
            .iter()
            .map(|property| {
                let options: Vec<Value> = property["options"]
                    .as_array()
                    .expect("the options")
                    .iter()
                    .map(|option| serde_json::json!([option["id"], option["color"]]))
                    .collect();
                serde_json::json!([property["id"], property["type"], options])
            })
            .collect();
        let views: Vec<Value> = file["views"]
            .as_array()
            .expect("the views")
            .iter()
            .map(|view| serde_json::json!([view["id"], view["fields"]]))
            .collect();
        let cards: Vec<Value> = file["cards"]
            .as_array()
            .expect("the cards")
            .iter()
            .map(|card| {
                let values: BTreeMap<&str, &Value> = card["fields"]["properties"]
                    .as_object()
                    .expect("the values")
                    .iter()
                    .filter(|(id, _)| !written.contains(id.as_str()))
                    .map(|(id, value)| (id.as_str(), value))
                    .collect();
                let mut nodes = Vec::new();
                outline(&card["body"], &mut nodes);
                serde_json::json!([
                    card["id"],
                    card["fields"]["icon"],
                    card["fields"]["isTemplate"],
                    values,
                    nodes
                ])
            })
            .collect();
        serde_json::json!({
            "onboarding": file["onboarding"],
            "board": [file["board"]["id"], file["board"]["icon"], definitions],
            "views": views,
            "cards": cards,
            "files": file["files"].as_array().expect("the files").len(),
        })
    }

    /// The nodes of a body in the order they are written in, each with what it has
    /// that no language changes: its level, whether it is checked, what it names.
    fn outline(node: &Value, nodes: &mut Vec<Value>) {
        let Some(kind) = node["type"].as_str() else {
            return;
        };
        if kind != "text" {
            let attrs = &node["attrs"];
            nodes.push(serde_json::json!([
                kind,
                attrs["level"],
                attrs["checked"],
                attrs["id"],
                attrs["latex"]
            ]));
        }
        for child in node["content"].as_array().into_iter().flatten() {
            outline(child, nodes);
        }
    }

    /// The type of every node of the bodies of the template files.
    fn node_types() -> BTreeSet<String> {
        fn gather(node: &Value, types: &mut BTreeSet<String>) {
            if let Some(kind) = node["type"].as_str() {
                types.insert(kind.to_owned());
            }
            for child in node["content"].as_array().into_iter().flatten() {
                gather(child, types);
            }
        }
        let mut types = BTreeSet::new();
        for (name, text) in TEMPLATE_FILES {
            let file: Value = serde_json::from_str(text).expect(name);
            for card in file["cards"].as_array().expect(name) {
                gather(&card["body"], &mut types);
            }
        }
        types
    }

    #[test]
    fn every_template_file_is_read_with_a_welcome_board_in_each_language() {
        let templates = builtin_templates("t-root").expect("the embedded template files are read");
        let boards = &templates.boards_and_blocks.containers;
        assert_eq!(boards.len(), TEMPLATE_FILES.len(), "a board per file");
        for language in Language::ALL {
            let marked = boards
                .iter()
                .filter(|board| {
                    written_in(board) == Some(language)
                        && board.properties.get(ONBOARDING_PROPERTY) == Some(&Value::Bool(true))
                })
                .count();
            assert_eq!(marked, 1, "one welcome board in {}", language.tag());
        }
        assert_eq!(
            initialization_reason(
                &boards
                    .iter()
                    .cloned()
                    .map(|board| Container {
                        is_builtin: true,
                        ..board
                    })
                    .collect::<Vec<_>>()
            ),
            None,
            "once stored they are current"
        );
    }

    #[test]
    fn every_template_is_written_in_every_language_and_differs_in_its_words_only() {
        for (key, written) in files_by_key() {
            let languages: Vec<&str> = written.keys().map(String::as_str).collect();
            let mut expected: Vec<&str> = Language::ALL.iter().map(|each| each.tag()).collect();
            expected.sort_unstable();
            assert_eq!(languages, expected, "the languages of {key}");
            let mut skeletons = written.values().map(skeleton);
            let first = skeletons.next().expect("a language");
            for other in skeletons {
                assert_eq!(other, first, "{key} is the same template in each language");
            }
        }
    }

    #[test]
    fn the_bodies_point_at_the_places_of_their_files() {
        let templates = builtin_templates("t-root").expect("the embedded template files are read");
        assert!(!templates.files.is_empty(), "the templates show files");
        let mut shown: BTreeSet<(String, String)> = BTreeSet::new();
        for block in &templates.boards_and_blocks.blocks {
            for file in block.shown_files() {
                shown.insert((block.container_id.clone(), file));
            }
        }
        let stored: BTreeSet<(String, String)> = templates
            .files
            .iter()
            .map(|file| {
                (
                    file.info.container_id.clone(),
                    file.info.stored_name.clone(),
                )
            })
            .collect();
        assert_eq!(
            shown, stored,
            "each file shown is one of its board, and stored"
        );
        for file in &templates.files {
            assert_eq!(
                usize::try_from(file.info.size).ok(),
                Some(file.bytes.len()),
                "{} is recorded at its size",
                file.info.name
            );
            assert!(
                fvoci_model::is_storage_path(&file.info.path),
                "{} is a place of the storage",
                file.info.path
            );
        }
        let used: BTreeSet<&[u8]> = templates.files.iter().map(|file| file.bytes).collect();
        for (name, bytes) in TEMPLATE_ASSETS {
            assert!(used.contains(bytes), "{name} is embedded and shown nowhere");
        }
    }

    /// The node types a body may hold that no template shows, each with why. Anything
    /// else the allowlist takes has to be in a template: a node that is added to the
    /// allowlist fails this test until a template shows it or it is listed here.
    const NODES_NOT_SHOWN: [(&str, &str); 1] = [(
        "mention",
        "names a user, and a template is copied to people it knows nothing of",
    )];

    #[test]
    fn the_templates_show_every_node_a_body_may_hold() {
        let shown = node_types();
        for node in fvoci_model::body_node_types() {
            let left_out = NODES_NOT_SHOWN.iter().any(|(name, _)| *name == node);
            assert_ne!(
                shown.contains(node),
                left_out,
                "{node} is shown by a template, or listed as not shown, and not both"
            );
        }
    }

    #[test]
    fn only_a_marked_template_of_the_system_is_the_onboarding_template() {
        assert!(
            is_onboarding_template(&marked(Language::English)),
            "the built-in one"
        );
        assert!(!is_onboarding_template(&unmarked()), "without the mark");

        let by_a_user = Container {
            created_by: "u-somebody".to_owned(),
            is_builtin: false,
            ..marked(Language::English)
        };
        assert!(
            !is_onboarding_template(&by_a_user),
            "a user's board with the mark and the title"
        );
        let no_template = Container {
            is_template: false,
            ..marked(Language::English)
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
    fn templates_are_imported_when_missing_old_or_without_a_welcome_board() {
        assert_eq!(
            initialization_reason(&[]),
            Some("no default templates found"),
            "none"
        );
        let old = Container {
            template_version: DEFAULT_TEMPLATE_VERSION - 1,
            ..marked(Language::English)
        };
        assert_eq!(
            initialization_reason(&[old]),
            Some("template_version too old"),
            "an old one"
        );
        assert_eq!(
            initialization_reason(&[unmarked()]),
            Some("a language has no welcome board"),
            "current ones without the mark"
        );
        assert_eq!(
            initialization_reason(&[unmarked(), marked(Language::English)]),
            Some("a language has no welcome board"),
            "a welcome board in one language only"
        );
        assert_eq!(
            initialization_reason(&[
                unmarked(),
                marked(Language::English),
                marked(Language::Korean)
            ]),
            None,
            "current ones with a welcome board in each language"
        );
    }
}
