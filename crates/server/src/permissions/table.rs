//! The table of who may do what: roles × scopes × permissions. Each row is one case of
//! what the product promises, and the whole grant of the case is pinned: a permission a
//! row does not name is one the user does not have.

use fvoci_model::{BoardRole, OrgRole, WorkspaceRole};

use super::BoardPermission::{
    CommentBoardCards, DeleteBoard, JoinBoard, ManageBoardCards, ManageBoardProperties,
    ManageBoardRoles, ManageBoardType, ManageOthersComments, SeeBoard, ShareBoard, ViewBoard,
};
use super::BoardScope::{BuiltIn, InvitedOnly, InvitedOnlyTemplate, Open, OpenTemplate};
use super::{Actor, BoardFacts, BoardPermission, BoardScope, Grant, grant, highest_board_role};

const VIEWER: &[BoardPermission] = &[SeeBoard, ViewBoard];
const COMMENTER: &[BoardPermission] = &[SeeBoard, ViewBoard, CommentBoardCards];
const EDITOR: &[BoardPermission] = &[
    SeeBoard,
    ViewBoard,
    CommentBoardCards,
    ManageBoardCards,
    ManageBoardProperties,
];
const ADMIN: &[BoardPermission] = &[
    SeeBoard,
    ViewBoard,
    CommentBoardCards,
    ManageBoardCards,
    ManageBoardProperties,
    ManageBoardType,
    ManageBoardRoles,
    DeleteBoard,
    ShareBoard,
    ManageOthersComments,
];
const NOTHING: &[BoardPermission] = &[];

const fn actor(org: Option<OrgRole>, workspace: Option<WorkspaceRole>) -> Actor {
    Actor { org, workspace }
}

const MEMBER: Actor = actor(Some(OrgRole::Member), Some(WorkspaceRole::Member));
const WORKSPACE_ADMIN: Actor = actor(Some(OrgRole::Member), Some(WorkspaceRole::Admin));
const GUEST: Actor = actor(Some(OrgRole::Member), Some(WorkspaceRole::Guest));
const OUTSIDE_GUEST: Actor = actor(None, Some(WorkspaceRole::Guest));
const ORG_ADMIN_ONLY: Actor = actor(Some(OrgRole::Admin), None);
const ORG_ADMIN_IN_WORKSPACE: Actor = actor(Some(OrgRole::Admin), Some(WorkspaceRole::Member));
const REMOVED: Actor = actor(Some(OrgRole::Member), None);
const STRANGER: Actor = actor(None, None);

/// One case: who, on what board, with which role there, may do exactly this.
struct Row {
    case: &'static str,
    actor: Actor,
    scope: BoardScope,
    role: Option<BoardRole>,
    allowed: &'static [BoardPermission],
}

const fn row(
    case: &'static str,
    actor: Actor,
    scope: BoardScope,
    role: Option<BoardRole>,
    allowed: &'static [BoardPermission],
) -> Row {
    Row {
        case,
        actor,
        scope,
        role,
        allowed,
    }
}

