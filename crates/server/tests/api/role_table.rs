//! The table of roles: what every operation on one board or wiki, its people, its
//! sharing, its files, its place in the sidebar and the trash, and a page's comments and
//! versions, answers to every kind of person, a board's and a wiki's each pinned in a
//! file of `role_table/`.
//!
//! The people are the roles on a container for the invited only (admin, editor,
//! commenter, viewer), a member of the workspace without a role on it and on an open
//! one, a guest invited to it and one on an open one, the workspace's administrator
//! who only manages it, the organization's administrator who is not in the workspace,
//! and a member of another workspace on an open one, who asks under the workspace they
//! work in. The two who are not in the workspace are answered exactly as for nothing:
//! their cells say so (`=`), compared with the same question about identifiers of
//! nothing. Another organization is asked with real identifiers in the comparison of
//! two organizations (`organization_boundary_round2`, PostgreSQL only). Every answer is
//! asked of a container as it was made: one that an answer changed is made anew.
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
    ApiResponse, Client, ROOT_WS, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer, UNKNOWN_ID,
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
    /// The comment on its page is in the trash.
    DeleteComment,
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
        op(
            "container: change type",
            Method::PATCH,
            "/boards/{c}",
            Json(|things, _| {
                let other = match things.scope {
                    Scope::Invited => Scope::Open,
                    Scope::Open => Scope::Invited,
                };
                json!({"type": other.wire()})
            }),
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
        op(
            "references: resolve",
            Method::POST,
            "/references/resolve",
            Json(|things, _| json!([things.block])),
        ),
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
        op(
            "board: duplicate",
            Method::POST,
            "/boards/{c}/duplicate",
            Json(|_, _| json!({})),
        ),
        op(
            "board: export",
            Method::GET,
            "/boards/{c}/archive/export",
            None,
        ),
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

/// The operations of a page's comments and of the versions of its body.
fn page_ops() -> Vec<Op> {
    use Body::{Json, None};
    const COMMENTS: &str = "/wikis/{c}/pages/{p}/comments";
    const COMMENT: &str = "/wikis/{c}/pages/{p}/comments/{comment}";
    const VERSIONS: &str = "/wikis/{c}/pages/{p}/body-versions";
    vec![
        op("page comments: list", Method::GET, COMMENTS, None),
        op(
            "page comments: write",
            Method::POST,
            COMMENTS,
            Json(|_, _| json!({"body": said("mine")})),
        ),
        op(
            "page comments: versions",
            Method::GET,
            "/wikis/{c}/pages/{p}/comments/{comment}/versions",
            None,
        ),
        op(
            "page comments: change",
            Method::PATCH,
            COMMENT,
            Json(|things, _| json!({"body": said("changed"), "readAt": things.comment_at})),
        ),
        op("page comments: delete", Method::DELETE, COMMENT, None),
        Op {
            prepare: Prepare::DeleteComment,
            ..op(
                "page comments: undelete",
                Method::POST,
                "/wikis/{c}/pages/{p}/comments/{comment}/undelete",
                Json(|_, _| json!({})),
            )
        },
        op("page versions: list", Method::GET, VERSIONS, None),
        op(
            "page versions: get",
            Method::GET,
            "/wikis/{c}/pages/{p}/body-versions/{version}",
            None,
        ),
        op(
            "page versions: restore",
            Method::POST,
            "/wikis/{c}/pages/{p}/body-versions/{version}/restore",
            None,
        ),
    ]
}

/// A body of one paragraph that says `text`.
fn said(text: &str) -> Value {
    json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": text}
    ]}]})
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
    /// The admin's comment on the page, and when it was written; empty for a board.
    comment: String,
    comment_at: Value,
    /// A version of the page's body, written twice; empty for a board.
    version: String,
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
/// a file, sharing on, a card (with a comment), or two pages, the first written twice
/// and commented on.
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
    let file = text(&uploaded.value(), "fileId");
    match kind {
        Kind::Board => {
            let card = owner
                .post(&format!("{at}/cards"), &json!({"title": "a card"}))
                .await;
            done(&card);
            Things {
                scope,
                container,
                block: text(&card.value(), "id"),
                second: String::new(),
                file,
                comment: String::new(),
                comment_at: Value::Null,
                version: String::new(),
            }
        }
        Kind::Wiki => {
            let pages = format!("{TEST_WS}/wikis/{container}/pages");
            let page = owner.post(&pages, &json!({"title": "a page"})).await;
            let second = owner.post(&pages, &json!({"title": "another"})).await;
            done(&page);
            done(&second);
            let block = text(&page.value(), "id");
            let at = format!("{pages}/{block}");
            for body in ["first", "second"] {
                let read = owner.get(&at).await.value();
                let change = json!({"readAt": read["updateAt"], "body": said(body)});
                done(&owner.patch(&at, &change).await);
            }
            let versions = owner.get(&format!("{at}/body-versions")).await.value();
            let comment = owner
                .post(&format!("{at}/comments"), &json!({"body": said("theirs")}))
                .await;
            done(&comment);
            let comment = comment.value();
            Things {
                scope,
                container,
                block,
                second: text(&second.value(), "id"),
                file,
                comment: text(&comment, "id"),
                comment_at: comment["updateAt"].clone(),
                version: versions[0]["version"].to_string(),
            }
        }
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
        Prepare::DeleteComment => {
            let comment = format!(
                "{TEST_WS}/wikis/{}/pages/{}/comments/{}",
                things.container, things.block, things.comment
            );
            done(&owner.delete(&comment).await);
        }
    }
}

