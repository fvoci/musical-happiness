//! Checks that many handlers share: who may do what on a board or in a team.

use fvoci_model::{Block, BlockPatch, BlockType, BoardType};

use crate::app::App;
use crate::error::{AppError, Reason};
use crate::permissions::{BoardPermission, TeamPermission};

/// Checks that the user is an administrator of the instance.
///
/// Who is one is no secret, and neither is that these routes exist: anybody else is
/// refused (403, reason `admin_required`). A route under a team checks first that the
/// team exists, as every team route does, so there the refusal is of a real team.
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
/// What a user without the permission is told depends on what they may know. A member
/// of the board is refused (403, `denial`). So is a user of the team on an open board,
/// which every user of its team can see. For anybody else the board does not exist
/// (404), whether it does or not.
pub(crate) async fn require_board_permission(
    app: &App,
    user_id: &str,
    board_id: &str,
    permission: BoardPermission,
    denial: &str,
) -> Result<(), AppError> {
    if app
        .has_permission_to_board(user_id, board_id, permission)
        .await?
    {
        Ok(())
    } else {
        Err(board_refusal(app, user_id, board_id, denial).await)
    }
}

/// What a user is told who may not do what they ask on the board named in the path:
/// refused (403, `denial`) if they can see the board, not found (404) if they cannot.
/// For handlers whose rule is more than one permission. If the store fails while this
/// is found out, the answer is that failure.
pub(crate) async fn board_refusal(
    app: &App,
    user_id: &str,
    board_id: &str,
    denial: &str,
) -> AppError {
    match sees_board(app, user_id, board_id).await {
        Ok(true) => AppError::forbidden(denial),
        Ok(false) => AppError::not_found("board"),
        Err(error) => error,
    }
}

/// The refusal of a change to a comment of somebody else, which only the board's admins
/// may make.
const OTHERS_COMMENTS_DENIED: &str = "access denied to change other users' comments";

/// Checks that the user may change the block, of a board the request names: a comment
/// is its author's to change with the commenter's role, and somebody else's only the
/// board's admins change (as only they delete it); any other block is the editors'.
/// A refusal says `denial`, except for the comment of somebody else, whose refusal says
/// so. [`require_block_patch_permission`] checks the patch itself.
pub(crate) async fn require_block_change_permission(
    app: &App,
    user_id: &str,
    block: &Block,
    denial: &str,
) -> Result<(), AppError> {
    let (permission, denial) = match block.block_type {
        BlockType::Comment => comment_permission(user_id, block, denial),
        _ => (BoardPermission::ManageBoardCards, denial),
    };
    require_board_permission(app, user_id, &block.board_id, permission, denial).await
}

/// Checks what a patch does beyond changing a block's text. Changing a comment is its
/// text (its body and the files that go with it): moving it to another card
/// (`parentId`) or making it a block of another type is for editors (the commenter's
/// role makes no card and moves none), and making a block a comment is for who may
/// change that comment, so that nobody makes a comment that says somebody else wrote
/// it. A refusal says what [`require_block_change_permission`]'s says.
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
    if block.block_type == BlockType::Comment && moves {
        require_board_permission(
            app,
            user_id,
            &block.board_id,
            BoardPermission::ManageBoardCards,
            denial,
        )
        .await?;
    }
    let Some(new_type) = patch.block_type.as_ref() else {
        return Ok(());
    };
    let (permission, denial) = match (&block.block_type, new_type) {
        (BlockType::Comment, BlockType::Comment) => return Ok(()),
        (BlockType::Comment, _) => (BoardPermission::ManageBoardCards, denial),
        (_, BlockType::Comment) => comment_permission(user_id, block, denial),
        _ => return Ok(()),
    };
    require_board_permission(app, user_id, &block.board_id, permission, denial).await
}

/// Checks, for deleting a block or bringing it back, that a comment of somebody else is
/// the board admins' to do (`ManageOthersComments`); refused with `denial`. Any other
/// block, and the user's own comment, need nothing more than the editor's role the
/// caller asked for.
pub(crate) async fn require_comment_owner_or_admin(
    app: &App,
    user_id: &str,
    block: &Block,
    denial: &str,
) -> Result<(), AppError> {
    if block.block_type != BlockType::Comment || block.created_by == user_id {
        return Ok(());
    }
    require_board_permission(
        app,
        user_id,
        &block.board_id,
        BoardPermission::ManageOthersComments,
        denial,
    )
    .await
}

