//! The isolation of a user's own space: nobody but its owner reaches anything of it,
//! by any way the server has. Who asks is down the side of the table (an administrator
//! of the organization the owner works in, a member of it, a guest of it, an
//! administrator of another organization, somebody without a session, an account that
//! waits to be deleted) and the way is across (the routes of a workspace, the routes
//! that name a thing by its identifier under a workspace of the asker's own, the
//! routes of an organization, a search, an archive, the audit log, the websocket, a
//! public link, the operator's webhooks). The routes come from the contract and from
//! the inventory beside this file, so one that is added is asked too. On SQLite, which
//! has no wall between organizations, every row is run but the other organization's.

use std::time::Duration;

use axum::Router;
use futures_util::{SinkExt as _, StreamExt as _};
use fvoci_model::{
    Block, BlockType, Category, Container, ContainerMember, ContainerType, OrgKind, UserWorkspace,
    Workspace, WorkspaceRole, new_id,
};
use fvoci_server::app::{App, ChangeNotification, FileToUpload};
use fvoci_store::{DbType, SharingChange, Tenant};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use super::common::{
    ApiResponse, Client, PASSWORD, ROOT_WORKSPACE_ID, Setup, TestServer, UNKNOWN_ID, carried_out,
};
use super::fake_search::{FakeSearch, document};
use super::fake_smtp::FakeSmtp;
use super::routes::{
    Asker, Items, WORKSPACE_ROUTES, a_comment_on, a_page_in, archive_entries,
    compare_resource_routes, compare_workspace_routes, fill_with_nothing,
};
use super::{answer, db_type_of_the_tests};

/// A word that is in the title of everything the space holds, and nowhere else.
const SECRET: &str = "ownspacesecret";

/// A user's own space with something of every kind in it, and its owner, who works in
/// the organization of the test server besides.
struct Space {
    owner: Client,
    owner_id: String,
    workspace: UserWorkspace,
    /// The server behind the wall of the space.
    app: App,
    board: Container,
    /// A private board the owner was not on and gave themselves a role on, as the
    /// workspace's administrator: the one record of the space's audit log.
    managed: Container,
    card: Block,
    page: Block,
    comment: Block,
    file: String,
    upload_id: String,
    category: String,
}

impl Space {
    async fn of_a_new_member(ts: &TestServer) -> Self {
        let owner = ts.add_user("space-owner").await;
        let owner_id = owner.me().await.id;
        let listed: Vec<UserWorkspace> = owner.get("/users/me/workspaces").await.ok_json();
        let workspace = listed
            .into_iter()
            .find(|workspace| workspace.kind == OrgKind::Personal)
            .expect("their own space is listed");
        let tenant = Tenant::org(&workspace.org_id).expect("an identifier");
        let app = ts.app.scoped(tenant);
        let store = app.store();
        let mut board = Container {
            id: new_id(),
            workspace_id: workspace.id.clone(),
            container_type: ContainerType::Open,
            title: format!("{SECRET} board"),
            ..Container::default()
        };
        store
            .insert_container_with_admin(&mut board, &owner_id)
            .await
            .expect("a board of the space");
        let mut managed = Container {
            id: new_id(),
            workspace_id: workspace.id.clone(),
            container_type: ContainerType::Private,
            title: format!("{SECRET} managed"),
            ..Container::default()
        };
        store
            .insert_container(&mut managed, &owner_id)
            .await
            .expect("a board of the space that its owner is not on");
        let role = json!({"userId": owner_id, "schemeAdmin": true});
        let path = format!("/w/{}/containers/{}/members", workspace.id, managed.id);
        let given = owner.post(&path, &role).await;
        assert!(given.status.is_success(), "{}", given.text());
        let mut card = Block {
            id: new_id(),
            container_id: board.id.clone(),
            block_type: BlockType::Card,
            title: format!("{SECRET} card"),
            ..Block::default()
        };
        store
            .insert_block(&mut card, &owner_id)
            .await
            .expect("its card");
        let mut data = SECRET.as_bytes();
        let file = app
            .save_file(&mut data, &board.id, "secret.txt")
            .await
            .expect("its file");
        let upload = FileToUpload {
            name: format!("{SECRET}.txt"),
            size: 0,
            content_type: String::new(),
        };
        let upload = app
            .create_upload(&owner_id, &board.id, upload)
            .await
            .expect("an upload of the owner's");
        let category = app
            .create_category(Category {
                name: format!("{SECRET} category"),
                user_id: owner_id.clone(),
                workspace_id: workspace.id.clone(),
                ..Category::default()
            })
            .await
            .expect("a category of the space")
            .id;
        let page = a_page_in(store, &workspace.id, &owner_id).await;
        let comment = a_comment_on(store, &page).await;
        Self {
            owner,
            owner_id,
            workspace,
            app,
            board,
            managed,
            card,
            page,
            comment,
            file,
            upload_id: upload.id,
            category,
        }
    }

