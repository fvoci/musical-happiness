//! Importing boards and wikis from the archive format of version 3 (`super::export`).
//!
//! Ports `server/app/import.go`. An import generates fresh identifiers for everything,
//! fixes up references, stores the files, points the blocks at them and adds the
//! importing user as an admin. A wiki is made as a wiki, its pages held to the rules of
//! their tree before anything is stored (`places`). An archive of another version is
//! refused (`docs/s6-design.md` 3.10).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom, Write as _};
use std::mem;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use fvoci_model::{
    ArchiveHeader, ArchiveLine, Block, Container, ContainerMember, ContainersAndBlocks,
    ContainersAndBlocksError, JsonMap, PageLimits, Renewed, WorkspaceRole,
    renew_containers_and_blocks_ids_told,
};
use serde_json::Value;
use tokio::fs;

use super::App;
use super::container_copies::counted_as;
use super::export::ARCHIVE_VERSION;
use super::files::{base_name, stored_extension};
use super::uploads::MAX_UPLOAD_NAME_CHARS;
use crate::auth::now_millis;
use crate::error::{AppError, ErrorKind};
use crate::metrics::ContainerWork;
use crate::permissions;
use fvoci_store::StoreError;

/// The most an archive's entries may decompress to together (`docs/s6-design.md` 4.2):
/// what keeps a small archive from unfolding into a disk's worth.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// The most entries an archive has (`docs/s6-design.md` 4.2).
const MAX_ENTRIES: u64 = 10_000;

/// The signature each entry of a ZIP's central directory begins with (APPNOTE 4.3.12).
const CENTRAL_HEADER: &[u8; 4] = b"PK\x01\x02";

/// How much of an archive is read at a time to count its central headers.
const SCAN_CHUNK_BYTES: usize = 1024 * 1024;

