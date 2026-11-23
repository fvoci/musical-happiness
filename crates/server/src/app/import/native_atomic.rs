//! Atomic commit path for native fvoci v3 archives.
//!
//! Validates every container and block before touching storage or the database, streams
//! archive files into `FileBackend` under `loose_objects` marks using a temporary staging
//! directory, rewrites body file IDs, attachment file IDs, and cross-container references
//! in memory, and commits all containers, files, blocks, creator admin memberships, and
//! kept workspace memberships in one single database transaction. Any failure cleans up
//! every temporary staging file and every marked object in `FileBackend` and rolls back
//! the database transaction so nothing is left behind.

use std::collections::{HashMap, HashSet};
use std::mem;
use std::path::Path;

use fvoci_model::{BlockType, ContainerKind, ContainerMember, ContainersAndBlocks, FileInfo};
use serde_json::Value;
use tokio::fs;

use super::archive::{Archive, entry_name, read_entry, remove_unpacked, unpack_entry};
use super::pointing::{came_with_more, pointed};
use super::{
    ArchiveFiles, ImportCounts, ImportOptions, Imported, MAX_JSONL_BYTES, name_in,
    no_more_than_holdable, parse_board_jsonl, summarize_native_import,
};
use crate::app::App;
use crate::app::container_copies::counted_as;
use crate::app::doomed::Doomed;
use crate::app::files::{FilePlace, file_info};
use crate::app::loose_objects::{WRITE_TIMEOUT, Written};
use crate::auth::secret::new_secret;
use crate::collab::AccessScope;
use crate::error::AppError;
use crate::metrics::ContainerWork;
use crate::permissions;
use fvoci_store::StoreError;

/// All containers, blocks, memberships, and archive metadata parsed and ready for
/// atomic storage and commit.
struct PreparedNativeArchive {
    combined: ContainersAndBlocks,
    extra_members: Vec<ContainerMember>,
    by_dir: HashMap<String, String>,
    files: HashMap<String, ArchiveFiles>,
    copies: HashMap<String, String>,
    created_order: Vec<(ContainerKind, String)>,
    members_left_out: usize,
}

/// Files staged and written to `FileBackend` prior to the database commit.
struct StagedNativeFiles {
    records: Vec<FileInfo>,
    written_paths: Vec<String>,
    /// Container ID -> original archive filename -> stored filename.
    file_map: HashMap<String, HashMap<String, String>>,
}

impl App {
    /// Parses, validates, stages files for, and atomically commits a native v3 archive.
    ///
    /// # Errors
    ///
    /// Returns 400/413 on archive validation or limit errors, 404 if the workspace is
    /// unreachable, or 500 on storage or database failure. On any error, zero
    /// containers, blocks, file records, stored objects, or temporary staging files stay
    /// behind.
    pub(super) async fn commit_native_archive(
        &self,
        zip: &Archive,
        containers: Vec<(usize, String)>,
        options: &ImportOptions,
        elsewhere: &HashSet<String>,
        diaries: u32,
        largest_file: u64,
    ) -> Result<(Imported, ImportCounts, Vec<String>), AppError> {
        if containers.is_empty() {
            return Ok((
                Imported {
                    members_left_out: 0,
                },
                ImportCounts::default(),
                Vec::new(),
            ));
        }
        let Some(org_id) = self.store().tenant().org_id() else {
            return Err(AppError::not_found("workspace"));
        };
        let mut prepared = self
            .prepare_native_archive(zip, containers, options, elsewhere)
            .await?;
        for container in &prepared.combined.containers {
            self.require_makes(container)?;
        }
        // Fast-fail before unpacking/writing files if the workspace cannot take the
        // archive's diaries; the commit transaction checks again under workspace lock so
        // concurrent requests cannot exceed `DIARIES_PER_SPACE` or leave partial data.
        if diaries > 0 {
            self.store()
                .check_room_for_diaries(&options.workspace_id, diaries)
                .await?;
        }

        let staged = self
            .stage_native_files(
                zip,
                largest_file,
                &org_id,
                &prepared.by_dir,
                &prepared.files,
            )
            .await?;
        if let Err(error) =
            rewrite_and_validate_blocks(&mut prepared.combined, &staged.file_map, &prepared.copies)
        {
            self.clean_written_paths(&staged.written_paths).await;
            return Err(error);
        }

        let commit_result = {
            let _structure = self.collab().changing_structure(self).await;
            self.store()
                .create_imported_containers_blocks_files_and_members(
                    &mut prepared.combined,
                    &staged.records,
                    &prepared.extra_members,
                    &options.modified_by,
                )
                .await
        };
        let members = match commit_result {
            Ok(members) => members,
            Err(error) => {
                self.clean_written_paths(&staged.written_paths).await;
                return Err(AppError::from(error).context("error inserting archive blocks"));
            }
        };

        self.finish_native_commit(&prepared.combined, &members)
            .await;
        tracing::debug!(
            containers_imported = prepared.combined.containers.len(),
            "import archive - done"
        );
        let (counts, container_ids) =
            summarize_native_import(&prepared.created_order, staged.records.len());
        Ok((
            Imported {
                members_left_out: prepared.members_left_out,
            },
            counts,
            container_ids,
        ))
    }

