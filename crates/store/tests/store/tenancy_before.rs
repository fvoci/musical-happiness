//! A database from before there were organizations: what the migrations make of it.

use std::collections::BTreeMap;

use fvoci_model::{OrgKind, OrgRole, User, WorkspaceRole, is_id, new_id};
use fvoci_store::{DbType, SYSTEM_ORG_ID, SYSTEM_WORKSPACE_ID, Store};
use sqlx::Connection as _;

use crate::common;

/// What a server that knew nothing of organizations left in every table: three users
/// (an administrator, a member, and one who left), two workspaces, boards of both and a
/// built-in template, and something in every other table, some of it hanging on the
/// template (a membership, a file, sharing, a subscription, a filing: what an older
/// server let a user have on one), some on a board in the trash, some on nothing that
/// is still there. The words that begin with a colon stand for identifiers.
pub(crate) const BEFORE_ORGANIZATIONS: &str = r#"
INSERT INTO users (id, username, email, password_hash, create_at, update_at, delete_at,
                   is_admin, display_name) VALUES
    (':admin', 'admin', 'admin@example.com', '', 10, 10, 0, true, ''),
    (':member', 'member', 'member@example.com', '', 20, 20, 0, false, ''),
    (':left', 'left', 'left@example.com', '', 30, 30, 40, false, '');
UPDATE instance SET bootstrapped = true;
INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at) VALUES
    (':younger_workspace', 'token-2', '{}', NULL, 200, 200),
    (':older_workspace', 'token-1', '{"kept": 1}', ':admin', 100, 100);
INSERT INTO boards (id, team_id, created_by, modified_by, type, title, description, icon,
                    show_description, is_template, is_builtin, template_version, properties,
                    card_properties, create_at, update_at) VALUES
    (':board', ':older_workspace', ':admin', ':admin', 'O', 'a board', '', '', false, false, false,
     0, '{}', '[]', 1, 1),
    (':template', ':older_workspace', NULL, NULL, 'O', 'a template', '', '', false, true, true, 7,
     '{}', '[]', 1, 1),
    (':other_board', ':younger_workspace', ':member', ':member', 'P', 'another', '', '', false,
     false, false, 0, '{}', '[]', 2, 2);
INSERT INTO board_members (board_id, user_id, scheme_admin, scheme_editor, scheme_commenter,
                           scheme_viewer) VALUES
    (':board', ':admin', true, true, false, false),
    (':other_board', ':member', true, true, false, false),
    (':template', ':member', false, true, false, false);
INSERT INTO blocks (id, board_id, parent_id, created_by, modified_by, "schema", type, title,
                    fields, create_at, update_at) VALUES
    (':card', ':board', NULL, ':admin', ':admin', 1, 'card', 'a card', '{}', 1, 1),
    (':template_card', ':template', NULL, NULL, NULL, 1, 'card', 'of the template', '{}', 1, 1);
INSERT INTO blocks (id, board_id, parent_id, created_by, modified_by, "schema", type, title,
                    fields, create_at, update_at) VALUES
    (':comment', ':board', ':card', ':member', ':member', 1, 'comment', '', '{}', 2, 2);
INSERT INTO deleted_boards (id, team_id, deleted_at, deleted_by, deletion_id, board, members,
                            files) VALUES
    (':deleted_board', ':older_workspace', 5, ':admin', ':deletion', '{}', '[]', '[]');
INSERT INTO deleted_blocks (id, board_id, parent_id, deleted_at, deleted_by, deletion_id,
                            block) VALUES
    (':deleted_card', ':board', NULL, 5, ':admin', ':other_deletion', '{"type": "card"}'),
    (':card_of_deleted_board', ':deleted_board', NULL, 5, ':admin', ':deletion',
     '{"type": "card"}');
INSERT INTO categories (id, name, user_id, team_id, create_at, update_at, collapsed, type,
                        sort_order) VALUES
    (':category', 'mine', ':admin', ':older_workspace', 1, 1, false, 'custom', 0),
    (':other_category', 'theirs', ':member', ':older_workspace', 1, 1, false, 'custom', 0);
