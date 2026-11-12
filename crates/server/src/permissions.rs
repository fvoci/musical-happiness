//! Who may do what on a board, as a function of facts and nothing else.
//!
//! What a user may do on a board follows from three facts: what the user is in the
//! board's workspace and organization ([`Actor`]), the board's scope, and the user's
//! role on the board ([`BoardFacts`]). [`grant`] turns them into a [`Grant`]. Nothing
//! here reads the database: `App::actor` (what `Store::resolve_workspace` answers) and `App::board_access`
//! are where the facts are read, and every check of a route goes through them.
//!
//! The table of `permissions/table.rs` pins every rule of this file, one row per
//! sentence of what the product promises.

use fvoci_model::{Board, BoardMember, BoardRole, BoardType, OrgRole, WorkspaceRole};
use fvoci_store::Reach;

#[cfg(test)]
mod table;

/// What a user is where a board lives: in the board's organization and in its
/// workspace. A user who is no member of the workspace has no `workspace` role, and for
/// that user the workspace and everything in it is not there, whatever else is true.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Actor {
    /// What the user is in the workspace's organization; `None` for somebody who is no
    /// member of it. It gives nothing on a board: an organization's administrators
    /// manage the organization, not what its workspaces hold.
    pub org: Option<OrgRole>,
    /// What the user is in the board's workspace; `None` for somebody who is no member
    /// of it, among them who was one and was removed.
    pub workspace: Option<WorkspaceRole>,
}

impl Actor {
    /// Whether the user works in the workspace: a member of it who is no guest. A guest
    /// is on the boards they were invited to, and sees nothing else of the workspace.
    #[must_use]
    pub fn works_there(self) -> bool {
        matches!(
            self.workspace,
            Some(WorkspaceRole::Admin | WorkspaceRole::Member)
        )
    }
}

/// Who a board is there for, besides its members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardScope {
    /// An open board: whoever works in the workspace sees it and may join it.
    Open,
    /// A board for the invited only: nobody but its members knows it is there.
    InvitedOnly,
    /// An open template of the workspace: whoever works there reads it, to copy it.
    OpenTemplate,
    /// A template for the invited only, as [`BoardScope::InvitedOnly`].
    InvitedOnlyTemplate,
    /// A template the server ships with: every user's to read and copy.
    BuiltIn,
}

impl BoardScope {
    /// The scope of a board as it is stored.
    #[must_use]
    pub fn of(board: &Board) -> Self {
        match (board.is_builtin, board.is_template, board.board_type) {
            (true, _, _) => Self::BuiltIn,
            (false, false, BoardType::Open) => Self::Open,
            (false, false, BoardType::Private) => Self::InvitedOnly,
            (false, true, BoardType::Open) => Self::OpenTemplate,
            (false, true, BoardType::Private) => Self::InvitedOnlyTemplate,
        }
    }

    /// The scope of the board once it is in the trash: open to nobody but who was its
    /// member. The trash is no list of the workspace's boards.
    #[must_use]
    pub fn in_the_trash(self) -> Self {
        match self {
            Self::Open => Self::InvitedOnly,
            Self::OpenTemplate => Self::InvitedOnlyTemplate,
            other => other,
        }
    }

    /// Whether whoever works in the workspace sees the board without being its member.
    fn open_to_the_workspace(self) -> bool {
        matches!(self, Self::Open | Self::OpenTemplate)
    }

    /// Whether the board is a template, which is read to be copied and not joined.
    fn is_template(self) -> bool {
        matches!(
            self,
            Self::OpenTemplate | Self::InvitedOnlyTemplate | Self::BuiltIn
        )
    }
}

/// What is known of a board for one user: its scope and the user's role on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardFacts {
    /// Who the board is there for besides its members.
    pub scope: BoardScope,
    /// The user's role on the board, as stored; `None` for who is no member.
    pub role: Option<BoardRole>,
}

/// Something a user can be allowed to do on a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoardPermission {
    /// Know that the board is there: read the board itself, find it in a list. Who may
    /// not is told that the board does not exist; who may and is refused something
    /// else is told it is refused.
    SeeBoard,
    /// Become a member of an open board with its minimum role.
    JoinBoard,
    /// Read what is on the board: its cards, blocks, files and comments, its members,
    /// its archive. Copying a board takes this too.
    ViewBoard,
    /// Comment on cards.
    CommentBoardCards,
    /// Create, change and delete cards and their content.
    ManageBoardCards,
    /// Change the board's title, properties and views.
    ManageBoardProperties,
    /// Change the board between open and private, and its minimum role.
    ManageBoardType,
    /// Add and remove members and change their roles.
    ManageBoardRoles,
    /// Delete the board, and bring it back from the trash.
    DeleteBoard,
    /// Turn public sharing on and off.
    ShareBoard,
    /// Change and delete comments written by others.
    ManageOthersComments,
}

impl BoardPermission {
    /// Every permission, in the order of the declaration.
    pub const ALL: [Self; 11] = [
        Self::SeeBoard,
        Self::JoinBoard,
        Self::ViewBoard,
        Self::CommentBoardCards,
        Self::ManageBoardCards,
        Self::ManageBoardProperties,
        Self::ManageBoardType,
        Self::ManageBoardRoles,
        Self::DeleteBoard,
        Self::ShareBoard,
        Self::ManageOthersComments,
    ];

    /// The bit of the permission in a [`Grant`].
    const fn bit(self) -> u16 {
        let place = match self {
            Self::SeeBoard => 0,
            Self::JoinBoard => 1,
            Self::ViewBoard => 2,
            Self::CommentBoardCards => 3,
            Self::ManageBoardCards => 4,
            Self::ManageBoardProperties => 5,
            Self::ManageBoardType => 6,
            Self::ManageBoardRoles => 7,
            Self::DeleteBoard => 8,
            Self::ShareBoard => 9,
            Self::ManageOthersComments => 10,
        };
        1 << place
    }
}