    /// What the routes that name a thing are asked about. A space has no people: no
    /// link into it and no group of them is there to name, and its owner, who works
    /// beside the askers, is theirs to see where they work together.
    fn items(&self) -> Items<'_> {
        Items {
            board: &self.board,
            card: &self.card,
            page: &self.page,
            comment: &self.comment,
            file: &self.file,
            upload_id: &self.upload_id,
            category: &self.category,
            user: UNKNOWN_ID,
            invitation: UNKNOWN_ID,
            team: UNKNOWN_ID,
            subscriber: (UNKNOWN_ID, UNKNOWN_ID),
        }
    }

    /// Whether `text` names anything of the space: its word, or an identifier.
    fn named_in(&self, text: &str) -> bool {
        [
            SECRET,
            &self.workspace.id,
            &self.workspace.org_id,
            &self.board.id,
            &self.managed.id,
            &self.card.id,
            &self.page.id,
            &self.page.container_id,
            &self.comment.id,
            &self.file,
            &self.upload_id,
            &self.category,
        ]
        .iter()
        .any(|part| text.contains(*part))
    }
}

/// An account that is a guest of the root workspace and nothing else in the
/// organization.
async fn a_guest(ts: &TestServer) -> Client {
    let guest = ts.add_user("space-guest").await;
    let guest_id = guest.me().await.id;
    let store = ts.app.store();
    for statement in [
        format!("DELETE FROM workspace_members WHERE user_id = '{guest_id}'"),
        format!("DELETE FROM organization_members WHERE user_id = '{guest_id}'"),
    ] {
        store
            .execute_unchecked(&statement)
            .await
            .expect("the account leaves the organization");
    }
    store
        .add_workspace_member(ROOT_WORKSPACE_ID, &guest_id, WorkspaceRole::Guest)
        .await
        .expect("a guest of the root workspace");
    guest
}

/// An account that administers another organization, and the workspace of that one.
async fn an_administrator_elsewhere(ts: &TestServer) -> (Client, String) {
    let client = ts.add_user("admin-elsewhere").await;
    let user_id = client.me().await.id;
    let store = ts.app.store();
    for table in ["workspace_members", "organization_members"] {
        store
            .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{user_id}'"))
            .await
            .expect("the account leaves the first organization");
    }
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let org_id = store
        .create_organization("Elsewhere", &workspace)
        .await
        .expect("another organization");
    let theirs = store.for_org(&org_id).expect("an identifier");
    theirs
        .execute_unchecked(&format!(
            "INSERT INTO organization_members (org_id, user_id, role, create_at) \
             VALUES ('{org_id}', '{user_id}', 'admin', 1)"
        ))
        .await
        .expect("its administrator");
    theirs
        .add_workspace_member(&workspace.id, &user_id, WorkspaceRole::Admin)
        .await
        .expect("who administers its workspace");
    // A board of theirs: the search of bodies asks the search server for who reads one.
    let mut board = Container {
        id: new_id(),
        workspace_id: workspace.id.clone(),
        ..Container::default()
    };
    theirs
        .insert_container_with_admin(&mut board, &user_id)
        .await
        .expect("a board of the other organization");
    (client, workspace.id)
}

/// A body the operation of an organization takes, so that what answers is not the
/// reading of the body.
fn organization_body(operation: &str) -> Value {
    match operation {
        "patchOrganization" | "createOrgWorkspace" | "renameOrgWorkspace" => {
            json!({"name": "renamed"})
        }
        "putOrgRole" => json!({"role": "member"}),
        "putOidcProvider" => {
            json!({"issuer": "https://provider.example", "clientId": "client"})
        }
        _ => json!({}),
    }
}

