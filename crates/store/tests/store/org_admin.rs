//! What an organization's administrators change of it: the roles of its members, its
//! workspaces and their trash, joining a workspace as its administrator, and the audit
//! log that records it and is never changed.

use fvoci_model::{
    AuditAction, Board, BoardMember, BoardType, FileInfo, OrgRole, Workspace, WorkspaceRole, new_id,
};
use fvoci_store::{
    AuditEntry, AuditPlace, InstanceRole, Joining, LooseState, OrgRoleChange, Paging, Purged,
    Store, StoreError, Taken, WorkspaceTrashing,
};

use crate::common::{self, TestStore};

/// A user who is a member of the test's organization with the role: a user made through
/// its handle joins it. An administrator of it administers the instance, as the two
/// are one for now.
async fn org_member(test: &TestStore, username: &str, role: OrgRole) -> String {
    let user_id = common::user(test, username).await;
    for statement in [
        format!(
            "UPDATE organization_members SET role = '{}' WHERE user_id = '{user_id}'",
            role.as_str()
        ),
        format!(
            "UPDATE users SET is_admin = {} WHERE id = '{user_id}'",
            role == OrgRole::Admin
        ),
    ] {
        test.execute_unchecked(&statement)
            .await
            .expect("the user has the role in the organization");
    }
    user_id
}

/// A workspace of the test's organization with the name, made by `actor_id`.
async fn named_workspace(store: &Store, name: &str, actor_id: &str) -> String {
    let workspace = Workspace {
        id: new_id(),
        title: name.to_owned(),
        signup_token: "signup-token".to_owned(),
        ..Workspace::default()
    };
    store
        .create_named_workspace(&workspace, actor_id)
        .await
        .expect("the workspace is made");
    workspace.id
}