/// What one user may do on one board.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[must_use]
pub struct Grant(u16);

impl Grant {
    /// Nothing at all: the board is not there for the user.
    pub const NONE: Self = Self(0);

    /// The grant of exactly these permissions.
    const fn of(permissions: &[BoardPermission]) -> Self {
        let mut bits = 0;
        let mut index = 0;
        while index < permissions.len() {
            bits |= permissions[index].bit();
            index += 1;
        }
        Self(bits)
    }

    /// This grant with those of `other` added.
    const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether the grant has the permission.
    #[must_use]
    pub const fn allows(self, permission: BoardPermission) -> bool {
        self.0 & permission.bit() != 0
    }

    /// Whether the user knows the board is there: refused something, they are told so
    /// (403); otherwise the board does not exist for them (404).
    #[must_use]
    pub const fn sees(self) -> bool {
        self.allows(BoardPermission::SeeBoard)
    }

    /// Whether the user reads what is on the board: refused something reached through
    /// a card or a block, they are told so (403); otherwise that card or block does not
    /// exist for them (404).
    #[must_use]
    pub const fn reads(self) -> bool {
        self.allows(BoardPermission::ViewBoard)
    }
}

/// What a role on a board lets its holder do, each role all that the one below it may.
const fn role_grant(role: BoardRole) -> Grant {
    use BoardPermission::{
        CommentBoardCards, DeleteBoard, ManageBoardCards, ManageBoardProperties, ManageBoardRoles,
        ManageBoardType, ManageOthersComments, SeeBoard, ShareBoard, ViewBoard,
    };
    let viewer = Grant::of(&[SeeBoard, ViewBoard]);
    let commenter = viewer.with(Grant::of(&[CommentBoardCards]));
    let editor = commenter.with(Grant::of(&[ManageBoardCards, ManageBoardProperties]));
    match role {
        BoardRole::Viewer => viewer,
        BoardRole::Commenter => commenter,
        BoardRole::Editor => editor,
        BoardRole::Admin => editor.with(Grant::of(&[
            ManageBoardType,
            ManageBoardRoles,
            DeleteBoard,
            ShareBoard,
            ManageOthersComments,
        ])),
    }
}

/// The highest role a member of a workspace may have on its boards: a guest may edit,
/// and no more.
#[must_use]
pub const fn highest_board_role(workspace_role: WorkspaceRole) -> BoardRole {
    match workspace_role {
        WorkspaceRole::Admin | WorkspaceRole::Member => BoardRole::Admin,
        WorkspaceRole::Guest => BoardRole::Editor,
    }
}

/// Whether a member of a workspace in `workspace_role` may be given `role` on one of
/// its boards: a guest no role above editor.
#[must_use]
pub fn may_hold(workspace_role: WorkspaceRole, role: BoardRole) -> bool {
    role <= highest_board_role(workspace_role)
}

/// Whom a member of a workspace, in `workspace_role`, sees of its people in lists,
/// searches and profiles: whoever works there sees its members, and a guest who is on
/// a board with them.
#[must_use]
pub const fn people_reach(workspace_role: WorkspaceRole) -> Reach {
    match workspace_role {
        WorkspaceRole::Admin | WorkspaceRole::Member => Reach::Workspace,
        WorkspaceRole::Guest => Reach::SharedBoards,
    }
}

/// What the user may do on the board.
///
/// - A user who is in no role in the board's workspace may do nothing: a role left on
///   a board after leaving the workspace opens nothing. A template the server ships
///   with is the exception, there for every user to read and copy.
/// - A member of the board may do what the role lets them, a guest of the workspace no
///   more than an editor.
/// - Whoever works in the workspace sees its open boards and may join those that are
///   no template, and reads its open templates.
/// - What the user is in the organization adds nothing.
pub fn grant(actor: Actor, facts: BoardFacts) -> Grant {
    use BoardPermission::{JoinBoard, SeeBoard, ViewBoard};
    if facts.scope == BoardScope::BuiltIn {
        return Grant::of(&[SeeBoard, ViewBoard]);
    }
    let Some(workspace_role) = actor.workspace else {
        return Grant::NONE;
    };
    let as_member = facts.role.map_or(Grant::NONE, |role| {
        role_grant(role.min(highest_board_role(workspace_role)))
    });
    if !actor.works_there() || !facts.scope.open_to_the_workspace() {
        return as_member;
    }
    let as_worker = if facts.scope.is_template() {
        Grant::of(&[SeeBoard, ViewBoard])
    } else {
        Grant::of(&[SeeBoard, JoinBoard])
    };
    as_member.with(as_worker)
}

/// The membership with exactly the flag of `role`, and none for `None`.
#[must_use]
pub fn with_role(member: BoardMember, role: Option<BoardRole>) -> BoardMember {
    BoardMember {
        scheme_admin: role == Some(BoardRole::Admin),
        scheme_editor: role == Some(BoardRole::Editor),
        scheme_commenter: role == Some(BoardRole::Commenter),
        scheme_viewer: role == Some(BoardRole::Viewer),
        ..member
    }
}

/// The role a membership has: the highest of its flags, `None` if it has none.
#[must_use]
pub fn role_of(member: &BoardMember) -> Option<BoardRole> {
    [
        (member.scheme_admin, BoardRole::Admin),
        (member.scheme_editor, BoardRole::Editor),
        (member.scheme_commenter, BoardRole::Commenter),
        (member.scheme_viewer, BoardRole::Viewer),
    ]
    .into_iter()
    .find_map(|(held, role)| held.then_some(role))
}
