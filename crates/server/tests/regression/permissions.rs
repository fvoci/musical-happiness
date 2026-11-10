//! The permission table: the status code of every board-related route for every kind of
//! caller, on a private and on an open board.
//!
//! The same request is sent as six kinds of caller: anonymous, a signed-in user who is no
//! member of the board, and a viewer, commenter, editor and admin of it. Every request
//! gets a board of its own, so that what one caller changes cannot influence the next
//! answer.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

use reqwest::Method;
use serde_json::{Value, json};

use crate::common::ROOT_TEAM_ID;
use crate::fixtures::{PNG, block, id_of, merged, role_flags};
use crate::golden;
use crate::harness::{
    API, Caller, Http, OCTET_STREAM, Payload, id_by_title, multipart, start_server,
};

/// The kinds of caller, in the order of the table's columns.
const ROLES: [&str; 6] = [
    "anonymous",
    "outsider",
    "viewer",
    "commenter",
    "editor",
    "admin",
];

/// The roles a fixture board gives a member each.
const MEMBER_ROLES: [&str; 4] = ["viewer", "commenter", "editor", "admin"];

/// The rows of the table, in order. [`request`] builds the request of each.
const ROUTES: [&str; 53] = [
    "board: get",
    "board: patch title",
    "board: patch properties",
    "board: patch type",
    "board: patch minimum role",
    "board: delete",
    "board: undelete",
    "board: duplicate",
    "board: duplicate as template",
    "board: metadata",
    "members: list",
    "members: add viewer",
    "members: add admin",
    "members: update",
    "members: delete another",
    "members: join",
    "members: leave",
    "blocks: list",
    "blocks: list all",
    "blocks: list by parent",
    "blocks: insert card",
    "blocks: insert view",
    "blocks: insert comment",
    "blocks: patch card",
    "blocks: patch another's comment",
    "blocks: patch several",
    "blocks: delete card",
    "blocks: delete view",
    "blocks: delete another's comment",
    "blocks: undelete",
    "blocks: duplicate",
    "boards-and-blocks: patch",
    "boards-and-blocks: delete",
    "cards: create",
    "cards: list",
    "cards: get",
    "cards: patch",
    "content: move",
    "sharing: get",
    "sharing: post",
    "subscriptions: create",
    "subscriptions: delete",
    "files: upload",
    "files: download",
    "files: info",
    "archive: export board",
    "archive: import",
    "archive: export team",
    "categories: list",
    "team: boards",
    "team: templates",
    "team: search",
    "team: users",
];

/// The users of a permission run: the board owner, one user per role, and `extra`, whom
/// the member routes add.
#[derive(Debug)]
struct Users {
    /// User identifiers by name.
    ids: HashMap<&'static str, String>,
    /// Session tokens by name.
    tokens: HashMap<&'static str, String>,
}

impl Users {
    async fn register(http: &Http) -> Self {
        let mut tokens = HashMap::new();
        tokens.insert("owner", http.register("owner", "").await);
        let signup_token = http.signup_token(&tokens["owner"]).await;
        for name in [
            "outsider",
            "viewer",
            "commenter",
            "editor",
            "admin",
            "extra",
        ] {
            tokens.insert(name, http.register(name, &signup_token).await);
        }
        let mut ids = HashMap::new();
        for (name, token) in &tokens {
            ids.insert(*name, http.user_id(token).await);
        }
        Self { ids, tokens }
    }

    fn caller(&self, name: &str) -> Caller {
        Caller::Token(self.tokens[name].clone())
    }
}

/// A board with one member per role, some blocks, and on demand a file.
#[derive(Debug)]
struct Fixture<'a> {
    http: &'a Http,
    users: &'a Users,
    board: String,
    card: String,
    text: String,
    text2: String,
    view: String,
    comment: String,
}

