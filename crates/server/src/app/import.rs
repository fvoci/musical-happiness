//! Importing boards and wikis from the archive format of version 3 (`super::export`)
//! and external exports (Trello JSON, Notion Markdown & CSV ZIP).
//!
//! Native `fvoci` imports stream each `board.jsonl` in two passes (`native_stream`):
//! Pass 1 scans and validates line by line without holding block bodies in RAM, and
//! Pass 2 streams rewritten blocks in bounded batches into a single database transaction
//! per board. External formats convert into a [`staged::StagedArchive`] and commit
//! atomically across containers, blocks, and local files.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use fvoci_model::{
    ArchiveHeader, Container, ContainerKind, ContainerMember, ContainersAndBlocks, JsonMap,
    PageLimits, WorkspaceRole,
};
use fvoci_store::{NewBodyFiles, StreamCreateError};
use serde_json::Value;
use tokio::fs;

use super::App;
use super::container_copies::counted_as;
use super::doomed::Doomed;
use super::export::ARCHIVE_VERSION;
use super::file_name::check_file_name;
use super::files::stored_extension;
use crate::collab::AccessScope;
use crate::error::{AppError, ErrorKind};
use crate::metrics::{ContainerWork, CourseSource};
use crate::permissions;
use archive::{
    Archive, check_entry, entry_limit, entry_name, open_archive, read_entry, too_many_entries,
    unpack_entry,
};
use native_stream::{BoardWritePlan, build_write_plan, recheck_planned_fields, scan_native_board};
use native_stream_write::{
    BlockIdCollection, StreamedBoardHandle, StreamedBoardStats, spawn_board_streamer,
};

/// The most an archive's entries may decompress to together (`docs/s6-design.md` 4.2):
/// what keeps a small archive from unfolding into a disk's worth.
const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;

/// The most entries an archive has (`docs/s6-design.md` 4.2).
const MAX_ENTRIES: u64 = 10_000;

/// The largest `board.jsonl` that is read (`docs/s6-design.md` 4.2). Streamed line by
/// line across two passes (`native_stream`) so peak RAM stays bounded by a single batch
/// instead of scaling with the full file's JSON AST.
const MAX_JSONL_BYTES: u64 = 64 * 1024 * 1024;

/// The largest `version.json` that is read: it is one small object.
const MAX_VERSION_BYTES: u64 = 64 * 1024;