/// The category of the workspace at `at` that `client` files boards under; an
/// identifier of nothing for who has none there.
async fn category_of(client: &Client, at: &str) -> String {
    let categories = client.get(&format!("{at}/categories")).await;
    categories.value()[0]["id"]
        .as_str()
        .map_or_else(|| UNKNOWN_ID.to_owned(), str::to_owned)
}

/// Asks `op` about `things` as `client`; `category` is that client's.
async fn ask(
    op: &Op,
    client: &Client,
    at: &str,
    things: &Things,
    people: &People,
    category: &str,
) -> ApiResponse {
    let path = op
        .path
        .replace("{c}", &things.container)
        .replace("{p}", &things.block)
        .replace("{f}", &things.file)
        .replace("{comment}", &things.comment)
        .replace("{version}", &things.version)
        .replace("{team}", &people.team)
        .replace("{viewer}", &people.viewer)
        .replace("{commenter}", &people.commenter)
        .replace("{category}", category);
    let request = client.request(op.method.clone(), &format!("{at}{path}"));
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

/// What one answer is in the table: its status; for the trash whether it lists the
/// container (`+`) or not (`-`), and for references whether they name the block; and
/// `=` after an answer that is to the letter the one about identifiers of nothing.
fn cell(op: &Op, answer: &ApiResponse, things: &Things, nothing: Option<&ApiResponse>) -> String {
    let status = answer.status.as_u16();
    let named = match op.row {
        "trash: list" => Some(&things.container),
        "references: resolve" => Some(&things.block),
        _ => None,
    };
    if let Some(named) = named.filter(|_| answer.status.is_success()) {
        let listed = if answer.text().contains(named.as_str()) {
            "+"
        } else {
            "-"
        };
        return format!("{status}{listed}");
    }
    match nothing {
        Some(nothing)
            if nothing.status == answer.status && nothing.problem() == answer.problem() =>
        {
            format!("{status}=")
        }
        _ => status.to_string(),
    }
}

/// Identifiers of nothing, in place of what is made for a question.
fn nothing(scope: Scope) -> Things {
    Things {
        scope,
        container: UNKNOWN_ID.to_owned(),
        block: UNKNOWN_ID.to_owned(),
        second: UNKNOWN_ID.to_owned(),
        file: "missing-file".to_owned(),
        comment: UNKNOWN_ID.to_owned(),
        comment_at: json!(1),
        version: "1".to_owned(),
    }
}

/// The workspace a column asks under: who works in another asks under theirs.
fn place_of(who: Who) -> &'static str {
    if who == Who::OtherWorkspace {
        ROOT_WS
    } else {
        TEST_WS
    }
}

/// The table of `kind`: for each row, the cells in the order of [`COLUMNS`].
async fn table(people: &People, kind: Kind) -> Vec<(&'static str, Vec<String>)> {
    let own = match kind {
        Kind::Board => board_ops(),
        Kind::Wiki => {
            let mut ops = wiki_ops();
            ops.extend(page_ops());
            ops
        }
    };
    let mut categories = BTreeMap::new();
    for (name, who, _) in COLUMNS {
        categories.insert(name, category_of(people.client(who), place_of(who)).await);
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
            let (at, category) = (place_of(who), &categories[name]);
            let answer = ask(&op, client, at, things, people, category).await;
            // Who is not in the workspace is compared with a question about nothing.
            let of_nothing = if matches!(who, Who::OtherWorkspace | Who::OrgAdmin) {
                Some(ask(&op, client, at, &nothing(scope), people, category).await)
            } else {
                None
            };
            cells.push(cell(&op, &answer, things, of_nothing.as_ref()));
            if answer.status.is_success() {
                let given: Value = serde_json::from_slice(&answer.body).unwrap_or(Value::Null);
                if let Some(upload) = given["uploadId"].as_str() {
                    done(&client.delete(&format!("{at}/uploads/{upload}")).await);
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
        "references: resolve",
        "pages: tree",
        "pages: get",
        "page comments: list",
        "page comments: versions",
        "page versions: list",
        "page versions: get",
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
    // Nor does who has not joined it write on its pages: a comment, a change or the
    // removal of the admin's, bringing that back, or a version of the body.
    for row in [
        "page comments: write",
        "page comments: change",
        "page comments: delete",
        "page comments: undelete",
        "page versions: restore",
    ] {
        assert_eq!(
            row_of(&wiki, row)[unjoined],
            "403",
            "an open wiki unjoined: {row}"
        );
    }
    // What is not a change of the wiki: joining it, leaving it (who reads it may leave
    // it, and leaving a container one is no member of changes nothing, 204), and what
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
                // What lists of their own answer has nothing of it (`-`); anything else
                // is to the letter what is answered about nothing.
                let cell = cells[index].as_str();
                assert!(cell == "404=" || cell == "200-", "{name}: {row}: {cell}");
            }
        }
    }
    ts.teardown().await;
}