/// The largest `board.jsonl` that is read (`docs/s6-design.md` 4.2).
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
    /// Imports the archive in the file at `path` into a workspace on behalf of a user. The
    /// entry point for the import of an upload.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`App::import_archive`].
    pub(crate) async fn import_workspace_archive(
        &self,
        path: &Path,
        workspace_id: &str,
        modified_by: &str,
    ) -> Result<Imported, AppError> {
        let options = ImportOptions {
            workspace_id: workspace_id.to_owned(),
            modified_by: modified_by.to_owned(),
            page_limits: self.limits().pages,
        };
        self.import_archive(path, &options).await
    }

    /// Imports the archive in the file at `path`, of zero or more boards with their
    /// blocks and files. The archive is read from the file at random, an entry at a time;
    /// a file entry is unpacked into a file beside it, `path` with the name `entry`, to be
    /// stored from there.
    ///
    /// # Errors
    ///
    /// Returns a bad request if the file is not a ZIP, has no `version.json` or one of
    /// another version, or holds what does not fit together; a 413 for an archive of
    /// more than 10,000 entries, whose entries unpack to more than 1 GiB together, or
    /// with a `board.jsonl` of more than 64 MiB or a file larger than `max_file_bytes`; and
    /// the store's error if a board cannot be stored.
    pub(crate) async fn import_archive(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> Result<Imported, AppError> {
        let zip = open_archive(path).await?;
        let unpacked_file = path.with_file_name("entry");
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
        // Every entry is read through once before anything is stored: an entry that is
        // broken or too large, or all of them unpacking to too much, refuses the archive
        // while nothing of it is stored yet.
        let mut unpacked: u64 = 0;
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            let limit = entry_limit(&dir, &filename, largest_file);
            unpacked = unpacked.saturating_add(check_entry(&zip, index, limit).await?);
            if unpacked > MAX_UNPACKED_BYTES {
                return Err(AppError::too_large(
                    format_args!(
                        "the archive's entries unpack to more than {MAX_UNPACKED_BYTES} bytes"
                    ),
                    MAX_UNPACKED_BYTES,
                ));
            }
        }

        let mut containers = Vec::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            if filename == "board.jsonl" {
                containers.push((index, dir));
            }
        }
        let elsewhere = named_elsewhere(&zip, &containers, options).await?;

        // First pass: the boards; a file entry needs its board already known.
        let mut left_out: HashSet<String> = HashSet::new();
        let StoredContainers {
            by_dir: stored,
            copies,
            file_names,
        } = self
            .store_archive_containers(&zip, containers, options, &elsewhere, &mut left_out)
            .await?;

        // Second pass: the files, placed under their board.
        let mut file_map: HashMap<(String, String), String> = HashMap::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            // An entry without a file name is a directory of a re-packed archive.
            if filename.is_empty() || filename == "version.json" || filename == "board.jsonl" {
                continue;
            }
            let Some(container) = stored.get(&dir) else {
                tracing::warn!(dir, filename, "skipping orphan image in archive");
                continue;
            };
            let name = name_in(&file_names, &dir, &filename);
            let entry = (&zip, index, largest_file, unpacked_file.as_path());
            let saved = self.save_archive_entry(entry, &container.id, name).await;
            let new_name = saved.map_err(|error| {
                error.context(format_args!(
                    "cannot import file {filename} for board {dir}"
                ))
            })?;
            file_map.insert((container.id.clone(), filename), new_name);
        }

        self.point_at_stored_files(&stored, &file_map, &copies, &options.modified_by)
            .await;
        tracing::debug!(containers_imported = stored.len(), "import archive - done");
        Ok(Imported {
            members_left_out: left_out.len(),
        })
    }

    /// Unpacks the entry of the archive at its index (`entry`: the archive, the index,
    /// the limit of its size and where it is unpacked to) and stores it as a file of
    /// `container_id` called `name`. Gives the new file's stored name; the unpacked copy
    /// is removed either way.
    async fn save_archive_entry(
        &self,
        (zip, index, limit, unpacked): (&Archive, usize, u64, &Path),
        container_id: &str,
        name: &str,
    ) -> Result<String, AppError> {
        let saved = match unpack_entry(zip, index, limit, unpacked).await {
            Ok(()) => match fs::File::open(unpacked).await {
                Ok(mut file) => self.save_file(&mut file, container_id, name).await,
                Err(error) => Err(AppError::internal(format_args!(
                    "cannot open an unpacked archive entry: {error}"
                ))),
            },
            Err(error) => Err(error),
        };
        remove_unpacked(unpacked).await;
        saved
    }

    /// Stores the boards of the archive, each its `board.jsonl` at an index under a
    /// directory, and returns them by their directory, with the new identifiers of the
    /// blocks among `elsewhere` (what a body of another board may name). Members left
    /// out are added to `left_out` ([`App::store_archive_container`]).
    ///
    /// # Errors
    ///
    /// As [`App::store_archive_container`], and a bad request for an entry that cannot be read.
    async fn store_archive_containers(
        &self,
        zip: &Archive,
        containers: Vec<(usize, String)>,
        options: &ImportOptions,
        elsewhere: &HashSet<String>,
        left_out: &mut HashSet<String>,
    ) -> Result<StoredContainers, AppError> {
        let mut stored = StoredContainers::default();
        for (index, dir) in containers {
            let jsonl = read_entry(zip, index, MAX_JSONL_BYTES).await?;
            let mut container = parse_board_jsonl(jsonl, options, elsewhere).await?;
            stored
                .file_names
                .insert(dir.clone(), mem::take(&mut container.file_names));
            // Two blocks of an archive that share an identifier are no archive of this
            // server's: the first is the one named.
            for (old, new) in mem::take(&mut container.renamed) {
                stored.copies.entry(old).or_insert(new);
            }
            let container = self
                .store_archive_container(container, options, left_out)
                .await?;
            stored.by_dir.insert(dir, container);
        }
        Ok(stored)
    }

    /// Stores one board of an archive as [`parse_board_jsonl`] read it, with its
    /// members who are in the workspace and the importing user as an admin, and
    /// returns the stored board. The members who are not in the workspace are left out,
    /// and added to `left_out`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the board or a membership cannot be stored. What
    /// the store refuses as a bad request, [`parse_board_jsonl`] has refused before.
    async fn store_archive_container(
        &self,
        container: ParsedContainer,
        options: &ImportOptions,
        left_out: &mut HashSet<String>,
    ) -> Result<Container, AppError> {
        let ParsedContainer {
            containers_and_blocks,
            members,
            renamed: _,
            file_names: _,
        } = container;
        // Keep only members who are in the workspace: a board takes no others. A guest of
        // the workspace keeps a role no higher than editor. A membership without a role
        // opens nothing, and is neither kept nor counted as left out.
        let mut kept_members = Vec::with_capacity(members.len());
        for member in members {
            if permissions::role_of(&member).is_none() {
                continue;
            }
            let actor = self.actor(&member.user_id, &options.workspace_id).await?;
            match actor.workspace {
                Some(workspace_role) => {
                    kept_members.push(no_more_than_holdable(member, workspace_role));
                }
                None => {
                    left_out.insert(member.user_id);
                }
            }
        }

        let created = self
            .create_imported_containers_and_blocks(containers_and_blocks, &options.modified_by)
            .await
            .map_err(|error| error.context("error inserting archive blocks"))?;

        for container in &created.containers {
            let admin = ContainerMember {
                container_id: container.id.clone(),
                user_id: options.modified_by.clone(),
                scheme_admin: true,
                ..ContainerMember::default()
            };
            self.add_member_to_container(&admin).await?;
            for member in &kept_members {
                let member = ContainerMember {
                    container_id: container.id.clone(),
                    ..member.clone()
                };
                self.add_member_to_container(&member).await?;
            }
            // Counted once it is made with its members, as it stays whatever comes after.
            self.metrics()
                .container_copied(ContainerWork::Import, counted_as(container.kind));
        }

        created
            .containers
            .into_iter()
            .next()
            .ok_or_else(|| AppError::bad_request("missing board in archive"))
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
    /// The names its board lines give its files ([`file_names_of`]).
    file_names: FileNames,
}

