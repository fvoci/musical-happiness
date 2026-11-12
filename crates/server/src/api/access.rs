//! Checks that many handlers share: who may do what on a board or in a workspace.
//!
//! Every check of a board asks `App::board_access` once and decides by the grant it
//! gives (`permissions::grant`).

use fvoci_model::{Block, BlockPatch, BlockType, Workspace};

use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::{self, BoardPermission, CommentAction, Grant};

/// Checks that the user is an administrator of the instance.
///
/// Who is one is no secret, and neither is that these routes exist: anybody else is
/// refused (403, reason `admin_required`). A route under a workspace checks first that the
/// workspace exists, as every workspace route does, so there the refusal is of a real workspace.
pub(crate) async fn require_admin(app: &App, user_id: &str) -> Result<(), AppError> {
    if app.user(user_id).await?.is_admin {
        Ok(())
    } else {
        Err(
            AppError::forbidden("only an administrator of the instance may do this")
                .with_reason(Reason::AdminRequired),
        )
    }
}

/// Checks that the user has the permission on the board that the request names in its
/// path.
///
/// What a user without the permission is told depends on what they may know. Who sees
/// the board (`Grant::sees`) is refused (403, `denial`); for anybody else the board does
/// not exist (404), whether it does or not.
pub(crate) async fn require_board_permission(
    app: &App,
    user_id: &str,
    board_id: &str,
    permission: BoardPermission,
    denial: &str,
) -> Result<(), AppError> {
    let grant = app.board_grant(user_id, board_id).await?;
    require_granted(grant, permission, denial)
}

/// Checks that the grant of the board named in the path has the permission: refused
/// (403, `denial`) if the user sees the board, not found (404) if not, and not found
/// for the content of a board the user only manages (`Grant::hides`).
pub(crate) fn require_granted(
    grant: Grant,
    permission: BoardPermission,
    denial: &str,
) -> Result<(), AppError> {
    if grant.allows(permission) {
        Ok(())
    } else if grant.hides(permission) {
        Err(AppError::not_found("board"))
    } else {
        Err(AppError::forbidden(denial))
    }
}

/// What a user is told who may not do what they ask on the board named in the path, if
/// it is not its content: refused (403, `denial`) if they see the board, not found
/// (404) if they do not.
pub(crate) fn refusal(grant: Grant, denial: &str) -> AppError {
    if grant.sees() {
        AppError::forbidden(denial)
    } else {
        AppError::not_found("board")
    }
}

/// The refusal of a comment to its author who did not delete it.
const SOMEBODY_ELSE_DELETED: &str =
    "a comment somebody else deleted is brought back by the board's admins";

/// What a user does to a block, which a comment of somebody else lets only the board's
/// admins do, each refused in its own words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockAction<'a> {
    /// Change it (`patchBlock`, `patchBlocks`, `patchBoardsAndBlocks`).
    Change,
    /// Delete it (`deleteBlock`, `deleteBoardsAndBlocks`).
    Delete,
    /// Bring it back from the trash (`undeleteBlock`), where the deletion of
    /// `deleted_by` put it.
    Undelete {
        /// Who deleted it; empty if nobody did.
        deleted_by: &'a str,
    },
}

impl<'a> BlockAction<'a> {
    /// The refusal of this to a comment of somebody else.
    fn others_comments_denial(self) -> &'static str {
        match self {
            Self::Change => "access denied to change other users' comments",
            Self::Delete => "access denied to delete other users' comments",
            Self::Undelete { .. } => "access denied to undelete other users' comments",
        }
    }

    /// What it is to a comment.
    fn on_comment(self) -> CommentAction<'a> {
        match self {
            Self::Change => CommentAction::Change,
            Self::Delete => CommentAction::Delete,
            Self::Undelete { deleted_by } => CommentAction::Undelete { deleted_by },
        }
    }
}

