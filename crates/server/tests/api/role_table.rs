//! The table of roles: what every operation on one board or wiki, its people, its
//! sharing, its files, its place in the sidebar and the trash, answers to every kind of
//! person, a board's and a wiki's each pinned in a file of `role_table/`.
//!
//! The people are the roles on a container for the invited only (admin, editor,
//! commenter, viewer), a member of the workspace without a role on it and on an open
//! one, a guest invited to it and one on an open one, the workspace's administrator
//! who only manages it, the organization's administrator who is not in the workspace,
//! and a member of another workspace on an open one. Another organization is the
//! comparison of two organizations' (PostgreSQL only). Every answer is asked of a
//! container as it was made: one that an answer changed is made anew.
//!
//! A board and a wiki answer alike but where an open wiki is read without joining it
//! ([`the_wiki_differs_from_the_board_only_where_an_open_wiki_is_read`]).
//!
//! With `FVOCI_REGRESSION_UPDATE=1` the files are written anew instead of compared.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use fvoci_model::WorkspaceRole;
use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use crate::common::{
    ApiResponse, Client, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer, UNKNOWN_ID,
};

/// A board, or a wiki.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Board,
    Wiki,
}

/// Who a container is for besides its members.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Scope {
    /// For the invited only.
    Invited,
    /// Open to the workspace.
    Open,
}

impl Scope {
    const fn wire(self) -> &'static str {
        match self {
            Self::Invited => "P",
            Self::Open => "O",
        }
    }
}

/// The people of the table: who asks, and of which container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    Admin,
    Editor,
    Commenter,
    Viewer,
    Member,
    Guest,
    WorkspaceAdmin,
    OrgAdmin,
    OtherWorkspace,
}

/// The columns of the table, in their order: the header, who asks, of which container.
const COLUMNS: [(&str, Who, Scope); 11] = [
    ("admin", Who::Admin, Scope::Invited),
    ("editor", Who::Editor, Scope::Invited),
    ("commenter", Who::Commenter, Scope::Invited),
    ("viewer", Who::Viewer, Scope::Invited),
    ("member", Who::Member, Scope::Invited),
    ("member@open", Who::Member, Scope::Open),
    ("guest", Who::Guest, Scope::Invited),
    ("guest@open", Who::Guest, Scope::Open),
    ("ws-admin", Who::WorkspaceAdmin, Scope::Invited),
    ("org-admin", Who::OrgAdmin, Scope::Invited),
    ("other-ws", Who::OtherWorkspace, Scope::Open),
];

/// The column whose answers may differ between a board and a wiki.
const READS_UNJOINED: &str = "member@open";

/// One operation: the name of its row, its method and path (after `/api/v1`), what it
/// is sent with, and what is done to the container before it is asked.
struct Op {
    row: &'static str,
    method: Method,
    path: &'static str,
    body: Body,
    prepare: Prepare,
}

