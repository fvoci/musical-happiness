//! Keep board assignments in trash without keeping a snapshot of a team's people.

use fvoci_model::{Board, MinimumRole};

use crate::db::{Id, Param, Scope};
use crate::{StoreError, board_team_access, trash};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRole {
    team_id: String,
    role: MinimumRole,
}

pub(crate) async fn keep(scope: &mut Scope<'_>, board_id: &str) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT team_id, role FROM board_teams WHERE board_id = ");
    sql.bind(Id(board_id))
        .push_bind(" AND org_id = ", scope.org()?)
        .push(" ORDER BY team_id");
    let roles = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(StoredRole {
                team_id: row.id(0)?,
                role: board_team_access::parse_role(&row.text(1)?)?,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let mut sql = scope.sql("UPDATE deleted_boards SET team_roles = ");
    sql.bind(Param::json(&roles)?)
        .push_bind(" WHERE id = ", Id(board_id));
    scope.execute(sql).await?;
    Ok(())
}

pub(crate) async fn restore(scope: &mut Scope<'_>, board: &Board) -> Result<(), StoreError> {
    let mut sql = scope.sql("SELECT team_roles FROM deleted_boards WHERE id = ");
    sql.bind(Id(&board.id));
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(|| StoreError::not_found("board"))?;
    let roles: Vec<StoredRole> = trash::from_json(&row, 0, "team_roles")?;
    for stored in roles {
        // The organization lock covers team deletion and membership changes too.
        // Only a surviving team in this workspace can regain the assignment.
        let mut sql = scope
            .sql("INSERT INTO board_teams (board_id, team_id, workspace_id, org_id, role) SELECT ");
        sql.bind(Id(&board.id))
            .push(", id, workspace_id, org_id, ")
            .bind(stored.role.as_str())
            .push_bind(" FROM teams WHERE id = ", Id(&stored.team_id))
            .push_bind(" AND workspace_id = ", Id(&board.workspace_id))
            .push_bind(" AND org_id = ", scope.org()?);
        scope.execute(sql).await?;
    }
    Ok(())
}
