//! Importing boards and wikis from the archive format of version 3 (`super::export`).
//!
//! Ports `server/app/import.go`. An import generates fresh identifiers for everything,
//! fixes up references, stores the files, points the blocks at them and adds the
//! importing user as an admin. A wiki is made as a wiki, its pages held to the rules of
//! their tree before anything is stored (`places`). An archive of another version is
//! refused (`docs/s6-design.md` 3.10).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bytes::Bytes;
use fvoci_model::{
    ArchiveHeader, ArchiveLine, Block, Container, ContainerKind, ContainerMember,
    ContainersAndBlocks, ContainersAndBlocksError, JsonMap, PageLimits, Renewed, WorkspaceRole,
    renew_containers_and_blocks_ids_told,
};
use serde_json::Value;

use super::App;
use super::export::ARCHIVE_VERSION;
use super::file_name::check_file_name;
use super::files::stored_extension;
use crate::auth::now_millis;
use crate::error::{AppError, ErrorKind};
use crate::permissions;
use archive::{
    Archive, check_entry, entry_limit, entry_name, open_archive, read_entry, too_many_entries,
};
use fvoci_store::StoreError;

/// The most an archive's entries may decompress to together (`docs/s6-design.md` 4.2):
/// what keeps a small archive from unfolding into a disk's worth.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// The most entries an archive has (`docs/s6-design.md` 4.2).
const MAX_ENTRIES: u64 = 10_000;

/// The largest `board.jsonl` that is read (`docs/s6-design.md` 4.2). It is read whole
/// and held as blocks until the board is stored: bodies of many small nodes take 33
/// times their bytes so (2.2 GB at this limit, release build), whatever the limit of a
/// body. So it holds 16 bodies of the largest size, and a board of more is exported but
/// not imported again.
const MAX_JSONL_BYTES: u64 = 64 * 1024 * 1024;

/// The largest `version.json` that is read: it is one small object.
const MAX_VERSION_BYTES: u64 = 64 * 1024;

/// How an archive is imported.
#[derive(Debug, Default)]
pub(crate) struct ImportOptions {
    /// The workspace the boards are imported into.
    pub(crate) workspace_id: String,
    /// The user who imports, recorded as the modifier and added as an admin.
    pub(crate) modified_by: String,
    /// How large the tree of a wiki of the archive may be: the organization's limits.
    pub(crate) page_limits: PageLimits,
}

impl App {
    /// Imports an archive or external export file (`fvoci`, `trello`, or `notion`) at
    /// `path` into `workspace_id` on behalf of `modified_by`, returning a structured
    /// summary of created items, skipped items, and non-fatal warnings.
    ///
    /// # Errors
    ///
    /// Returns 404 if `workspace_id` does not exist in this organization or `modified_by`
    /// is not a non-guest member; 400 or 413 if the input fails security, format, size,
    /// depth, or allowlist validation; and 500 on storage or database failure. For
    /// `trello` and `notion`, any error leaves zero containers, blocks, and files stored.
    pub async fn import_external_archive(
        &self,
        path: &Path,
        format: ImportFormat,
        workspace_id: &str,
        modified_by: &str,
    ) -> Result<ImportSummary, AppError> {
        let largest_file = u64::try_from(self.limits().largest_file).unwrap_or(0);
        let limits = BundleLimits::for_largest_file(largest_file);
        self.import_external_archive_with_limits(path, format, workspace_id, modified_by, limits)
            .await
    }

