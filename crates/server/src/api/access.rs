//! Checks that many handlers share: who may do what on a board or in a team.

use fvoci_model::BoardType;

use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission};

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

/// Fails with a 403 that says `denial` unless the user may see the team.
pub(crate) fn require_team_access(
    app: &App,
    user_id: &str,
    team_id: &str,
    denial: &str,
) -> Result<(), AppError> {
    if app.has_permission_to_team(user_id, team_id, TeamPermission::ViewTeam) {
        Ok(())
    } else {
        Err(AppError::forbidden(denial))
    }
}

/// Returns whether a user is a guest. There are no guests on a standalone server, but
/// the user must exist: a missing user is a 404.
pub(crate) async fn user_is_guest(app: &App, user_id: &str) -> Result<bool, AppError> {
    app.user_is_guest(user_id).await
}