INSERT INTO category_boards (id, user_id, category_id, board_id, create_at, update_at,
                             sort_order, hidden) VALUES
    (':filed', ':admin', ':category', ':board', 1, 1, 0, false),
    (':filed_template', ':member', ':other_category', ':template', 1, 1, 0, false);
INSERT INTO file_info (id, board_id, name, original_name, extension, size, path, create_at)
VALUES (':file', ':board', '7shown.png', 'shown.png', '.png', 1, 'somewhere', 1),
       (':template_file', ':template', '7template.png', 'template.png', '.png', 1, 'elsewhere', 1);
INSERT INTO file_refs (block_id, board_id, name) VALUES
    (':card', ':board', '7shown.png'),
    (':card_of_deleted_board', ':deleted_board', '7gone.png');
INSERT INTO sharing (board_id, enabled, token, modified_by, update_at)
VALUES (':board', true, 'the-read-token', ':admin', 1),
       (':template', true, 'the-template-token', ':admin', 1);
INSERT INTO subscriptions (block_type, block_id, subscriber_type, subscriber_id, notified_at,
                           create_at) VALUES
    ('card', ':card', 'user', ':member', 0, 1),
    ('card', ':template_card', 'user', ':admin', 0, 1);
INSERT INTO uploads (id, user_id, board_id, name, size, content_type, part_size, create_at,
                     expires_at, closed_at, multipart_id)
VALUES ('an-upload', ':admin', ':board', 'upload.bin', 1, '', 1, 1, 9000000000000, 0, '');
INSERT INTO upload_parts (upload_id, number, size, part_id) VALUES ('an-upload', 1, 1, '');
INSERT INTO comment_versions (block_id, version, body, files, written_by, written_at) VALUES
    (':comment', 1, NULL, '[]', ':admin', 1),
    (':comment_that_is_gone', 1, NULL, '[]', ':admin', 1);
"#;