    /// Like [`App::import_external_archive`], with caller-supplied [`BundleLimits`] applied
    /// to external formats (`trello` and `notion`). Native `fvoci` archives use the
    /// server's fixed native archive bounds and organization file limit.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::import_external_archive`].
    pub async fn import_external_archive_with_limits(
        &self,
        path: &Path,
        format: ImportFormat,
        workspace_id: &str,
        modified_by: &str,
        limits: BundleLimits,
    ) -> Result<ImportSummary, AppError> {
        self.require_import_workspace(workspace_id, modified_by)
            .await?;
        let largest_file = u64::try_from(self.limits().largest_file).unwrap_or(0);
        let limits = BundleLimits {
            max_file_bytes: limits.max_file_bytes.min(largest_file),
            ..limits
        };
        let options = ImportOptions {
            workspace_id: workspace_id.to_owned(),
            modified_by: modified_by.to_owned(),
            page_limits: self.limits().pages,
        };
        match format {
            ImportFormat::Fvoci => self.import_native_summary(path, &options).await,
            ImportFormat::Trello => {
                let staged = trello::convert_trello_file(path, &limits).await?;
                self.commit_staged_archive(staged, None, &options, &limits)
                    .await
            }
            ImportFormat::Notion => {
                let temp_root = self.config().tmp_dir();
                let bundle = bundle::SafeBundle::open(path, &temp_root, limits).await?;
                let staged = notion::convert_notion_bundle(&bundle, &limits).await?;
                self.commit_staged_archive(staged, Some(&bundle), &options, &limits)
                    .await
            }
        }
    }

    /// Runs the native fvoci v3 archive import and wraps its outcome in [`ImportSummary`].
    async fn import_native_summary(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> Result<ImportSummary, AppError> {
        let (imported, summary_counts, container_ids) =
            self.import_archive_detailed(path, options).await?;
        Ok(ImportSummary {
            format: ImportFormat::Fvoci,
            imported: summary_counts,
            members_left_out: imported.members_left_out,
            container_ids,
            ..ImportSummary::default()
        })
    }

    /// Imports the native fvoci v3 archive in the file at `path`, of zero or more boards
    /// with their blocks and files. The archive is read from the file at random, an entry
    /// at a time; file entries are unpacked into a temporary staging directory under
    /// `tmp_dir` (`import-stage-<secret>/entry`) and committed atomically with all
    /// containers, blocks, and memberships in a single transaction.
    async fn import_archive_detailed(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> Result<(Imported, ImportCounts, Vec<String>), AppError> {
        let zip = open_archive(path).await?;
        // The original's streaming reader rejects a ZIP with no entries as invalid.
        if zip.is_empty() {
            return Err(AppError::bad_request("zip: not a valid zip file"));
        }
        // Not reached while the headers are counted before the directory is read (see
        // `open_archive`); kept as the check of what was read.
        if u64::try_from(zip.len()).map_or(true, |entries| entries > MAX_ENTRIES) {
            return Err(too_many_entries());
        }
        let largest_file = u64::try_from(self.limits().largest_file).unwrap_or(0);

        // The version first: nothing of an archive of another version is stored.
        let version = (0..zip.len())
            .map(|index| entry_name(&zip, index).map(|name| (index, name)))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|(_, (dir, filename))| dir == "." && filename == "version.json")
            .map(|(index, _)| index)
            .ok_or_else(|| {
                AppError::bad_request(format_args!(
                    "the archive has no version.json; this server reads version {ARCHIVE_VERSION}"
                ))
            })?;
        // A `version.json` too large to be one is no archive of this server's, as one of
        // another version is not.
        let version = read_entry(&zip, version, MAX_VERSION_BYTES)
            .await
            .map_err(|error| {
                if error.kind() == ErrorKind::PayloadTooLarge {
                    AppError::bad_request("version.json is not that of an archive")
                } else {
                    error
                }
            })?;
        check_version(&version)?;
        check_native_unpacked_budget(&zip, largest_file).await?;

        let mut containers = Vec::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            if filename == "board.jsonl" {
                containers.push((index, dir));
            }
        }
        let (elsewhere, diaries) = named_elsewhere(&zip, &containers, options).await?;
        self.commit_native_archive(&zip, containers, options, &elsewhere, diaries, largest_file)
            .await
    }
}

/// What an import did beyond making the archive's boards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Imported {
    /// How many of the users who were members of the archive's boards are no members of
    /// them now: they are not in the workspace (each counted once).
    pub members_left_out: usize,
}

