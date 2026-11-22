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
    /// A wiki for everybody in the workspace, which the member joined, a page of it, and
    /// a comment of the founder's on the page.
    wiki: String,
    page: String,
    comment: String,
    file: String,
    upload: String,
    category: String,
    invitation: String,
    invitation_id: String,
    read_token: String,
    /// The read token of the wiki, which is shared too: the pages of a link are a wiki's.
    wiki_token: String,
    /// A team of the workspace with a role on the board, which the member is in.
    team: String,
}

/// What a plain member who is on the board is answered 404 in the first organization
/// too, for what the sweep names: one board and one card for every operation. Any
/// other 404 fails the test, as an answer of the second organization equal to a
/// wrong first one would hide a lookup in another organization.
const NOT_THERE_FOR_A_MEMBER: [&str; 30] = [
    // The organization's own routes are its administrators'.
    "createOrgWorkspace",
    "deleteOidcFormerProvider",
    "deleteOidcPending",
    "deleteOidcProvider",
    "deleteOrgWorkspace",
    "getOidcProvider",
    "getOrgAudit",
    "getOrgMembers",
    "getOrgWorkspaces",
    "getOrganization",
    "joinOrgWorkspace",
    "leaveOrgWorkspace",
    "patchOrganization",
    "putOidcProvider",
    "putOrgRole",
    "renameOrgWorkspace",
    "restoreOrgWorkspace",
    "startOidcTrial",
    // A link the member did not make, and the empty body of the sweep.
    "acceptInvitation",
    "revokeInvitation",
    // The card is no comment, and has no versions of one.
    "getCommentVersions",
    // The board is not in the trash, nor the card, nor the wiki's page, nor its comment.
    "undeleteContainer",
    "undeleteBlock",
    "undeletePage",
    "undeletePageComment",
    // The member's subscription is to the card, by its own identifier.
    "deleteSubscription",
    // The upload asked of is done with by then, or was never begun in parts.
    "completeUpload",
    "putUploadPart",
    "importArchive",
    // The member's account waits for no change of its address: the sweep asked for
    // none it could, with its empty body.
    "resendEmailChange",
];

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
    // What takes the member off the board, or a thing away, goes after the rest, and
    // what ends the member's session last, so that everything else is asked first.
    let rank = |id: &str| match id {
        "logout" => 3,
        "changePassword" => 2,
        "leaveContainer"
        | "deleteContainer"
        | "deleteBlock"
        | "deletePage"
        | "deleteBoardsAndBlocks"
        | "deleteUpload"
        | "deleteCategory"
        | "removeWorkspaceMember"
        | "removeMember" => 1,
        _ => 0,
    };
    operations.sort_by_key(|(_, _, id)| rank(id));
    operations
}

impl Named {
    fn path(&self, template: &str) -> String {
        // The pages of a link are read by the wiki's link, its other routes by the board's.
        let template = if template.starts_with("/api/v1/shared/{containerId}/pages") {
            template.replacen("{containerId}", &self.wiki, 1)
        } else {
            template.to_owned()
        };
        template
            .trim_start_matches("/api/v1")
            .replace("{workspaceId}", &self.workspace)
            .replace("{orgId}", &self.org)
            .replace("{containerId}", &self.board)
            .replace("{boardId}", &self.board)
            .replace("{cardId}", &self.card)
            .replace("{blockId}", &self.card)
            .replace("{wikiId}", &self.wiki)
            .replace("{pageId}", &self.page)
            .replace("{commentId}", &self.comment)
            .replace("{fileId}", &self.file)
            .replace("{uploadId}", &self.upload)
            .replace("{userId}", &self.member_id)
            .replace("{subscriberId}", &self.member_id)
            .replace("{categoryId}", &self.category)
            .replace("{invitationId}", &self.invitation_id)
            .replace("{teamId}", &self.team)
            .replace("{partNumber}", "1")
            .replace("{version}", "1")
            // A lease never given: its release is the member's answer as an editor's or not.
            .replace("{leaseId}", "a-lease-never-given")
    }

    fn body(&self, operation: &str) -> Value {
        match operation {
            "getWorkspaceUsersById" => json!([self.member_id]),
            "resolveReferences" => json!([self.card, self.page]),
            "createBlocks" | "reorderCardProperties" => json!([]),
            "importArchive" => json!({ "uploadId": self.upload }),
            "previewInvitation" => json!({ "token": self.invitation }),
            "patchMe" => json!({ "customName": "a member" }),
            _ => json!({}),
        }
    }

