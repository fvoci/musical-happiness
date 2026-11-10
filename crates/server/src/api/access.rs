//! Checks that many handlers share: who may do what on a board or in a team.

use crate::app::App;
use crate::error::AppError;
use crate::permissions::{BoardPermission, TeamPermission};

/// Fails with a 403 that says `denial` unless the user has the permission on the board.
pub(crate) async fn require_board_permission(
    app: &App,
    user_id: &str,
    board_id: &str,
    permission: BoardPermission,
    denial: &str,
) -> Result<(), AppError> {
    if app
        .has_permission_to_board(user_id, board_id, permission)
        .await
    {
        Ok(())
    } else {
        Err(AppError::forbidden(denial))
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
/// outside single-user mode the user must exist: a missing user is a 404.
pub(crate) async fn user_is_guest(app: &App, user_id: &str) -> Result<bool, AppError> {
    if app.single_user_token().is_some() {
        return Ok(false);
    }
    app.user_is_guest(user_id).await
}