/// Asks every operation under `/org/{orgId}` about the space and about nothing, and,
/// where the path names a workspace too, about the space's workspace under the
/// organization the asker is in: the answers must be the same, and none a success.
/// The operations come from the contract.
async fn compare_organization_routes(client: &Client, space: &Space, own_org: &str) -> Vec<String> {
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract");
    let mut violations = Vec::new();
    let mut asked = 0;
    for (template, methods) in contract["paths"].as_object().expect("paths") {
        let Some(rest) = template.strip_prefix("/api/v1/org/{orgId}") else {
            continue;
        };
        let rest = rest.replace("{workspaceId}", "{workspace}");
        // The organization and the workspace the path names: of the space, of nothing,
        // and the space's workspace as one of the asker's organization.
        let mut pairs = vec![(
            (space.workspace.org_id.as_str(), space.workspace.id.as_str()),
            (UNKNOWN_ID, UNKNOWN_ID),
        )];
        if rest.contains("{workspace}") {
            pairs.push((
                (own_org, space.workspace.id.as_str()),
                (own_org, UNKNOWN_ID),
            ));
        }
        for (method, operation) in methods.as_object().expect("operations") {
            let Some(operation) = operation["operationId"].as_str() else {
                continue;
            };
            let method: reqwest::Method = method.to_uppercase().parse().expect("an HTTP method");
            for (hidden, absent) in &pairs {
                let ask = async |(org, workspace): (&str, &str)| {
                    let rest = fill_with_nothing(&rest.replace("{workspace}", workspace));
                    let path = format!("/org/{org}{rest}");
                    let body = organization_body(operation);
                    Client::send(client.request(method.clone(), &path).json(&body)).await
                };
                let (hidden, absent) = (ask(*hidden).await, ask(*absent).await);
                asked += 1;
                if hidden.status != StatusCode::NOT_FOUND || answer(&hidden) != answer(&absent) {
                    violations.push(format!(
                        "{operation} ({method} {template}): hidden={:?}, absent={:?}",
                        answer(&hidden),
                        answer(&absent)
                    ));
                }
            }
        }
    }
    if asked < 10 {
        violations.push(format!("the contract was read: {asked} requests"));
    }
    violations
}

/// What the searches, the lists, the references, the archive and the audit log of a
/// workspace the asker works in say of the space: nothing. The space's owner is asked
/// the same under a workspace they share with the others: what is of the space is
/// found in the space, on a database without a wall between organizations as on one
/// with it.
async fn compare_what_is_found(
    client: &Client,
    space: &Space,
    workspace: &str,
    index: &FakeSearch,
) -> Vec<String> {
    let mut violations = Vec::new();
    // The search of bodies asks the search server only for who reads a board of the
    // workspace, and the server here answers every asker with the space's card: what
    // comes back is what the server let through. An answer for which the search
    // server was not asked says nothing, and is told as a failure of the test.
    let path = format!("/w/{workspace}/search?q={SECRET}");
    let asked_before = index.searches().len();
    let found = client.get(&path).await;
    if found.status != StatusCode::OK || space.named_in(&found.text()) {
        violations.push(format!("GET {path}: {}", found.text()));
    }
    if index.searches().len() == asked_before {
        violations.push(format!("GET {path}: the search server was not asked"));
    }
    let paths = [
        format!("/w/{workspace}/boards/search?q={SECRET}"),
        format!("/w/{workspace}/references/search?q={SECRET}"),
        format!("/w/{workspace}/boards"),
        format!("/w/{workspace}/wikis"),
        format!("/w/{workspace}/categories"),
        format!("/w/{workspace}/audit"),
    ];
    for path in paths {
        let found = client.get(&path).await;
        if space.named_in(&found.text()) {
            violations.push(format!("GET {path}: {}", found.text()));
        }
    }
    // A reference names its block by the identifier alone: one of the space resolves
    // to what one of nothing does.
    let path = format!("/w/{workspace}/references/resolve");
    let named = json!([space.card.id, space.page.id, space.board.id]);
    let nothing = json!([UNKNOWN_ID, UNKNOWN_ID, UNKNOWN_ID]);
    let (hidden, absent) = (
        client.post(&path, &named).await,
        client.post(&path, &nothing).await,
    );
    if hidden.text().contains(SECRET) || hidden.status != absent.status {
        violations.push(format!(
            "POST {path}: hidden={:?}, absent={:?}",
            answer(&hidden),
            answer(&absent)
        ));
    }
    let path = format!("/w/{workspace}/archive/export");
    let archive = client.get(&path).await;
    if archive.status == StatusCode::OK {
        for (name, bytes) in archive_entries(&archive.body) {
            if space.named_in(&name) || space.named_in(&String::from_utf8_lossy(&bytes)) {
                violations.push(format!("GET {path}: the entry {name}"));
            }
        }
    }
    violations
}