/// A private board of the workspace whose admin is `user_id`.
async fn private_board(store: &Store, workspace_id: &str, user_id: &str) -> String {
    let mut board = Board {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    store
        .insert_board_with_admin(&mut board, user_id)
        .await
        .expect("the board is stored");
    board.id
}

fn file_of(board_id: &str, stored_name: &str) -> FileInfo {
    FileInfo {
        id: new_id(),
        board_id: board_id.to_owned(),
        stored_name: stored_name.to_owned(),
        name: format!("uploaded as {stored_name}"),
        extension: ".png".to_owned(),
        size: 2048,
        path: format!("boards/{board_id}/{stored_name}"),
        create_at: 1,
    }
}

/// What the workspace's audit log says, as `(action, actor, subject)`.
async fn log_of(store: &Store, workspace_id: &str) -> Vec<(AuditAction, String, String)> {
    store
        .get_audit_log(workspace_id)
        .await
        .expect("the log is read")
        .into_iter()
        .map(|entry| (entry.action, entry.actor_id, entry.subject_id))
        .collect()
}

/// What the records of the organization itself say, as `(action, target)`.
async fn organization_records(store: &Store) -> Vec<(AuditAction, String)> {
    let every = Paging {
        page: 0,
        per_page: 0,
    };
    store
        .audit_records(AuditPlace::Organization, every)
        .await
        .expect("the organization's records")
        .into_iter()
        .map(|record| (record.action, record.target_id))
        .collect()
}

/// What the user is in the workspace; `None` for no member.
async fn role_in(store: &Store, workspace_id: &str, user_id: &str) -> Option<WorkspaceRole> {
    store
        .resolve_workspace(user_id, workspace_id)
        .await
        .expect("the membership is read")
        .map(|access| access.workspace_role)
}

/// A workspace in the trash goes for good as its boards do: they, the boards it had in
/// the trash, their blocks and their files go, the objects of the files are doomed,
/// and its members, categories and audit log go with it. Another workspace's board and
/// file stay, and so does what is recorded of the organization.
#[tokio::test]
async fn a_workspace_in_the_trash_goes_for_good_as_its_boards_do() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let doomed = named_workspace(&test, "Doomed", &admin).await;
    let kept = named_workspace(&test, "Kept", &admin).await;
    let board = private_board(&test, &doomed, &admin).await;
    let trashed_before = private_board(&test, &doomed, &admin).await;
    let other = private_board(&test, &kept, &admin).await;
    let files = [
        file_of(&board, "7a.png"),
        file_of(&trashed_before, "7b.png"),
    ];
    for file in files.iter().chain([&file_of(&other, "7c.png")]) {
        test.save_file_info(file)
            .await
            .expect("the file is recorded");
    }
    test.delete_board(&trashed_before, &admin)
        .await
        .expect("a board of it is in the trash already");
    assert_eq!(
        test.join_workspace_as_org_admin(&doomed, &admin)
            .await
            .expect("joined"),
        Joining::Joined
    );

    // In use, or in the trash since the cutoff or later, it stays.
    let none = test
        .purge_workspace(&doomed, i64::MAX)
        .await
        .expect("nothing to purge");
    assert_eq!(none, Purged::default(), "a workspace in use stays");
    assert_eq!(
        test.trash_workspace(&doomed, &admin)
            .await
            .expect("trashed"),
        WorkspaceTrashing::Done
    );
    assert_eq!(
        test.purge_workspace(&doomed, 1).await.expect("too early"),
        Purged::default(),
        "trashed later than the cutoff"
    );

    let Purged {
        records,
        files: mut gone,
    } = test
        .purge_workspace(&doomed, i64::MAX)
        .await
        .expect("the workspace is purged");
    gone.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(gone, files, "the files of both of its boards");
    assert_eq!(records, 2, "its two boards");
    let doomed_paths: Vec<String> = test
        .loose_objects()
        .await
        .expect("the listed objects")
        .into_iter()
        .filter(|object| object.state == LooseState::Doomed)
        .map(|object| object.path)
        .collect();
    assert_eq!(
        doomed_paths,
        files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>(),
        "the objects of its files are doomed, and no other"
    );
    let workspaces = test.org_workspaces(&admin).await.expect("listed");
    assert!(
        workspaces.iter().all(|workspace| workspace.id != doomed),
        "{workspaces:?}"
    );
    assert!(
        log_of(&test, &doomed).await.is_empty(),
        "its audit log went"
    );
    for board_id in [&board, &trashed_before] {
        let error = test.get_board(board_id).await.expect_err("gone");
        assert!(error.is_not_found(), "{error:?}");
        let error = test.get_deleted_board(board_id).await.expect_err("gone");
        assert!(error.is_not_found(), "{error:?}");
    }
    test.get_board(&other).await.expect("the other board stays");
    test.get_file_info(&other, "7c.png")
        .await
        .expect("the other board's file stays");
    let of_the_organization = organization_records(&test).await;
    assert!(
        of_the_organization.contains(&(AuditAction::WorkspaceDeleted, doomed.clone())),
        "what was done to the organization stays: {of_the_organization:?}"
    );
    test.teardown().await;
}

/// A record of the audit log is not changed, and is not removed while its workspace is
/// there, past the store too; nor is a record of the organization.
#[tokio::test]
async fn a_record_of_the_audit_log_is_never_changed_or_removed() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let workspace = named_workspace(&test, "Kept", &admin).await;
    test.join_workspace_as_org_admin(&workspace, &admin)
        .await
        .expect("joined");

    for statement in [
        "UPDATE audit_log SET action = 'org_admin_left'",
        "UPDATE audit_log SET create_at = 0",
        "DELETE FROM audit_log",
        "DELETE FROM audit_log WHERE workspace_id IS NULL",
    ] {
        let refused = test.execute_unchecked(statement).await;
        assert!(refused.is_err(), "{statement}: {refused:?}");
    }
    assert_eq!(
        log_of(&test, &workspace).await,
        [(AuditAction::OrgAdminJoined, admin.clone(), String::new())]
    );
    test.teardown().await;
}

