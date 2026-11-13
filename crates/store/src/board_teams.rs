//! Assigning workspace groups to boards, with authorization and audit in the transaction.

use fvoci_model::{
    AuditAction, Board, BoardMember, BoardTeam, BoardType, MinimumRole, WorkspaceRole,
};

use crate::audit::{self, AuditEntry};
use crate::db::{Id, Scope};
use crate::{Store, StoreError, board_team_access, boards, org_admin, teams, workspace_membership};

impl Store {
    /// Lists a board's assigned teams, including their current members.
    ///
    /// # Errors
    /// Returns the database error if the teams or their roles cannot be read.
    pub async fn board_teams(
        &self,
        workspace_id: &str,
        board_id: &str,
    ) -> Result<Vec<BoardTeam>, StoreError> {
        self.with_conn(async |scope| {
            let groups = teams::list(scope, workspace_id).await?;
            let mut sql = scope.sql("SELECT team_id, role FROM board_teams WHERE board_id = ");
            sql.bind(Id(board_id))
                .push_bind(" AND workspace_id = ", Id(workspace_id))
                .push_bind(" AND org_id = ", scope.org()?);
            let rows = scope.fetch_all(sql).await?;
            let mut assigned = Vec::new();
            for team in groups {
                for row in &rows {
                    if row.id(0)? == team.id {
                        assigned.push(BoardTeam {
                            team,
                            role: board_team_access::parse_role(&row.text(1)?)?,
                        });
                        break;
                    }
                }
            }
            Ok(assigned)
        })
        .await
    }

    /// Gives a team its board role, or removes its assignment with `None`.
    /// Direct roles are untouched; only a workspace admin or direct board admin acts.
    ///
    /// # Errors
    /// Returns not found for a missing or mismatched board/team or a revoked actor,
    /// or the database error. Audit records and the assignment commit together.
    pub async fn set_board_team(
        &self,
        workspace_id: &str,
        board_id: &str,
        team_id: &str,
        role: Option<MinimumRole>,
        by: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.transaction(async |scope| {
            org_admin::hold_organization_row(scope).await?;
            if !workspace_membership::hold_workspace(scope, workspace_id).await? {
                return Err(StoreError::not_found("workspace"));
            }
            boards::hold_board(scope, board_id).await?;
            let board = boards::get_board(scope, board_id).await?;
            if board.workspace_id != workspace_id { return Err(StoreError::not_found("board")); }
            require_manager(scope, &board, by).await?;
            teams::require_team(scope, workspace_id, team_id).await?;
            let users = people(scope, team_id).await?;
            let changes: Vec<BoardMember> = users.iter().map(|user_id| BoardMember { board_id: board_id.to_owned(), user_id: user_id.clone(), ..BoardMember::default() }).collect();
            if let Some(role) = role {
                record_new_access(scope, &board, &users, by).await?;
                let mut sql = scope.sql("INSERT INTO board_teams (board_id, team_id, workspace_id, org_id, role) VALUES (");
                sql.bind(Id(board_id)).push_bind(", ", Id(team_id)).push_bind(", ", Id(workspace_id))
                    .push_bind(", ", scope.org()?).push_bind(", ", role.as_str())
                    .push(") ON CONFLICT (board_id, team_id, org_id) DO UPDATE SET role = EXCLUDED.role");
                scope.execute(sql).await?;
            } else {
                let mut sql = scope.sql("DELETE FROM board_teams WHERE board_id = ");
                sql.bind(Id(board_id)).push_bind(" AND team_id = ", Id(team_id)).push_bind(" AND org_id = ", scope.org()?);
                scope.execute(sql).await?;
            }
            end_lost_subscriptions(scope, &changes).await?;
            Ok(changes)
        }).await
    }
}

async fn require_manager(scope: &mut Scope<'_>, board: &Board, by: &str) -> Result<(), StoreError> {
    let role = org_admin::membership(scope, &board.workspace_id, by)
        .await?
        .map(|(role, _)| role);
    let direct = boards::find_member(scope, &board.id, by).await?;
    if role == Some(WorkspaceRole::Admin)
        || (role == Some(WorkspaceRole::Member) && direct.is_some_and(|m| m.scheme_admin))
    {
        Ok(())
    } else {
        Err(StoreError::not_found("board"))
    }
}

