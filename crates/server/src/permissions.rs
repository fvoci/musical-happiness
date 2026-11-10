//! Who may do what. A standalone server has no system roles: every signed-in user may
//! use the one team, and what a user may do on a board follows from their membership.

use fvoci_model::BoardMember;

/// Something a user can be allowed to do in a team.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamPermission {
    /// See the team and its open boards.
    ViewTeam,
    /// Administer the team. Nobody has this on a standalone server.
    ManageTeam,
    /// Create open boards.
    CreatePublicChannel,
    /// Create private boards.
    CreatePrivateChannel,
}

/// Something a user can be allowed to do on a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardPermission {
    /// Change the board between open and private, and its minimum role.
    ManageBoardType,
    /// Delete and undelete the board.
    DeleteBoard,
    /// See the board and its content.
    ViewBoard,
    /// Add and remove members and change their roles.
    ManageBoardRoles,
    /// Turn public sharing on and off.
    ShareBoard,
    /// Create, change and delete cards and their content.
    ManageBoardCards,
    /// Change the board's title, properties and views.
    ManageBoardProperties,
    /// Comment on cards.
    CommentBoardCards,
    /// Delete comments written by others.
    DeleteOthersComments,
}

/// Returns whether a user has a permission in a team.
///
/// Every user has every team permission except [`TeamPermission::ManageTeam`]. An empty
/// user or team identifier has none.
pub fn has_permission_to_team(user_id: &str, team_id: &str, permission: TeamPermission) -> bool {
    !user_id.is_empty() && !team_id.is_empty() && permission != TeamPermission::ManageTeam
}

/// Returns whether a board membership grants a permission.
///
/// What counts is the role the membership has. The board's minimum role is what a user
/// gets who joins or is added without a say about roles; it is not added on top of the
/// role an administrator gave a member. Every board has a minimum role, so counting it
/// would leave no board with a member below it: nobody could be given less.
pub fn member_has_permission(member: &BoardMember, permission: BoardPermission) -> bool {
    let admin = member.scheme_admin;
    let editor = member.scheme_editor;
    let commenter = member.scheme_commenter;
    let viewer = member.scheme_viewer;
    match permission {
        BoardPermission::ManageBoardType
        | BoardPermission::DeleteBoard
        | BoardPermission::ManageBoardRoles
        | BoardPermission::ShareBoard
        | BoardPermission::DeleteOthersComments => admin,
        BoardPermission::ManageBoardCards | BoardPermission::ManageBoardProperties => {
            admin || editor
        }
        BoardPermission::CommentBoardCards => admin || editor || commenter,
        BoardPermission::ViewBoard => admin || editor || commenter || viewer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BoardPermission::{
        DeleteBoard, ManageBoardCards, ManageBoardProperties, ManageBoardRoles, ManageBoardType,
        ShareBoard, ViewBoard,
    };

    /// `TestHasPermissionToTeam` in
    /// `server/services/permissions/localpermissions/localpermissions_test.go`.
    #[test]
    fn has_permission_to_team_cases() {
        // "empty input should always unauthorize" (the nil-permission case has no
        // counterpart: a permission is always given)
        assert!(
            !has_permission_to_team("", "team-id", TeamPermission::ViewTeam),
            "no user"
        );
        assert!(
            !has_permission_to_team("user-id", "", TeamPermission::ViewTeam),
            "no team"
        );

        // "all users have all permissions on teams"
        for permission in [
            TeamPermission::ViewTeam,
            TeamPermission::CreatePublicChannel,
            TeamPermission::CreatePrivateChannel,
        ] {
            assert!(
                has_permission_to_team("user-id", "team-id", permission),
                "{permission:?}"
            );
        }

        // "no users have PermissionManageTeam on teams"
        assert!(
            !has_permission_to_team("user-id", "team-id", TeamPermission::ManageTeam),
            "nobody manages the team"
        );
    }

    fn check(
        role: &str,
        member: &BoardMember,
        granted: &[BoardPermission],
        denied: &[BoardPermission],
    ) {
        for permission in granted {
            assert!(
                member_has_permission(member, *permission),
                "{role} should have {permission:?}"
            );
        }
        for permission in denied {
            assert!(
                !member_has_permission(member, *permission),
                "{role} should not have {permission:?}"
            );
        }
    }

    /// The role cases of `TestHasPermissionToBoard` in the same file. Its "empty input"
    /// and "nonexistent user" cases are covered through the API tests, where the
    /// membership is looked up.
    #[test]
    fn has_permission_to_board_by_role() {
        // "board admin"
        let admin = BoardMember {
            scheme_admin: true,
            ..BoardMember::default()
        };
        check(
            "admin",
            &admin,
            &[
                ManageBoardType,
                DeleteBoard,
                ManageBoardRoles,
                ShareBoard,
                ManageBoardCards,
                ViewBoard,
                ManageBoardProperties,
            ],
            &[],
        );

        // "board editor"
        let editor = BoardMember {
            scheme_editor: true,
            ..BoardMember::default()
        };
        check(
            "editor",
            &editor,
            &[ManageBoardCards, ViewBoard, ManageBoardProperties],
            &[ManageBoardType, DeleteBoard, ManageBoardRoles, ShareBoard],
        );

        // "board commenter"
        let commenter = BoardMember {
            scheme_commenter: true,
            ..BoardMember::default()
        };
        let not_for_readers = [
            ManageBoardType,
            DeleteBoard,
            ManageBoardRoles,
            ShareBoard,
            ManageBoardCards,
            ManageBoardProperties,
        ];
        check("commenter", &commenter, &[ViewBoard], &not_for_readers);

        // "board viewer" and "Manage Team Permission " (the same case twice in the
        // original)
        let viewer = BoardMember {
            scheme_viewer: true,
            ..BoardMember::default()
        };
        check("viewer", &viewer, &[ViewBoard], &not_for_readers);
    }

    #[test]
    fn comment_permissions_and_the_minimum_role() {
        let commenter = BoardMember {
            scheme_commenter: true,
            ..BoardMember::default()
        };
        assert!(
            member_has_permission(&commenter, BoardPermission::CommentBoardCards),
            "a commenter comments"
        );
        assert!(
            !member_has_permission(&commenter, BoardPermission::DeleteOthersComments),
            "only an admin deletes others' comments"
        );

        // A viewer is a viewer whatever the minimum role of the board: the minimum role
        // is what joining gives, not something added to a role that was given. A
        // membership does not even say what its board's minimum role is.
        let below_the_minimum = BoardMember {
            scheme_viewer: true,
            ..BoardMember::default()
        };
        check(
            "viewer, on any board",
            &below_the_minimum,
            &[ViewBoard],
            &[
                ManageBoardCards,
                ManageBoardProperties,
                ManageBoardType,
                DeleteBoard,
            ],
        );

        let no_role = BoardMember::default();
        assert!(
            !member_has_permission(&no_role, ViewBoard),
            "a member without a role sees nothing"
        );
    }
}