/// An administrator of the organization who joins a workspace is its administrator,
/// and leaving gives back what they were: a member with their role, and the role on a
/// board they had before, without the roles they gave themselves meanwhile. One who
/// was no member is none again. Each step is recorded in the workspace.
#[tokio::test]
async fn leaving_a_workspace_joined_gives_back_what_one_was() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let owner = org_member(&test, "owner", OrgRole::Member).await;
    let workspace = named_workspace(&test, "Work", &admin).await;
    for (user, role) in [
        (&admin, WorkspaceRole::Member),
        (&owner, WorkspaceRole::Member),
    ] {
        test.add_workspace_member(&workspace, user, role)
            .await
            .expect("a member");
    }
    let had = private_board(&test, &workspace, &admin).await;
    let taken = private_board(&test, &workspace, &owner).await;

    assert_eq!(
        test.join_workspace_as_org_admin(&workspace, &admin)
            .await
            .expect("joined"),
        Joining::Joined
    );
    assert_eq!(
        test.join_workspace_as_org_admin(&workspace, &admin)
            .await
            .expect("joined again"),
        Joining::Already,
        "nothing changes for its administrator"
    );
    assert_eq!(
        role_in(&test, &workspace, &admin).await,
        Some(WorkspaceRole::Admin)
    );
    let as_viewer = BoardMember {
        board_id: taken.clone(),
        user_id: admin.clone(),
        scheme_viewer: true,
        ..BoardMember::default()
    };
    test.add_member_recorded(
        &as_viewer,
        &AuditEntry::new(&workspace, &admin, AuditAction::SelfAddedToBoard, &taken),
    )
    .await
    .expect("the admin makes themselves a member of a private board");

    assert!(
        test.leave_workspace_as_org_admin(&workspace, &admin)
            .await
            .expect("left")
    );
    assert!(
        !test
            .leave_workspace_as_org_admin(&workspace, &owner)
            .await
            .expect("not joined"),
        "who did not join stays as they are"
    );
    assert_eq!(
        role_in(&test, &workspace, &admin).await,
        Some(WorkspaceRole::Member),
        "a member again"
    );
    test.get_member_for_board(&had, &admin)
        .await
        .expect("the role they had before stays");
    let error = test
        .get_member_for_board(&taken, &admin)
        .await
        .expect_err("the role they gave themselves goes");
    assert!(error.is_not_found(), "{error:?}");
    assert_eq!(
        log_of(&test, &workspace).await,
        [
            (AuditAction::OrgAdminJoined, admin.clone(), String::new()),
            (AuditAction::SelfAddedToBoard, admin.clone(), String::new()),
            (AuditAction::OrgAdminLeft, admin.clone(), String::new()),
        ]
    );
    test.teardown().await;
}

/// Who stops administering the organization is taken out of every workspace they
/// joined as its administrator, in use or in the trash: what they were before, or no
/// member. Each workspace records who did it to whom; the organization records the
/// change of the role.
#[tokio::test]
async fn who_stops_administering_the_organization_is_taken_out_of_what_they_joined() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let other_admin = org_member(&test, "other.admin", OrgRole::Admin).await;
    let was_member = named_workspace(&test, "Was member", &admin).await;
    let was_none = named_workspace(&test, "Was none", &admin).await;
    let in_the_trash = named_workspace(&test, "In the trash", &admin).await;
    test.add_workspace_member(&was_member, &admin, WorkspaceRole::Guest)
        .await
        .expect("a guest of it");
    for workspace in [&was_member, &was_none, &in_the_trash] {
        test.join_workspace_as_org_admin(workspace, &admin)
            .await
            .expect("joined");
    }
    test.trash_workspace(&in_the_trash, &other_admin)
        .await
        .expect("trashed");

    assert_eq!(
        test.set_org_role(&admin, OrgRole::Member, &other_admin)
            .await
            .expect("the role is changed"),
        OrgRoleChange::Done
    );
    assert_eq!(
        role_in(&test, &was_member, &admin).await,
        Some(WorkspaceRole::Guest),
        "what they were before"
    );
    assert_eq!(role_in(&test, &was_none, &admin).await, None);
    test.restore_workspace(&in_the_trash, &other_admin)
        .await
        .expect("restored");
    assert_eq!(
        role_in(&test, &in_the_trash, &admin).await,
        None,
        "the one in the trash too"
    );
    for workspace in [&was_member, &was_none, &in_the_trash] {
        assert_eq!(
            log_of(&test, workspace).await,
            [
                (AuditAction::OrgAdminJoined, admin.clone(), String::new()),
                (
                    AuditAction::OrgAdminRevoked,
                    other_admin.clone(),
                    admin.clone()
                ),
            ]
        );
    }
    assert_eq!(
        test.join_workspace_as_org_admin(&was_none, &admin)
            .await
            .expect("asked"),
        Joining::NoOrgAdmin,
        "a member of the organization does not join so"
    );
    test.teardown().await;
}