/// Chunk size used when loading newly imported blocks to broadcast to WS/webhooks.
const ANNOUNCE_CHUNK_BLOCKS: usize = 16;

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
    /// with their blocks and files, using two-pass streaming line-by-line reads.
    async fn import_archive_detailed(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> Result<(Imported, ImportCounts, Vec<String>), AppError> {
        let zip = open_archive(path).await?;
        let unpacked_file = path.with_file_name("entry");
        if zip.is_empty() {
            return Err(AppError::bad_request("zip: not a valid zip file"));
        }
        if u64::try_from(zip.len()).map_or(true, |entries| entries > MAX_ENTRIES) {
            return Err(too_many_entries());
        }
        let largest_file = u64::try_from(self.limits().largest_file).unwrap_or(0);

        verify_native_header(&zip).await?;
        check_native_unpacked_budget(&zip, largest_file).await?;

        let mut container_entries = Vec::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(&zip, index)?;
            if filename == "board.jsonl" {
                container_entries.push((index, dir));
            }
        }

        // Pass 1: stream every `board.jsonl` line by line for validation & cross-board refs.
        let mut scanned_boards = Vec::with_capacity(container_entries.len());
        let mut elsewhere: HashSet<String> = HashSet::new();
        let mut diaries: u32 = 0;
        for (index, dir) in container_entries {
            let scanned = scan_native_board(&zip, index, dir, options).await?;
            for (c_idx, container) in scanned.containers.iter().enumerate() {
                self.require_makes(container)
                    .map_err(|error| error.under(&format!("/containers/{c_idx}")))?;
            }
            diaries = diaries.saturating_add(scanned.diaries);
            elsewhere.extend(scanned.external_refs.iter().cloned());
            scanned_boards.push(scanned);
        }
        if diaries > 0 {
            self.store()
                .check_room_for_diaries(&options.workspace_id, diaries)
                .await?;
        }

        // Build compact write plans and complete cross-board copy map before Pass 2 writes.
        let mut plans = Vec::with_capacity(scanned_boards.len());
        let mut all_copies: HashMap<String, String> = HashMap::new();
        for scanned in scanned_boards {
            let (plan, copies) = build_write_plan(scanned, &elsewhere);
            for (old, new) in copies {
                all_copies.entry(old).or_insert(new);
            }
            plans.push(plan);
        }
        for plan in &plans {
            recheck_planned_fields(&zip, plan, &options.modified_by).await?;
        }
        let all_copies = Arc::new(all_copies);

        // Pass 2: stream each `board.jsonl` in bounded batches inside a transaction.
        let mut left_out: HashSet<String> = HashSet::new();
        let StoredContainers {
            by_dir: stored,
            created_order,
            files,
            blocks_to_point,
            mut counts,
        } = self
            .store_archive_containers(&zip, plans, options, &all_copies, &mut left_out)
            .await?;

        // Store file entries, placed under their board, then point referenced blocks.
        let entries = (&zip, largest_file, unpacked_file.as_path());
        let file_map = self.store_archive_files(entries, &stored, &files).await?;
        self.point_at_stored_files(
            &stored,
            &blocks_to_point,
            &file_map,
            &all_copies,
            &options.modified_by,
        )
        .await;
        tracing::debug!(containers_imported = stored.len(), "import archive - done");

        counts.files = file_map.len();
        let container_ids = tally_container_counts(&created_order, &mut counts);
        Ok((
            Imported {
                members_left_out: left_out.len(),
            },
            counts,
            container_ids,
        ))
    }

    /// Stores the file entries of the archive (`entries`: the archive, the limit of a
    /// file's size and where an entry is unpacked to), each under the board of its
    /// directory among `stored`, skipping entries the board lists as missing.
    async fn store_archive_files(
        &self,
        (zip, largest_file, unpacked): (&Archive, u64, &Path),
        stored: &HashMap<String, Container>,
        files: &HashMap<String, ArchiveFiles>,
    ) -> Result<HashMap<(String, String), String>, AppError> {
        let mut file_map: HashMap<(String, String), String> = HashMap::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(zip, index)?;
            if filename.is_empty() || filename == "version.json" || filename == "board.jsonl" {
                continue;
            }
            let Some(container) = stored.get(&dir) else {
                tracing::warn!(dir, filename, "skipping orphan image in archive");
                continue;
            };
            let said = files.get(&dir);
            if said.is_some_and(|said| said.missing.contains(&filename)) {
                tracing::warn!(
                    dir,
                    filename,
                    "skipping an entry of the archive that its board says is missing"
                );
                continue;
            }
            let name = name_in(files, &dir, &filename);
            let entry = (zip, index, largest_file, unpacked);
            let saved = self.save_archive_entry(entry, &container.id, name).await;
            let new_name = saved.map_err(|error| {
                error.context(format_args!(
                    "cannot import file {filename} for board {dir}"
                ))
            })?;
            file_map.insert((container.id.clone(), filename), new_name);
        }
        Ok(file_map)
    }

    /// Unpacks one archive entry to a doomed staging file and stores it under
    /// `container_id`, guaranteeing staging cleanup on success, error, or cancellation.
    async fn save_archive_entry(
        &self,
        (zip, index, limit, unpacked): (&Archive, usize, u64, &Path),
        container_id: &str,
        name: &str,
    ) -> Result<String, AppError> {
        let staged = Doomed::file(unpacked.to_owned());
        let saved = match unpack_entry(zip, index, limit, staged.path()).await {
            Ok(()) => match fs::File::open(staged.path()).await {
                Ok(mut file) => self.save_file(&mut file, container_id, name).await,
                Err(error) => Err(AppError::internal(format_args!(
                    "cannot open an unpacked archive entry: {error}"
                ))),
            },
            Err(error) => Err(error),
        };
        staged.remove().await;
        saved
    }

    /// Streams and stores every planned board of the archive one by one.
    async fn store_archive_containers(
        &self,
        zip: &Archive,
        plans: Vec<BoardWritePlan>,
        options: &ImportOptions,
        all_copies: &Arc<HashMap<String, String>>,
        left_out: &mut HashSet<String>,
    ) -> Result<StoredContainers, AppError> {
        let mut stored = StoredContainers::default();
        for plan in plans {
            let dir = plan.dir.clone();
            stored.files.insert(dir.clone(), plan.files);
            let (container, stats) = self
                .store_streamed_board(
                    zip,
                    plan.zip_index,
                    plan.containers,
                    plan.members,
                    &plan.schedules,
                    plan.stream_plan,
                    Arc::clone(all_copies),
                    options,
                    left_out,
                )
                .await?;
            stored.counts.cards += stats.counts.cards;
            stored.counts.pages += stats.counts.pages;
            stored.counts.units += stats.counts.units;
            stored.counts.comments += stats.counts.comments;
            for (cid, ids) in stats.blocks_to_point {
                stored.blocks_to_point.entry(cid).or_default().extend(ids);
            }
            stored
                .created_order
                .push((container.kind, container.id.clone()));
            stored.by_dir.insert(dir, container);
        }
        Ok(stored)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "internal helper wiring one board's Pass-2 stream into the store transaction"
    )]
    async fn store_streamed_board(
        &self,
        zip: &Archive,
        zip_index: usize,
        mut containers: Vec<Container>,
        members: Vec<ContainerMember>,
        schedules: &[(usize, fvoci_model::ValidatedCourseSchedule)],
        stream_plan: native_stream::StreamPlan,
        all_copies: Arc<HashMap<String, String>>,
        options: &ImportOptions,
        left_out: &mut HashSet<String>,
    ) -> Result<(Container, StreamedBoardStats), AppError> {
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

        let collect_ids = if self.ws().has_workspace_listeners(&options.workspace_id)
            || !self.config().block_change_webhooks.is_empty()
        {
            BlockIdCollection::All
        } else {
            BlockIdCollection::None
        };
        let StreamedBoardHandle {
            rx,
            stats: streamer_handle,
        } = spawn_board_streamer(
            zip.clone(),
            zip_index,
            stream_plan,
            all_copies,
            options.modified_by.clone(),
            collect_ids,
        );

        let write_result = {
            let _structure = self.collab().changing_structure(self).await;
            self.store()
                .create_containers_and_blocks_streamed_with_schedules(
                    &mut containers,
                    &options.modified_by,
                    NewBodyFiles::Unchecked,
                    rx,
                    schedules,
                )
                .await
        };
        let streamer_outcome = streamer_handle.await;

        match write_result {
            Ok(()) => {}
            Err(StreamCreateError::Producer(error)) => return Err(error),
            Err(StreamCreateError::Incomplete) => {
                return Err(AppError::internal("archive stream ended before completion"));
            }
            Err(StreamCreateError::Store(error)) => {
                return Err(AppError::from(error).context("error inserting archive blocks"));
            }
        }
        let stats = streamer_outcome.map_err(|error| {
            AppError::internal(format_args!("archive stream task failed: {error}"))
        })?;

        self.finalize_streamed_containers(
            &containers,
            &kept_members,
            &stats.inserted_block_ids,
            options,
        )
        .await?;

        let primary = containers
            .into_iter()
            .next()
            .ok_or_else(|| AppError::bad_request("missing board in archive"))?;
        Ok((primary, stats))
    }

    async fn finalize_streamed_containers(
        &self,
        containers: &[Container],
        kept_members: &[ContainerMember],
        inserted_block_ids: &[String],
        options: &ImportOptions,
    ) -> Result<(), AppError> {
        if !containers.is_empty() {
            let made = containers.iter().map(|container| &container.id);
            self.collab()
                .recheck_access(self, AccessScope::containers(made))
                .await;
        }
        let workspace_id = containers
            .first()
            .map_or("", |container| container.workspace_id.as_str());
        for container in containers {
            self.ws()
                .broadcast_container_change(workspace_id, container)
                .await;
        }
        for chunk in inserted_block_ids.chunks(ANNOUNCE_CHUNK_BLOCKS) {
            match self.store().get_blocks_by_ids(chunk).await {
                Ok(blocks) => {
                    for block in &blocks {
                        self.ws().broadcast_block_change(workspace_id, block).await;
                        self.tell_webhooks(block).await;
                    }
                }
                Err(error) => {
                    tracing::error!(
                        %error,
                        workspace_id,
                        "cannot load imported blocks for broadcast"
                    );
                }
            }
        }
        for container in containers {
            if !container.is_template {
                self.add_containers_to_default_category(
                    &options.modified_by,
                    &container.workspace_id,
                    std::slice::from_ref(&container.id),
                )
                .await?;
            }
            let admin = ContainerMember {
                container_id: container.id.clone(),
                user_id: options.modified_by.clone(),
                scheme_admin: true,
                ..ContainerMember::default()
            };
            self.add_member_to_container(&admin).await?;
            for member in kept_members {
                let member = ContainerMember {
                    container_id: container.id.clone(),
                    ..member.clone()
                };
                self.add_member_to_container(&member).await?;
            }
            self.metrics()
                .container_copied(ContainerWork::Import, counted_as(container.kind));
            if container.kind == ContainerKind::Course && !container.is_template {
                self.metrics().course_created(CourseSource::Import);
            }
        }
        Ok(())
    }
}

