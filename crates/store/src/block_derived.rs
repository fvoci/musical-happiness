//! What is kept beside a block and follows from it: the files it shows, and that the
//! search server is to be told of it. Every write of a block, and every way a block
//! or a board goes and comes back, passes through here in its own transaction, so
//! that nothing that follows from a block is left as it was when the block changes.

use fvoci_model::{Block, SearchKind};

use crate::db::Scope;
use crate::error::StoreError;
use crate::file_refs::{self, NewImages};
use crate::search_outbox;

/// Whether the search server holds a document of a block of this type.
fn is_searched(block: &Block) -> bool {
    SearchKind::of_block(&block.block_type).is_some()
}

/// What is known of a block as it was stored before a write of it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Before<'a> {
    /// It was not among the blocks: a new block, or one that comes back from the trash.
    Absent,
    /// It was stored so.
    Stored(&'a Block),
    /// It was stored, and what it was is not known here: its body was written past it.
    Unknown,
    /// It is as it was stored: nothing of it that a search reads changed.
    Unchanged,
}

/// Brings what follows from `block` up to it, as it was just written: the files it
/// shows ([`file_refs::refresh`], which asks about the images of its body as
/// `new_images` says), and the search server's document of it.
///
/// The search server is told of a block that is searched now if what it holds of it
/// may have changed (its title, its body, its kind, or that it is searched at all),
/// and of one that was searched and is no more.
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    before: Before<'_>,
    new_images: NewImages,
) -> Result<(), StoreError> {
    file_refs::refresh(scope, block, new_images).await?;
    let told = match before {
        Before::Absent | Before::Unknown => is_searched(block),
        Before::Unchanged => false,
        Before::Stored(before) => match (is_searched(before), is_searched(block)) {
            (true, true) => {
                before.block_type != block.block_type
                    || before.title != block.title
                    || before.body != block.body
            }
            (false, false) => false,
            (true, false) | (false, true) => true,
        },
    };
    if told {
        // Boxed: every write of a block carries this future, and most never run it.
        Box::pin(search_outbox::note_blocks(scope, &[&block.id])).await?;
    }
    Ok(())
}

/// Notes what follows from `blocks` going to the trash, before their rows go: the
/// search server is to hold no document of them. The files they show, the trash keeps
/// for them.
pub(crate) async fn trashed(scope: &mut Scope<'_>, blocks: &[Block]) -> Result<(), StoreError> {
    let searched: Vec<&str> = blocks
        .iter()
        .filter(|block| is_searched(block))
        .map(|block| block.id.as_str())
        .collect();
    if searched.is_empty() {
        return Ok(());
    }
    search_outbox::note_blocks(scope, &searched).await
}

/// Notes what follows from a board going to the trash with everything on it, while
/// its row is among the boards: the search server is to hold nothing of it. A board
/// that comes back needs no note of its own: each of its blocks comes back through
/// [`refresh`].
pub(crate) async fn board_trashed(scope: &mut Scope<'_>, board_id: &str) -> Result<(), StoreError> {
    search_outbox::note_board(scope, board_id).await
}