/// The identifiers of [`BEFORE_ORGANIZATIONS`], by their names there.
pub(crate) struct Before(BTreeMap<&'static str, String>);

impl Before {
    const NAMES: [&'static str; 23] = [
        "admin",
        "member",
        "left",
        "younger_workspace",
        "older_workspace",
        "board",
        "template",
        "other_board",
        "card",
        "template_card",
        "comment",
        "deleted_board",
        "deletion",
        "other_deletion",
        "deleted_card",
        "card_of_deleted_board",
        "category",
        "other_category",
        "filed",
        "filed_template",
        "file",
        "template_file",
        "comment_that_is_gone",
    ];

    pub(crate) fn new() -> Self {
        Self(Self::NAMES.iter().map(|name| (*name, new_id())).collect())
    }

    pub(crate) fn id(&self, name: &str) -> &str {
        &self.0[name]
    }

    /// The statements, with the identifiers in the places of their names: the longest
    /// name first, as some names begin with another.
    pub(crate) fn statements(&self) -> String {
        let mut names: Vec<&&str> = self.0.keys().collect();
        names.sort_by_key(|name| std::cmp::Reverse(name.len()));
        let mut statements = BEFORE_ORGANIZATIONS.to_owned();
        for name in names {
            statements = statements.replace(&format!(":{name}"), &self.0[*name]);
        }
        assert!(!statements.contains("':"), "a name without an identifier");
        statements
    }
}

/// The two workspaces of the old database are the organization's, as they were, every user is in
/// both, and who registers now gets into the oldest.
async fn assert_the_workspaces_are_its_workspaces(
    unset: &Store,
    ours: &Store,
    before: &Before,
    org_id: &str,
) {
    // Both workspaces are its own, as they were, the oldest first.
    let workspaces = ours.get_workspaces().await.expect("the workspaces");
    let ids: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [before.id("older_workspace"), before.id("younger_workspace")],
        "the oldest first"
    );
    assert_eq!(
        workspaces[0].modified_by,
        before.id("admin"),
        "{:?}",
        workspaces[0]
    );
    assert_eq!(workspaces[0].settings["kept"], 1, "{:?}", workspaces[0]);
    assert_eq!(workspaces[0].title, "", "no name yet");

    // Every user is in both, the one who left too; the administrator administers.
    for (user, org_role, role) in [
        ("admin", OrgRole::Admin, WorkspaceRole::Admin),
        ("member", OrgRole::Member, WorkspaceRole::Member),
        ("left", OrgRole::Member, WorkspaceRole::Member),
    ] {
        let listed = unset
            .workspaces_of_user(before.id(user), None)
            .await
            .expect("the workspaces are listed");
        // The user's own space, which every account that was there got, is last.
        let (own, listed) = listed.split_last().expect("the user's own space");
        assert_eq!(
            (own.kind, own.role, own.name.as_str()),
            (OrgKind::Personal, WorkspaceRole::Admin, ""),
            "{user}"
        );
        assert_ne!(own.org_id, org_id, "{user}: an organization of its own");
        assert_eq!(
            common::own_space_shape(&unset.system(), before.id(user)).await,
            common::own_space(),
            "{user}: as the space made with a new account is"
        );
        let listed: Vec<(&str, &str, WorkspaceRole)> = listed
            .iter()
            .map(|workspace| {
                (
                    workspace.id.as_str(),
                    workspace.org_id.as_str(),
                    workspace.role,
                )
            })
            .collect();
        assert_eq!(
            listed,
            [
                (before.id("older_workspace"), org_id, role),
                (before.id("younger_workspace"), org_id, role)
            ],
            "{user}"
        );
        let access = unset
            .resolve_workspace(before.id(user), before.id("younger_workspace"))
            .await
            .expect("the workspace is resolved")
            .expect("a member");
        assert_eq!(access.org_role, Some(org_role), "{user}");
    }
    // The default workspace is the oldest workspace: who registers now gets into it.
    let mut newcomer = User {
        id: new_id(),
        username: "newcomer".to_owned(),
        email: "newcomer@example.com".to_owned(),
        ..User::default()
    };
    ours.create_user(&mut newcomer)
        .await
        .expect("a user registers");
    assert!(!newcomer.is_admin, "the instance had its first user");
    let listed = unset
        .workspaces_of_user(&newcomer.id, None)
        .await
        .expect("the workspaces are listed");
    let ids: Vec<(&str, OrgKind)> = listed
        .iter()
        .map(|workspace| (workspace.id.as_str(), workspace.kind))
        .collect();
    let own = unset
        .system()
        .texts_unchecked(&format!(
            "SELECT CAST(default_workspace_id AS TEXT) FROM organizations WHERE owner_id = '{}'",
            newcomer.id
        ))
        .await
        .expect("read");
    assert_eq!(
        ids,
        [
            (before.id("older_workspace"), OrgKind::Team),
            (own[0].as_str(), OrgKind::Personal)
        ],
        "the default workspace, and the one of the space made with the account"
    );
}

/// The built-in template is the system organization's and is still read by ours; the
/// boards of the workspaces are where they were.
async fn assert_the_template_is_the_systems(
    unset: &Store,
    ours: &Store,
    before: &Before,
    org_id: &str,
) {
    // The built-in template is the system organization's, in its workspace, and is
    // still read by ours; the boards of the workspaces are where they were.
    let system_org = unset.for_org(SYSTEM_ORG_ID).expect("an identifier");
    let templates = system_org
        .get_builtin_templates()
        .await
        .expect("the built-in templates");
    let found: Vec<(&str, &str)> = templates
        .iter()
        .map(|board| (board.id.as_str(), board.workspace_id.as_str()))
        .collect();
    assert_eq!(
        found,
        [(before.id("template"), SYSTEM_WORKSPACE_ID)],
        "in the system's workspace"
    );
    let read = ours
        .get_container(before.id("template"))
        .await
        .expect("ours reads the template");
    assert!(read.is_builtin, "{read:?}");
    ours.get_block(before.id("template_card"))
        .await
        .expect("and its card");
    // What a user had on the template under an older server is the system's with it:
    // behind PostgreSQL's wall it is out of the user's reach, and the template is
    // nobody's to change. SQLite has no wall, and leaves it where it was.
    let membership = ours
        .get_member_for_container(before.id("template"), before.id("member"))
        .await;
    match unset.db_type() {
        DbType::Postgres => assert!(
            membership.is_err_and(|error| error.is_not_found()),
            "the membership of the template is out of reach"
        ),
        DbType::Sqlite => assert!(
            membership.is_ok(),
            "the membership of the template is where it was"
        ),
    }
    assert_eq!(
        unset
            .shared_container(before.id("template"), "the-template-token")
            .await
            .expect("the shared board is looked up"),
        None,
        "the template is shared with nobody"
    );
    assert_eq!(
        ours.get_container(before.id("board"))
            .await
            .expect("a board")
            .workspace_id,
        before.id("older_workspace"),
        "a board stays in its workspace"
    );
    assert_eq!(
        unset
            .shared_container(before.id("board"), "the-read-token")
            .await
            .expect("the shared board is looked up")
            .map(|shared| shared.org_id),
        Some(org_id.to_owned()),
        "whose a shared board is"
    );
}

