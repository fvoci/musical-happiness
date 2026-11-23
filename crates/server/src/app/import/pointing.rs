//! Pointing what an archive brought at what came with it, once all of it is stored:
//! blocks at their files, and bodies at the copies of what they name on other boards.

use std::collections::HashMap;

use fvoci_model::{BlockType, Body, Container, JsonMap};
use serde_json::Value;

use crate::app::App;

/// Maximum number of blocks fetched at once while rewriting stored file and cross-board
/// references so large boards stay bounded in memory.
const POINT_CHUNK_BLOCKS: usize = 16;

impl App {
    /// Points the imported blocks listed in `blocks_to_point` (by container ID) at the
    /// newly stored files and cross-board copies, in bounded chunks.
    ///
    /// A name the archive brought no file for becomes the empty name in an attachment's
    /// fields, and its node in a body an empty paragraph. A body that names a block of
    /// another board of the archive is pointed at its copy (`copies`, old identifier to
    /// new). Failures are logged, as in the original.
    pub(super) async fn point_at_stored_files(
        &self,
        container_map: &HashMap<String, Container>,
        blocks_to_point: &HashMap<String, Vec<String>>,
        file_map: &HashMap<(String, String), String>,
        copies: &HashMap<String, String>,
        user_id: &str,
    ) {
        for container in container_map.values() {
            let Some(ids) = blocks_to_point.get(&container.id) else {
                continue;
            };
            if ids.is_empty() {
                continue;
            }
            for chunk in ids.chunks(POINT_CHUNK_BLOCKS) {
                let blocks = match self.store().get_blocks_by_ids(chunk).await {
                    Ok(blocks) => blocks,
                    Err(error) => {
                        tracing::error!(
                            %error,
                            container_id = container.id,
                            "cannot retrieve imported blocks for board"
                        );
                        continue;
                    }
                };
                self.point_block_chunk(container, blocks, file_map, copies, user_id)
                    .await;
            }
        }
    }

    async fn point_block_chunk(
        &self,
        container: &Container,
        blocks: Vec<fvoci_model::Block>,
        file_map: &HashMap<(String, String), String>,
        copies: &HashMap<String, String>,
        user_id: &str,
    ) {
        let mut bodies = Vec::new();
        let mut block_ids = Vec::new();
        let mut block_patches = Vec::new();
        for block in blocks {
            if let Some(body) = block.body.filter(|body| came_with_more(body, copies)) {
                let file_of = |old: &str| {
                    file_map
                        .get(&(container.id.clone(), old.to_owned()))
                        .cloned()
                };
                bodies.push((block.id, pointed(body, file_of, copies)));
                continue;
            }
            if block.block_type != BlockType::Attachment {
                continue;
            }
            let mut updated_fields = JsonMap::new();
            for key in ["fileId", "attachmentId"] {
                if let Some(old) = block.fields.get(key).and_then(Value::as_str) {
                    let new_name = file_map
                        .get(&(container.id.clone(), old.to_owned()))
                        .cloned()
                        .unwrap_or_default();
                    updated_fields.insert(key.to_owned(), Value::String(new_name));
                }
            }
            if !updated_fields.is_empty() {
                block_ids.push(block.id);
                block_patches.push(fvoci_model::BlockPatch {
                    updated_fields,
                    ..fvoci_model::BlockPatch::default()
                });
            }
        }
        // The bodies as the archive wrote them, with the new files and copies: part
        // of the import, not a change of them (a comment gets no version).
        if !bodies.is_empty()
            && let Err(error) = self
                .store()
                .replace_bodies_as_written(&container.id, bodies)
                .await
        {
            tracing::error!(
                %error,
                container_id = container.id,
                "the imported bodies could not be pointed at their files"
            );
        }
        if block_ids.is_empty() {
            return;
        }

        let batch = fvoci_model::BlockPatchBatch {
            block_ids,
            block_patches,
        };
        if let Err(error) = self
            .patch_blocks(
                &container.workspace_id,
                &batch,
                user_id,
                crate::app::ChangeNotification::Send,
            )
            .await
        {
            tracing::error!(
                %error,
                container_id = container.id,
                "the imported blocks could not be pointed at their files"
            );
        }
    }
}

/// Whether the body shows a file or names a block that came with a copy (`copies`): a
/// body that does neither stays as it was stored.
pub(super) fn came_with_more(body: &Body, copies: &HashMap<String, String>) -> bool {
    let named = body.reference_ids();
    !body.file_ids().is_empty() || named.iter().any(|id| copies.contains_key(*id))
}

/// The body pointed at the files (`file_of` gives the new name of one) and at the
/// copies (`copies`, old identifier to new) that came with it. `None` if nothing is
/// left of it without its files: that is no body.
fn pointed(
    body: Body,
    file_of: impl Fn(&str) -> Option<String>,
    copies: &HashMap<String, String>,
) -> Option<Body> {
    let mut body = body.with_files_renamed(file_of)?;
    body.rename_references(|id| copies.get(id).cloned());
    Some(body)
}