impl<'a> Fixture<'a> {
    async fn create(http: &'a Http, users: &'a Users, board_type: &str) -> Self {
        let owner = users.caller("owner");
        let board = http
            .api(
                Method::POST,
                &format!("{API}/boards"),
                Payload::Json(json!({
                    "teamId": ROOT_TEAM_ID, "type": board_type, "title": "Matrix",
                    "properties": {}, "cardProperties": [],
                })),
                &owner,
            )
            .await
            .string("id");
        for role in MEMBER_ROLES {
            let member = merged(
                &json!({"boardId": board, "userId": users.ids[role]}),
                role_flags(role),
            );
            let added = http
                .api(
                    Method::POST,
                    &format!("{API}/boards/{board}/members"),
                    Payload::Json(member),
                    &owner,
                )
                .await;
            assert_eq!(added.status, 200, "adding the {role} to the board");
        }
        let blocks = format!("{API}/boards/{board}/blocks");
        let created = http
            .api(
                Method::POST,
                &blocks,
                Payload::Json(json!([
                    block("card", &board, "card", "card", "", json!({})),
                    block("text", &board, "text", "text", "card", json!({})),
                    block("text2", &board, "text", "text2", "card", json!({})),
                    block(
                        "view",
                        &board,
                        "view",
                        "view",
                        "",
                        json!({"viewType": "board"})
                    ),
                ])),
                &owner,
            )
            .await;
        let created = created.json();
        let card = id_by_title(created, "card");
        let comment = http
            .api(
                Method::POST,
                &blocks,
                Payload::Json(json!([block(
                    "comment",
                    &board,
                    "comment",
                    "comment",
                    &card,
                    json!({})
                )])),
                &owner,
            )
            .await;
        Self {
            http,
            users,
            text: id_by_title(created, "text"),
            text2: id_by_title(created, "text2"),
            view: id_by_title(created, "view"),
            comment: id_of(&comment.json()[0]),
            card,
            board,
        }
    }

    /// A request as the board's owner, to arrange what the route under test needs. It
    /// must succeed: a cell recorded after a failed arrangement would pin a status that
    /// has another reason than the one the table is about.
    async fn owner(&self, method: Method, path: &str, payload: Payload) -> crate::harness::Reply {
        let reply = self
            .http
            .api(method.clone(), path, payload, &self.users.caller("owner"))
            .await;
        assert!(
            (200..300).contains(&reply.status),
            "arranging with {method} {path} as the owner answered {}",
            reply.status
        );
        reply
    }

    /// Uploads a file to the board as its owner and returns the file's name.
    async fn upload_file(&self) -> String {
        self.owner(
            Method::POST,
            &format!("{API}/boards/{}/files", self.board),
            multipart("file", "pixel.png", &PNG, "image/png"),
        )
        .await
        .string("fileId")
    }
}

/// The request of one row of the table.
#[derive(Debug)]
struct Request {
    method: Method,
    path: String,
    payload: Payload,
}

impl Request {
    fn new(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            payload: Payload::None,
        }
    }

    fn json(method: Method, path: String, body: Value) -> Self {
        Self {
            method,
            path,
            payload: Payload::Json(body),
        }
    }
}