/// The permission that changing a comment, or making the block one, needs, and the
/// refusal without it.
fn comment_permission<'a>(
    user_id: &str,
    block: &Block,
    denial: &'a str,
) -> (BoardPermission, &'a str) {
    if block.created_by == user_id {
        (BoardPermission::CommentBoardCards, denial)
    } else {
        (
            BoardPermission::ManageOthersComments,
            OTHERS_COMMENTS_DENIED,
        )
    }
}

/// Checks that the user has the permission on a board that the request does not name
/// itself but reaches through `what`, such as a block or a card of the board.
///
/// A user who may not read the board's contents is told that `what` does not exist
/// (404): the identifier in the request says nothing about a board, so not even an open
/// board is given away. A member without the permission is refused (403, `denial`).
pub(crate) async fn require_permission_through(
    app: &App,
    user_id: &str,
    board_id: &str,
    permission: BoardPermission,
    denial: &str,
    what: &str,
) -> Result<(), AppError> {
    if app
        .has_permission_to_board(user_id, board_id, permission)
        .await?
    {
        Ok(())
    } else if reads_board(app, user_id, board_id).await? {
        Err(AppError::forbidden(denial))
    } else {
        Err(AppError::not_found(what))
    }
}

/// Checks that the user can see the board at all: as a member, or because it is an open
/// board of their team. For anybody else it does not exist (404).
pub(crate) async fn require_board_visible(
    app: &App,
    user_id: &str,
    board_id: &str,
) -> Result<(), AppError> {
    if sees_board(app, user_id, board_id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("board"))
    }
}

/// Whether the user can see the board at all: as a member, or because it is an open
/// board of their team.
async fn sees_board(app: &App, user_id: &str, board_id: &str) -> Result<bool, AppError> {
    Ok(reads_board(app, user_id, board_id).await?
        || sees_open_board(app, user_id, board_id).await?)
}

/// Whether the user may read what is on the board: its members may.
async fn reads_board(app: &App, user_id: &str, board_id: &str) -> Result<bool, AppError> {
    app.has_permission_to_board(user_id, board_id, BoardPermission::ViewBoard)
        .await
}

/// Whether the board is an open board of a team the user may see. Such a board is no
/// secret to the user, though its contents are until they join. A board that is not
/// there is seen by nobody; a store that fails is an error, not a "no".
async fn sees_open_board(app: &App, user_id: &str, board_id: &str) -> Result<bool, AppError> {
    if user_id.is_empty() {
        return Ok(false);
    }
    match app.board(board_id).await {
        Ok(board) => Ok(board.board_type == BoardType::Open
            && app.has_permission_to_team(user_id, &board.team_id, TeamPermission::ViewTeam)),
        Err(error) if error.is_not_found() => Ok(false),
        Err(error) => Err(error),
    }
}

/// Fails with a 404 if there is no such team, and with a 403 that says `denial` unless
/// the user may see it.
///
/// Every user may see every team of the server's one organization. TEMPORARY, with
/// `only_organization_scope`: a route that is moved under a workspace asks who is a
/// member of it (`api/workspace.rs`), and this check goes with the last that is not.
pub(crate) async fn require_team_access(
    app: &App,
    user_id: &str,
    team_id: &str,
    denial: &str,
) -> Result<(), AppError> {
    app.team(team_id).await?;
    if app.has_permission_to_team(user_id, team_id, TeamPermission::ViewTeam) {
        Ok(())
    } else {
        Err(AppError::forbidden(denial))
    }
}

/// Fails with a 400 that names the member `field` if the team a request body names
/// does not exist.
pub(crate) async fn require_team_in_body(
    app: &App,
    team_id: &str,
    field: &'static str,
) -> Result<(), AppError> {
    match app.team(team_id).await {
        Ok(_) => Ok(()),
        Err(error) if error.is_not_found() => {
            Err(AppError::bad_request("no such team").with_field(field))
        }
        Err(error) => Err(error),
    }
}

/// Returns whether a user is a guest. There are no guests on a standalone server, but
/// the user must exist: a missing user is a 404.
pub(crate) async fn user_is_guest(app: &App, user_id: &str) -> Result<bool, AppError> {
    app.user_is_guest(user_id).await
}