    async fn prepare_native_archive(
        &self,
        zip: &Archive,
        containers: Vec<(usize, String)>,
        options: &ImportOptions,
        elsewhere: &HashSet<String>,
    ) -> Result<PreparedNativeArchive, AppError> {
        let mut combined = ContainersAndBlocks::default();
        let mut extra_members = Vec::new();
        let mut by_dir = HashMap::with_capacity(containers.len());
        let mut files = HashMap::with_capacity(containers.len());
        let mut copies = HashMap::new();
        let mut created_order = Vec::with_capacity(containers.len());
        let mut left_out = HashSet::new();

        for (index, dir) in containers {
            let jsonl = read_entry(zip, index, MAX_JSONL_BYTES).await?;
            let mut parsed = parse_board_jsonl(jsonl, options, elsewhere).await?;
            if parsed.containers_and_blocks.containers.is_empty() {
                return Err(AppError::bad_request("missing board in archive"));
            }
            files.insert(dir.clone(), mem::take(&mut parsed.files));
            for (old, new) in mem::take(&mut parsed.renamed) {
                copies.entry(old).or_insert(new);
            }
            let kept = self
                .resolve_kept_members(parsed.members, &options.workspace_id, &mut left_out)
                .await?;
            if let Some(first) = parsed.containers_and_blocks.containers.first() {
                by_dir.insert(dir, first.id.clone());
            }
            for container in &parsed.containers_and_blocks.containers {
                created_order.push((container.kind, container.id.clone()));
                for member in &kept {
                    extra_members.push(ContainerMember {
                        container_id: container.id.clone(),
                        ..member.clone()
                    });
                }
            }
            combined
                .containers
                .append(&mut parsed.containers_and_blocks.containers);
            combined
                .blocks
                .append(&mut parsed.containers_and_blocks.blocks);
        }

        Ok(PreparedNativeArchive {
            combined,
            extra_members,
            by_dir,
            files,
            copies,
            created_order,
            members_left_out: left_out.len(),
        })
    }

    async fn resolve_kept_members(
        &self,
        members: Vec<ContainerMember>,
        workspace_id: &str,
        left_out: &mut HashSet<String>,
    ) -> Result<Vec<ContainerMember>, AppError> {
        let mut kept = Vec::with_capacity(members.len());
        for member in members {
            if permissions::role_of(&member).is_none() {
                continue;
            }
            let actor = self.actor(&member.user_id, workspace_id).await?;
            match actor.workspace {
                Some(workspace_role) => kept.push(no_more_than_holdable(member, workspace_role)),
                None => {
                    left_out.insert(member.user_id);
                }
            }
        }
        Ok(kept)
    }

    /// Unpacks every non-orphan, non-missing file entry of `zip` through a doomed temp
    /// directory and writes it to `FileBackend` under a `writing` mark. Any failure
    /// removes the temp directory and gives up every written object.
    async fn stage_native_files(
        &self,
        zip: &Archive,
        largest_file: u64,
        org_id: &str,
        by_dir: &HashMap<String, String>,
        files: &HashMap<String, ArchiveFiles>,
    ) -> Result<StagedNativeFiles, AppError> {
        let mut entries = Vec::new();
        for index in 0..zip.len() {
            let (dir, filename) = entry_name(zip, index)?;
            if filename.is_empty() || filename == "version.json" || filename == "board.jsonl" {
                continue;
            }
            let Some(container_id) = by_dir.get(&dir) else {
                tracing::warn!(dir, filename, "skipping orphan image in archive");
                continue;
            };
            if files
                .get(&dir)
                .is_some_and(|said| said.missing.contains(&filename))
            {
                tracing::warn!(
                    dir,
                    filename,
                    "skipping an entry of the archive that its board says is missing"
                );
                continue;
            }
            entries.push((index, dir, filename, container_id.clone()));
        }
        if entries.is_empty() {
            return Ok(StagedNativeFiles {
                records: Vec::new(),
                written_paths: Vec::new(),
                file_map: HashMap::new(),
            });
        }

        let work_dir = self
            .config()
            .tmp_dir()
            .join(format!("import-stage-{}", new_secret()));
        fs::create_dir_all(&work_dir).await.map_err(|error| {
            AppError::internal(format_args!("cannot create staging directory: {error}"))
        })?;
        let doomed_dir = Doomed::directory(work_dir);
        let unpacked = doomed_dir.path().join("entry");

        let mut records = Vec::with_capacity(entries.len());
        let mut written_paths = Vec::with_capacity(entries.len());
        let mut file_map: HashMap<String, HashMap<String, String>> = HashMap::new();

        for (index, dir, filename, container_id) in entries {
            let upload_name = name_in(files, &dir, &filename);
            let place = FilePlace::new(org_id, &container_id, upload_name);
            let written = self
                .write_one_native_file(zip, index, largest_file, &unpacked, &place.path)
                .await;
            let size = match written {
                Ok(size) => size,
                Err(error) => {
                    self.clean_written_paths(&written_paths).await;
                    doomed_dir.remove().await;
                    return Err(error.context(format_args!(
                        "cannot import file {filename} for board {dir}"
                    )));
                }
            };
            written_paths.push(place.path.clone());
            let info = file_info(&container_id, place, size, "");
            file_map
                .entry(container_id)
                .or_default()
                .insert(filename, info.stored_name.clone());
            records.push(info);
        }

        doomed_dir.remove().await;
        Ok(StagedNativeFiles {
            records,
            written_paths,
            file_map,
        })
    }