/// Checks that the user may do `action` to the block, of a board the request names: a
/// comment is its author's from the commenter's role up, and somebody else's the board
/// admins' only (`ManageOthersComments`); any other block is the editors'. A refusal
/// says `denial`, except for the comment of somebody else, whose refusal says so.
/// [`require_block_patch_permission`] checks a patch itself, and
/// [`require_deletion_permission`] what a deletion takes along.
pub(crate) async fn require_block_permission(
    app: &App,
    user_id: &str,
    block: &Block,
    action: BlockAction<'_>,
    denial: &str,
) -> Result<(), AppError> {
    let (permission, denial) = block_permission(user_id, block, action, denial);
    let grant = app.board_grant(user_id, &block.board_id).await?;
    require_granted(grant, permission, denial)
}

/// The permission that doing `action` to the block needs, and the refusal without it.
fn block_permission<'a>(
    user_id: &str,
    block: &Block,
    action: BlockAction<'_>,
    denial: &'a str,
) -> (BoardPermission, &'a str) {
    match block.block_type {
        BlockType::Comment => comment_permission(user_id, block, action, denial),
        _ => (BoardPermission::ManageBoardCards, denial),
    }
}

/// Checks what a patch does beyond changing a block's text. Changing a comment is its
/// text (its body and the files that go with it): moving it to another card
/// (`parentId`) or making it a block of another type is for editors (the commenter's
/// role makes no card and moves none), and making a block a comment is for who may
/// change that comment, so that nobody makes a comment that says somebody else wrote
/// it. A refusal says what [`require_block_permission`]'s says.
pub(crate) async fn require_block_patch_permission(
    app: &App,
    user_id: &str,
    block: &Block,
    patch: &BlockPatch,
    denial: &str,
) -> Result<(), AppError> {
    // A block directly on its board has no parent, which clients of the original name
    // by the board: naming it so moves nothing.
    let moves = patch.parent_id.as_ref().is_some_and(|parent_id| {
        *parent_id != block.parent_id
            && !(block.parent_id.is_empty() && *parent_id == block.board_id)
    });
    let changes_type = patch.block_type.as_ref().is_some_and(|new_type| {
        (block.block_type == BlockType::Comment) != (*new_type == BlockType::Comment)
    });
    let moves_comment = block.block_type == BlockType::Comment && moves;
    if !(changes_type || moves_comment) {
        return Ok(());
    }
    let grant = app.board_grant(user_id, &block.board_id).await?;
    if moves_comment {
        require_granted(grant, BoardPermission::ManageBoardCards, denial)?;
    }
    let Some(new_type) = patch.block_type.as_ref() else {
        return Ok(());
    };
    let (permission, denial) = match (&block.block_type, new_type) {
        (BlockType::Comment, BlockType::Comment) => return Ok(()),
        (BlockType::Comment, _) => (BoardPermission::ManageBoardCards, denial),
        (_, BlockType::Comment) => comment_permission(user_id, block, BlockAction::Change, denial),
        _ => return Ok(()),
    };
    require_granted(grant, permission, denial)
}

/// Checks that the user may delete the block, of a board the request names, with what
/// deleting it takes along: [`require_deletion_permission`] with the blocks below it.
pub(crate) async fn require_delete_permission(
    app: &App,
    user_id: &str,
    block: &Block,
    denial: &str,
) -> Result<(), AppError> {
    // Only a comment's replies have a say: what goes with a card is the card's.
    let below = if block.block_type == BlockType::Comment {
        app.blocks_below(&block.id).await?
    } else {
        Vec::new()
    };
    require_deletion_permission(app, user_id, block, &below, BlockAction::Delete, denial).await
}

/// Checks that the user may delete a block and what goes with it, or bring them back
/// (`action`): `top` is the block that is deleted, `below` the blocks it takes along.
/// The top decides for a card, or any block that is no comment: the comments on a card
/// go with it. A comment takes the replies below it, so who deletes it, or brings it
/// back, must be allowed that for each of them too: nobody takes away the comment of
/// somebody else by deleting their own. The top's permission is asked first.
/// For a comment this is a guard: the store keeps anything from being below one.
pub(crate) async fn require_deletion_permission(
    app: &App,
    user_id: &str,
    top: &Block,
    below: &[Block],
    action: BlockAction<'_>,
    denial: &str,
) -> Result<(), AppError> {
    let replies = if top.block_type == BlockType::Comment {
        below
    } else {
        &[]
    };
    let grant = app.board_grant(user_id, &top.board_id).await?;
    for block in std::iter::once(top).chain(replies) {
        let (permission, denial) = block_permission(user_id, block, action, denial);
        require_granted(grant, permission, denial)?;
    }
    Ok(())
}

