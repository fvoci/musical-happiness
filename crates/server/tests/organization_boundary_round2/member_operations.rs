//! Every operation of the contract asked by a plain member (no administrator of the
//! workspace or of the organization) of the first organization and of one made later by
//! signing up: each is answered with the same status in both.

use std::collections::BTreeMap;

use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use super::common::{Client, PASSWORD, Setup, TestServer};
use super::{DbType, OrgRole, WorkspaceRole, db_type_of_the_tests, open};

/// What the sweep names in one organization, made by its founder, and the plain member
/// who asks.
struct Named {
    member: Client,
    member_id: String,
    org: String,
    workspace: String,
    board: String,
    card: String,
    file: String,
    upload: String,
    category: String,
    invitation: String,
    invitation_id: String,
}

/// The operations the contract publishes: method, path and identifier, in its order.
fn operations() -> Vec<(String, String, String)> {
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract is JSON");
    let mut operations = Vec::new();
    for (template, methods) in contract["paths"].as_object().expect("paths") {
        for (method, operation) in methods.as_object().expect("operations") {
            if let Some(id) = operation["operationId"].as_str() {
                operations.push((method.to_uppercase(), template.clone(), id.to_owned()));
            }
        }
    }
    // What ends the member's session goes last, so that it asks everything else.
    let last = |id: &str| matches!(id, "logout" | "changePassword");
    operations.sort_by_key(|(_, _, id)| last(id));
    operations
}

impl Named {
    fn path(&self, template: &str) -> String {
        template
            .trim_start_matches("/api/v1")
            .replace("{workspaceId}", &self.workspace)
            .replace("{orgId}", &self.org)
            .replace("{boardId}", &self.board)
            .replace("{cardId}", &self.card)
            .replace("{blockId}", &self.card)
            .replace("{fileId}", &self.file)
            .replace("{uploadId}", &self.upload)
            .replace("{userId}", &self.member_id)
            .replace("{subscriberId}", &self.member_id)
            .replace("{categoryId}", &self.category)
            .replace("{invitationId}", &self.invitation_id)
            .replace("{partNumber}", "1")
    }

    fn body(&self, operation: &str) -> Value {
        match operation {
            "getWorkspaceUsersById" => json!([self.member_id]),
            "createBlocks" | "reorderCardProperties" => json!([]),
            "importArchive" => json!({ "uploadId": self.upload }),
            "previewInvitation" => json!({ "token": self.invitation }),
            "patchMe" => json!({ "displayName": "a member" }),
            _ => json!({}),
        }
    }

    /// Asks every operation as the member: the status of each, by its identifier.
    async fn sweep(&self) -> BTreeMap<String, u16> {
        let mut answers = BTreeMap::new();
        for (method, template, operation) in operations() {
            let path = self.path(&template);
            assert!(
                !path.contains('{'),
                "unfilled parameter of {operation}: {path}"
            );
            let request = self
                .member
                .request(method.parse().expect("a method"), &path)
                .json(&self.body(&operation));
            let response = Client::send(request).await;
            answers.insert(operation, response.status.as_u16());
        }
        answers
    }
}