/// The name of each file of a directory of an archive, by the name of its entry.
type FileNames = HashMap<String, String>;

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
/// nothing. What is read is dropped and read again to be stored, so that one board at a
/// time is held. Returns what the bodies name that is no block of their own board: it
/// may be a block of another board of the archive, whose new identifier is asked for
/// when that board is stored.
///
/// # Errors
///
/// As [`parse_board_jsonl`], and a bad request for an entry that cannot be read.
async fn named_elsewhere(
    zip: &Archive,
    containers: &[(usize, String)],
    options: &ImportOptions,
) -> Result<HashSet<String>, AppError> {
    let mut elsewhere = HashSet::new();
    for (index, _) in containers {
        let jsonl = read_entry(zip, *index, MAX_JSONL_BYTES).await?;
        let container = parse_board_jsonl(jsonl, options, &HashSet::new()).await?;
        elsewhere.extend(container.named_elsewhere());
    }
    Ok(elsewhere)
}

/// The boards of an archive as they were stored.
#[derive(Default)]
struct StoredContainers {
    /// Each board by the directory of the archive it came in.
    by_dir: HashMap<String, Container>,
    /// The new identifier of each block that a body of another board names, by its old
    /// one.
    copies: HashMap<String, String>,
    /// The names the board lines give the files, by the directory of the archive.
    file_names: HashMap<String, FileNames>,
}

/// The name a file of the archive is stored with: the one the board line of its
/// directory gives it ([`file_names_of`]), or failing that the name of its entry.
fn name_in<'a>(
    file_names: &'a HashMap<String, FileNames>,
    dir: &str,
    filename: &'a str,
) -> &'a str {
    file_names
        .get(dir)
        .and_then(|names| names.get(filename))
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
    let (mut containers_and_blocks, members, file_names) = tokio::task::spawn_blocking(move || {
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
        file_names,
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
/// of their entries), as an upload's name may be: not blank (the part after the last
/// `/`, which is what a record keeps), without U+0000, at most
/// [`MAX_UPLOAD_NAME_CHARS`] characters, and with the extension of the entry's name.
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
            let fits = !base_name(name).trim().is_empty()
                && !name.contains('\0')
                && name.chars().count() <= MAX_UPLOAD_NAME_CHARS
                && stored_extension(name) == stored_extension(entry);
            fits.then(|| (entry.clone(), name.to_owned()))
        })
        .collect()
}