/// The membership with its role lowered to the highest the user may hold in the
/// workspace (`permissions::highest_container_role`), and left as it is if it is not higher.
fn no_more_than_holdable(
    member: ContainerMember,
    workspace_role: WorkspaceRole,
) -> ContainerMember {
    let highest = permissions::highest_container_role(workspace_role);
    match permissions::role_of(&member) {
        Some(role) if role > highest => permissions::with_role(member, Some(highest)),
        _ => member,
    }
}

/// One board of an archive, read and checked, with new identifiers, before it is
/// stored.
struct ParsedContainer {
    containers_and_blocks: ContainersAndBlocks,
    members: Vec<ContainerMember>,
    /// The new identifier of each of its blocks that was asked for, by its old one.
    renamed: HashMap<String, String>,
    /// What its board lines say of its files.
    files: ArchiveFiles,
}

/// The name of each file of a directory of an archive, by the name of its entry.
type FileNames = HashMap<String, String>;

/// What the board lines of a directory of an archive say of its files.
#[derive(Debug, Default)]
struct ArchiveFiles {
    /// The name of each file, by the name of its entry ([`file_names_of`]).
    names: FileNames,
    /// The entries the export did not find (`missingFiles`, [`missing_files_of`]): none
    /// is stored, and what shows them shows no file.
    missing: HashSet<String>,
}

impl ParsedContainer {
    /// What its bodies name that is no block of its own: what was a block of its own
    /// is named by its new identifier, which no archive holds.
    fn named_elsewhere(&self) -> impl Iterator<Item = String> + '_ {
        let blocks = &self.containers_and_blocks.blocks;
        let own: HashSet<&str> = blocks.iter().map(|block| block.id.as_str()).collect();
        let named = blocks
            .iter()
            .filter_map(|block| block.body.as_ref())
            .flat_map(fvoci_model::Body::reference_ids);
        named.filter(move |id| !own.contains(id)).map(str::to_owned)
    }
}

/// Reads and checks every board of the archive ([`parse_board_jsonl`]) as the store
/// checks it, before the first is stored: an archive that one of them refuses stores
/// nothing. Returns what the bodies name that is no block of their own board (which
/// may be a block of another board of the archive, whose new identifier is recorded when
/// the archive is prepared for its single-transaction commit) and how many diaries the
/// archive holds.
///
/// # Errors
///
/// As [`parse_board_jsonl`], and a bad request for an entry that cannot be read.
async fn named_elsewhere(
    zip: &Archive,
    containers: &[(usize, String)],
    options: &ImportOptions,
) -> Result<(HashSet<String>, u32), AppError> {
    let mut elsewhere = HashSet::new();
    let mut diaries: u32 = 0;
    for (index, _) in containers {
        let jsonl = read_entry(zip, *index, MAX_JSONL_BYTES).await?;
        let container = parse_board_jsonl(jsonl, options, &HashSet::new()).await?;
        let kinds = container.containers_and_blocks.containers.iter();
        let of_diaries = kinds.filter(|one| one.kind == ContainerKind::Diary).count();
        diaries = diaries.saturating_add(u32::try_from(of_diaries).unwrap_or(u32::MAX));
        elsewhere.extend(container.named_elsewhere());
    }
    Ok((elsewhere, diaries))
}

/// The name a file of the archive is stored with: the one the board line of its
/// directory gives it ([`file_names_of`]), or failing that the name of its entry.
fn name_in<'a>(files: &'a HashMap<String, ArchiveFiles>, dir: &str, filename: &'a str) -> &'a str {
    files
        .get(dir)
        .and_then(|said| said.names.get(filename))
        .map_or(filename, String::as_str)
}

