//! Copying a block of a board with what is directly below it.

use fvoci_model::{Block, BlockType, generate_block_ids};
use serde_json::Value;

use crate::blocks::{DuplicateAs, OLDEST_FIRST, fetch_blocks, insert_blocks, select_blocks};
use crate::db::Id;
use crate::file_refs::NewBodyFiles;
use crate::notification_noting::Telling;
use crate::{Store, StoreError, card_values, containers};

impl Store {
    /// Copies a block of a board and its direct children, except comments, under new
    /// identifiers. Returns the copies, the copy of the block itself first.
    ///
    /// The copy of the block gets its `isTemplate` field set according to `duplicate_as`.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board has no such block.
    pub async fn duplicate_block(
        &self,
        container_id: &str,
        block_id: &str,
        user_id: &str,
        duplicate_as: DuplicateAs,
    ) -> Result<Vec<Block>, StoreError> {
        self.transaction(async |scope| {
            let mut sql = select_blocks(scope);
            sql.push_bind(" WHERE (id = ", Id(block_id))
                .push_bind(" OR parent_id = ", Id(block_id))
                .push_bind(") AND container_id = ", Id(container_id))
                .push(OLDEST_FIRST);
            let family = fetch_blocks(scope, sql).await?;
            let not_found = || {
                StoreError::not_found(format_args!(
                    "block subtree BoardID={container_id} BlockID={block_id}"
                ))
            };

            let (roots, children): (Vec<Block>, Vec<Block>) = family
                .into_iter()
                .filter(|block| block.block_type != BlockType::Comment)
                .partition(|block| block.id == block_id);
            let mut root = roots.into_iter().next().ok_or_else(not_found)?;
            root.fields.insert(
                "isTemplate".to_owned(),
                Value::Bool(duplicate_as == DuplicateAs::Template),
            );

            let mut copies = Vec::with_capacity(children.len() + 1);
            copies.push(root);
            copies.extend(children);
            // What the source holds is settled before the copies are named, so that a
            // value that named its own card names no card; a copy on the same board then
            // names the cards its source named.
            let definitions = containers::find_container(scope, container_id)
                .await?
                .map(|container| container.card_properties)
                .unwrap_or_default();
            card_values::clear_copied_values(&mut copies, &definitions);
            generate_block_ids(&mut copies);
            card_values::keep_cards_of_board(scope, &mut copies, container_id, &definitions)
                .await?;
            // A copy names whom its original named: nobody is told again.
            let nobody = Telling::Nobody;
            insert_blocks(scope, &mut copies, user_id, NewBodyFiles::Unchecked, nobody).await?;
            Ok(copies)
        })
        .await
    }
}