/// What the lists that are the asker's own, and those of the organization they work
/// in, which its administrators read, say of the space: nothing.
async fn compare_own_lists(client: &Client, space: &Space, own_org: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let paths = [
        "/users/me/workspaces".to_owned(),
        "/users/me/memberships".to_owned(),
        format!("/org/{own_org}"),
        format!("/org/{own_org}/workspaces"),
        format!("/org/{own_org}/members"),
        format!("/org/{own_org}/audit"),
    ];
    for path in paths {
        let found = client.get(&path).await;
        if space.named_in(&found.text()) {
            violations.push(format!("GET {path}: {}", found.text()));
        }
    }
    violations
}

/// Without a session every route of a workspace answers for the space as for nothing,
/// and a public link of another board opens nothing of it.
async fn compare_without_a_session(ts: &TestServer, space: &Space, token: &str) -> Vec<String> {
    let anonymous = ts.anonymous();
    let mut violations = Vec::new();
    for (method, suffix, operation) in WORKSPACE_ROUTES {
        let suffix = fill_with_nothing(suffix);
        let ask = async |workspace: &str| {
            let method = method.parse().expect("an HTTP method");
            let path = format!("/w/{workspace}{suffix}");
            Client::send(anonymous.request(method, &path).json(&json!({}))).await
        };
        let (hidden, absent) = (ask(&space.workspace.id).await, ask(UNKNOWN_ID).await);
        if hidden.status != StatusCode::UNAUTHORIZED || answer(&hidden) != answer(&absent) {
            violations.push(format!(
                "{operation} without a session: hidden={:?}, absent={:?}",
                answer(&hidden),
                answer(&absent)
            ));
        }
    }
    for suffix in ["", "/blocks", "/cards", "/pages"] {
        let ask = async |container: &str| {
            let path = format!("/shared/{container}{suffix}?readToken={token}");
            anonymous.get(&path).await
        };
        for hidden in [&space.board.id, &space.page.container_id] {
            let (hidden, absent) = (ask(hidden).await, ask(UNKNOWN_ID).await);
            if hidden.status != StatusCode::NOT_FOUND || answer(&hidden) != answer(&absent) {
                violations.push(format!(
                    "/shared{suffix} with the link of another board: hidden={:?}, absent={:?}",
                    answer(&hidden),
                    answer(&absent)
                ));
            }
        }
    }
    violations
}

/// The answers of a session whose account waits to be deleted: for the space as for
/// nothing, and no success.
async fn compare_for_who_waits(client: &Client, space: &Space) -> Vec<String> {
    let mut violations = Vec::new();
    for suffix in ["", "/boards", "/archive/export", "/audit"] {
        let hidden = client
            .get(&format!("/w/{}{suffix}", space.workspace.id))
            .await;
        let absent = client.get(&format!("/w/{UNKNOWN_ID}{suffix}")).await;
        if hidden.status != StatusCode::UNAUTHORIZED || answer(&hidden) != answer(&absent) {
            violations.push(format!(
                "GET {suffix} as an account that waits: hidden={:?}, absent={:?}",
                answer(&hidden),
                answer(&absent)
            ));
        }
    }
    violations
}

/// Everything one asker is asked through the routes.
async fn compare_routes(
    (who, asker): (&str, Asker),
    client: &Client,
    (space, index): (&Space, &FakeSearch),
    workspace: &str,
) -> Vec<String> {
    let user_id = client.me().await.id;
    let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    let own_org = listed
        .iter()
        .find(|workspace| workspace.kind == OrgKind::Team)
        .map_or(UNKNOWN_ID, |workspace| &workspace.org_id);
    let hidden = &space.workspace.id;
    let mut violations = compare_workspace_routes(client, &user_id, asker, hidden).await;
    violations.extend(compare_resource_routes(client, workspace, &space.items()).await);
    violations.extend(compare_organization_routes(client, space, own_org).await);
    violations.extend(compare_what_is_found(client, space, workspace, index).await);
    violations.extend(compare_own_lists(client, space, own_org).await);
    violations
        .into_iter()
        .map(|violation| format!("{who}: {violation}"))
        .collect()
}