/// Reads and verifies `version.json` at the root of a native archive.
async fn verify_native_header(zip: &Archive) -> Result<(), AppError> {
    let version = (0..zip.len())
        .map(|index| entry_name(zip, index).map(|name| (index, name)))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|(_, (dir, filename))| dir == "." && filename == "version.json")
        .map(|(index, _)| index)
        .ok_or_else(|| {
            AppError::bad_request(format_args!(
                "the archive has no version.json; this server reads version {ARCHIVE_VERSION}"
            ))
        })?;
    let version = read_entry(zip, version, MAX_VERSION_BYTES)
        .await
        .map_err(|error| {
            if error.kind() == ErrorKind::PayloadTooLarge {
                AppError::bad_request("version.json is not that of an archive")
            } else {
                error
            }
        })?;
    check_version(&version)
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

/// The boards of an archive as they were stored.
#[derive(Default)]
struct StoredContainers {
    /// Each board by the directory of the archive it came in.
    by_dir: HashMap<String, Container>,
    /// Containers created, in creation order (kind and new ID), for deterministic summaries.
    created_order: Vec<(ContainerKind, String)>,
    /// What the board lines say of the files, by the directory of the archive.
    files: HashMap<String, ArchiveFiles>,
    /// New block IDs per new container ID that reference files or cross-board copies.
    blocks_to_point: HashMap<String, Vec<String>>,
    /// Running counts of imported blocks across all boards.
    counts: ImportCounts,
}

