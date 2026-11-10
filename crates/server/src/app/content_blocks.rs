//! Content blocks: moving a block of a card to another place in the card's content.

use std::str::FromStr;

use fvoci_model::{Block, BlockPatch, JsonMap};
use serde_json::Value;

use super::{App, ChangeNotification};
use crate::error::AppError;

/// The field of a card that lists its content blocks in display order.
const CONTENT_ORDER: &str = "contentOrder";

/// Where a moved content block goes, relative to the block it is moved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovePosition {
    /// Right before the destination block.
    Before,
    /// Right after the destination block.
    After,
}

impl FromStr for MovePosition {
    type Err = AppError;

    /// Reads `before` or `after`, as the route spells them.
    fn from_str(position: &str) -> Result<Self, Self::Err> {
        match position {
            "before" => Ok(Self::Before),
            "after" => Ok(Self::After),
            _ => Err(AppError::bad_request(
                "invalid where parameter, use before or after",
            )),
        }
    }
}

impl App {
    /// Moves `block` right before or after `dst_block` in the content order of the
    /// card both belong to; listeners are told about the changed card as for any
    /// patched block.
    ///
    /// Only entries of the content order that are block identifiers take part. Other
    /// entries, such as the nested lists of side-by-side content, keep their place, and
    /// blocks inside them are not found. The card is patched even if the order comes
    /// out as it was.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::BadRequest`] if the blocks have different parents or one of
    /// them is not in the card's content order (which is what moving a block to itself
    /// amounts to), [`AppError::NotFound`] if the parent does not exist, and
    /// [`AppError::Internal`] if the card's content order is not a list.
    pub async fn move_content_block(
        &self,
        block: &Block,
        dst_block: &Block,
        position: MovePosition,
        user_id: &str,
    ) -> Result<(), AppError> {
        if block.parent_id != dst_block.parent_id {
            return Err(AppError::bad_request(format_args!(
                "not matching parent {} and {}",
                block.parent_id, dst_block.parent_id
            )));
        }
        let card = self.block(&block.parent_id).await?;
        let content_order = match card.fields.get(CONTENT_ORDER) {
            None => [].as_slice(),
            Some(Value::Array(content_order)) => content_order,
            // The original's type assertion panics here, which its clients see as the
            // same server error.
            Some(_) => {
                return Err(AppError::internal(format_args!(
                    "the content order of card {} is not a list",
                    card.id
                )));
            }
        };

        let new_content_order = moved(content_order, &block.id, &dst_block.id, position)?;
        let patch = BlockPatch {
            updated_fields: JsonMap::from_iter([(
                CONTENT_ORDER.to_owned(),
                Value::Array(new_content_order),
            )]),
            ..BlockPatch::default()
        };
        self.patch_block(&card.id, &patch, user_id, ChangeNotification::Send)
            .await?;
        Ok(())
    }
}

/// Returns `content_order` with every entry for `block_id` taken out and `block_id` put
/// right before or after every entry for `dst_block_id`.
///
/// Preserved quirk: an entry is looked at as the destination first, so a block moved to
/// itself is never seen as the source and the move is refused
/// (server/app/content_blocks.go:37-52).
fn moved(
    content_order: &[Value],
    block_id: &str,
    dst_block_id: &str,
    position: MovePosition,
) -> Result<Vec<Value>, AppError> {
    let mut new_content_order = Vec::with_capacity(content_order.len() + 1);
    let mut found_src = false;
    let mut found_dst = false;
    for entry in content_order {
        match entry.as_str() {
            Some(id) if id == dst_block_id => {
                found_dst = true;
                let moved_block = Value::from(block_id);
                match position {
                    MovePosition::Before => new_content_order.extend([moved_block, entry.clone()]),
                    MovePosition::After => new_content_order.extend([entry.clone(), moved_block]),
                }
            }
            Some(id) if id == block_id => found_src = true,
            _ => new_content_order.push(entry.clone()),
        }
    }

    if !found_src {
        return Err(AppError::bad_request(format_args!(
            "source block {block_id} not found"
        )));
    }
    if !found_dst {
        return Err(AppError::bad_request(format_args!(
            "destination block {dst_block_id} not found"
        )));
    }
    Ok(new_content_order)
}