#[derive(Clone, Copy)]
enum Body {
    None,
    Json(fn(&Things, &People) -> Value),
    File,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prepare {
    Nothing,
    /// The container is in the trash.
    DeleteContainer,
    /// Its page is in the trash.
    DeletePage,
}

const fn op(row: &'static str, method: Method, path: &'static str, body: Body) -> Op {
    Op {
        row,
        method,
        path,
        body,
        prepare: Prepare::Nothing,
    }
}

/// The operations a board and a wiki share, in the order of the rows.
fn shared_ops() -> Vec<Op> {
    let mut ops = container_ops();
    ops.extend(holding_ops());
    ops
}

/// The operations of the container itself and of who is on it.
fn container_ops() -> Vec<Op> {
    use Body::{Json, None};
    vec![
        op("container: get", Method::GET, "/boards/{c}", None),
        op(
            "container: patch title",
            Method::PATCH,
            "/boards/{c}",
            Json(|_, _| json!({"title": "changed"})),
        ),
        op(
            "container: patch type",
            Method::PATCH,
            "/boards/{c}",
            Json(|things, _| json!({"type": things.scope.wire()})),
        ),
        op("container: delete", Method::DELETE, "/boards/{c}", None),
        Op {
            prepare: Prepare::DeleteContainer,
            ..op(
                "container: undelete",
                Method::POST,
                "/boards/{c}/undelete",
                Json(|_, _| json!({})),
            )
        },
        Op {
            prepare: Prepare::DeleteContainer,
            ..op("trash: list", Method::GET, "/trash/boards", None)
        },
        op("members: list", Method::GET, "/boards/{c}/members", None),
        op(
            "members: add",
            Method::POST,
            "/boards/{c}/members",
            Json(|things, people| {
                let (board, user) = (&things.container, &people.extra);
                json!({"boardId": board, "userId": user, "schemeViewer": true})
            }),
        ),
        op(
            "members: update",
            Method::PUT,
            "/boards/{c}/members/{viewer}",
            Json(|things, people| {
                let (board, user) = (&things.container, &people.viewer);
                json!({"boardId": board, "userId": user, "schemeCommenter": true})
            }),
        ),
        op(
            "members: remove",
            Method::DELETE,
            "/boards/{c}/members/{commenter}",
            None,
        ),
        op(
            "members: join",
            Method::POST,
            "/boards/{c}/join",
            Json(|_, _| json!({})),
        ),
        op(
            "members: leave",
            Method::POST,
            "/boards/{c}/leave",
            Json(|_, _| json!({})),
        ),
        op("teams: list", Method::GET, "/boards/{c}/teams", None),
        op(
            "teams: set",
            Method::PUT,
            "/boards/{c}/teams/{team}",
            Json(|_, _| json!({"role": "editor"})),
        ),
        op(
            "teams: remove",
            Method::DELETE,
            "/boards/{c}/teams/{team}",
            None,
        ),
    ]
}

/// The operations of its sharing, its files, its place in the sidebar and what names
/// what is on it.
fn holding_ops() -> Vec<Op> {
    use Body::{File, Json, None};
    vec![
        op("sharing: get", Method::GET, "/boards/{c}/sharing", None),
        op(
            "sharing: set",
            Method::POST,
            "/boards/{c}/sharing",
            Json(|_, _| json!({"enabled": true})),
        ),
        op("files: upload", Method::POST, "/boards/{c}/files", File),
        op("files: get", Method::GET, "/boards/{c}/files/{f}", None),
        op(
            "files: info",
            Method::GET,
            "/boards/{c}/files/{f}/info",
            None,
        ),
        op(
            "uploads: open",
            Method::POST,
            "/boards/{c}/uploads",
            Json(|_, _| json!({"name": "parts.bin", "size": 4})),
        ),
        op(
            "sidebar: category",
            Method::POST,
            "/categories/{category}/boards/{c}",
            Json(|_, _| json!({})),
        ),
        op(
            "sidebar: hide",
            Method::PUT,
            "/categories/{category}/boards/{c}/hide",
            None,
        ),
        op(
            "sidebar: unhide",
            Method::PUT,
            "/categories/{category}/boards/{c}/unhide",
            None,
        ),
        op("references: to", Method::GET, "/references/to/{p}", None),
    ]
}

/// The operations of what a board holds.
fn board_ops() -> Vec<Op> {
    use Body::{Json, None};
    vec![
        op("cards: list", Method::GET, "/boards/{c}/cards", None),
        op("cards: get", Method::GET, "/cards/{p}", None),
        op(
            "cards: create",
            Method::POST,
            "/boards/{c}/cards",
            Json(|_, _| json!({"title": "made"})),
        ),
        op(
            "cards: patch",
            Method::PATCH,
            "/cards/{p}",
            Json(|_, _| json!({"title": "changed"})),
        ),
        op("blocks: list", Method::GET, "/boards/{c}/blocks", None),
        op(
            "comments: create",
            Method::POST,
            "/boards/{c}/blocks",
            Json(|things, _| {
                json!([{"boardId": things.container, "parentId": things.block, "type": "comment",
                        "title": "said", "createAt": 1, "updateAt": 1}])
            }),
        ),
        op(
            "cards: body versions",
            Method::GET,
            "/cards/{p}/body-versions",
            None,
        ),
        op("cards: trash", Method::GET, "/boards/{c}/trash/cards", None),
    ]
}

/// The operations of what a wiki holds.
fn wiki_ops() -> Vec<Op> {
    use Body::{Json, None};
    vec![
        op("pages: tree", Method::GET, "/wikis/{c}/pages", None),
        op("pages: get", Method::GET, "/wikis/{c}/pages/{p}", None),
        op(
            "pages: create",
            Method::POST,
            "/wikis/{c}/pages",
            Json(|_, _| json!({"title": "made"})),
        ),
        op(
            "pages: patch",
            Method::PATCH,
            "/wikis/{c}/pages/{p}",
            Json(|_, _| json!({"title": "changed"})),
        ),
        op(
            "pages: move",
            Method::POST,
            "/wikis/{c}/pages/{p}/move",
            Json(|things, _| json!({"parentId": things.second})),
        ),
        op(
            "pages: duplicate",
            Method::POST,
            "/wikis/{c}/pages/{p}/duplicate",
            Json(|_, _| json!({})),
        ),
        op(
            "pages: delete",
            Method::DELETE,
            "/wikis/{c}/pages/{p}",
            None,
        ),
        op("pages: trash", Method::GET, "/wikis/{c}/trash/pages", None),
        Op {
            prepare: Prepare::DeletePage,
            ..op(
                "pages: undelete",
                Method::POST,
                "/wikis/{c}/pages/{p}/undelete",
                Json(|_, _| json!({})),
            )
        },
    ]
}

/// The people, signed in, and who the operations name.
struct People {
    clients: BTreeMap<&'static str, Client>,
    editor: String,
    commenter: String,
    viewer: String,
    guest: String,
    /// A member of the workspace who asks nothing: whom `members: add` adds.
    extra: String,
    team: String,
}

impl People {
    async fn of(ts: &TestServer) -> Self {
        let mut clients = BTreeMap::new();
        clients.insert("admin", ts.client.clone());
        let mut ids = BTreeMap::new();
        for (name, role) in [
            ("editor", Some(WorkspaceRole::Member)),
            ("commenter", Some(WorkspaceRole::Member)),
            ("viewer", Some(WorkspaceRole::Member)),
            ("member", Some(WorkspaceRole::Member)),
            ("guest", Some(WorkspaceRole::Guest)),
            ("ws-admin", Some(WorkspaceRole::Admin)),
            ("extra", Some(WorkspaceRole::Member)),
            ("org-admin", None),
            ("other-ws", None),
        ] {
            let client = ts.add_user(&format!("table.{name}")).await;
            let id = client.me().await.id;
            // A new account is a member of the test workspace: what it is there is set,
            // or it is taken out, to be of the root workspace alone.
            let statement = match role {
                Some(role) => format!(
                    "UPDATE workspace_members SET role = '{}' WHERE user_id = '{id}' \
                     AND workspace_id = '{TEST_WORKSPACE_ID}'",
                    role.as_str()
                ),
                None => format!(
                    "DELETE FROM workspace_members WHERE user_id = '{id}' \
                     AND workspace_id = '{TEST_WORKSPACE_ID}'"
                ),
            };
            ts.app
                .store()
                .execute_unchecked(&statement)
                .await
                .expect("what the account is in the test workspace");
            clients.insert(name, client);
            ids.insert(name, id);
        }
        ts.app
            .store()
            .execute_unchecked(&format!(
                "UPDATE organization_members SET role = 'admin' WHERE user_id = '{}'",
                ids["org-admin"]
            ))
            .await
            .expect("the organization's administrator");
        // The workspace's administrators make its teams.
        let team = clients["ws-admin"]
            .post(&format!("{TEST_WS}/teams"), &json!({"name": "Table"}))
            .await
            .value();
        Self {
            clients,
            editor: ids["editor"].clone(),
            commenter: ids["commenter"].clone(),
            viewer: ids["viewer"].clone(),
            guest: ids["guest"].clone(),
            extra: ids["extra"].clone(),
            team: text(&team, "id"),
        }
    }

