//! Board-side consequences of changing workspace teams. These transactions keep
//! board audit and revocation together with the reusable team membership changes.

use crate::db::{Id, Scope};
use crate::{Store, StoreError, board_teams, teams};
use fvoci_model::BoardMember;

impl Store {
    /// Deletes a team, its members, and every role it was given on a board.
    ///
    /// # Errors
    /// As [`Store::rename_team`].
    pub async fn delete_team(
        &self,
        workspace_id: &str,
        team_id: &str,
        by: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            teams::managing(scope, workspace_id, by).await?;
            let changes = board_teams::affected(scope, team_id, None).await?;
            teams::delete(scope, workspace_id, team_id).await?;
            board_teams::end_lost_subscriptions(scope, &changes).await?;
            Ok(changes)
        })
        .await
    }

    /// Adds a nonguest workspace member to a team, idempotently.
    ///
    /// # Errors
    /// As [`Store::rename_team`]; not found for a guest or a person outside the workspace.
    pub async fn add_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            teams::managing(scope, workspace_id, by).await?;
            teams::require_team(scope, workspace_id, team_id).await?;
            teams::require_addition(scope, workspace_id, user_id, by).await?;
            let changes = board_teams::joining(scope, team_id, user_id, by).await?;
            teams::add_member(scope, workspace_id, team_id, user_id).await?;
            Ok(changes)
        })
        .await
    }

    /// Removes a person from a team, idempotently.
    ///
    /// # Errors
    /// As [`Store::rename_team`].
    pub async fn remove_team_member(
        &self,
        workspace_id: &str,
        team_id: &str,
        user_id: &str,
        by: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            teams::managing(scope, workspace_id, by).await?;
            teams::require_team(scope, workspace_id, team_id).await?;
            let changes = board_teams::affected(scope, team_id, Some(user_id)).await?;
            teams::remove_member(scope, workspace_id, team_id, user_id).await?;
            board_teams::end_lost_subscriptions(scope, &changes).await?;
            Ok(changes)
        })
        .await
    }
}

/// Demotion removes the stored links, so promoting the person later restores none.
pub(crate) async fn remove_workspace_teams(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT DISTINCT bt.board_id FROM board_teams bt JOIN team_members tm ON tm.team_id = bt.team_id AND tm.org_id = bt.org_id WHERE tm.workspace_id = ");
    sql.bind(Id(workspace_id))
        .push_bind(" AND tm.user_id = ", Id(user_id))
        .push_bind(" AND tm.org_id = ", scope.org()?);
    let changes = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(BoardMember {
                board_id: row.id(0)?,
                user_id: user_id.to_owned(),
                ..BoardMember::default()
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    teams::remove_workspace_memberships(scope, workspace_id, user_id).await?;
    board_teams::end_lost_subscriptions(scope, &changes).await
}
