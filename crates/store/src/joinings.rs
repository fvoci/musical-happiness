//! The roles on the boards of a workspace that a user held when they joined it as the
//! organization's administrator: written when they join, and given back when the
//! joining ends, whatever they did with their roles meanwhile. Who joined a workspace
//! was its administrator for that time only; what they made themselves on its boards
//! (a member of a board they could not read, an admin of one they read, a member of an
//! open board, the maker of a new one) goes with the joining.

use fvoci_model::BoardMember;
use serde::{Deserialize, Serialize};

use crate::db::{Id, Param, Scope};
use crate::error::StoreError;
use crate::trash;

/// One role on a board as it was held: the board, and the flags of the membership
/// (admin, editor, commenter, viewer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HeldRole(String, [bool; 4]);

/// The user's memberships of the boards of the workspace, those in the trash among them,
/// as the text the joining keeps (`workspace_members.prior_board_roles`), by board.
pub(crate) async fn held_roles(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<String, StoreError> {
    let mut sql = scope.sql(
        "SELECT bm.board_id, bm.scheme_admin, bm.scheme_editor, bm.scheme_commenter, \
         bm.scheme_viewer FROM board_members AS bm JOIN boards AS b ON b.id = bm.board_id \
         WHERE b.workspace_id = ",
    );
    sql.bind(Id(workspace_id))
        .push_bind(" AND bm.user_id = ", Id(user_id))
        .push(" ORDER BY bm.board_id");
    let mut held: Vec<HeldRole> = scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| {
            Ok(HeldRole(
                row.id(0)?,
                [row.bool(1)?, row.bool(2)?, row.bool(3)?, row.bool(4)?],
            ))
        })
        .collect::<Result<_, StoreError>>()?;
    for (board_id, members) in trashed_members(scope, workspace_id).await? {
        if let Some(member) = members.iter().find(|member| member.user_id == user_id) {
            held.push(HeldRole(board_id, flags_of(member)));
        }
    }
    serde_json::to_string(&held).map_err(|error| StoreError::Corrupt {
        column: "prior_board_roles",
        reason: error.to_string(),
    })
}

/// Gives the user back the memberships of the boards of the workspace they held when
/// they joined it (`held`, as [`held_roles`] wrote it): every other one of theirs there
/// goes, and one of a board that is gone meanwhile is not made again.
pub(crate) async fn give_back_roles(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
    held: &str,
) -> Result<(), StoreError> {
    let held: Vec<HeldRole> = if held.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(held).map_err(|error| StoreError::Corrupt {
            column: "prior_board_roles",
            reason: error.to_string(),
        })?
    };
    let mut sql = scope.sql("DELETE FROM board_members WHERE user_id = ");
    sql.bind(Id(user_id))
        .push(" AND board_id IN (SELECT id FROM boards WHERE workspace_id = ")
        .bind(Id(workspace_id))
        .push(")");
    scope.execute(sql).await?;
    give_back_roles_in_the_trash(scope, workspace_id, user_id, &held).await?;
    for HeldRole(board_id, [admin, editor, commenter, viewer]) in held {
        let mut sql = scope.sql(
            "INSERT INTO board_members (board_id, user_id, scheme_admin, scheme_editor, \
             scheme_commenter, scheme_viewer) SELECT id, ",
        );
        sql.bind(Id(user_id))
            .push_bind(", ", admin)
            .push_bind(", ", editor)
            .push_bind(", ", commenter)
            .push_bind(", ", viewer)
            .push_bind(" FROM boards WHERE id = ", Id(&board_id))
            .push_bind(" AND workspace_id = ", Id(workspace_id));
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Gives the user back, on each board of the workspace in the trash, the membership
/// they held when they joined, and no other: a board deleted while they were joined
/// comes back without what they made themselves on it.
async fn give_back_roles_in_the_trash(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
    held: &[HeldRole],
) -> Result<(), StoreError> {
    for (board_id, mut members) in trashed_members(scope, workspace_id).await? {
        let theirs = members.iter().find(|member| member.user_id == user_id);
        let given_back = held.iter().find(|role| role.0 == board_id);
        if theirs.map(flags_of) == given_back.map(|role| role.1) {
            continue;
        }
        members.retain(|member| member.user_id != user_id);
        if let Some(HeldRole(_, [admin, editor, commenter, viewer])) = given_back {
            members.push(BoardMember {
                board_id: board_id.clone(),
                user_id: user_id.to_owned(),
                scheme_admin: *admin,
                scheme_editor: *editor,
                scheme_commenter: *commenter,
                scheme_viewer: *viewer,
                ..BoardMember::default()
            });
        }
        let mut sql = scope.sql("UPDATE deleted_boards SET members = ");
        sql.bind(Param::json(&members)?)
            .push_bind(" WHERE id = ", Id(&board_id));
        scope.execute(sql).await?;
    }
    Ok(())
}

/// The memberships kept with each board of the workspace in the trash, by board.
async fn trashed_members(
    scope: &mut Scope<'_>,
    workspace_id: &str,
) -> Result<Vec<(String, Vec<BoardMember>)>, StoreError> {
    let mut sql = scope.sql("SELECT id, members FROM deleted_boards WHERE workspace_id = ");
    sql.bind(Id(workspace_id)).push(" ORDER BY id");
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(|row| Ok((row.id(0)?, trash::from_json(row, 1, "members")?)))
        .collect()
}

/// The flags of a membership as a held role keeps them.
const fn flags_of(member: &BoardMember) -> [bool; 4] {
    [
        member.scheme_admin,
        member.scheme_editor,
        member.scheme_commenter,
        member.scheme_viewer,
    ]
}