    async fn write_one_native_file(
        &self,
        zip: &Archive,
        index: usize,
        largest_file: u64,
        unpacked: &Path,
        storage_path: &str,
    ) -> Result<u64, AppError> {
        unpack_entry(zip, index, largest_file, unpacked).await?;
        let marked = self.mark_writing(storage_path).await;
        if let Err(error) = marked {
            remove_unpacked(unpacked).await;
            return Err(error);
        }
        let mut file = match fs::File::open(unpacked).await {
            Ok(file) => file,
            Err(error) => {
                remove_unpacked(unpacked).await;
                self.give_up_writing(storage_path, Written::Settled).await;
                return Err(AppError::internal(format_args!(
                    "cannot open an unpacked archive entry: {error}"
                )));
            }
        };
        let write = self.files().write(storage_path, &mut file);
        let outcome = tokio::time::timeout(WRITE_TIMEOUT, write).await;
        remove_unpacked(unpacked).await;
        match outcome {
            Ok(Ok(size)) => Ok(size),
            failed => {
                self.give_up_writing(storage_path, Written::Unsettled).await;
                let message = match failed {
                    Ok(Err(error)) => error.to_string(),
                    _ => "it took longer than a write may take".to_owned(),
                };
                Err(AppError::internal(format_args!(
                    "unable to store the file in the files storage: {message}"
                )))
            }
        }
    }

    async fn clean_written_paths(&self, paths: &[String]) {
        for path in paths {
            self.give_up_writing(path, Written::Settled).await;
        }
    }

    async fn finish_native_commit(
        &self,
        combined: &ContainersAndBlocks,
        members: &[ContainerMember],
    ) {
        if !combined.containers.is_empty() {
            let made = combined.containers.iter().map(|container| &container.id);
            self.collab()
                .recheck_access(self, AccessScope::containers(made))
                .await;
        }
        self.announce_created(combined, members).await;

        let by_id: HashMap<&str, (&str, bool)> = combined
            .containers
            .iter()
            .map(|c| (c.id.as_str(), (c.workspace_id.as_str(), c.is_template)))
            .collect();
        for member in members {
            if let Some(&(workspace_id, false)) = by_id.get(member.container_id.as_str())
                && let Err(error) = self
                    .add_containers_to_default_category(
                        &member.user_id,
                        workspace_id,
                        std::slice::from_ref(&member.container_id),
                    )
                    .await
            {
                tracing::warn!(
                    container_id = %member.container_id,
                    user_id = %member.user_id,
                    %error,
                    "failed to add imported container to default sidebar category after commit"
                );
            }
        }
        for container in &combined.containers {
            self.metrics()
                .container_copied(ContainerWork::Import, counted_as(container.kind));
        }
    }
}

/// Points every body and attachment block at its newly stored file name (or empty if
/// missing) and remaps cross-container block references in memory, then validates every
/// block before the database transaction begins.
fn rewrite_and_validate_blocks(
    combined: &mut ContainersAndBlocks,
    file_map: &HashMap<String, HashMap<String, String>>,
    copies: &HashMap<String, String>,
) -> Result<(), AppError> {
    for block in &mut combined.blocks {
        let container_files = file_map.get(block.container_id.as_str());
        if let Some(body) = block.body.take() {
            block.body = if came_with_more(&body, copies) {
                pointed(
                    body,
                    |old| container_files.and_then(|files| files.get(old)).cloned(),
                    copies,
                )
            } else {
                Some(body)
            };
        }
        if block.block_type == BlockType::Attachment {
            for key in ["fileId", "attachmentId"] {
                if let Some(old) = block.fields.get(key).and_then(Value::as_str) {
                    let new_name = container_files
                        .and_then(|files| files.get(old))
                        .cloned()
                        .unwrap_or_default();
                    block.fields.insert(key.to_owned(), Value::String(new_name));
                }
            }
        }
        block.validate().map_err(|source| {
            AppError::from(StoreError::InvalidBlock {
                id: block.id.clone(),
                source,
            })
        })?;
    }
    Ok(())
}