    fn client(&self, who: Who) -> &Client {
        let name = match who {
            Who::Admin => "admin",
            Who::Editor => "editor",
            Who::Commenter => "commenter",
            Who::Viewer => "viewer",
            Who::Member => "member",
            Who::Guest => "guest",
            Who::WorkspaceAdmin => "ws-admin",
            Who::OrgAdmin => "org-admin",
            Who::OtherWorkspace => "other-ws",
        };
        &self.clients[name]
    }
}

/// A container made for the operations, as its admin made it.
struct Things {
    scope: Scope,
    container: String,
    /// A card of the board, a page of the wiki.
    block: String,
    /// Another page of the wiki, to move the page below; empty for a board.
    second: String,
    file: String,
}

fn text(value: &Value, member: &str) -> String {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("{member} in {value}"))
        .to_owned()
}

#[track_caller]
fn done(response: &ApiResponse) {
    assert!(
        response.status.is_success(),
        "{}: {}",
        response.status,
        response.text()
    );
}

/// Makes a container of `kind` and `scope` with its people: an editor, a commenter and a
/// viewer, the guest as an editor on one for the invited only, the team as a viewer;
/// a file, sharing on, a card (with a comment) or two pages.
async fn make(people: &People, kind: Kind, scope: Scope) -> Things {
    let owner = &people.clients["admin"];
    let at = match kind {
        Kind::Board => format!("{TEST_WS}/boards"),
        Kind::Wiki => format!("{TEST_WS}/wikis"),
    };
    let made = owner.post(&at, &json!({"type": scope.wire()})).await;
    done(&made);
    let container = text(&made.value(), "id");
    let at = format!("{TEST_WS}/boards/{container}");
    let mut roles = vec![
        (&people.editor, "schemeEditor"),
        (&people.commenter, "schemeCommenter"),
        (&people.viewer, "schemeViewer"),
    ];
    if scope == Scope::Invited {
        roles.push((&people.guest, "schemeEditor"));
    }
    for (user, flag) in roles {
        let mut member = json!({"boardId": container, "userId": user});
        member[flag] = json!(true);
        done(&owner.post(&format!("{at}/members"), &member).await);
        done(&owner.put(&format!("{at}/members/{user}"), &member).await);
    }
    let team = format!("{at}/teams/{}", people.team);
    done(&owner.put(&team, &json!({"role": "viewer"})).await);
    let file = Part::bytes(b"kept".to_vec()).file_name("kept.txt");
    let uploaded = Client::send(
        owner
            .request(Method::POST, &format!("{at}/files"))
            .multipart(Form::new().part("file", file)),
    )
    .await;
    done(&uploaded);
    done(
        &owner
            .post(&format!("{at}/sharing"), &json!({"enabled": true}))
            .await,
    );
    let (block, second) = match kind {
        Kind::Board => {
            let card = owner
                .post(&format!("{at}/cards"), &json!({"title": "a card"}))
                .await;
            done(&card);
            (text(&card.value(), "id"), String::new())
        }
        Kind::Wiki => {
            let pages = format!("{TEST_WS}/wikis/{container}/pages");
            let page = owner.post(&pages, &json!({"title": "a page"})).await;
            let second = owner.post(&pages, &json!({"title": "another"})).await;
            done(&page);
            done(&second);
            (text(&page.value(), "id"), text(&second.value(), "id"))
        }
    };
    Things {
        scope,
        container,
        block,
        second,
        file: text(&uploaded.value(), "fileId"),
    }
}