    /// Asks every operation as the member: the status of each, by its identifier.
    async fn sweep(&self) -> BTreeMap<String, u16> {
        // An answer that sends the member elsewhere (the sign-in callback's, to
        // `server_url`) is the answer: what is there is not the server's.
        let member = self.member.not_following_redirects();
        let mut answers = BTreeMap::new();
        for (method, template, operation) in operations() {
            let path = self.path(&template);
            assert!(
                !path.contains('{'),
                "unfilled parameter of {operation}: {path}"
            );
            let path = if template.starts_with("/api/v1/shared/{containerId}/pages") {
                format!("{path}?readToken={}", self.wiki_token)
            } else if template.starts_with("/api/v1/shared/") {
                format!("{path}?readToken={}", self.read_token)
            } else {
                path
            };
            let request = member
                .request(method.parse().expect("a method"), &path)
                .json(&self.body(&operation));
            let response = Client::send(request).await;
            answers.insert(operation, response.status.as_u16());
        }
        answers
    }
}

/// Writes a body to the card: a version of it, which the member reads.
async fn write_a_body(founder: &Client, at: &str, card: &Value) {
    founder
        .patch(
            &format!("{at}/cards/{}", text(card, "id")),
            &json!({"readAt": card["updateAt"], "body": {"type": "doc", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "a body"}]}]}}),
        )
        .await
        .check_ok();
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
    let (wiki, page, comment) = wiki_with_a_page(&founder, &at, name).await;
    write_a_body(&founder, &at, &card).await;
    let file = Client::send(
        founder
            .request(Method::POST, &format!("{at}/containers/{board}/files"))
            .multipart(Form::new().part("file", Part::bytes(b"of it".to_vec()).file_name("a.txt"))),
    )
    .await
    .value();
    let (invitation, member) = invited_member(server, &founder, &at, name).await;
    let token = text(&invitation, "token");
    let member_id = member.me().await.id;
    let categories = member.get(&format!("{at}/categories")).await.value();
    let sharing = founder
        .post(
            &format!("{at}/containers/{board}/sharing"),
            &json!({"enabled": true}),
        )
        .await
        .value();
    let wiki_sharing = founder
        .post(
            &format!("{at}/containers/{wiki}/sharing"),
            &json!({"enabled": true}),
        )
        .await
        .value();
    // On the board and the wiki, the member asks what their members are asked.
    for joined in [&board, &wiki] {
        member
            .post(&format!("{at}/containers/{joined}/join"), &json!({}))
            .await
            .check_ok();
    }
    let card_id = text(&card, "id");
    member
        .post(
            &format!("{at}/subscriptions"),
            &json!({"blockType": "card", "blockId": card_id,
                "subscriberType": "user", "subscriberId": member_id}),
        )
        .await
        .check_created();
    let upload = member
        .post(
            &format!("{at}/containers/{board}/uploads"),
            &json!({"name": "later.bin", "size": 1}),
        )
        .await
        .value();
    let team = team_on_board(&founder, &at, &board, &member_id).await;
    plain_member(server, &member_id, &workspace).await;
    Named {
        member,
        member_id,
        category: text(&categories[0], "id"),
        card: card_id,
        read_token: text(&sharing, "token"),
        wiki_token: text(&wiki_sharing, "token"),
        file: text(&file, "fileId"),
        upload: text(&upload, "uploadId"),
        invitation: token,
        invitation_id: text(&invitation, "id"),
        org,
        workspace,
        board,
        wiki,
        page,
        comment,
        team,
    }
}

/// The invitation for members the founder makes in the workspace at `at`, and the
/// member who signs up by it and is logged in.
async fn invited_member(
    server: &TestServer,
    founder: &Client,
    at: &str,
    name: &str,
) -> (Value, Client) {
    let invitation = founder
        .post(&format!("{at}/invitations"), &json!({"role": "member"}))
        .await
        .value();
    let member_name = format!("{name}-member");
    let accepted = server
        .anonymous()
        .post(
            "/invitations/accept",
            &json!({"token": text(&invitation, "token"), "account": {"username": member_name,
                "email": format!("{member_name}@example.test"), "password": PASSWORD}}),
        )
        .await;
    assert!(accepted.status.is_success(), "{}", accepted.text());
    (invitation, log_in(server, &member_name).await)
}