/// Prepares the fixture for a route and returns the request to send. `me` is the
/// caller's user identifier, empty for the anonymous caller.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per row of the table; splitting it would scatter the table"
)]
async fn request(route: &str, fx: &Fixture<'_>, me: &str) -> Request {
    let board = &fx.board;
    let board_path = format!("{API}/boards/{board}");
    let blocks = format!("{board_path}/blocks");
    let viewer = &fx.users.ids["viewer"];
    let extra = &fx.users.ids["extra"];
    match route {
        "board: get" => Request::new(Method::GET, board_path),
        "board: patch title" => {
            Request::json(Method::PATCH, board_path, json!({"title": "renamed"}))
        }
        "board: patch properties" => Request::json(
            Method::PATCH,
            board_path,
            json!({"updatedProperties": {"a": 1}}),
        ),
        "board: patch type" => Request::json(Method::PATCH, board_path, json!({"type": "O"})),
        "board: patch minimum role" => {
            Request::json(Method::PATCH, board_path, json!({"minimumRole": "editor"}))
        }
        "board: delete" => Request::new(Method::DELETE, board_path),
        "board: undelete" => {
            fx.owner(Method::DELETE, &board_path, Payload::None).await;
            Request::new(Method::POST, format!("{board_path}/undelete"))
        }
        "board: duplicate" => Request::new(Method::POST, format!("{board_path}/duplicate")),
        "board: duplicate as template" => Request::new(
            Method::POST,
            format!("{board_path}/duplicate?asTemplate=true"),
        ),
        "board: metadata" => Request::new(Method::GET, format!("{board_path}/metadata")),
        "members: list" => Request::new(Method::GET, format!("{board_path}/members")),
        "members: add viewer" => Request::json(
            Method::POST,
            format!("{board_path}/members"),
            json!({"boardId": board, "userId": extra, "schemeViewer": true}),
        ),
        "members: add admin" => Request::json(
            Method::POST,
            format!("{board_path}/members"),
            json!({"boardId": board, "userId": extra, "schemeAdmin": true}),
        ),
        "members: update" => Request::json(
            Method::PUT,
            format!("{board_path}/members/{viewer}"),
            json!({"boardId": board, "userId": viewer, "schemeEditor": true}),
        ),
        "members: delete another" => {
            Request::new(Method::DELETE, format!("{board_path}/members/{viewer}"))
        }
        "members: join" => Request::new(Method::POST, format!("{board_path}/join")),
        "members: leave" => Request::new(Method::POST, format!("{board_path}/leave")),
        "blocks: list" => Request::new(Method::GET, blocks),
        "blocks: list all" => Request::new(Method::GET, format!("{blocks}?all=true")),
        "blocks: list by parent" => {
            Request::new(Method::GET, format!("{blocks}?parentId={}", fx.card))
        }
        "blocks: insert card" => Request::json(
            Method::POST,
            blocks,
            json!([block("n", board, "card", "n", "", json!({}))]),
        ),
        "blocks: insert view" => Request::json(
            Method::POST,
            blocks,
            json!([block("n", board, "view", "n", "", json!({}))]),
        ),
        "blocks: insert comment" => Request::json(
            Method::POST,
            blocks,
            json!([block("n", board, "comment", "n", &fx.card, json!({}))]),
        ),
        "blocks: patch card" => Request::json(
            Method::PATCH,
            format!("{blocks}/{}", fx.card),
            json!({"title": "patched"}),
        ),
        "blocks: patch another's comment" => Request::json(
            Method::PATCH,
            format!("{blocks}/{}", fx.comment),
            json!({"title": "patched"}),
        ),
        "blocks: patch several" => Request::json(
            Method::PATCH,
            blocks,
            json!({"blockIds": [fx.card], "blockPatches": [{"title": "p"}]}),
        ),
        "blocks: delete card" => Request::new(Method::DELETE, format!("{blocks}/{}", fx.card)),
        "blocks: delete view" => Request::new(Method::DELETE, format!("{blocks}/{}", fx.view)),
        "blocks: delete another's comment" => {
            Request::new(Method::DELETE, format!("{blocks}/{}", fx.comment))
        }
        "blocks: undelete" => {
            let view = format!("{blocks}/{}", fx.view);
            fx.owner(Method::DELETE, &view, Payload::None).await;
            Request::new(Method::POST, format!("{view}/undelete"))
        }
        "blocks: duplicate" => {
            Request::new(Method::POST, format!("{blocks}/{}/duplicate", fx.card))
        }
        "boards-and-blocks: patch" => Request::json(
            Method::PATCH,
            format!("{API}/boards-and-blocks"),
            json!({
                "boardIds": [board], "boardPatches": [{"title": "p"}],
                "blockIds": [fx.card], "blockPatches": [{"title": "p"}],
            }),
        ),
        "boards-and-blocks: delete" => Request::json(
            Method::DELETE,
            format!("{API}/boards-and-blocks"),
            json!({"boards": [board], "blocks": [fx.card]}),
        ),
        "cards: create" => Request::json(
            Method::POST,
            format!("{board_path}/cards"),
            json!({"title": "new"}),
        ),
        "cards: list" => Request::new(Method::GET, format!("{board_path}/cards")),
        "cards: get" => Request::new(Method::GET, format!("{API}/cards/{}", fx.card)),
        "cards: patch" => Request::json(
            Method::PATCH,
            format!("{API}/cards/{}", fx.card),
            json!({"title": "patched"}),
        ),
        "content: move" => {
            fx.owner(
                Method::PATCH,
                &format!("{API}/cards/{}", fx.card),
                Payload::Json(json!({"contentOrder": [fx.text, fx.text2]})),
            )
            .await;
            Request::new(
                Method::POST,
                format!("{API}/content-blocks/{}/moveto/after/{}", fx.text, fx.text2),
            )
        }
        "sharing: get" => {
            fx.owner(
                Method::POST,
                &format!("{board_path}/sharing"),
                Payload::Json(json!({"enabled": true})),
            )
            .await;
            Request::new(Method::GET, format!("{board_path}/sharing"))
        }
        "sharing: post" => Request::json(
            Method::POST,
            format!("{board_path}/sharing"),
            json!({"enabled": true}),
        ),
        "subscriptions: create" => Request::json(
            Method::POST,
            format!("{API}/subscriptions"),
            subscription(fx, me),
        ),
        "subscriptions: delete" => {
            let subscriber = if me.is_empty() { "nobody" } else { me };
            if !me.is_empty() {
                let (name, _) = fx
                    .users
                    .ids
                    .iter()
                    .find(|(_, id)| id.as_str() == me)
                    .expect("the caller is a registered user");
                let token = fx.users.caller(name);
                let reply = fx
                    .http
                    .api(
                        Method::POST,
                        &format!("{API}/subscriptions"),
                        Payload::Json(subscription(fx, me)),
                        &token,
                    )
                    .await;
                // A member has a subscription to end. Who is no member cannot subscribe,
                // and their cell says what ending a subscription one does not have
                // answers.
                let wanted = if MEMBER_ROLES.contains(name) {
                    200
                } else {
                    404
                };
                assert_eq!(
                    reply.status, wanted,
                    "subscribing as {name} before the subscription is ended"
                );
            }
            Request::new(
                Method::DELETE,
                format!("{API}/subscriptions/{}/{subscriber}", fx.card),
            )
        }
        "files: upload" => Request {
            method: Method::POST,
            path: format!("{API}/boards/{board}/files"),
            payload: multipart("file", "pixel.png", &PNG, "image/png"),
        },
        "files: download" => {
            let file = fx.upload_file().await;
            Request::new(Method::GET, format!("{API}/boards/{board}/files/{file}"))
        }
        "files: info" => {
            let file = fx.upload_file().await;
            Request::new(
                Method::GET,
                format!("{API}/boards/{board}/files/{file}/info"),
            )
        }
        "archive: export board" => {
            Request::new(Method::GET, format!("{board_path}/archive/export"))
        }
        "archive: import" => {
            let exported = fx
                .owner(
                    Method::GET,
                    &format!("{board_path}/archive/export"),
                    Payload::None,
                )
                .await;
            Request {
                method: Method::POST,
                path: format!("{API}/teams/{ROOT_TEAM_ID}/archive/import"),
                payload: multipart("file", "a.boardarchive", exported.bytes(), OCTET_STREAM),
            }
        }
        "archive: export team" => Request::new(
            Method::GET,
            format!("{API}/teams/{ROOT_TEAM_ID}/archive/export"),
        ),
        "categories: list" => Request::new(
            Method::GET,
            format!("{API}/teams/{ROOT_TEAM_ID}/categories"),
        ),
        "team: boards" => Request::new(Method::GET, format!("{API}/teams/{ROOT_TEAM_ID}/boards")),
        "team: templates" => {
            Request::new(Method::GET, format!("{API}/teams/{ROOT_TEAM_ID}/templates"))
        }
        "team: search" => Request::new(
            Method::GET,
            format!("{API}/teams/{ROOT_TEAM_ID}/boards/search?q=Matrix"),
        ),
        "team: users" => Request::new(Method::GET, format!("{API}/teams/{ROOT_TEAM_ID}/users")),
        other => panic!("no request is defined for the route {other:?}"),
    }
}

