//! What the store keeps beside a block that follows from what the block holds: the
//! files it shows ([`crate::file_refs`]) and the blocks its body names
//! ([`crate::body_links`]).
//!
//! Every write of a block's body ends here, in the write's transaction: a block that is
//! made or comes back from the trash, one that is changed, a body an import wrote after
//! its block, and the body of a document its editors typed. What is kept of a body by
//! another table is put down here too, so that no way of writing a body leaves one
//! behind.

use fvoci_model::Block;

use crate::body_links;
use crate::db::Scope;
use crate::error::StoreError;
use crate::file_refs::{self, NewImages};

/// Puts down what follows from the block as it is now, in place of what followed from
/// it before: the files it shows, whose new images are asked as `new_images` says
/// ([`file_refs::refresh`]), and the blocks its body names ([`body_links::refresh`]).
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    new_images: NewImages,
) -> Result<(), StoreError> {
    file_refs::refresh(scope, block, new_images).await?;
    body_links::refresh(scope, block).await
}