/// The organization's last administrator stays one, and a user who is no member of it
/// is told so.
#[tokio::test]
async fn the_last_administrator_of_the_organization_stays_one() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let member = org_member(&test, "member", OrgRole::Member).await;
    let stranger = new_id();

    assert_eq!(
        test.set_org_role(&admin, OrgRole::Member, &admin)
            .await
            .expect("asked"),
        OrgRoleChange::LastAdmin
    );
    assert_eq!(
        test.set_org_role(&stranger, OrgRole::Admin, &admin)
            .await
            .expect("asked"),
        OrgRoleChange::NotAMember
    );
    assert_eq!(
        test.set_org_role(&member, OrgRole::Admin, &admin)
            .await
            .expect("asked"),
        OrgRoleChange::Done
    );
    assert_eq!(
        test.set_org_role(&admin, OrgRole::Member, &member)
            .await
            .expect("asked"),
        OrgRoleChange::Done,
        "with another administrator, one steps down"
    );
    assert_eq!(
        test.set_org_role(&member, OrgRole::Member, &member)
            .await
            .expect("asked"),
        OrgRoleChange::LastAdmin
    );
    test.teardown().await;
}

/// A name is one workspace's among those in use: one in the trash leaves it to be
/// taken, and comes back only under a name nobody took meanwhile. The default
/// workspace is not put in the trash.
#[tokio::test]
async fn a_name_is_one_workspaces_in_use_and_the_default_one_stays() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let first = named_workspace(&test, "Design", &admin).await;
    let again = Workspace {
        id: new_id(),
        title: "Design".to_owned(),
        signup_token: "signup-token".to_owned(),
        ..Workspace::default()
    };
    let taken = test
        .create_named_workspace(&again, &admin)
        .await
        .expect_err("the name is taken");
    assert!(
        matches!(taken, StoreError::Conflict(Taken::WorkspaceName)),
        "{taken:?}"
    );
    test.trash_workspace(&first, &admin).await.expect("trashed");
    let second = named_workspace(&test, "Design", &admin).await;
    let refused = test
        .restore_workspace(&first, &admin)
        .await
        .expect_err("its name is taken");
    assert!(
        matches!(refused, StoreError::Conflict(Taken::WorkspaceName)),
        "{refused:?}"
    );
    test.rename_workspace(&second, "Design 2", &admin)
        .await
        .expect("renamed");
    test.restore_workspace(&first, &admin)
        .await
        .expect("restored under its name");

    assert_eq!(
        test.trash_workspace(&test.workspace_id, &admin)
            .await
            .expect("asked"),
        WorkspaceTrashing::Default
    );
    assert_eq!(
        test.trash_workspace(&new_id(), &admin)
            .await
            .expect("asked"),
        WorkspaceTrashing::NotFound
    );
    test.teardown().await;
}