const ROWS: &[Row] = &[
    // The roles of a board, for a member of its workspace.
    row(
        "a viewer views",
        MEMBER,
        InvitedOnly,
        Some(BoardRole::Viewer),
        VIEWER,
    ),
    row(
        "a commenter comments",
        MEMBER,
        InvitedOnly,
        Some(BoardRole::Commenter),
        COMMENTER,
    ),
    row(
        "an editor edits",
        MEMBER,
        InvitedOnly,
        Some(BoardRole::Editor),
        EDITOR,
    ),
    row(
        "an admin manages",
        MEMBER,
        InvitedOnly,
        Some(BoardRole::Admin),
        ADMIN,
    ),
    row(
        "a role is the same on an open board",
        MEMBER,
        Open,
        Some(BoardRole::Viewer),
        &[SeeBoard, ViewBoard, JoinBoard],
    ),
    // A workspace is unknown to who is no member of it.
    row(
        "a stranger: no invited-only board",
        STRANGER,
        InvitedOnly,
        None,
        NOTHING,
    ),
    row("a stranger: no open board", STRANGER, Open, None, NOTHING),
    row(
        "a stranger: no open template",
        STRANGER,
        OpenTemplate,
        None,
        NOTHING,
    ),
    row(
        "a stranger with a stored role: nothing",
        STRANGER,
        InvitedOnly,
        Some(BoardRole::Admin),
        NOTHING,
    ),
    // Whoever works in the workspace sees its open boards, and joins them to read.
    row(
        "a member sees and joins an open board",
        MEMBER,
        Open,
        None,
        &[SeeBoard, JoinBoard],
    ),
    row(
        "a member: no invited-only board",
        MEMBER,
        InvitedOnly,
        None,
        NOTHING,
    ),
    row(
        "a member reads an open template",
        MEMBER,
        OpenTemplate,
        None,
        VIEWER,
    ),
    row(
        "a member: no invited-only template",
        MEMBER,
        InvitedOnlyTemplate,
        None,
        NOTHING,
    ),
    row(
        "an editor of an open template edits it",
        MEMBER,
        OpenTemplate,
        Some(BoardRole::Editor),
        EDITOR,
    ),
    // A template the server ships with is every user's to read and copy.
    row(
        "a member reads a built-in template",
        MEMBER,
        BuiltIn,
        None,
        VIEWER,
    ),
    row(
        "a guest reads a built-in template",
        GUEST,
        BuiltIn,
        None,
        VIEWER,
    ),
    row(
        "a stranger reads a built-in template",
        STRANGER,
        BuiltIn,
        None,
        VIEWER,
    ),
    // A guest sees the boards they were invited to, one by one, as no more than an editor.
    row("a guest: no open board", GUEST, Open, None, NOTHING),
    row(
        "a guest: no open template",
        GUEST,
        OpenTemplate,
        None,
        NOTHING,
    ),
    row(
        "a guest: no invited-only board",
        GUEST,
        InvitedOnly,
        None,
        NOTHING,
    ),
    row(
        "an invited guest views",
        GUEST,
        InvitedOnly,
        Some(BoardRole::Viewer),
        VIEWER,
    ),
    row(
        "an invited guest edits",
        GUEST,
        InvitedOnly,
        Some(BoardRole::Editor),
        EDITOR,
    ),
    row(
        "a guest on an open board is no more than invited",
        GUEST,
        Open,
        Some(BoardRole::Editor),
        EDITOR,
    ),
    row(
        "a guest stored as admin is an editor: no link, no members",
        GUEST,
        InvitedOnly,
        Some(BoardRole::Admin),
        EDITOR,
    ),
    row(
        "a guest from outside the organization alike",
        OUTSIDE_GUEST,
        InvitedOnly,
        Some(BoardRole::Admin),
        EDITOR,
    ),
    // A workspace's administrator is, on its boards, what a member is (until the
    // management of private boards is given to them).
    row(
        "a workspace admin: an open board as a member",
        WORKSPACE_ADMIN,
        Open,
        None,
        &[SeeBoard, JoinBoard],
    ),
    row(
        "a workspace admin: no invited-only board",
        WORKSPACE_ADMIN,
        InvitedOnly,
        None,
        NOTHING,
    ),
    // An organization's administrator manages the organization; a workspace they are no
    // member of is not there for them.
    row(
        "an org admin outside the workspace: nothing",
        ORG_ADMIN_ONLY,
        Open,
        None,
        NOTHING,
    ),
    row(
        "an org admin outside the workspace, stored as board admin: nothing",
        ORG_ADMIN_ONLY,
        InvitedOnly,
        Some(BoardRole::Admin),
        NOTHING,
    ),
    row(
        "an org admin in the workspace is what the workspace role is",
        ORG_ADMIN_IN_WORKSPACE,
        Open,
        None,
        &[SeeBoard, JoinBoard],
    ),
    // Who was removed from the workspace opens nothing with the roles left on its boards.
    row(
        "removed: an admin role left opens nothing",
        REMOVED,
        InvitedOnly,
        Some(BoardRole::Admin),
        NOTHING,
    ),
    row(
        "removed: an open board is gone",
        REMOVED,
        Open,
        Some(BoardRole::Editor),
        NOTHING,
    ),
    row(
        "removed: an open template is gone",
        REMOVED,
        OpenTemplate,
        None,
        NOTHING,
    ),
    // A board in the trash is open to nobody but who was its member.
    row(
        "trash: an open board is no member's",
        MEMBER,
        IN_THE_TRASH,
        None,
        NOTHING,
    ),
    row(
        "trash: its admin brings it back",
        MEMBER,
        IN_THE_TRASH,
        Some(BoardRole::Admin),
        ADMIN,
    ),
    row(
        "trash: who has left the workspace brings nothing back",
        REMOVED,
        IN_THE_TRASH,
        Some(BoardRole::Admin),
        NOTHING,
    ),
];

