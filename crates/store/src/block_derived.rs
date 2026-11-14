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

/// Brings what follows from `block` up to it, as it was just written: the files it
/// shows ([`file_refs::refresh`], which asks about the images of its body as
/// `new_images` says), and the search server's document of it. `before` is the block
/// as it was stored until now, `None` for a block that was not, or whose body was
/// written past it.
///
/// The search server is told of a block that is searched now if what it holds of it
/// may have changed (its title, its body, or that it is searched at all), and of one
/// that was searched and is no more.
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    before: Option<&Block>,
    new_images: NewImages,
) -> Result<(), StoreError> {
    file_refs::refresh(scope, block, new_images).await?;
    let was_searched = before.is_some_and(is_searched);
    let changed = before.is_none_or(|before| {
        !was_searched || before.title != block.title || before.body != block.body
    });
    if (is_searched(block) && changed) || (was_searched && !is_searched(block)) {
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

/// Notes what follows from a board going to the trash with everything on it, or
/// coming back from there, while its row is among the boards: the search server is
/// to hold what is on it now, and nothing else of it.
pub(crate) async fn board_moved(scope: &mut Scope<'_>, board_id: &str) -> Result<(), StoreError> {
    search_outbox::note_board(scope, board_id).await
}
