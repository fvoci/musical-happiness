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
use fvoci_store::{Looking, Reach};

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

    /// Whether the user administers the organization: what the routes of an
    /// organization (`/org/{orgId}`) take. It opens nothing of its workspaces' boards.
    #[must_use]
    pub fn administers_the_organization(self) -> bool {
        matches!(self.org, Some(OrgRole::Admin))
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
    /// Read what is on the board: its cards, blocks, files and comments, its archive.
    /// Copying a board takes this too.
    ViewBoard,
    /// List the board's members.
    ListMembers,
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
    pub const ALL: [Self; 12] = [
        Self::SeeBoard,
        Self::JoinBoard,
        Self::ViewBoard,
        Self::ListMembers,
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
            Self::ListMembers => 11,
        };
        1 << place
    }

    /// Whether the permission is over what is on the board: its cards, comments and
    /// files, and the link that shows them. Refused it, who only manages the board is
    /// told that it is not there ([`Grant::hides`]).
    #[must_use]
    pub const fn is_content(self) -> bool {
        matches!(
            self,
            Self::ViewBoard
                | Self::CommentBoardCards
                | Self::ManageBoardCards
                | Self::ManageOthersComments
                | Self::ShareBoard
        )
    }
}

/// The bit of a [`Grant`] that says the user sees the board only as one who manages it:
/// an administrator of the workspace on a board for the invited only that they are no
/// reader of. No permission has it.
const MANAGES_ONLY: u16 = 1 << 15;

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

    /// Whether a refusal of `permission` tells that the board is not there (404) rather
    /// than that it is refused (403): for who does not see the board, and for who only
    /// manages it and asks for its content.
    #[must_use]
    pub const fn hides(self, permission: BoardPermission) -> bool {
        !self.sees() || (self.manages_only() && permission.is_content())
    }

    /// Whether the user sees the board only to manage it: an administrator of the
    /// workspace who reads none of it. What is on it, and what the board says of itself
    /// beyond its name and kind, is not for them.
    #[must_use]
    pub const fn manages_only(self) -> bool {
        self.0 & MANAGES_ONLY != 0
    }

    /// Whether the user reads what is on the board: refused something reached through
    /// a card or a block, they are told so (403); otherwise that card or block does not
    /// exist for them (404).
    #[must_use]
    pub const fn reads(self) -> bool {
        self.allows(BoardPermission::ViewBoard)
    }
}

/// What a user does to a comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentAction<'a> {
    /// Change it.
    Change,
    /// Delete it.
    Delete,
    /// Bring it back from the trash, where it went by the deletion of `deleted_by`
    /// (empty if nobody's).
    Undelete {
        /// Who deleted it.
        deleted_by: &'a str,
    },
}

/// The permission doing `action` to a comment of `author_id` takes of `user_id`: a
/// comment is its author's to change and delete, and to bring back if they deleted it
/// themselves ([`BoardPermission::CommentBoardCards`]); a comment of somebody else, and
/// one of their own that somebody else deleted, is the board's admins'
/// ([`BoardPermission::ManageOthersComments`]).
#[must_use]
pub fn comment_permission(
    user_id: &str,
    author_id: &str,
    action: CommentAction<'_>,
) -> BoardPermission {
    let undoes_their_own = match action {
        CommentAction::Change | CommentAction::Delete => true,
        CommentAction::Undelete { deleted_by } => deleted_by == user_id,
    };
    if author_id == user_id && undoes_their_own {
        BoardPermission::CommentBoardCards
    } else {
        BoardPermission::ManageOthersComments
    }
}

/// What is known of one user (the person) for another who looks at them in a
/// workspace the seer is a member of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Person {
    /// The seer.
    Themselves,
    /// A member of the workspace now.
    Member {
        /// The two hold roles on a board of the workspace.
        shares_a_board: bool,
        /// The person created or last changed a card, block or comment of a board the
        /// seer reads.
        wrote_where_seer_reads: bool,
    },
    /// No member of the workspace: who left it, with what they wrote and any role
    /// left on its boards, or never was in it.
    Outside {
        /// As for a member.
        wrote_where_seer_reads: bool,
    },
}

/// Whether the seer, `seer` in the workspace, sees the person, `looking` for what:
/// whoever works there sees its members, a guest the members who share a board with it.
/// Who left the workspace is seen by name only, and only where they wrote on a board
/// the seer reads, so that what they wrote keeps its author; a role left on a board
/// shows nobody. The store's queries of people are this rule in SQL
/// (`fvoci_store::Sight`).
#[must_use]
pub const fn sees_person(seer: WorkspaceRole, person: Person, looking: Looking) -> bool {
    let by_name = matches!(looking, Looking::ByName);
    match person {
        Person::Themselves => true,
        Person::Member {
            shares_a_board,
            wrote_where_seer_reads,
        } => {
            !matches!(seer, WorkspaceRole::Guest)
                || shares_a_board
                || (by_name && wrote_where_seer_reads)
        }
        Person::Outside {
            wrote_where_seer_reads,
        } => by_name && wrote_where_seer_reads,
    }
}

/// What a role on a board lets its holder do, each role all that the one below it may.
const fn role_grant(role: BoardRole) -> Grant {
    use BoardPermission::{
        CommentBoardCards, DeleteBoard, ListMembers, ManageBoardCards, ManageBoardProperties,
        ManageBoardRoles, ManageBoardType, ManageOthersComments, SeeBoard, ShareBoard, ViewBoard,
    };
    let viewer = Grant::of(&[SeeBoard, ViewBoard, ListMembers]);
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
/// - An administrator of the workspace sees its boards, lists and changes their
///   members, and deletes and restores them; what is on a board for the invited only
///   they read as its member, and are told it is not there otherwise
///   ([`Grant::hides`]).
/// - What the user is in the organization adds nothing.
pub fn grant(actor: Actor, facts: BoardFacts) -> Grant {
    use BoardPermission::{
        DeleteBoard, JoinBoard, ListMembers, ManageBoardRoles, SeeBoard, ViewBoard,
    };
    if facts.scope == BoardScope::BuiltIn {
        return Grant::of(&[SeeBoard, ViewBoard, ListMembers]);
    }
    let Some(workspace_role) = actor.workspace else {
        return Grant::NONE;
    };
    let as_member = facts.role.map_or(Grant::NONE, |role| {
        role_grant(role.min(highest_board_role(workspace_role)))
    });
    let as_worker = match (actor.works_there(), facts.scope) {
        (true, BoardScope::Open) => Grant::of(&[SeeBoard, JoinBoard]),
        (true, BoardScope::OpenTemplate) => Grant::of(&[SeeBoard, ViewBoard, ListMembers]),
        _ => Grant::NONE,
    };
    let as_manager = if workspace_role == WorkspaceRole::Admin {
        Grant::of(&[SeeBoard, ListMembers, ManageBoardRoles, DeleteBoard])
    } else {
        Grant::NONE
    };
    let granted = as_member.with(as_worker).with(as_manager);
    if as_manager != Grant::NONE && !granted.reads() && !facts.scope.open_to_the_workspace() {
        Grant(granted.0 | MANAGES_ONLY)
    } else {
        granted
    }
}

/// Whether a user who is `role` in a workspace (`None`: no member) reads its audit log:
/// whoever works there, an administrator of the organization who joined it among them.
/// A guest does not: what the log names is the workspace's, not a board's.
#[must_use]
pub const fn reads_the_audit_log(role: Option<WorkspaceRole>) -> bool {
    matches!(role, Some(WorkspaceRole::Admin | WorkspaceRole::Member))
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