/// Parses a board's `board.jsonl` into its boards, blocks, members and the names of its
/// files, with fresh modification metadata but the identifiers from the archive.
///
/// # Errors
///
/// Returns a bad request if a line is malformed or of an unknown type.
fn parse_archive_lines(
    jsonl: &[u8],
    workspace_id: &str,
    modified_by: &str,
) -> Result<(ContainersAndBlocks, Vec<ContainerMember>, FileNames), AppError> {
    let modifier_user = modified_by;
    let now = now_millis();
    let mut containers_and_blocks = ContainersAndBlocks::default();
    let mut container_members: Vec<ContainerMember> = Vec::new();
    let mut file_names = FileNames::new();
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
                file_names.extend(file_names_of(&archive_line.data));
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
    Ok((containers_and_blocks, container_members, file_names))
}

/// A filter for all of a board's blocks.
fn container_blocks(container_id: &str) -> fvoci_store::BlockFilter {
    fvoci_store::BlockFilter::container(container_id)
}

/// The archive as it is read while it is imported. Cloning it copies no content.
type Archive = zip::ZipArchive<SharedFile>;

/// A file read at positions of its own: each clone has its own position over the one
/// open file, so that the archive can be cloned into each task that reads an entry.
#[derive(Debug, Clone)]
struct SharedFile {
    file: Arc<std::fs::File>,
    len: u64,
    position: u64,
}

impl Read for SharedFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&*self.file, buf, self.position)?;
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(&*self.file, buf, self.position)?;
        #[cfg(not(any(unix, windows)))]
        compile_error!("an archive is read at positions, which unix and windows have");
        self.position = self
            .position
            .saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        Ok(read)
    }
}

impl Seek for SharedFile {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let position = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(offset) => self.len.checked_add_signed(offset),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
        };
        self.position = position.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.position)
    }
}

/// Opens the archive in the file at `path` and reads its directory, off the runtime's
/// workers.
async fn open_archive(path: &Path) -> Result<Archive, AppError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(&path).map_err(|error| {
            AppError::internal(format_args!("cannot open the archive: {error}"))
        })?;
        let len = file
            .metadata()
            .map_err(|error| AppError::internal(format_args!("cannot read the archive: {error}")))?
            .len();
        let mut file = SharedFile {
            file: Arc::new(file),
            len,
            position: 0,
        };
        // Counted before the directory is read, as reading it holds every entry in memory.
        // Every entry the reader keeps begins with the central header's signature, so it
        // keeps at most as many entries as signatures, whatever the end records say. The
        // capacity it reserves first follows the declared count, which the crate bounds
        // only by the file's length (about a 47th of it, as entries): that reservation is
        // not bounded here (`docs/contract-pending.md` 5).
        let headers = count_central_headers(&mut file, MAX_ENTRIES).map_err(|error| {
            AppError::internal(format_args!("cannot read the archive: {error}"))
        })?;
        if headers > MAX_ENTRIES {
            return Err(too_many_entries());
        }
        file.position = 0;
        zip::ZipArchive::new(file).map_err(|error| AppError::bad_request(format_args!("{error}")))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// The refusal of an archive of more than [`MAX_ENTRIES`] entries.
fn too_many_entries() -> AppError {
    AppError::too_large(
        format_args!("the archive has more than {MAX_ENTRIES} entries"),
        MAX_ENTRIES,
    )
}