/// Whatever a joined administrator made themselves on the workspace's boards goes when
/// the joining ends: a role raised on a board they read before is lowered again, and the
/// membership of a board made meanwhile goes. What they held before stays.
#[tokio::test]
async fn the_roles_on_boards_go_back_to_what_they_were_when_one_joined() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let owner = org_member(&test, "owner", OrgRole::Member).await;
    let workspace = named_workspace(&test, "Work", &admin).await;
    for user in [&admin, &owner] {
        test.add_workspace_member(&workspace, user, WorkspaceRole::Member)
            .await
            .expect("a member");
    }
    let read = private_board(&test, &workspace, &owner).await;
    let as_viewer = BoardMember {
        board_id: read.clone(),
        user_id: admin.clone(),
        scheme_viewer: true,
        ..BoardMember::default()
    };
    test.add_member(&as_viewer).await.expect("a viewer of it");

    test.join_workspace_as_org_admin(&workspace, &admin)
        .await
        .expect("joined");
    test.change_member(&BoardMember {
        scheme_admin: true,
        ..as_viewer.clone()
    })
    .await
    .expect("the admin raises their own role");
    let made = private_board(&test, &workspace, &admin).await;
    test.leave_workspace_as_org_admin(&workspace, &admin)
        .await
        .expect("left");

    let held = test
        .get_member_for_board(&read, &admin)
        .await
        .expect("the role they had stays");
    assert!(
        held.scheme_viewer && !held.scheme_admin,
        "a viewer again: {held:?}"
    );
    let error = test
        .get_member_for_board(&made, &admin)
        .await
        .expect_err("the board made meanwhile is not theirs");
    assert!(error.is_not_found(), "{error:?}");
    test.teardown().await;
}

/// A change of the organization is made by its administrator alone, also one whose
/// request was let in before they stopped being one; and a role on a board is given as
/// its manager only by who still administers the workspace.
#[tokio::test]
async fn who_administers_no_more_changes_nothing() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let former = org_member(&test, "former", OrgRole::Member).await;
    let workspace = named_workspace(&test, "Work", &admin).await;
    let board = private_board(&test, &workspace, &admin).await;

    let refused = [
        test.set_org_role(&former, OrgRole::Admin, &former)
            .await
            .map(|_| ()),
        test.trash_workspace(&workspace, &former).await.map(|_| ()),
        test.rename_workspace(&workspace, "Mine", &former).await,
        test.update_organization(
            &fvoci_model::OrganizationPatch {
                public_links: Some(false),
                ..fvoci_model::OrganizationPatch::default()
            },
            &former,
        )
        .await
        .map(|_| ()),
    ];
    for refusal in refused {
        let error = refusal.expect_err("refused");
        assert!(error.is_not_found(), "{error:?}");
    }
    let added = test
        .add_member_recorded(
            &BoardMember {
                board_id: board.clone(),
                user_id: former.clone(),
                scheme_viewer: true,
                ..BoardMember::default()
            },
            &AuditEntry::new(&workspace, &former, AuditAction::SelfAddedToBoard, &board),
        )
        .await
        .expect_err("no manager of it");
    assert!(added.is_not_found(), "{added:?}");
    assert_eq!(
        log_of(&test, &workspace).await,
        Vec::new(),
        "nothing is recorded"
    );
    test.teardown().await;
}

/// The organization's administrators are the instance's for now: one made a member of
/// the organization stops administering the instance, unless they are its last
/// administrator, and a change through the instance is recorded as one through the
/// organization.
#[tokio::test]
async fn the_instance_and_the_organization_keep_one_administration() {
    let test = common::setup().await;
    let admin = org_member(&test, "admin", OrgRole::Admin).await;
    let other = org_member(&test, "other", OrgRole::Member).await;

    test.set_user_admin(&other, InstanceRole::Administrator, &admin)
        .await
        .expect("made the instance's administrator");
    assert_eq!(
        test.set_org_role(&admin, OrgRole::Member, &other)
            .await
            .expect("asked"),
        OrgRoleChange::Done
    );
    let stepped_down = test.get_user_by_id(&admin).await.expect("the user");
    assert!(!stepped_down.is_admin, "no administrator of the instance");
    assert_eq!(
        organization_records(&test).await,
        [
            (AuditAction::OrgAdminRemoved, test.org_id.clone()),
            (AuditAction::OrgAdminGranted, test.org_id.clone()),
        ],
        "both changes are recorded, the newest first"
    );
    test.teardown().await;
}