/// The permission that doing `action` to a comment, or making the block one, needs
/// ([`permissions::comment_permission`]), and the refusal without it: `denial` for the
/// author, the action's own words for anybody else, and for the author of a comment
/// somebody else deleted that it is the board's admins' to bring back.
fn comment_permission<'a>(
    user_id: &str,
    block: &Block,
    action: BlockAction<'_>,
    denial: &'a str,
) -> (BoardPermission, &'a str) {
    let permission =
        permissions::comment_permission(user_id, &block.created_by, action.on_comment());
    match action {
        _ if permission == BoardPermission::CommentBoardCards => (permission, denial),
        BlockAction::Undelete { deleted_by }
            if block.created_by == user_id && !deleted_by.is_empty() =>
        {
            (permission, SOMEBODY_ELSE_DELETED)
        }
        _ => (permission, action.others_comments_denial()),
    }
}

/// Checks that the user has the permission on a board that the request does not name
/// itself but reaches through `what`, such as a block or a card of the board.
///
/// A user who may not read the board's contents is told that `what` does not exist
/// (404): the identifier in the request says nothing about a board, so not even an open
/// board is given away. A reader without the permission is refused (403, `denial`).
pub(crate) async fn require_permission_through(
    app: &App,
    user_id: &str,
    board_id: &str,
    permission: BoardPermission,
    denial: &str,
    what: &str,
) -> Result<(), AppError> {
    let grant = app.board_grant(user_id, board_id).await?;
    if grant.allows(permission) {
        Ok(())
    } else if grant.reads() {
        Err(AppError::forbidden(denial))
    } else {
        Err(AppError::not_found(what))
    }
}

/// Checks that the user sees the board at all (`Grant::sees`). For anybody else it does
/// not exist (404).
pub(crate) async fn require_board_visible(
    app: &App,
    user_id: &str,
    board_id: &str,
) -> Result<(), AppError> {
    if app.board_grant(user_id, board_id).await?.sees() {
        Ok(())
    } else {
        Err(AppError::not_found("board"))
    }
}

/// The workspace, or a 404 unless the user works in it (`App::works_in_workspace`): a workspace
/// the user is no member of, and a workspace of another organization, answer as one that
/// does not exist.
///
/// For the copy of a board, whose query names the workspace it goes to: the routes that name
/// no workspace in their path work in the server's one organization (TEMPORARY, with
/// `only_organization_scope`), and ask the same fact the routes under a workspace ask
/// (`api/workspace.rs`, `WorksIn`).
pub(crate) async fn require_workspace_access(
    app: &App,
    user_id: &str,
    workspace_id: &str,
) -> Result<Workspace, AppError> {
    if !app.works_in_workspace(user_id, workspace_id).await? {
        return Err(AppError::not_found("workspace"));
    }
    app.workspace(workspace_id).await
}

/// Fails with a 400 that names the member `field` if the workspace a request body names
/// does not exist, or is one the user does not work in: the two answer alike.
pub(crate) async fn require_workspace_in_body(
    app: &App,
    user_id: &str,
    workspace_id: &str,
    field: &'static str,
) -> Result<(), AppError> {
    let no_such_workspace = || AppError::bad_request("no such workspace").with_field(field);
    if !app.works_in_workspace(user_id, workspace_id).await? {
        return Err(no_such_workspace());
    }
    match app.workspace(workspace_id).await {
        Ok(_) => Ok(()),
        Err(error) if error.is_not_found() => Err(no_such_workspace()),
        Err(error) => Err(error),
    }
}

/// Returns whether a user is a guest. There are no guests on a standalone server, but
/// the user must exist: a missing user is a 404.
pub(crate) async fn user_is_guest(app: &App, user_id: &str) -> Result<bool, AppError> {
    app.user_is_guest(user_id).await
}