async fn people(scope: &mut Scope<'_>, team_id: &str) -> Result<Vec<String>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT tm.user_id FROM {} tm WHERE tm.team_id = ",
        teams::present_members(scope)
    ));
    sql.bind(Id(team_id))
        .push_bind(" AND tm.org_id = ", scope.org()?)
        .push(" ORDER BY tm.user_id");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| row.id(0))
        .collect()
}

async fn reads(scope: &mut Scope<'_>, board_id: &str, user_id: &str) -> Result<bool, StoreError> {
    let direct = boards::find_member(scope, board_id, user_id)
        .await?
        .and_then(|m| m.role());
    Ok(direct
        .max(board_team_access::role_for(scope, board_id, user_id).await?)
        .is_some())
}

/// Checked before the grant, while the organization/workspace/board are held. This
/// also covers an administrator joining a team already assigned to private boards.
async fn record_new_access(
    scope: &mut Scope<'_>,
    board: &Board,
    users: &[String],
    by: &str,
) -> Result<(), StoreError> {
    if board.board_type == BoardType::Open || reads(scope, &board.id, by).await? {
        return Ok(());
    }
    if org_admin::membership(scope, &board.workspace_id, by)
        .await?
        .map(|(role, _)| role)
        != Some(WorkspaceRole::Admin)
    {
        return Ok(());
    }
    for user_id in users {
        if reads(scope, &board.id, user_id).await? {
            continue;
        }
        let entry = if user_id == by {
            AuditEntry::new(
                &board.workspace_id,
                by,
                AuditAction::SelfAddedToBoard,
                &board.id,
            )
        } else {
            AuditEntry::new(
                &board.workspace_id,
                by,
                AuditAction::AddedToBoard,
                &board.id,
            )
            .with_subject(user_id)
        };
        audit::record_entry(scope, &entry).await?;
    }
    Ok(())
}

/// The people whose inherited roles can change, captured before a team is removed.
pub(crate) async fn affected(
    scope: &mut Scope<'_>,
    team_id: &str,
    only_user: Option<&str>,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = scope.sql("SELECT bt.board_id, tm.user_id FROM board_teams bt JOIN team_members tm ON tm.team_id = bt.team_id AND tm.org_id = bt.org_id WHERE bt.team_id = ");
    sql.bind(Id(team_id))
        .push_bind(" AND bt.org_id = ", scope.org()?);
    if let Some(user_id) = only_user {
        sql.push_bind(" AND tm.user_id = ", Id(user_id));
    }
    sql.push(" ORDER BY bt.board_id, tm.user_id");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(BoardMember {
                board_id: row.id(0)?,
                user_id: row.id(1)?,
                ..BoardMember::default()
            })
        })
        .collect()
}

/// Records the boards a newly added team member will read before adding the link.
pub(crate) async fn joining(
    scope: &mut Scope<'_>,
    team_id: &str,
    user_id: &str,
    by: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = scope.sql("SELECT board_id FROM board_teams WHERE team_id = ");
    sql.bind(Id(team_id))
        .push_bind(" AND org_id = ", scope.org()?)
        .push(" ORDER BY board_id");
    let rows = scope.fetch_all(sql).await?;
    let mut changes = Vec::new();
    for row in rows {
        let board_id = row.id(0)?;
        boards::hold_board(scope, &board_id).await?;
        let board = boards::get_board(scope, &board_id).await?;
        record_new_access(scope, &board, &[user_id.to_owned()], by).await?;
        changes.push(BoardMember {
            board_id,
            user_id: user_id.to_owned(),
            ..BoardMember::default()
        });
    }
    Ok(changes)
}

/// Removing one source preserves subscriptions while a direct or other team role remains.
pub(crate) async fn end_lost_subscriptions(
    scope: &mut Scope<'_>,
    changes: &[BoardMember],
) -> Result<(), StoreError> {
    for member in changes {
        if !reads(scope, &member.board_id, &member.user_id).await? {
            crate::subscriptions::end_subscriptions_to_board(
                scope,
                &member.board_id,
                &member.user_id,
            )
            .await?;
        }
    }
    Ok(())
}