/// Signs up `name` with an organization of that name, and makes, as its founder, what the sweep
/// names; then lets a plain member in by a link for members.
async fn organization(server: &TestServer, name: &str) -> Named {
    let founder = sign_up(server, name).await;
    let workspaces = founder.get("/users/me/workspaces").await.value();
    let workspace = text(&workspaces[0], "id");
    let org = text(&workspaces[0], "orgId");
    let at = format!("/w/{workspace}");
    let board = founder
        .post(
            &format!("{at}/boards"),
            &json!({"title": format!("{name} board"), "type": "O"}),
        )
        .await
        .value();
    let board = text(&board, "id");
    let card = founder
        .post(
            &format!("{at}/boards/{board}/cards"),
            &json!({"title": format!("{name} card")}),
        )
        .await
        .value();
    let file = Client::send(
        founder
            .request(Method::POST, &format!("{at}/boards/{board}/files"))
            .multipart(Form::new().part("file", Part::bytes(b"of it".to_vec()).file_name("a.txt"))),
    )
    .await
    .value();
    let upload = founder
        .post(
            &format!("{at}/boards/{board}/uploads"),
            &json!({"name": "later.bin", "size": 1}),
        )
        .await
        .value();
    let invitation = founder
        .post(&format!("{at}/invitations"), &json!({"role": "member"}))
        .await
        .value();
    let token = text(&invitation, "token");
    let member_name = format!("{name}-member");
    let accepted = server
        .anonymous()
        .post(
            "/invitations/accept",
            &json!({"token": token, "account": {"username": member_name,
                "email": format!("{member_name}@example.test"), "password": PASSWORD}}),
        )
        .await;
    assert!(accepted.status.is_success(), "{}", accepted.text());
    let member = log_in(server, &member_name).await;
    let member_id = member.me().await.id;
    let categories = member.get(&format!("{at}/categories")).await.value();
    let access = server
        .app
        .store()
        .resolve_workspace(&member_id, &workspace)
        .await
        .expect("the directory answers")
        .expect("the member is in the workspace");
    assert_eq!(
        (access.workspace_role, access.org_role),
        (WorkspaceRole::Member, Some(OrgRole::Member)),
        "a plain member asks"
    );
    Named {
        member,
        member_id,
        category: text(&categories[0], "id"),
        card: text(&card, "id"),
        file: text(&file, "fileId"),
        upload: text(&upload, "uploadId"),
        invitation: token,
        invitation_id: text(&invitation, "id"),
        org,
        workspace,
        board,
    }
}

fn text(value: &Value, field: &str) -> String {
    value[field]
        .as_str()
        .unwrap_or_else(|| panic!("{field} in {value}"))
        .to_owned()
}

async fn sign_up(server: &TestServer, name: &str) -> Client {
    server
        .anonymous()
        .post(
            "/register",
            &json!({"username": name, "email": format!("{name}@example.test"),
                "password": PASSWORD, "organizationName": name}),
        )
        .await
        .check_created();
    log_in(server, name).await
}

async fn log_in(server: &TestServer, name: &str) -> Client {
    let logged_in = server
        .anonymous()
        .post("/login", &json!({"username": name, "password": PASSWORD}))
        .await;
    assert_eq!(logged_in.status.as_u16(), 204, "{name} signs in");
    Client::new(
        &server.base_url,
        &logged_in.session_cookie().expect("a session cookie"),
    )
}

#[tokio::test]
async fn every_contract_operation_answers_a_plain_member_alike_in_both_organizations() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let server = Setup::local_mode()
        .configure(open)
        .on_an_empty_database()
        .with_templates()
        .start()
        .await;
    let first = organization(&server, "member-first").await;
    let second = organization(&server, "member-other").await;
    assert_ne!(first.org, second.org, "two organizations");
    let of_first = first.sweep().await;
    let of_second = second.sweep().await;
    assert_eq!(
        of_first.len(),
        operations().len(),
        "every operation of the contract is asked"
    );
    // What a member reads of their workspace is there for them in the second one too.
    for read in [
        "getWorkspace",
        "getCategories",
        "getWorkspaceBoards",
        "searchWorkspaceBoards",
        "getBoard",
        "getTemplates",
        "getWorkspaceUsers",
        "getMyMemberships",
    ] {
        assert_eq!(
            of_second.get(read),
            Some(&200),
            "{read} for a member: {of_second:?}"
        );
    }
    let differing: Vec<String> = of_first
        .iter()
        .filter(|(operation, status)| of_second.get(*operation) != Some(status))
        .map(|(operation, status)| {
            format!("{operation}: {status} and {:?}", of_second.get(operation))
        })
        .collect();
    assert!(
        differing.is_empty(),
        "a plain member is answered otherwise in the second organization:\n{}",
        differing.join("\n")
    );
    server.teardown().await;
}