/// The space's owner reaches what the others are asked about: the table would hold of
/// a space that is not there too.
async fn check_the_owner_reaches_it(space: &Space) {
    let here = format!("/w/{}", space.workspace.id);
    space.owner.get(&here).await.check_ok();
    let listed: Vec<UserWorkspace> = space.owner.get("/users/me/workspaces").await.ok_json();
    assert!(
        listed.contains(&space.workspace),
        "the space is what it was: {listed:?}"
    );
    let boards: Vec<Container> = space.owner.get(&format!("{here}/boards")).await.ok_json();
    assert!(
        boards.iter().any(|board| board.id == space.board.id),
        "the owner lists the board: {boards:?}"
    );
    let found = space
        .owner
        .get(&format!("{here}/boards/search?q={SECRET}"))
        .await;
    found.check_ok();
    assert!(found.text().contains(&space.board.id), "{}", found.text());
    let archive = space.owner.get(&format!("{here}/archive/export")).await;
    archive.check_ok();
    assert!(
        archive_entries(&archive.body)
            .iter()
            .any(|(_, bytes)| String::from_utf8_lossy(bytes).contains(SECRET)),
        "the owner's archive has the board"
    );
    let file = format!("{here}/containers/{}/files/{}", space.board.id, space.file);
    space.owner.get(&file).await.check_ok();
    let audit = space.owner.get(&format!("{here}/audit")).await;
    audit.check_ok();
    assert!(
        audit.text().contains(&space.managed.id),
        "the space's audit log has its record: {}",
        audit.text()
    );
    let found = space.owner.get(&format!("{here}/search?q={SECRET}")).await;
    found.check_ok();
    assert!(
        found.text().contains(&space.card.id),
        "the owner's search of bodies finds the card: {}",
        found.text()
    );
    let named = json!([space.card.id, space.page.id]);
    let resolved = space
        .owner
        .post(&format!("{here}/references/resolve"), &named)
        .await;
    resolved.check_ok();
    assert!(
        resolved.text().contains(SECRET) && resolved.text().contains(&space.card.id),
        "the owner's references resolve in the space: {}",
        resolved.text()
    );
}

/// Nobody but its owner reaches anything of a space through a route: every operation
/// of a workspace, every operation that names a thing under a workspace of the
/// asker's, every operation of an organization, and what the asker's own searches,
/// lists, archive and audit log say. The owner too finds nothing of it under another
/// workspace they work in.
#[tokio::test]
async fn nobody_but_its_owner_reaches_a_space_by_a_route() {
    // A search server that answers every search with what was planted in it, whoever
    // asks: what a search answers is what the server lets through.
    let index = FakeSearch::start().await;
    let ts = Setup::local_mode().search(index.settings()).start().await;
    fvoci_server::search::tend(&ts.app).await;
    let ts = ts.init_basic().await;
    let space = Space::of_a_new_member(&ts).await;
    let planted = document(
        &space.workspace.org_id,
        &space.workspace.id,
        &space.board.id,
        &space.card.title,
        "",
    );
    index.plant(&space.card.id, planted);
    check_the_owner_reaches_it(&space).await;
    let root = ROOT_WORKSPACE_ID;

    let guest = a_guest(&ts).await;
    // A board under the root workspace that everybody asked there reads: without one
    // their search of bodies is answered before the search server is asked.
    let read_by_all = ts.create_board(root, ContainerType::Private).await;
    for client in [&ts.client2, &guest, &space.owner] {
        let member = ContainerMember {
            container_id: read_by_all.id.clone(),
            user_id: client.me().await.id,
            scheme_viewer: true,
            ..ContainerMember::default()
        };
        let saved = ts.app.store().save_member(&member).await;
        saved.expect("a viewer of the board");
    }
    let mut violations = Vec::new();
    for (who, asker, client) in [
        (
            "an administrator of the organization",
            Asker::Outsider,
            &ts.client,
        ),
        ("a member of the organization", Asker::Outsider, &ts.client2),
        ("a guest of the organization", Asker::GuestElsewhere, &guest),
    ] {
        violations.extend(compare_routes((who, asker), client, (&space, &index), root).await);
    }
    // The owner under the workspace they share with the others: what is of the space
    // is not there, on a database without a wall as on one with.
    let owner = &space.owner;
    let under_root = [
        compare_resource_routes(owner, root, &space.items()).await,
        compare_what_is_found(owner, &space, root, &index).await,
    ];
    for violation in under_root.into_iter().flatten() {
        violations.push(format!("the owner under another workspace: {violation}"));
    }

    let shared = ts.create_board(root, ContainerType::Private).await;
    let token = ts.share(&shared.id).await;
    violations.extend(compare_without_a_session(&ts, &space, &token).await);

    let waiting = ts.add_user("space-leaver").await;
    waiting
        .post(
            "/users/me/deletion",
            &json!({"username": "space-leaver", "password": PASSWORD}),
        )
        .await
        .check_ok();
    violations.extend(compare_for_who_waits(&waiting, &space).await);

    if db_type_of_the_tests() == DbType::Postgres {
        let (elsewhere, workspace) = an_administrator_elsewhere(&ts).await;
        let who = ("an administrator of another organization", Asker::Outsider);
        violations.extend(compare_routes(who, &elsewhere, (&space, &index), &workspace).await);
    }
    check_the_owner_reaches_it(&space).await;
    ts.teardown().await;
    assert!(
        violations.is_empty(),
        "something of a space is reached by somebody else:\n{}",
        violations.join("\n")
    );
}