/// Reads and checks one board's `board.jsonl`: its lines, the types of its blocks,
/// whether its boards and blocks fit together, and each board and block as the store
/// checks it (the card property definitions of a board; the title, the fields with the
/// limit of their type, the property values and the body of a block). Values of the
/// other kind are cleared, blocks that nothing leads to are dropped, and everything gets
/// a new identifier; those of the blocks among `told` are given back by the old ones.
///
/// The parent of each block is checked against the archive as the store would check
/// it against what is stored ([`places::check`]): what the store refuses of an archive
/// is refused before any of it is stored.
///
/// # Errors
///
/// Returns a bad request if a line is malformed or of an unknown
/// type, a block is not where the kind of its board lets it be ([`places::check`]), the
/// boards and blocks do not fit together, or a board or block is not one the store
/// takes, with the store's message.
async fn parse_board_jsonl(
    jsonl: Bytes,
    options: &ImportOptions,
    told: &HashSet<String>,
) -> Result<ParsedContainer, AppError> {
    // Parsing tens of megabytes of JSON, and checking what it holds, would hold up a
    // worker of the runtime.
    let (workspace_id, modified_by) = (options.workspace_id.clone(), options.modified_by.clone());
    let limits = options.page_limits;
    let (mut containers_and_blocks, members, files) = tokio::task::spawn_blocking(move || {
        let parsed = parse_archive_lines(&jsonl, &workspace_id, &modified_by)?;
        // As the store would refuse them, before the first board is stored: an archive
        // exported before a card's content became its body may hold content blocks,
        // which no write makes again (`docs/contract-pending.md` 1); a wiki and its pages
        // are held to the rules and limits of their tree.
        for container in &parsed.0.containers {
            container
                .check_storable()
                .map_err(|error| AppError::from(StoreError::InvalidContainer(error)))?;
        }
        places::check(&parsed.0, limits)?;
        Ok::<_, AppError>(parsed)
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))??;
    clear_values_of_another_kind(&mut containers_and_blocks);
    let orphans = drop_orphans(&mut containers_and_blocks);
    if orphans > 0 {
        tracing::warn!(
            orphans,
            "skipped blocks of the archive whose parent is not in the archive"
        );
    }

    let Renewed {
        containers_and_blocks,
        renamed,
    } = match containers_and_blocks.validate() {
        // A board without blocks is a board all the same, and what exporting one gives.
        // The original refuses such an archive.
        Ok(()) | Err(ContainersAndBlocksError::NoBlocks) => {
            renew_containers_and_blocks_ids_told(containers_and_blocks, |id| told.contains(id))
        }
        Err(error) => {
            return Err(AppError::bad_request(format_args!(
                "the archive's boards and blocks do not fit together: {error}"
            )));
        }
    };
    // As the store refuses them, with its messages, before anything is stored.
    for block in &containers_and_blocks.blocks {
        block.validate().map_err(|source| {
            AppError::from(StoreError::InvalidBlock {
                id: block.id.clone(),
                source,
            })
        })?;
    }
    Ok(ParsedContainer {
        containers_and_blocks,
        members,
        renamed,
        files,
    })
}

/// Removes the blocks that nothing in the archive leads to, and returns how many went:
/// those whose parent is no block of the archive, the blocks below them, and blocks that
/// are each other's parents without any of them being on a board.
///
/// Such a block belongs to nothing that is imported: no card lists it, and its parent
/// would be a reference to something outside the import.
///
/// One walk down from the blocks that are directly on a board; the time it takes grows
/// with the number of blocks and not with how deep they are nested.
fn drop_orphans(containers_and_blocks: &mut ContainersAndBlocks) -> usize {
    let blocks = &mut containers_and_blocks.blocks;
    let mut below: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut reached: Vec<usize> = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if block.parent_id.is_empty() {
            reached.push(index);
        } else {
            below
                .entry(block.parent_id.as_str())
                .or_default()
                .push(index);
        }
    }
    let mut keep = vec![false; blocks.len()];
    while let Some(index) = reached.pop() {
        if !std::mem::replace(&mut keep[index], true) {
            reached.extend(below.get(blocks[index].id.as_str()).into_iter().flatten());
        }
    }

    let before = blocks.len();
    let mut kept = keep.into_iter();
    blocks.retain(|_| kept.next().unwrap_or(false));
    before - blocks.len()
}