/// A wiki of the workspace at `at` for everybody in it, with a page and a comment on it:
/// the identifiers of the three.
async fn wiki_with_a_page(founder: &Client, at: &str, name: &str) -> (String, String, String) {
    let wiki = founder
        .post(
            &format!("{at}/wikis"),
            &json!({"title": format!("{name} wiki"), "type": "O"}),
        )
        .await
        .value();
    let wiki = text(&wiki, "id");
    let page = founder
        .post(
            &format!("{at}/wikis/{wiki}/pages"),
            &json!({"title": format!("{name} page")}),
        )
        .await
        .value();
    let said = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": format!("{name} comment")}]}]});
    // Two bodies written to the page are two versions of it: the member reads the first
    // and goes back to it, which writes it to the page again.
    let at_page = format!("{at}/wikis/{wiki}/pages/{}", text(&page, "id"));
    let mut read_at = page["updateAt"].clone();
    let again = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": format!("{name} body again")}]}]});
    for body in [said.clone(), again] {
        let written = founder
            .patch(&at_page, &json!({"readAt": read_at, "body": body}))
            .await;
        written.check_ok();
        read_at = written.value()["updateAt"].clone();
    }
    let page = text(&page, "id");
    let comment = founder
        .post(
            &format!("{at}/wikis/{wiki}/pages/{page}/comments"),
            &json!({ "body": said }),
        )
        .await
        .value();
    (wiki, page, text(&comment, "id"))
}

/// The member of the workspace `workspace` asks as a plain member: no administrator of
/// the workspace or of the organization.
async fn plain_member(server: &TestServer, member_id: &str, workspace: &str) {
    let access = server
        .app
        .store()
        .resolve_workspace(member_id, workspace)
        .await
        .expect("the directory answers")
        .expect("the member is in the workspace");
    assert_eq!(
        (access.workspace_role, access.org_role),
        (WorkspaceRole::Member, Some(OrgRole::Member)),
        "a plain member asks"
    );
}

/// A team of the workspace at `at` with the member in it and a role on the board: the
/// member asks its teams' routes too.
async fn team_on_board(founder: &Client, at: &str, board: &str, member_id: &str) -> String {
    let team = founder
        .post(&format!("{at}/teams"), &json!({"name": "Design"}))
        .await
        .value();
    let team = text(&team, "id");
    founder
        .put(
            &format!("{at}/teams/{team}/members/{member_id}"),
            &json!({}),
        )
        .await
        .check_no_content();
    founder
        .put(
            &format!("{at}/containers/{board}/teams/{team}"),
            &json!({"role": "viewer"}),
        )
        .await
        .check_no_content();
    team
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
    // What a member reads of their workspace and its board is there for them in the
    // second one too.
    for read in [
        "getWorkspace",
        "getCategories",
        "getWorkspaceBoards",
        "searchWorkspaceBoards",
        "getContainer",
        "getBlocks",
        "getCards",
        "getCard",
        "getWikis",
        "getPageTree",
        "getPage",
        "getPageComments",
        "getPageCommentVersions",
        "getPageBodyVersions",
        "getPageBodyVersion",
        "getCardBodyVersions",
        "getCardBodyVersion",
        "getUpload",
        "getSharedContainer",
        "getSharedCards",
        "getSharedPageTree",
        "getSharedPage",
        "getTemplates",
        "getWorkspaceUsers",
        "resolveReferences",
        "getReferencesTo",
        "searchReferences",
        "getMyMemberships",
    ] {
        assert_eq!(
            of_second.get(read),
            Some(&200),
            "{read} for a member: {of_second:?}"
        );
    }
    let failed: Vec<_> = of_first
        .iter()
        .chain(&of_second)
        .filter(|(_, status)| **status >= 500)
        .collect();
    assert!(failed.is_empty(), "the server failed: {failed:?}");
    let unexpected: Vec<&String> = of_first
        .iter()
        .filter(|(operation, status)| {
            **status == 404 && !NOT_THERE_FOR_A_MEMBER.contains(&operation.as_str())
        })
        .map(|(operation, _)| operation)
        .collect();
    assert!(
        unexpected.is_empty(),
        "not there for a member of the first organization: {unexpected:?}"
    );
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