fn subscription(fx: &Fixture<'_>, subscriber: &str) -> Value {
    json!({
        "blockType": "card", "blockId": fx.card,
        "subscriberType": "user", "subscriberId": subscriber,
    })
}

/// The status codes of one route, in the order of [`ROLES`].
type Row = [u16; ROLES.len()];

/// Records the table for boards of one type (`P` or `O`) on a server of its own and
/// compares it with `golden/<name>.txt`.
pub(crate) async fn record(name: &str, board_type: &str) {
    let server = start_server().await;
    let http = Http::new(&server.base_url);
    let users = Users::register(&http).await;

    let mut rows: Vec<(&str, Row)> = Vec::with_capacity(ROUTES.len());
    for route in ROUTES {
        let mut row = [0; ROLES.len()];
        for (cell, role) in row.iter_mut().zip(ROLES) {
            let fx = Fixture::create(&http, &users, board_type).await;
            let (me, caller) = if role == "anonymous" {
                ("", Caller::Anonymous)
            } else {
                (users.ids[role].as_str(), users.caller(role))
            };
            let request = request(route, &fx, me).await;
            *cell = http
                .api(request.method, &request.path, request.payload, &caller)
                .await
                .status;
        }
        rows.push((route, row));
    }
    server.teardown().await;

    golden::check_table(name, &render(board_type, &rows), |expected| {
        differences(expected, &rows)
    });
}