/// The name a file of the archive is stored with: the one the board line of its
/// directory gives it ([`file_names_of`]), or failing that the name of its entry.
fn name_in<'a>(files: &'a HashMap<String, ArchiveFiles>, dir: &str, filename: &'a str) -> &'a str {
    files
        .get(dir)
        .and_then(|said| said.names.get(filename))
        .map_or(filename, String::as_str)
}

/// Removes the blocks that nothing in the archive leads to, and returns how many went:
/// those whose parent is no block of the archive, the blocks below them, and blocks that
/// are each other's parents without any of them being on a board.
pub(super) fn drop_orphans(containers_and_blocks: &mut ContainersAndBlocks) -> usize {
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

/// Tallies created container counts into `counts` and returns container IDs in creation order.
fn tally_container_counts(
    created_order: &[(ContainerKind, String)],
    counts: &mut ImportCounts,
) -> Vec<String> {
    let mut container_ids = Vec::with_capacity(created_order.len());
    for (kind, id) in created_order {
        match kind {
            ContainerKind::Board => counts.boards += 1,
            ContainerKind::Wiki => counts.wikis += 1,
            ContainerKind::Course => counts.courses += 1,
            ContainerKind::Diary => {}
        }
        container_ids.push(id.clone());
    }
    container_ids
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
/// values what a write refuses, as a copy of stored cards does.
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
        native_stream::clear_block_values_of_another_kind(block, definitions);
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
mod native_stream;
mod native_stream_write;
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