/// Brings `things` to where `op` does something.
async fn prepare(people: &People, op: &Op, things: &Things) {
    let owner = &people.clients["admin"];
    match op.prepare {
        Prepare::Nothing => {}
        Prepare::DeleteContainer => {
            done(
                &owner
                    .delete(&format!("{TEST_WS}/boards/{}", things.container))
                    .await,
            );
        }
        Prepare::DeletePage => {
            let page = format!(
                "{TEST_WS}/wikis/{}/pages/{}",
                things.container, things.block
            );
            done(&owner.delete(&page).await);
        }
    }
}

/// The category of the test workspace that `client` files boards under; an
/// identifier of nothing for who has none there.
async fn category_of(client: &Client) -> String {
    let categories = client.get(&format!("{TEST_WS}/categories")).await;
    categories.value()[0]["id"]
        .as_str()
        .map_or_else(|| UNKNOWN_ID.to_owned(), str::to_owned)
}

/// Asks `op` about `things` as `client`; `category` is that client's.
async fn ask(
    op: &Op,
    client: &Client,
    things: &Things,
    people: &People,
    category: &str,
) -> ApiResponse {
    let path = op
        .path
        .replace("{c}", &things.container)
        .replace("{p}", &things.block)
        .replace("{f}", &things.file)
        .replace("{team}", &people.team)
        .replace("{viewer}", &people.viewer)
        .replace("{commenter}", &people.commenter)
        .replace("{category}", category);
    let request = client.request(op.method.clone(), &format!("{TEST_WS}{path}"));
    Client::send(match op.body {
        Body::None => request,
        Body::Json(body) => request.json(&body(things, people)),
        Body::File => {
            let file = Part::bytes(b"more".to_vec()).file_name("more.txt");
            request.multipart(Form::new().part("file", file))
        }
    })
    .await
}