fn render(board_type: &str, rows: &[(&str, Row)]) -> String {
    let kind = if board_type == "P" { "private" } else { "open" };
    let mut table = format!("# Status code by route and caller, on a board of the {kind} type.\n");
    let _ = write!(table, "{:36}", "route");
    for role in ROLES {
        let _ = write!(table, "{role:>11}");
    }
    table.push('\n');
    for (route, row) in rows {
        let _ = write!(table, "{route:36}");
        for status in row {
            let _ = write!(table, "{status:>11}");
        }
        table.push('\n');
    }
    table
}

/// Reads the rows of a rendered table: the route, then one status code per role.
fn parse(table: &str) -> BTreeMap<String, Vec<String>> {
    table
        .lines()
        .skip(2)
        .filter_map(|line| {
            let mut cells: Vec<&str> = line.split_whitespace().collect();
            if cells.len() <= ROLES.len() {
                return None;
            }
            let codes = cells.split_off(cells.len() - ROLES.len());
            Some((
                cells.join(" "),
                codes.into_iter().map(str::to_owned).collect(),
            ))
        })
        .collect()
}

fn differences(expected: &str, rows: &[(&str, Row)]) -> String {
    let mut expected = parse(expected);
    let mut report = String::new();
    for (route, row) in rows {
        let Some(want) = expected.remove(*route) else {
            let _ = writeln!(report, "  {route}: not in the golden file");
            continue;
        };
        for ((role, want), got) in ROLES.iter().zip(&want).zip(row) {
            if *want != got.to_string() {
                let _ = writeln!(report, "  {route}: {role}: expected {want}, got {got}");
            }
        }
    }
    for route in expected.keys() {
        let _ = writeln!(report, "  {route}: only in the golden file");
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rendered_table_reads_back_and_shows_changed_cells() {
        let rows = [
            ("board: get", [401, 403, 200, 200, 200, 200]),
            (
                "blocks: delete another's comment",
                [401, 403, 403, 403, 403, 200],
            ),
        ];
        let table = render("P", &rows);
        assert_eq!(differences(&table, &rows), "", "a table equals itself");

        let mut changed = rows;
        changed[1].1[4] = 200;
        assert_eq!(
            differences(&table, &changed),
            "  blocks: delete another's comment: editor: expected 403, got 200\n",
            "the changed cell is named by route and role"
        );
    }
}