/// What a socket is sent within a moment, as text.
async fn heard(socket: &mut super::common::WebSocket) -> Vec<String> {
    let mut messages = Vec::new();
    while let Ok(Some(Ok(message))) =
        tokio::time::timeout(Duration::from_millis(300), socket.next()).await
    {
        if let Message::Text(text) = message {
            messages.push(text.to_string());
        }
    }
    messages
}

/// A change in a space is told to its owner's connection and to no other: not to an
/// administrator of the organization who asks to follow the space's workspace or its
/// card, with or without the link of another board, and not to who asks without a
/// session.
#[tokio::test]
async fn a_change_in_a_space_is_told_to_its_owner_alone() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let space = Space::of_a_new_member(&ts).await;
    let shared = ts
        .create_board(ROOT_WORKSPACE_ID, ContainerType::Private)
        .await;
    let token = ts.share(&shared.id).await;
    let workspace = &space.workspace.id;

    let mut owner = space.owner.connect_websocket().await;
    let mut sockets = [
        ts.client.connect_websocket().await,
        ts.client2.connect_websocket().await,
        ts.anonymous().as_browser().connect_websocket().await,
    ];
    let follow = [
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": workspace}),
        json!({"action": "SUBSCRIBE_BLOCKS", "workspaceId": workspace, "blockIds": [space.card.id]}),
        json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": workspace,
            "blockIds": [space.card.id], "readToken": token,
        }),
        json!({
            "action": "SUBSCRIBE_BLOCKS", "workspaceId": ROOT_WORKSPACE_ID,
            "blockIds": [space.card.id], "readToken": token,
        }),
        json!({"action": "SUBSCRIBE_WORKSPACE", "workspaceId": ROOT_WORKSPACE_ID}),
    ];
    for socket in sockets.iter_mut().chain([&mut owner]) {
        for command in &follow {
            socket
                .send(Message::Text(command.to_string().into()))
                .await
                .expect("the command is sent");
        }
        carried_out(space.app.ws(), socket).await;
    }
    for socket in sockets.iter_mut().chain([&mut owner]) {
        // What the commands were answered with is not what the change is told with.
        heard(socket).await;
    }

    let mut card = Block {
        id: new_id(),
        container_id: space.board.id.clone(),
        block_type: BlockType::Card,
        title: format!("{SECRET} told"),
        ..Block::default()
    };
    space
        .app
        .insert_block(&mut card, &space.owner_id, ChangeNotification::Suppress)
        .await
        .expect("the owner's change");
    assert!(
        space.app.finish_background(Duration::from_secs(5)).await,
        "the change is told"
    );
    let told = heard(&mut owner).await;
    assert!(
        told.iter().any(|message| message.contains(&card.id)),
        "the owner's connection is told: {told:?}"
    );
    for (index, socket) in sockets.iter_mut().enumerate() {
        let told = heard(socket).await;
        assert!(
            !told.iter().any(|message| space.named_in(message)),
            "connection {index} is told of the space: {told:?}"
        );
    }
    ts.teardown().await;
}

