//! Effective board memberships, without turning inherited roles into stored direct roles.

use fvoci_model::{BoardMember, BoardRole, MinimumRole};

use crate::db::{Id, Scope, Sql};
use crate::{Store, StoreError, boards};

/// Joins only current nonguest members, within the same workspace and organization.
const INHERITED: &str = "SELECT bt.board_id, tm.user_id, bt.role FROM board_teams bt \
    JOIN team_members tm ON tm.team_id = bt.team_id AND tm.workspace_id = bt.workspace_id AND tm.org_id = bt.org_id \
    JOIN workspace_members wm ON wm.workspace_id = tm.workspace_id AND wm.user_id = tm.user_id \
    JOIN workspaces w ON w.id = tm.workspace_id AND w.org_id = tm.org_id \
    WHERE wm.role IN ('admin', 'member') AND w.delete_at = 0";

impl Store {
    /// The highest team role of a current nonguest member on this board.
    ///
    /// # Errors
    /// Returns the database error, including an unreadable stored role.
    pub async fn board_team_role(
        &self,
        board_id: &str,
        user_id: &str,
    ) -> Result<Option<BoardRole>, StoreError> {
        self.with_conn(async |scope| role_for(scope, board_id, user_id).await)
            .await
    }

    /// Current board members, with inherited roles separate from their direct flags.
    ///
    /// # Errors
    /// Returns the database error if either membership source cannot be read.
    pub async fn effective_board_members(
        &self,
        board_id: &str,
    ) -> Result<Vec<BoardMember>, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = boards::select_members(scope);
            sql.push(" JOIN boards b ON b.id = bm.board_id JOIN workspace_members wm ON wm.workspace_id = b.workspace_id AND wm.user_id = bm.user_id")
                .push_bind(" WHERE bm.board_id = ", Id(board_id));
            let mut members = boards::fetch_members(scope, sql).await?;
            let mut sql = inherited(scope)?;
            sql.push_bind(" AND bt.board_id = ", Id(board_id));
            extend(scope, sql, &mut members).await?;
            members.sort_by(|a, b| a.user_id.cmp(&b.user_id));
            Ok(members)
        }).await
    }
}

pub(crate) async fn role_for(
    scope: &mut Scope<'_>,
    board_id: &str,
    user_id: &str,
) -> Result<Option<BoardRole>, StoreError> {
    let mut sql = inherited(scope)?;
    sql.push_bind(" AND bt.board_id = ", Id(board_id))
        .push_bind(" AND tm.user_id = ", Id(user_id));
    let mut highest = None;
    for row in scope.fetch_all(sql).await? {
        highest = highest.max(Some(BoardRole::from(parse_role(&row.text(2)?)?)));
    }
    Ok(highest)
}

pub(crate) fn parse_role(value: &str) -> Result<MinimumRole, StoreError> {
    MinimumRole::ALL
        .into_iter()
        .find(|role| role.as_str() == value)
        .ok_or_else(|| StoreError::Corrupt {
            column: "role",
            reason: format!("invalid team board role: {value}"),
        })
}

fn inherited(scope: &Scope<'_>) -> Result<Sql, StoreError> {
    let mut sql = scope.sql(INHERITED);
    sql.push_bind(" AND bt.org_id = ", scope.org()?);
    Ok(sql)
}

async fn extend_for_user(
    scope: &mut Scope<'_>,
    user_id: &str,
    members: &mut Vec<BoardMember>,
) -> Result<(), StoreError> {
    let mut sql = inherited(scope)?;
    sql.push_bind(" AND tm.user_id = ", Id(user_id));
    extend(scope, sql, members).await?;
    members.sort_by(|a, b| a.board_id.cmp(&b.board_id));
    Ok(())
}

async fn extend(
    scope: &mut Scope<'_>,
    sql: Sql,
    members: &mut Vec<BoardMember>,
) -> Result<(), StoreError> {
    for row in scope.fetch_all(sql).await? {
        let board_id = row.id(0)?;
        let user_id = row.id(1)?;
        let role = parse_role(&row.text(2)?)?;
        if let Some(member) = members
            .iter_mut()
            .find(|m| m.board_id == board_id && m.user_id == user_id)
        {
            if member
                .inherited_role
                .is_none_or(|old| BoardRole::from(old) < BoardRole::from(role))
            {
                member.inherited_role = Some(role);
            }
        } else {
            members.push(BoardMember {
                board_id,
                user_id,
                inherited_role: Some(role),
                synthetic: true,
                ..BoardMember::default()
            });
        }
    }
    Ok(())
}

/// A role from a team on board `b`, for the named user expression. This is also used
/// by the system directory, so the organization is bound through the board's workspace.
pub(crate) fn push_has_team_role(sql: &mut Sql, scope: &Scope<'_>, user_id: &str) {
    sql.push("EXISTS (SELECT 1 FROM board_teams bt JOIN team_members tm ON tm.team_id = bt.team_id AND tm.org_id = bt.org_id AND tm.workspace_id = bt.workspace_id JOIN workspaces tw ON tw.id = bt.workspace_id AND tw.org_id = bt.org_id JOIN workspace_members tmw ON tmw.workspace_id = tm.workspace_id AND tmw.user_id = tm.user_id WHERE bt.board_id = b.id AND bt.workspace_id = b.workspace_id AND tw.delete_at = 0 AND tmw.role IN ('admin', 'member') AND tm.user_id = ")
        .bind(Id(user_id)).push(&scope.same_organization(&[("tmw", "bt"), ("bt", "b")])).push(")");
}

/// Directory membership rows. Inherited rows only need a presence flag, not their rank.
pub(crate) fn directory_members(scope: &Scope<'_>) -> String {
    let direct_wall = scope.same_organization(&[("dm", "db"), ("db", "dw")]);
    let team_wall = scope.same_organization(&[("gw", "tm")]);
    format!(
        "(SELECT dm.board_id, dm.user_id, dm.scheme_admin, dm.scheme_editor, dm.scheme_commenter, dm.scheme_viewer, dw.org_id FROM board_members dm JOIN boards db ON db.id = dm.board_id JOIN workspaces dw ON dw.id = db.workspace_id WHERE true{direct_wall} UNION ALL SELECT bt.board_id, tm.user_id, false, true, false, false, bt.org_id FROM board_teams bt JOIN team_members tm ON tm.team_id = bt.team_id AND tm.org_id = bt.org_id AND tm.workspace_id = bt.workspace_id JOIN workspace_members gw ON gw.workspace_id = tm.workspace_id AND gw.user_id = tm.user_id WHERE gw.role IN ('admin', 'member'){team_wall})"
    )
}

pub(crate) async fn get_members_for_user(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<Vec<BoardMember>, StoreError> {
    let mut sql = boards::select_members(scope);
    sql.push(" JOIN boards AS b ON b.id = bm.board_id WHERE ");
    crate::members::push_own_membership_in_workspace(&mut sql, user_id)
        .push(" ORDER BY bm.board_id");
    let mut members = boards::fetch_members(scope, sql).await?;
    extend_for_user(scope, user_id, &mut members).await?;
    Ok(members)
}
