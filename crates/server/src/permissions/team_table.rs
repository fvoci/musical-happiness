//! Team roles combine with direct roles without admitting guests or board admins.

use super::table::{GUEST, MEMBER, REMOVED, WORKSPACE_ADMIN};
use super::{BoardFacts, BoardScope, grant};
use fvoci_model::BoardRole::{Admin, Commenter, Editor, Viewer};

#[test]
fn direct_and_team_roles_take_the_highest_within_the_team_limit() {
    let rows = [
        ("only a team", MEMBER, None, Some(Editor), Some(Editor)),
        (
            "team raises viewer",
            MEMBER,
            Some(Viewer),
            Some(Editor),
            Some(Editor),
        ),
        (
            "direct editor beats commenter",
            MEMBER,
            Some(Editor),
            Some(Commenter),
            Some(Editor),
        ),
        (
            "direct admin stays admin",
            MEMBER,
            Some(Admin),
            Some(Editor),
            Some(Admin),
        ),
        (
            "team cannot grant admin",
            MEMBER,
            None,
            Some(Admin),
            Some(Editor),
        ),
        ("guest ignores team", GUEST, None, Some(Editor), None),
        (
            "guest keeps direct viewer",
            GUEST,
            Some(Viewer),
            Some(Editor),
            Some(Viewer),
        ),
        ("departed ignores team", REMOVED, None, Some(Editor), None),
        (
            "manager reads through team",
            WORKSPACE_ADMIN,
            None,
            Some(Viewer),
            Some(Viewer),
        ),
    ];
    for (case, actor, role, team_role, effective) in rows {
        for scope in [
            BoardScope::InvitedOnly,
            BoardScope::Open,
            BoardScope::InvitedOnlyTemplate,
        ] {
            assert_eq!(
                grant(
                    actor,
                    BoardFacts {
                        scope,
                        role,
                        team_role
                    }
                ),
                grant(
                    actor,
                    BoardFacts {
                        scope,
                        role: effective,
                        team_role: None
                    }
                ),
                "{case}: {scope:?}"
            );
        }
    }
}
