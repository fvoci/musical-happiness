//! What is kept beside a block and follows from it: the files it shows, the blocks its
//! body names, the day of a diary it is the entry of, and that the search server is to
//! be told of it. Every write of a block,
//! and every way a block or a board goes and comes back, passes through here in its own
//! transaction, so that nothing that follows from a block is left as it was when the
//! block changes.

use fvoci_model::{Block, SearchKind};

use crate::db::{Id, Scope};
use crate::error::StoreError;
use crate::file_refs::{self, NewBodyFiles};
use crate::search_outbox;
use crate::{
    body_docs, body_links, body_versions, comment_versions, diaries, unit_progress, unit_reviews,
};

/// Whether the search server holds a document of the block: of a card or a page, but
/// not of one whose `isTemplate` is `true` ([`Block::is_template`]: for a card, a
/// template of its board's cards), as `search_docs` reads them. A card that becomes a
/// template, or stops being one, is so told.
fn is_searched(block: &Block) -> bool {
    SearchKind::of_block(&block.block_type).is_some() && !block.is_template()
}

/// What only this module makes: what [`file_refs::refresh`] and [`body_links::refresh`]
/// take, so that no other way of writing a block can put down one of the things that
/// follow from it and leave the others as they were. Its field is this module's alone.
#[derive(Debug)]
pub(crate) struct Refreshing(());

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
/// shows ([`file_refs::refresh`], which asks about the files of its body as
/// `new_files` says), the blocks its body names ([`body_links::refresh`]), the day an
/// entry is of ([`diaries::refresh`], which refuses a second entry of a day), and the
/// search server's document of it.
///
/// The search server is told of a block that is searched now if what it holds of it
/// may have changed (its title, its body, its kind, or that it is searched at all),
/// and of one that was searched and is no more.
pub(crate) async fn refresh(
    scope: &mut Scope<'_>,
    block: &Block,
    before: Before<'_>,
    new_files: NewBodyFiles,
) -> Result<(), StoreError> {
    diaries::refresh(scope, block, Refreshing(())).await?;
    file_refs::refresh(scope, block, new_files, Refreshing(())).await?;
    body_links::refresh(scope, block, Refreshing(())).await?;
    let told = match before {
        Before::Absent | Before::Unknown => is_searched(block),
        Before::Unchanged => false,
        Before::Stored(before) => match (is_searched(before), is_searched(block)) {
            (true, true) => {
                before.block_type != block.block_type
                    || before.title != block.title
                    || before.body != block.body
                    || before.number != block.number
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
pub(crate) async fn container_trashed(
    scope: &mut Scope<'_>,
    container_id: &str,
) -> Result<(), StoreError> {
    search_outbox::note_container(scope, container_id).await
}

/// Forgets what a single block `block_id` on `container_id` that was removed for good
/// (without entering the trash) leaves behind, using direct indexed deletes instead of
/// scanning every derived table across the organization.
pub(crate) async fn forget_block(
    scope: &mut Scope<'_>,
    block_id: &str,
    container_id: &str,
) -> Result<(), StoreError> {
    for prefix in [
        "DELETE FROM comment_versions WHERE block_id = ",
        "DELETE FROM body_updates WHERE block_id = ",
        "DELETE FROM body_docs WHERE block_id = ",
        "DELETE FROM body_versions WHERE block_id = ",
        "DELETE FROM body_version_editors WHERE block_id = ",
    ] {
        let mut sql = scope.sql(prefix);
        sql.bind(Id(block_id));
        scope.execute(sql).await?;
    }

    let mut links = scope.sql("DELETE FROM body_links WHERE source_id = ");
    links
        .bind(Id(block_id))
        .push_bind(" OR target_id = ", Id(block_id));
    scope.execute(links).await?;

    let org = scope.has_org().then(|| scope.org()).transpose()?;
    for prefix in [
        "DELETE FROM unit_progress WHERE unit_id = ",
        "DELETE FROM unit_reviews WHERE unit_id = ",
        "DELETE FROM unit_notes WHERE unit_id = ",
    ] {
        let mut sql = scope.sql(prefix);
        sql.changing_no_access().bind(Id(block_id));
        if let Some(org) = org.clone() {
            sql.push_bind(" AND org_id = ", org);
        }
        scope.execute(sql).await?;
    }

    let mut refs = scope.sql("DELETE FROM file_refs WHERE block_id = ");
    refs.bind(Id(block_id)).push(" RETURNING name");
    let mut names = scope
        .fetch_all(refs)
        .await?
        .iter()
        .map(|row| row.text(0))
        .collect::<Result<Vec<_>, _>>()?;
    if !names.is_empty() {
        names.sort_unstable();
        names.dedup();
        file_refs::mark(scope, Some((container_id, &names))).await?;
    }
    Ok(())
}

/// Forgets what blocks that are gone for good, neither on a board nor in the trash,
/// leave behind: what they showed is shown no more (a file no block shows is marked, to
/// be collected after the grace period), their bodies, versions and links go, and the
/// texts of the comments among them.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    comment_versions::forget_gone_comments(scope).await?;
    body_docs::forget_gone_blocks(scope).await?;
    body_versions::forget_gone_blocks(scope).await?;
    body_links::forget_gone_blocks(scope).await?;
    unit_progress::forget_gone_blocks(scope).await?;
    unit_reviews::forget_gone_blocks(scope).await?;
    file_refs::forget_gone_blocks(scope).await?;
    file_refs::mark(scope, None).await
}