async fn receive(
    axum::extract::State(sender): axum::extract::State<mpsc::Sender<Value>>,
    axum::Json(block): axum::Json<Value>,
) -> axum::http::StatusCode {
    sender.send(block).await.expect("the test receiver is open");
    axum::http::StatusCode::NO_CONTENT
}

/// No mail names anything of a space. The mail the server sends is of invitations, of
/// passwords, of addresses and of the operator's, and none of it carries what a
/// workspace holds: with a space filled, the invitation an administrator of the
/// organization mails and the mail that recovers the owner's password arrive and name
/// nothing of the space, and the space itself mails nobody an invitation.
#[tokio::test]
async fn no_mail_names_anything_of_a_space() {
    let smtp = FakeSmtp::start_on(0).await;
    let ts = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let space = Space::of_a_new_member(&ts).await;
    let invitations = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link = ts
        .client
        .post(&invitations, &json!({"role": "member"}))
        .await;
    let token = link.value()["token"]
        .as_str()
        .expect("the invitation's token")
        .to_owned();
    let mailed = json!({"token": token, "recipients": ["newcomer@sample.com"]});
    ts.client
        .post(&format!("{invitations}/mail"), &mailed)
        .await
        .check_no_content();
    // The address `Space::of_a_new_member` registers its owner with.
    let owner = "space-owner@sample.com";
    ts.anonymous()
        .post("/password-reset/request", &json!({"email": owner}))
        .await
        .check_no_content();

    // From the space nobody is invited, by a link of its own or by another's.
    let own = format!("/w/{}/invitations", space.workspace.id);
    let made = space.owner.post(&own, &json!({"role": "member"})).await;
    made.check_not_found();
    let sent = space.owner.post(&format!("{own}/mail"), &mailed).await;
    sent.check_not_found();

    // Both have arrived, and whatever else was sent by the time nothing is under way.
    smtp.received(2).await;
    assert!(ts.app.finish_background(Duration::from_secs(5)).await);
    let letters = smtp.messages().await;
    let mut recipients: Vec<&str> = letters
        .iter()
        .map(|letter| letter.recipient.as_str())
        .collect();
    recipients.sort_unstable();
    assert_eq!(recipients, ["newcomer@sample.com", owner]);
    for letter in &letters {
        assert!(
            !space.named_in(&letter.decoded) && !space.named_in(&letter.message),
            "mail to {} names the space: {}",
            letter.recipient,
            letter.decoded
        );
    }
    smtp.stop().await;
}