/// On PostgreSQL every row that was stored is some organization's and behind the wall.
async fn assert_every_row_is_behind_the_wall(unset: &Store, ours: &Store) {
    let system_org = unset.for_org(SYSTEM_ORG_ID).expect("an identifier");
    // Every row is the organization's, but what hangs on the template, which
    // is the system's; ours reads the template's board and card besides.
    let expected = [
        ("organizations", 1, 1),
        ("workspaces", 2, 1),
        ("organization_members", 4, 0),
        ("workspace_members", 7, 0),
        ("containers", 2, 1),
        ("container_members", 2, 1),
        ("blocks", 2, 1),
        ("deleted_containers", 1, 0),
        ("deleted_blocks", 2, 0),
        ("categories", 2, 0),
        ("category_containers", 2, 0),
        ("file_info", 1, 1),
        ("file_refs", 2, 0),
        ("sharing", 1, 1),
        ("subscriptions", 1, 1),
        ("uploads", 1, 0),
        ("upload_parts", 1, 0),
        ("comment_versions", 2, 0),
    ];
    for (table, of_ours, of_the_system) in expected {
        let count = async |store: &Store| store.count_rows(table).await.expect("counted");
        let read_besides = if ["containers", "blocks", "file_info"].contains(&table) {
            of_the_system
        } else {
            0
        };
        assert_eq!(count(ours).await, of_ours + read_besides, "{table}: ours");
        assert_eq!(
            count(&system_org).await,
            of_the_system,
            "{table}: the system's"
        );
        // Besides what was there, the own space of each account, behind its wall.
        let of_spaces = common::rows_of_own_spaces(unset, table).await;
        assert_eq!(
            count(&unset.system()).await,
            of_ours + of_the_system + of_spaces,
            "{table}: all"
        );
        assert_eq!(count(unset).await, 0, "{table}: no organization named");
    }
}

/// A database that a server left which knew nothing of organizations becomes one
/// organization, whose default workspace is the oldest workspace; every user is a member of
/// it and of each of its workspaces, an administrator of the instance of both as an
/// administrator; and the built-in template is the system organization's. On
/// PostgreSQL every row that was stored is some organization's and behind the wall.
#[tokio::test]
async fn a_database_from_before_organizations_becomes_one_organization() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let before = Before::new();
    common::execute_raw(&common::raw_connection(&database), &before.statements()).await;

    let unset = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("the store migrates the database");
    // The one organization: found, not made.
    let org_id = unset
        .instance_organization()
        .await
        .expect("the organization")
        .expect("an existing organization");
    assert!(is_id(&org_id), "{org_id}");
    assert_eq!(org_id.as_bytes()[14], b'7', "a version 7 UUID: {org_id}");
    assert!(
        b"89ab".contains(&org_id.as_bytes()[19]),
        "of the usual variant: {org_id}"
    );
    let ours = unset.for_org(&org_id).expect("an identifier");

    assert_the_workspaces_are_its_workspaces(&unset, &ours, &before, &org_id).await;
    assert_the_template_is_the_systems(&unset, &ours, &before, &org_id).await;
    match database.db_type() {
        DbType::Postgres => assert_every_row_is_behind_the_wall(&unset, &ours).await,
        DbType::Sqlite => {
            // The table of the workspaces was made again (0014): every row that refers to one
            // still finds it.
            unset.close().await;
            let path = database.sqlite_path().expect("an SQLite file");
            let mut conn = sqlx::sqlite::SqliteConnection::connect(&path.display().to_string())
                .await
                .expect("the database file opens");
            let broken = sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&mut conn)
                .await
                .expect("the keys are checked");
            conn.close().await.expect("the connection closes");
            assert!(broken.is_empty(), "{} rows refer to nothing", broken.len());
        }
    }

    unset.close().await;
    database.remove().await;
}