/// The names a board line gives the files of its directory (`fileNames`, by the names
/// of their entries), as an upload's name may be ([`check_file_name`]), and with the
/// extension of the entry's name.
/// Any other is left out, and that file keeps the name of its entry, as before the
/// names were written.
fn file_names_of(board: &Value) -> FileNames {
    let Some(names) = board.get("fileNames").and_then(Value::as_object) else {
        return FileNames::new();
    };
    names
        .iter()
        .filter_map(|(entry, name)| {
            let name = name.as_str()?;
            let fits =
                check_file_name(name).is_ok() && stored_extension(name) == stored_extension(entry);
            fits.then(|| (entry.clone(), name.to_owned()))
        })
        .collect()
}

/// The stored names a board line says the export did not find (`missingFiles`): the
/// strings of the list, anything else in it left out.
fn missing_files_of(board: &Value) -> HashSet<String> {
    board
        .get("missingFiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// Parses a board's `board.jsonl` into its boards, blocks, members and what it says of its
/// files, with fresh modification metadata but the identifiers from the archive.
///
/// # Errors
///
/// Returns a bad request if a line is malformed or of an unknown type.
fn parse_archive_lines(
    jsonl: &[u8],
    workspace_id: &str,
    modified_by: &str,
) -> Result<(ContainersAndBlocks, Vec<ContainerMember>, ArchiveFiles), AppError> {
    let modifier_user = modified_by;
    let now = now_millis();
    let mut containers_and_blocks = ContainersAndBlocks::default();
    let mut container_members: Vec<ContainerMember> = Vec::new();
    let mut files = ArchiveFiles::default();
    let mut container_id = String::new();

    for (number, raw) in jsonl.split(|&byte| byte == b'\n').enumerate() {
        let line_number = number + 1;
        let line = trim(raw);
        if line.is_empty() {
            continue;
        }
        let archive_line: ArchiveLine = serde_json::from_slice(line).map_err(|error| {
            AppError::bad_request(format_args!(
                "error parsing archive line {line_number}: {error}"
            ))
        })?;
        match archive_line.line_type.as_str() {
            "board" => {
                files.names.extend(file_names_of(&archive_line.data));
                files.missing.extend(missing_files_of(&archive_line.data));
                let mut container: Container = serde_json::from_value(with_workspace_named(
                    archive_line.data,
                ))
                .map_err(|error| {
                    AppError::bad_request(format_args!(
                        "invalid board in archive line {line_number}: {error}"
                    ))
                })?;
                modifier_user.clone_into(&mut container.modified_by);
                container.update_at = now;
                // The workspace the archive names is not read: the boards go to the
                // workspace imported into.
                workspace_id.clone_into(&mut container.workspace_id);
                container_id.clone_from(&container.id);
                containers_and_blocks.containers.push(container);
            }
            "block" => {
                let mut block: Block =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid block in archive line {line_number}: {error}"
                        ))
                    })?;
                modifier_user.clone_into(&mut block.modified_by);
                block.update_at = now;
                block.container_id.clone_from(&container_id);
                containers_and_blocks.blocks.push(block);
            }
            "boardMember" => {
                let member: ContainerMember =
                    serde_json::from_value(archive_line.data).map_err(|error| {
                        AppError::bad_request(format_args!(
                            "invalid board Member in archive line {line_number}: {error}"
                        ))
                    })?;
                container_members.push(member);
            }
            other => {
                return Err(AppError::bad_request(format_args!(
                    "unsupported archive line type; got {other}, line {line_number}"
                )));
            }
        }
    }
    Ok((containers_and_blocks, container_members, files))
}