/// The server operator's webhooks are posted the changes of the organizations people
/// work in, and nothing of a space: a card made there, changed and deleted is posted
/// nowhere, and the card made in the organization after it is the first the receiver
/// gets.
#[tokio::test]
async fn nothing_of_a_space_is_posted_to_the_operators_webhooks() {
    let ts = Setup::native().start().await.init_basic().await;
    let space = Space::of_a_new_member(&ts).await;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a receiver");
    let address = listener.local_addr().expect("its address");
    let (sender, mut received) = mpsc::channel(16);
    let router = Router::new()
        .route("/hook", axum::routing::post(receive))
        .with_state(sender);
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .expect("the receiver serves");
    });
    let mut config = ts.app.config().clone();
    config.block_change_webhooks = vec![format!("http://{address}/hook")];
    let hooked = App::new(config, ts.app.store().clone(), ts.app.files().clone())
        .expect("a server with the hook");
    let in_space = hooked.scoped(space.app.store().tenant());

    let mut secret = Block {
        id: new_id(),
        container_id: space.board.id.clone(),
        block_type: BlockType::Card,
        title: format!("{SECRET} posted"),
        ..Block::default()
    };
    in_space
        .insert_block(&mut secret, &space.owner_id, ChangeNotification::Suppress)
        .await
        .expect("a card of the space");
    let patch = fvoci_model::BlockPatch {
        title: Some(format!("{SECRET} changed")),
        ..fvoci_model::BlockPatch::default()
    };
    in_space
        .patch_block(
            &secret.id,
            &patch,
            &space.owner_id,
            ChangeNotification::Suppress,
        )
        .await
        .expect("the card is changed");
    in_space
        .delete_block(&secret.id, &space.owner_id, ChangeNotification::Suppress)
        .await
        .expect("the card is deleted");
    assert!(in_space.finish_background(Duration::from_secs(5)).await);

    let board = ts
        .create_board(ROOT_WORKSPACE_ID, ContainerType::Private)
        .await;
    let mut open = Block {
        id: new_id(),
        container_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "posted".to_owned(),
        ..Block::default()
    };
    hooked
        .insert_block(
            &mut open,
            &ts.client.me().await.id,
            ChangeNotification::Suppress,
        )
        .await
        .expect("a card of the organization");
    assert!(hooked.finish_background(Duration::from_secs(5)).await);
    let first = tokio::time::timeout(Duration::from_secs(5), received.recv())
        .await
        .expect("a webhook arrives")
        .expect("the receiver is open");
    assert_eq!(
        first["id"],
        json!(open.id),
        "the organization's card: {first}"
    );
    let more = tokio::time::timeout(Duration::from_millis(300), received.recv()).await;
    assert!(more.is_err(), "nothing else was posted: {more:?}");
    let _ = stop.send(());
    serving.await.expect("the receiver stops");
    ts.teardown().await;
}

/// While the account of a space waits to be deleted, the public links of the space
/// open nothing, as links that are not there, and who followed a board by one is
/// told no more of it; the links open again if the account takes the deletion back.
#[tokio::test]
async fn the_links_of_a_space_are_closed_while_its_account_waits() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let space = Space::of_a_new_member(&ts).await;
    let token = space
        .app
        .set_sharing(&space.board.id, SharingChange::On, &space.owner_id)
        .await
        .expect("the owner shares the board")
        .token;
    let link = format!("/shared/{}?readToken={token}", space.board.id);
    let cards = format!("/shared/{}/cards?readToken={token}", space.board.id);
    let nothing = format!("/shared/{UNKNOWN_ID}?readToken={token}");
    ts.anonymous().get(&link).await.check_ok();
    ts.anonymous().get(&cards).await.check_ok();

    let mut reader = ts.anonymous().as_browser().connect_websocket().await;
    let follow = json!({
        "action": "SUBSCRIBE_BLOCKS", "workspaceId": space.workspace.id,
        "blockIds": [space.card.id], "readToken": token,
    });
    reader
        .send(Message::Text(follow.to_string().into()))
        .await
        .expect("the reader follows the card");
    carried_out(space.app.ws(), &mut reader).await;
    space
        .app
        .ws()
        .broadcast_block_change(&space.workspace.id, &space.card)
        .await;
    let told = heard(&mut reader).await;
    assert!(
        told.iter().any(|message| message.contains(&space.card.id)),
        "a reader of the link is told of the card: {told:?}"
    );

    space
        .owner
        .post(
            "/users/me/deletion",
            &json!({"username": "space-owner", "password": PASSWORD}),
        )
        .await
        .check_ok();
    let absent = ts.anonymous().get(&nothing).await;
    for path in [&link, &cards] {
        let closed: ApiResponse = ts.anonymous().get(path).await;
        closed.check_not_found();
        assert_eq!(
            answer(&closed),
            answer(&absent),
            "as a link that is not there"
        );
    }
    heard(&mut reader).await;
    space
        .app
        .ws()
        .broadcast_block_change(&space.workspace.id, &space.card)
        .await;
    let told = heard(&mut reader).await;
    assert!(
        !told.iter().any(|message| message.contains(&space.card.id)),
        "the reader is told no more: {told:?}"
    );

    // The account takes the deletion back by signing in: the link opens as before.
    let mut back = ts.anonymous();
    let signed_in = back.login("space-owner", PASSWORD).await;
    assert!(signed_in.status.is_success(), "{}", signed_in.text());
    back.delete("/users/me/deletion").await.check_no_content();
    ts.anonymous().get(&link).await.check_ok();
    ts.anonymous().get(&cards).await.check_ok();
    ts.teardown().await;
}