const IN_THE_TRASH: BoardScope = InvitedOnly;

#[test]
fn the_trash_is_open_to_nobody() {
    assert_eq!(Open.in_the_trash(), IN_THE_TRASH);
    assert_eq!(OpenTemplate.in_the_trash(), InvitedOnlyTemplate);
    assert_eq!(InvitedOnly.in_the_trash(), InvitedOnly);
}

fn exactly(permissions: &[BoardPermission]) -> Vec<BoardPermission> {
    BoardPermission::ALL
        .into_iter()
        .filter(|permission| permissions.contains(permission))
        .collect()
}

fn granted(grant: Grant) -> Vec<BoardPermission> {
    BoardPermission::ALL
        .into_iter()
        .filter(|permission| grant.allows(*permission))
        .collect()
}

#[test]
fn every_row_of_the_table_holds() {
    let mut wrong = Vec::new();
    for row in ROWS {
        let got = granted(grant(
            row.actor,
            BoardFacts {
                scope: row.scope,
                role: row.role,
            },
        ));
        let want = exactly(row.allowed);
        if got != want {
            wrong.push(format!("{}: got {got:?}, want {want:?}", row.case));
        }
    }
    assert!(
        wrong.is_empty(),
        "rows that do not hold:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn who_sees_is_who_is_refused_and_who_reads_is_who_sees() {
    // A refusal tells that the board is there: whoever may do anything on it sees it,
    // and whoever reads it sees it.
    for row in ROWS {
        let grant = grant(
            row.actor,
            BoardFacts {
                scope: row.scope,
                role: row.role,
            },
        );
        if grant != Grant::NONE {
            assert!(grant.sees(), "{}: allowed something unseen", row.case);
        }
        if grant.reads() {
            assert!(grant.sees(), "{}: reads what it does not see", row.case);
        }
    }
}

#[test]
fn a_guest_is_given_no_more_than_editor() {
    use super::may_hold;
    for role in [BoardRole::Viewer, BoardRole::Commenter, BoardRole::Editor] {
        assert!(
            may_hold(WorkspaceRole::Guest, role),
            "a guest may be {role:?}"
        );
    }
    assert!(
        !may_hold(WorkspaceRole::Guest, BoardRole::Admin),
        "no guest is a board admin"
    );
    assert!(
        may_hold(WorkspaceRole::Member, BoardRole::Admin),
        "a member may be"
    );
    assert!(
        may_hold(WorkspaceRole::Admin, BoardRole::Admin),
        "a workspace admin may be"
    );
    assert_eq!(highest_board_role(WorkspaceRole::Guest), BoardRole::Editor);
    assert_eq!(highest_board_role(WorkspaceRole::Member), BoardRole::Admin);
    assert_eq!(highest_board_role(WorkspaceRole::Admin), BoardRole::Admin);
}

#[test]
fn a_role_is_the_highest_of_its_flags() {
    use fvoci_model::BoardMember;
    let member = |admin, editor, commenter, viewer| BoardMember {
        scheme_admin: admin,
        scheme_editor: editor,
        scheme_commenter: commenter,
        scheme_viewer: viewer,
        ..BoardMember::default()
    };
    assert_eq!(super::role_of(&member(false, false, false, false)), None);
    assert_eq!(
        super::role_of(&member(false, false, false, true)),
        Some(BoardRole::Viewer)
    );
    assert_eq!(
        super::role_of(&member(false, true, false, true)),
        Some(BoardRole::Editor)
    );
    assert_eq!(
        super::role_of(&member(true, false, false, false)),
        Some(BoardRole::Admin)
    );
}