/// What one answer is in the table: its status, and for the trash whether it lists the
/// container (`+`) or not (`-`).
fn cell(op: &Op, answer: &ApiResponse, things: &Things) -> String {
    let status = answer.status.as_u16();
    if op.row == "trash: list" && answer.status.is_success() {
        let listed = if answer.text().contains(&things.container) {
            "+"
        } else {
            "-"
        };
        return format!("{status}{listed}");
    }
    status.to_string()
}

/// The table of `kind`: for each row, the cells in the order of [`COLUMNS`].
async fn table(people: &People, kind: Kind) -> Vec<(&'static str, Vec<String>)> {
    let own = match kind {
        Kind::Board => board_ops(),
        Kind::Wiki => wiki_ops(),
    };
    let mut categories = BTreeMap::new();
    for (name, who, _) in COLUMNS {
        categories.insert(name, category_of(people.client(who)).await);
    }
    let mut rows = Vec::new();
    for op in shared_ops().into_iter().chain(own) {
        let mut made: BTreeMap<Scope, Things> = BTreeMap::new();
        let mut cells = Vec::new();
        for (name, who, scope) in COLUMNS {
            if let std::collections::btree_map::Entry::Vacant(entry) = made.entry(scope) {
                let things = make(people, kind, scope).await;
                prepare(people, &op, &things).await;
                entry.insert(things);
            }
            let things = &made[&scope];
            let client = people.client(who);
            let answer = ask(&op, client, things, people, &categories[name]).await;
            cells.push(cell(&op, &answer, things));
            if answer.status.is_success() {
                let given: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
                if let Some(upload) = given["uploadId"].as_str() {
                    drop(client.delete(&format!("{TEST_WS}/uploads/{upload}")).await);
                }
                if op.method != Method::GET {
                    made.remove(&scope);
                }
            }
        }
        rows.push((op.row, cells));
    }
    rows
}

/// The table as its file has it.
fn printed(kind: Kind, rows: &[(&str, Vec<String>)]) -> String {
    let mut out = format!(
        "# Status code by operation and person, on a {}. Columns: see role_table.rs.\n",
        match kind {
            Kind::Board => "board",
            Kind::Wiki => "wiki",
        }
    );
    let _ = write!(out, "{:<24}", "operation");
    for (name, _, _) in COLUMNS {
        let _ = write!(out, "{name:>12}");
    }
    out.push('\n');
    for (row, cells) in rows {
        let _ = write!(out, "{row:<24}");
        for cell in cells {
            let _ = write!(out, "{cell:>12}");
        }
        out.push('\n');
    }
    out
}