/// Reads every entry of a native archive once before anything is stored so any broken
/// or oversized entry, or cumulative unpacked total past [`MAX_UNPACKED_BYTES`], refuses
/// the archive with nothing stored yet.
async fn check_native_unpacked_budget(zip: &Archive, largest_file: u64) -> Result<(), AppError> {
    let mut unpacked: u64 = 0;
    for index in 0..zip.len() {
        let (dir, filename) = entry_name(zip, index)?;
        let limit = entry_limit(&dir, &filename, largest_file);
        unpacked = unpacked.saturating_add(check_entry(zip, index, limit).await?);
        if unpacked > MAX_UNPACKED_BYTES {
            return Err(AppError::too_large(
                format_args!(
                    "the archive's entries unpack to more than {MAX_UNPACKED_BYTES} bytes"
                ),
                MAX_UNPACKED_BYTES,
            ));
        }
    }
    Ok(())
}

/// Tallies created container counts and identifiers in creation order for a native import.
fn summarize_native_import(
    created_order: &[(ContainerKind, String)],
    file_count: usize,
) -> (ImportCounts, Vec<String>) {
    let mut counts = ImportCounts {
        files: file_count,
        ..ImportCounts::default()
    };
    let mut container_ids = Vec::with_capacity(created_order.len());
    for (kind, id) in created_order {
        match kind {
            ContainerKind::Board => counts.boards += 1,
            ContainerKind::Wiki => counts.wikis += 1,
            ContainerKind::Diary | ContainerKind::Course => {}
        }
        container_ids.push(id.clone());
    }
    (counts, container_ids)
}

/// Validates an archive's `version.json`.
fn check_version(bytes: &[u8]) -> Result<(), AppError> {
    let header: ArchiveHeader = serde_json::from_slice(bytes).map_err(|error| {
        AppError::bad_request(format_args!("cannot parse version.json: {error}"))
    })?;
    if header.version == ARCHIVE_VERSION {
        Ok(())
    } else {
        Err(AppError::bad_request(format_args!(
            "unsupported archive version; got {}, want {ARCHIVE_VERSION}",
            header.version
        )))
    }
}

/// Trims ASCII whitespace from both ends of a line, as `bytes.TrimSpace` does.
fn trim(line: &[u8]) -> &[u8] {
    let start = line.iter().position(|byte| !byte.is_ascii_whitespace());
    let Some(start) = start else { return &[] };
    let end = line
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(start);
    &line[start..=end]
}

/// Clears the card values that do not fit the kind of their property, and of relation
/// values what a write refuses, as a copy of stored cards does: an archive holds what
/// its board held, and a change of the definitions leaves such values behind
/// (`docs/contract-pending.md` 2).
fn clear_values_of_another_kind(containers_and_blocks: &mut ContainersAndBlocks) {
    let definitions: HashMap<&str, &[JsonMap]> = containers_and_blocks
        .containers
        .iter()
        .map(|container| (container.id.as_str(), container.card_properties.as_slice()))
        .collect();
    for block in &mut containers_and_blocks.blocks {
        let Some(definitions) = definitions.get(block.container_id.as_str()) else {
            continue;
        };
        let id = block.id.clone();
        if let Some(Value::Object(values)) = block.fields.get_mut("properties") {
            fvoci_model::clear_values_of_another_kind(values, definitions);
            fvoci_model::tidy_copied_relations(values, &id, definitions);
        }
    }
}

/// A board of an archive with its workspace under the name it has now: an archive written
/// before the workspaces were named so names it `teamId`.
fn with_workspace_named(mut data: Value) -> Value {
    if let Value::Object(container) = &mut data
        && !container.contains_key("workspaceId")
        && let Some(workspace) = container.remove("teamId")
    {
        container.insert("workspaceId".to_owned(), workspace);
    }
    data
}

mod archive;
mod bundle;
#[cfg(test)]
mod external_tests;
mod markdown;
mod markdown_nodes;
mod native_atomic;
mod notion;
mod notion_csv;
mod places;
mod pointing;
mod staged;
mod summary;
#[cfg(test)]
mod tests;
mod trello;
mod trello_wire;

pub use summary::{
    BundleLimits, ImportCounts, ImportFormat, ImportSummary, ImportWarning, ImportWarningCode,
    MAX_REPORTED_WARNINGS, ParseImportFormatError,
};