/// Counts the central headers' signatures in what `reader` gives, up to the first count
/// over `limit`, where it stops reading.
fn count_central_headers(reader: &mut impl Read, limit: u64) -> io::Result<u64> {
    let mut chunk = vec![0; SCAN_CHUNK_BYTES];
    // The last bytes of the chunk before: a signature may span two chunks.
    let mut carried = 0;
    let mut count: u64 = 0;
    loop {
        let read = match reader.read(chunk.get_mut(carried..).unwrap_or_default()) {
            Ok(0) => return Ok(count),
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let filled = chunk.get(..carried + read).unwrap_or_default();
        let found = filled
            .windows(CENTRAL_HEADER.len())
            .filter(|window| window == CENTRAL_HEADER)
            .count();
        count = count.saturating_add(u64::try_from(found).unwrap_or(u64::MAX));
        if count > limit {
            return Ok(count);
        }
        // Three bytes are kept: four would count a signature at the end twice.
        let keep = filled.len().min(CENTRAL_HEADER.len() - 1);
        let start = filled.len() - keep;
        chunk.copy_within(start..start + keep, 0);
        carried = keep;
    }
}

/// The most an entry may unpack to: a `board.jsonl`, the archive's `version.json` (the
/// one at its root), or a file, which is held to the limit of an upload.
fn entry_limit(dir: &str, filename: &str, largest_file: u64) -> u64 {
    match filename {
        "board.jsonl" => MAX_JSONL_BYTES,
        "version.json" if dir == "." => MAX_VERSION_BYTES,
        _ => largest_file,
    }
}

/// Returns the cleaned directory and the base file name of an archive entry, without
/// reading its content.
fn entry_name(zip: &Archive, index: usize) -> Result<(String, String), AppError> {
    let name = zip
        .name_for_index(index)
        .ok_or_else(|| AppError::bad_request("invalid archive entry"))?
        .map_err(|error| {
            AppError::bad_request(format_args!("invalid archive entry name: {error}"))
        })?;
    Ok(split_entry_name(&name))
}

/// Reads the content of an archive entry, at most `limit` bytes of it. Decompression is
/// done off the runtime's workers, which it would hold up.
async fn read_entry(zip: &Archive, index: usize, limit: u64) -> Result<Bytes, AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || {
        let mut bytes = Vec::new();
        decompress(&mut zip, index, limit, &mut bytes)?;
        Ok(Bytes::from(bytes))
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Unpacks an archive entry, at most `limit` bytes of it, into the file `to`, off the
/// runtime's workers.
async fn unpack_entry(zip: &Archive, index: usize, limit: u64, to: &Path) -> Result<(), AppError> {
    let mut zip = zip.clone();
    let to = to.to_owned();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::create(&to).map_err(|error| {
            AppError::internal(format_args!("cannot unpack an archive entry: {error}"))
        })?;
        let mut out = io::BufWriter::new(file);
        decompress(&mut zip, index, limit, &mut out)?;
        out.flush().map_err(|error| {
            AppError::internal(format_args!("cannot unpack an archive entry: {error}"))
        })
    })
    .await
    .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Removes an unpacked entry once it is stored, or failed to be.
async fn remove_unpacked(path: &Path) {
    match fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => tracing::error!(%error, "an unpacked archive entry could not be removed"),
    }
}

/// Reads an archive entry through without keeping it, as [`read_entry`] would read it:
/// its checksum and its bound are checked. Returns how much it unpacks to.
async fn check_entry(zip: &Archive, index: usize, limit: u64) -> Result<u64, AppError> {
    let mut zip = zip.clone();
    tokio::task::spawn_blocking(move || decompress(&mut zip, index, limit, &mut io::sink()))
        .await
        .map_err(|error| AppError::internal(format_args!("archive task failed: {error}")))?
}

/// Decompresses an archive entry into `out`, at most `limit` bytes of it, and returns
/// how many. The reader checks the entry's checksum when it reaches the end.
fn decompress(
    zip: &mut Archive,
    index: usize,
    limit: u64,
    out: &mut impl io::Write,
) -> Result<u64, AppError> {
    let mut entry = zip
        .by_index(index)
        .map_err(|error| AppError::bad_request(format_args!("{error}")))?;
    let name = entry.name().map(Cow::into_owned).unwrap_or_default();
    let read =
        io::copy(&mut entry.by_ref().take(limit.saturating_add(1)), out).map_err(|error| {
            AppError::bad_request(format_args!("cannot read archive entry: {error}"))
        })?;
    if read > limit {
        return Err(AppError::too_large(
            format_args!("the archive entry {name} unpacks to more than {limit} bytes"),
            limit,
        ));
    }
    Ok(read)
}

/// Splits an archive entry name into its cleaned directory and its base file name, as
/// the original does with `filepath.Split` and `path.Clean`. The directory is only ever
/// a map key, and the file name never has a path separator, so a `..` in the name cannot
/// escape the storage root.
fn split_entry_name(name: &str) -> (String, String) {
    match name.rsplit_once('/') {
        Some((dir, filename)) => (clean_dir(dir), filename.to_owned()),
        None => (".".to_owned(), name.to_owned()),
    }
}

/// Normalises a directory path the way Go's `path.Clean` does for the names this import
/// meets: it resolves `.` and `..` segments. The result is used only as a map key.
fn clean_dir(dir: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for segment in dir.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(&last) if last != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
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

mod places;
mod pointing;
#[cfg(test)]
mod tests;