/// Compares the table with its file, or writes the file when asked to.
fn check_file(kind: Kind, actual: &str) {
    let name = match kind {
        Kind::Board => "board.txt",
        Kind::Wiki => "wiki.txt",
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/api/role_table")
        .join(name);
    if std::env::var("FVOCI_REGRESSION_UPDATE").as_deref() == Ok("1") {
        assert!(std::env::var_os("CI").is_none(), "no recording under CI");
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(&path, actual).expect("the table is written");
        return;
    }
    let pinned = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    if pinned != actual {
        let mut differences = String::new();
        for (want, got) in pinned.lines().zip(actual.lines()) {
            if want != got {
                let _ = writeln!(differences, "- {want}\n+ {got}");
            }
        }
        panic!("the table of {name} differs from its file:\n{differences}");
    }
}

/// The cells of one row of a table, by column.
fn row_of<'t>(rows: &'t [(&str, Vec<String>)], row: &str) -> &'t [String] {
    &rows
        .iter()
        .find(|(name, _)| *name == row)
        .unwrap_or_else(|| panic!("no row {row}"))
        .1
}

fn column(name: &str) -> usize {
    COLUMNS
        .iter()
        .position(|(header, _, _)| *header == name)
        .unwrap_or_else(|| panic!("no column {name}"))
}

/// The answers of both tables are their files, and a wiki answers as a board does
/// where both have the operation, but for who works in the workspace and has not
/// joined an open one: that one reads an open wiki, and writes in neither.
#[tokio::test]
async fn the_wiki_differs_from_the_board_only_where_an_open_wiki_is_read() {
    // Sharing is on, as it is by default on a server: its links are among what is asked.
    let ts = Setup::local_mode().start().await.init_basic().await;
    let people = People::of(&ts).await;
    let board = table(&people, Kind::Board).await;
    let wiki = table(&people, Kind::Wiki).await;
    check_file(Kind::Board, &printed(Kind::Board, &board));
    check_file(Kind::Wiki, &printed(Kind::Wiki, &wiki));

    let unjoined = column(READS_UNJOINED);
    for op in shared_ops() {
        let (of_board, of_wiki) = (row_of(&board, op.row), row_of(&wiki, op.row));
        for (index, (name, _, _)) in COLUMNS.iter().enumerate() {
            if index != unjoined {
                assert_eq!(of_board[index], of_wiki[index], "{}: {name}", op.row);
            }
        }
    }
    // Unjoined, an open wiki is read as by its viewer, and nothing of it is changed;
    // an open board is seen, and read only once joined.
    let reads = [
        "container: get",
        "members: list",
        "teams: list",
        "files: get",
        "files: info",
        "references: to",
        "pages: tree",
        "pages: get",
    ];
    let viewer = column("viewer");
    for row in reads {
        let cells = row_of(&wiki, row);
        assert_eq!(cells[unjoined], cells[viewer], "{row}");
        assert!(
            cells[unjoined].starts_with('2'),
            "{row}: {}",
            cells[unjoined]
        );
    }
    for row in ["members: list", "teams: list", "files: get", "cards: list"] {
        assert_eq!(
            row_of(&board, row)[unjoined],
            "403",
            "an open board unjoined: {row}"
        );
    }
    assert_eq!(row_of(&board, "references: to")[unjoined], "404");
    // What is not a change of the wiki: joining it, leaving it (as nothing), and what
    // the reader keeps of it in their own sidebar and trash.
    let not_a_change = [
        "members: join",
        "members: leave",
        "sidebar: category",
        "sidebar: hide",
        "sidebar: unhide",
        "trash: list",
    ];
    for (row, cells) in &wiki {
        if !reads.contains(row) && !not_a_change.contains(row) {
            assert!(
                !cells[unjoined].starts_with('2'),
                "{row}: {}",
                cells[unjoined]
            );
        }
    }
    // Who is not in the workspace finds nothing there.
    for (name, _, _) in COLUMNS
        .iter()
        .filter(|(name, _, _)| ["org-admin", "other-ws"].contains(name))
    {
        let index = column(name);
        for rows in [&board, &wiki] {
            for (row, cells) in rows {
                assert_eq!(cells[index], "404", "{name}: {row}");
            }
        }
    }
    ts.teardown().await;
}