/// The plain case of a database from before: one workspace and three users. It becomes one
/// organization with that workspace, and every user is a member of both.
#[tokio::test]
async fn a_database_with_one_workspace_becomes_one_organization_with_one_workspace() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_organizations(&database).await;
    let (workspace_id, admin, member, left) = (new_id(), new_id(), new_id(), new_id());
    common::execute_raw(
        &common::raw_connection(&database),
        &format!(
            "INSERT INTO users (id, username, email, password_hash, create_at, update_at, \
             delete_at, is_admin, display_name) VALUES \
             ('{admin}', 'admin', 'admin@example.com', '', 10, 10, 0, true, ''), \
             ('{member}', 'member', 'member@example.com', '', 20, 20, 0, false, ''), \
             ('{left}', 'left', 'left@example.com', '', 30, 30, 40, false, ''); \
             UPDATE instance SET bootstrapped = true; \
             INSERT INTO teams (id, signup_token, settings, modified_by, create_at, update_at) \
             VALUES ('{workspace_id}', 'the-token', '{{}}', NULL, 100, 100);"
        ),
    )
    .await;

    let unset = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("the store migrates the database");
    // One organization, and the system's; and the space of each of the three users.
    let kinds = unset
        .system()
        .texts_unchecked("SELECT kind FROM organizations ORDER BY kind")
        .await
        .expect("read");
    let own = OrgKind::Personal.as_str();
    let shared = OrgKind::Team.as_str();
    assert_eq!(kinds, [own, own, own, shared, shared]);
    let org_id = unset
        .instance_organization()
        .await
        .expect("the organization")
        .expect("an existing organization");
    let ours = unset.for_org(&org_id).expect("an identifier");
    // One workspace: the one of the old database.
    let workspaces = ours.get_workspaces().await.expect("the workspaces");
    let ids: Vec<&str> = workspaces
        .iter()
        .map(|workspace| workspace.id.as_str())
        .collect();
    assert_eq!(ids, [workspace_id.as_str()], "the one workspace");
    assert_eq!(
        unset
            .system()
            .count_rows("workspaces")
            .await
            .expect("counted"),
        5,
        "the workspace, the system's, and that of each user's own space"
    );
    // Every user is a member of the organization and of the workspace.
    for (user, org_role, role) in [
        (&admin, OrgRole::Admin, WorkspaceRole::Admin),
        (&member, OrgRole::Member, WorkspaceRole::Member),
        (&left, OrgRole::Member, WorkspaceRole::Member),
    ] {
        let access = unset
            .resolve_workspace(user, &workspace_id)
            .await
            .expect("the workspace is resolved")
            .expect("a member of the workspace");
        assert_eq!(access.org_id, org_id, "of the one organization");
        assert_eq!(
            (access.org_role, access.workspace_role),
            (Some(org_role), role),
            "a member of both"
        );
        let listed = unset
            .workspaces_of_user(user, None)
            .await
            .expect("the workspaces are listed");
        let kinds: Vec<OrgKind> = listed.iter().map(|workspace| workspace.kind).collect();
        assert_eq!(
            kinds,
            [OrgKind::Team, OrgKind::Personal],
            "one workspace, and the one of the user's own space: {listed:?}"
        );
    }
    assert_eq!(
        unset
            .system()
            .count_rows("organization_members")
            .await
            .expect("counted"),
        6,
        "the three users, each in their own space besides, and nobody else"
    );
    assert_eq!(
        unset
            .system()
            .count_rows("workspace_members")
            .await
            .expect("counted"),
        6,
        "the three users, in the one workspace and in their own"
    );

    unset.close().await;
    database.remove().await;
}
