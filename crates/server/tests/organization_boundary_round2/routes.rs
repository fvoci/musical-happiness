//! Inventory of every legacy workspace route, including body and query targets.

use std::collections::BTreeSet;
use std::io::{Cursor, Read as _};

use fvoci_model::{
    Block, BlockType, Board, BoardMember, BoardType, CategoryBoards, UserWorkspace, Workspace,
    WorkspaceRole, new_id,
};
use fvoci_store::DbType;
use reqwest::StatusCode;
use serde_json::{Value, json};

use super::common::ROOT_WS;
use super::common::{
    Client, HIDDEN_WORKSPACE_ID, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, UNKNOWN_ID,
};
use super::{answer, db_type_of_the_tests};

// Method, path suffix, operation ID: every operation under `/w/{workspaceId}`, also
// compared against the published contract. A parameter of the path is filled in with an
// identifier of nothing; `{partNumber}` with 1.
const WORKSPACE_ROUTES: &[(&str, &str, &str)] = &[
    ("GET", "", "getWorkspace"),
    ("GET", "/teams", "getTeams"),
    ("POST", "/teams", "createTeam"),
    ("GET", "/teams/{teamId}", "getTeam"),
    ("PUT", "/teams/{teamId}", "renameTeam"),
    ("DELETE", "/teams/{teamId}", "deleteTeam"),
    ("PUT", "/teams/{teamId}/members/{userId}", "addTeamMember"),
    (
        "DELETE",
        "/teams/{teamId}/members/{userId}",
        "removeTeamMember",
    ),
    ("GET", "/archive/export", "exportWorkspaceArchive"),
    ("POST", "/archive/import", "importArchive"),
    ("POST", "/archive/uploads", "createImportUpload"),
    ("GET", "/audit", "getWorkspaceAudit"),
    ("POST", "/boards", "createBoard"),
    ("GET", "/boards", "getWorkspaceBoards"),
    ("POST", "/boards-and-blocks", "createBoardsAndBlocks"),
    ("DELETE", "/boards-and-blocks", "deleteBoardsAndBlocks"),
    ("PATCH", "/boards-and-blocks", "patchBoardsAndBlocks"),
    ("GET", "/boards/search?q=secret", "searchWorkspaceBoards"),
    ("DELETE", "/boards/{boardId}", "deleteBoard"),
    ("GET", "/boards/{boardId}", "getBoard"),
    ("PATCH", "/boards/{boardId}", "patchBoard"),
    (
        "GET",
        "/boards/{boardId}/archive/export",
        "exportBoardArchive",
    ),
    ("POST", "/boards/{boardId}/blocks", "createBlocks"),
    ("GET", "/boards/{boardId}/blocks", "getBlocks"),
    ("PATCH", "/boards/{boardId}/blocks", "patchBlocks"),
    (
        "DELETE",
        "/boards/{boardId}/blocks/{blockId}",
        "deleteBlock",
    ),
    ("PATCH", "/boards/{boardId}/blocks/{blockId}", "patchBlock"),
    (
        "POST",
        "/boards/{boardId}/blocks/{blockId}/duplicate",
        "duplicateBlock",
    ),
    (
        "POST",
        "/boards/{boardId}/blocks/{blockId}/undelete",
        "undeleteBlock",
    ),
    (
        "GET",
        "/boards/{boardId}/blocks/{blockId}/versions",
        "getCommentVersions",
    ),
    ("POST", "/boards/{boardId}/cards", "createCard"),
    ("GET", "/boards/{boardId}/cards", "getCards"),
    ("POST", "/boards/{boardId}/duplicate", "duplicateBoard"),
    ("POST", "/boards/{boardId}/files", "uploadFile"),
    ("GET", "/boards/{boardId}/files/{fileId}", "getFile"),
    (
        "GET",
        "/boards/{boardId}/files/{fileId}/info",
        "getFileInfo",
    ),
    ("POST", "/boards/{boardId}/join", "joinBoard"),
    ("POST", "/boards/{boardId}/leave", "leaveBoard"),
    ("POST", "/boards/{boardId}/members", "addMember"),
    ("GET", "/boards/{boardId}/members", "getMembers"),
    (
        "DELETE",
        "/boards/{boardId}/members/{userId}",
        "removeMember",
    ),
    ("PUT", "/boards/{boardId}/members/{userId}", "updateMember"),
    (
        "PUT",
        "/boards/{boardId}/property-order",
        "reorderCardProperties",
    ),
    ("GET", "/boards/{boardId}/sharing", "getSharing"),
    ("POST", "/boards/{boardId}/sharing", "setSharing"),
    ("GET", "/boards/{boardId}/trash/cards", "getDeletedCards"),
    ("POST", "/boards/{boardId}/undelete", "undeleteBoard"),
    ("POST", "/boards/{boardId}/uploads", "createUpload"),
    ("GET", "/cards/{cardId}", "getCard"),
    ("PATCH", "/cards/{cardId}", "patchCard"),
    ("POST", "/categories", "createCategory"),
    ("GET", "/categories", "getCategories"),
    ("DELETE", "/categories/{categoryId}", "deleteCategory"),
    ("PUT", "/categories/{categoryId}", "updateCategory"),
    (
        "PUT",
        "/categories/{categoryId}/board-order",
        "reorderCategoryBoards",
    ),
    (
        "POST",
        "/categories/{categoryId}/boards/{boardId}",
        "setBoardCategory",
    ),
    (
        "PUT",
        "/categories/{categoryId}/boards/{boardId}/hide",
        "hideBoard",
    ),
    (
        "PUT",
        "/categories/{categoryId}/boards/{boardId}/unhide",
        "unhideBoard",
    ),
    ("PUT", "/category-order", "reorderCategories"),
    ("GET", "/members", "getWorkspaceMembers"),
    ("PUT", "/members/{user}", "setWorkspaceRole"),
    ("DELETE", "/members/{user}", "removeWorkspaceMember"),
    ("POST", "/onboard", "onboard"),
    ("PUT", "/guest-invitations", "setGuestInvitations"),
    ("GET", "/invitations", "getInvitations"),
    ("POST", "/invitations", "createInvitation"),
    ("DELETE", "/invitations/{invitation}", "revokeInvitation"),
    ("POST", "/subscriptions", "createSubscription"),
    (
        "DELETE",
        "/subscriptions/{blockId}/{subscriberId}",
        "deleteSubscription",
    ),
    ("GET", "/subscriptions/{subscriberId}", "getSubscriptions"),
    ("GET", "/templates", "getTemplates"),
    ("GET", "/trash/boards", "getDeletedBoards"),
    ("DELETE", "/uploads/{uploadId}", "deleteUpload"),
    ("GET", "/uploads/{uploadId}", "getUpload"),
    ("POST", "/uploads/{uploadId}/complete", "completeUpload"),
    (
        "PUT",
        "/uploads/{uploadId}/parts/{partNumber}",
        "putUploadPart",
    ),
    ("GET", "/users", "getWorkspaceUsers"),
    ("POST", "/users", "getWorkspaceUsersById"),
    ("GET", "/users/{userId}", "getWorkspaceUser"),
];

/// What a guest of the workspace is told by an operation of `REACHED_BY_ITEM` about
/// nothing: the body is refused, the operation, or what the path names.
const REFUSED_IN_REACH: [StatusCode; 3] = [
    StatusCode::BAD_REQUEST,
    StatusCode::FORBIDDEN,
    StatusCode::NOT_FOUND,
];

/// The operations of `WORKSPACE_ROUTES` that a guest of the workspace reaches by what
/// they name, as a member does: for the guest the workspace is there, and what is refused
/// is what the path names (a board, a card, an upload, a user), or the operation.
const REACHED_BY_ITEM: &[&str] = &[
    "createBoard",
    "createBoardsAndBlocks",
    "deleteBoardsAndBlocks",
    "patchBoardsAndBlocks",
    "deleteBoard",
    "getBoard",
    "patchBoard",
    "exportBoardArchive",
    "createBlocks",
    "getBlocks",
    "patchBlocks",
    "deleteBlock",
    "patchBlock",
    "duplicateBlock",
    "undeleteBlock",
    "getCommentVersions",
    "createCard",
    "getCards",
    "duplicateBoard",
    "uploadFile",
    "getFile",
    "getFileInfo",
    "joinBoard",
    "leaveBoard",
    "addMember",
    "getMembers",
    "removeMember",
    "updateMember",
    "reorderCardProperties",
    "getSharing",
    "setSharing",
    "getDeletedCards",
    "undeleteBoard",
    "createUpload",
    "getCard",
    "patchCard",
    "createSubscription",
    "deleteSubscription",
    "getSubscriptions",
    "deleteUpload",
    "getUpload",
    "completeUpload",
    "putUploadPart",
    "getWorkspaceUser",
];

#[test]
fn the_workspace_route_inventory_covers_every_operation_in_the_contract() {
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract is JSON");
    let operations: BTreeSet<&str> = contract["paths"]
        .as_object()
        .expect("paths")
        .iter()
        .filter(|(path, _)| path.starts_with("/api/v1/w/{workspaceId}"))
        .flat_map(|(_, methods)| methods.as_object().expect("operations").values())
        .filter_map(|operation| operation["operationId"].as_str())
        .collect();
    assert_eq!(
        operations,
        WORKSPACE_ROUTES
            .iter()
            .map(|(_, _, operation)| *operation)
            .collect(),
        "every workspace route is exercised, not only its read routes"
    );
}

/// A suffix of `WORKSPACE_ROUTES` with every parameter filled in with an identifier of
/// nothing, and a part number with 1.
fn fill_with_nothing(suffix: &str) -> String {
    let mut filled = suffix.replace("{partNumber}", "1");
    while let Some(start) = filled.find('{') {
        let end = filled[start..].find('}').expect("a parameter ends") + start;
        filled.replace_range(start..=end, UNKNOWN_ID);
    }
    filled
}

fn archive_entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("an export is a ZIP");
    let mut entries = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("an entry");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("the entry contents");
        if entry.name().expect("an entry name") == "version.json" {
            let mut header: Value = serde_json::from_slice(&bytes).expect("the archive header");
            assert!(header["date"].is_number(), "an export timestamp is present");
            header["date"] = json!(0);
            bytes = serde_json::to_vec(&header).expect("the normalized header");
        }
        entries.push((
            entry.name().expect("a UTF-8 entry name").into_owned(),
            bytes,
        ));
    }
    entries.sort();
    entries
}

/// Who asks: somebody who is no member of the hidden workspace, or a guest of it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Asker {
    Outsider,
    Guest,
}

/// The workspace is looked at before the body is read: a body that is no JSON changes
/// nothing of the refusal of a workspace the asker is not in (a guest is in it, and is
/// told). A body that is JSON but names nothing found answers 404 of its own, which hid
/// a refusal made after the body was read (`importArchive` did so once).
async fn compare_malformed_bodies(
    client: &Client,
    method: &str,
    suffix: &str,
    operation: &str,
) -> Option<String> {
    let malformed = async |workspace: &str| {
        client
            .send_raw_body(
                method.parse().expect("an HTTP method"),
                &format!("/w/{workspace}{suffix}"),
                "{",
            )
            .await
    };
    let (hidden, absent) = (
        malformed(HIDDEN_WORKSPACE_ID).await,
        malformed(UNKNOWN_ID).await,
    );
    (hidden.status != StatusCode::NOT_FOUND || answer(&hidden) != answer(&absent)).then(|| {
        format!(
            "{method} {suffix} ({operation}) with a body that is no JSON: hidden={:?}, absent={:?}",
            answer(&hidden),
            answer(&absent)
        )
    })
}

/// The body an operation of the table is sent with: one it takes, naming nothing there.
fn route_body(operation: &str, user_id: &str, workspace: &str) -> Value {
    match operation {
        "createCategory" => json!({"name": "mine", "userId": user_id, "workspaceId": workspace}),
        "reorderCategories" | "reorderCategoryBoards" | "getWorkspaceUsersById" => json!([]),
        "importArchive" => json!({"uploadId": "missing-upload"}),
        "createImportUpload" => json!({"name": "archive.zip", "size": 0}),
        _ => json!({}),
    }
}

async fn compare_workspace_routes(client: &Client, user_id: &str, asker: Asker) -> Vec<String> {
    let mut violations = Vec::new();
    for (method, suffix, operation) in WORKSPACE_ROUTES {
        let suffix = fill_with_nothing(suffix);
        let body = |workspace: &str| route_body(operation, user_id, workspace);
        let send = async |workspace: &str| {
            Client::send(
                client
                    .request(
                        method.parse().expect("an HTTP method"),
                        &format!("/w/{workspace}{suffix}"),
                    )
                    .json(&body(workspace)),
            )
            .await
        };
        let hidden = send(HIDDEN_WORKSPACE_ID).await;
        let absent = send(UNKNOWN_ID).await;
        let equal = if *operation == "getWorkspaceBoards" && asker == Asker::Guest {
            // A guest of the workspace is a member of it: the boards are those the
            // guest is on, none here (the route of `api/workspaces.rs`).
            hidden.status == StatusCode::OK
                && answer(&hidden).1 == json!([])
                && absent.status == StatusCode::NOT_FOUND
        } else if *operation == "getCategories" && asker == Asker::Guest {
            // The guest's own sidebar: the default category, with no board here.
            hidden.status == StatusCode::OK
                && answer(&hidden).1[0]["boardMetadata"] == json!([])
                && absent.status == StatusCode::NOT_FOUND
        } else if *operation == "getWorkspaceUsers" && asker == Asker::Guest {
            // The people a guest sees: who is on its boards, none here, and itself.
            hidden.status == StatusCode::OK
                && answer(&hidden).1.as_array().map(Vec::len) == Some(1)
                && absent.status == StatusCode::NOT_FOUND
        } else if *operation == "getWorkspaceUsersById" && asker == Asker::Guest {
            // The workspace is there for its guest, and the empty list is refused.
            hidden.status == StatusCode::BAD_REQUEST && absent.status == StatusCode::NOT_FOUND
        } else if asker == Asker::Guest && REACHED_BY_ITEM.contains(operation) {
            // The workspace is there for its guest: what is refused is what the path
            // names, the operation, or the empty body; nothing is done.
            REFUSED_IN_REACH.contains(&hidden.status) && absent.status == StatusCode::NOT_FOUND
        } else if *operation == "exportWorkspaceArchive" && asker == Asker::Guest {
            // The guest's archive has the boards the guest is on, none here.
            hidden.status == StatusCode::OK
                && archive_entries(&hidden.body).len() == 1
                && absent.status == StatusCode::NOT_FOUND
        } else {
            answer(&hidden) == answer(&absent) && hidden.status == StatusCode::NOT_FOUND
        };
        if *method != "GET" && asker != Asker::Guest {
            violations.extend(compare_malformed_bodies(client, method, &suffix, operation).await);
        }
        if !equal {
            violations.push(format!(
                "{method} {suffix} ({operation}): hidden={:?}, absent={:?}",
                answer(&hidden),
                answer(&absent)
            ));
        }
    }
    // Workspace IDs supplied in a JSON body, rather than in the path.
    for bulk in [false, true] {
        let send = async |workspace: &str| {
            let board =
                json!({"id": "board-ref", "workspaceId": workspace, "type": "P", "title": "new"});
            let (path, body) = if bulk {
                (
                    &format!("{ROOT_WS}/boards-and-blocks"),
                    json!({"boards": [board], "blocks": [{"id": "card-ref", "boardId": "board-ref", "type": "card", "createAt": 1, "updateAt": 1}]}),
                )
            } else {
                (&format!("{ROOT_WS}/boards"), board)
            };
            client.post(path, &body).await
        };
        let hidden = send(HIDDEN_WORKSPACE_ID).await;
        let absent = send(UNKNOWN_ID).await;
        // Refused for the path's workspace (400), or the path's workspace is not there.
        let refused = matches!(
            hidden.status,
            StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
        );
        if !refused || answer(&hidden) != answer(&absent) {
            violations.push(format!(
                "workspace in body (bulk={bulk}): {:?} versus {:?}",
                answer(&hidden),
                answer(&absent)
            ));
        }
    }
    let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
    // A guest is a member, listed with the role; anybody else does not see it.
    let hidden: Vec<WorkspaceRole> = listed
        .iter()
        .filter(|workspace| workspace.id == HIDDEN_WORKSPACE_ID)
        .map(|workspace| workspace.role)
        .collect();
    let expected = match asker {
        Asker::Outsider => vec![],
        Asker::Guest => vec![WorkspaceRole::Guest],
    };
    if hidden != expected {
        violations.push(format!(
            "getMyWorkspaces listed the hidden workspace as {hidden:?}"
        ));
    }
    violations
}

/// Every route under `/w/{workspaceId}` answers for a workspace the asker is not in, a
/// workspace of another organization, and a workspace the asker is a guest of as for one
/// that does not exist; but `getWorkspaceBoards`, `getCategories`, `exportWorkspaceArchive`
/// and the users of the workspace, which let a guest see, file and export the boards it is
/// on and their people.
#[tokio::test]
async fn every_workspace_route_hides_the_workspace_from_nonmembers_guests_and_other_org_users() {
    let ts = Setup::native().start().await.init_basic().await;
    let guest = ts.add_user("workspace-guest").await;
    let guest_id = guest.me().await.id;
    ts.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &guest_id, WorkspaceRole::Guest)
        .await
        .expect("a workspace guest");
    let mut violations =
        compare_workspace_routes(&ts.client, &ts.client.me().await.id, Asker::Outsider).await;
    violations.extend(compare_workspace_routes(&guest, &guest_id, Asker::Guest).await);
    if db_type_of_the_tests() == DbType::Postgres {
        let user_id = ts.client2.me().await.id;
        for table in ["workspace_members", "organization_members"] {
            ts.app
                .store()
                .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{user_id}'"))
                .await
                .expect("the other user leaves the first org");
        }
        let workspace = Workspace {
            id: new_id(),
            ..Workspace::default()
        };
        let org = ts
            .app
            .store()
            .create_organization("Other", &workspace)
            .await
            .expect("another organization");
        ts.app
            .store()
            .for_org(&org)
            .expect("an org ID")
            .add_workspace_member(&workspace.id, &user_id, WorkspaceRole::Member)
            .await
            .expect("the user works elsewhere");
        violations.extend(compare_workspace_routes(&ts.client2, &user_id, Asker::Outsider).await);
    }
    ts.teardown().await;
    assert!(
        violations.is_empty(),
        "legacy route refusals differ from absent workspaces:\n{}",
        violations.join("\n")
    );
}

#[tokio::test]
async fn a_category_path_rejects_a_normal_board_from_another_workspace() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;
    let categories: Vec<CategoryBoards> = ts
        .client
        .get(&format!("{ROOT_WS}/categories"))
        .await
        .ok_json();
    let category = &categories[0].category;
    let hidden = ts
        .client
        .post(
            &format!("{ROOT_WS}/categories/{}/boards/{}", category.id, board.id),
            &json!({}),
        )
        .await;
    let absent = ts
        .client
        .post(
            &format!("{ROOT_WS}/categories/{}/boards/{UNKNOWN_ID}", category.id),
            &json!({}),
        )
        .await;
    let after: Vec<CategoryBoards> = ts
        .client
        .get(&format!("{ROOT_WS}/categories"))
        .await
        .ok_json();
    let filed = after
        .iter()
        .flat_map(|entry| &entry.board_metadata)
        .any(|metadata| metadata.board_id == board.id);
    ts.teardown().await;
    assert_eq!(
        (answer(&hidden), filed),
        (answer(&absent), false),
        "a board of another workspace must not be filed through this workspace's path"
    );
}

/// Hiding and showing a board through a category compare the workspaces as filing
/// does: a board of another workspace the user works in answers as one that is not
/// there. A built-in template, the system organization's, is in no workspace of the
/// user's and is filed, hidden and shown in any.
#[tokio::test]
async fn a_category_path_hides_no_board_of_another_workspace_but_takes_a_template() {
    let ts = Setup::native().start().await.init_basic().await;
    ts.app
        .init_templates()
        .await
        .expect("the built-in templates are installed");
    let template = ts
        .app
        .store()
        .get_builtin_templates()
        .await
        .expect("the built-in templates")
        .remove(0);
    let board = ts.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;
    let categories: Vec<CategoryBoards> = ts
        .client
        .get(&format!("{ROOT_WS}/categories"))
        .await
        .ok_json();
    let category = format!("{ROOT_WS}/categories/{}/boards", categories[0].category.id);
    let mut answers = Vec::new();
    for change in ["hide", "unhide"] {
        let hidden = ts
            .client
            .put(&format!("{category}/{}/{change}", board.id), &json!({}))
            .await;
        let absent = ts
            .client
            .put(&format!("{category}/{UNKNOWN_ID}/{change}"), &json!({}))
            .await;
        answers.push((change, answer(&hidden) == answer(&absent), absent.status));
    }
    let filed = ts
        .client
        .post(&format!("{category}/{}", template.id), &json!({}))
        .await
        .status;
    let hidden = ts
        .client
        .put(&format!("{category}/{}/hide", template.id), &json!({}))
        .await
        .status;
    let after: Vec<CategoryBoards> = ts
        .client
        .get(&format!("{ROOT_WS}/categories"))
        .await
        .ok_json();
    let template_hidden = after
        .iter()
        .flat_map(|entry| &entry.board_metadata)
        .find(|metadata| metadata.board_id == template.id)
        .map(|metadata| metadata.hidden);
    ts.teardown().await;
    assert_eq!(
        answers,
        [
            ("hide", true, StatusCode::NOT_FOUND),
            ("unhide", true, StatusCode::NOT_FOUND),
        ],
        "a board of another workspace answers as one that is not there"
    );
    assert_eq!(
        (filed, hidden, template_hidden),
        (StatusCode::NO_CONTENT, StatusCode::NO_CONTENT, Some(true)),
        "a built-in template is filed and hidden in the workspace's category"
    );
}

/// A board of a workspace the user is not in, where the user is still on the board,
/// is not there for them: a board role opens nothing outside the workspace. Through a
/// category of their own workspace it is not filed either, and answers as a board that
/// is not there.
#[tokio::test]
async fn a_category_path_rejects_a_board_the_user_is_on_in_a_workspace_they_are_not_in() {
    let ts = Setup::native().start().await.init_basic().await;
    let user_id = ts.client.me().await.id;
    let mut board = Board {
        id: new_id(),
        workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    // Stored past the rule that only who works in a workspace is made the admin of a
    // board there: the state is what the route must not be led astray by.
    ts.app
        .store()
        .insert_board(&mut board, &user_id)
        .await
        .expect("a board of a workspace the user is not in");
    ts.app
        .store()
        .save_member(&BoardMember {
            board_id: board.id.clone(),
            user_id: user_id.clone(),
            scheme_admin: true,
            scheme_editor: true,
            ..BoardMember::default()
        })
        .await
        .expect("the user is on a board of a workspace they are not in");
    let categories: Vec<CategoryBoards> = ts
        .client
        .get(&format!("{ROOT_WS}/categories"))
        .await
        .ok_json();
    let category = format!("{ROOT_WS}/categories/{}/boards", categories[0].category.id);
    let seen = ts
        .client
        .get(&format!("{ROOT_WS}/boards/{}", board.id))
        .await
        .status;
    let filed = ts
        .client
        .post(&format!("{category}/{}", board.id), &json!({}))
        .await;
    let absent = ts
        .client
        .post(&format!("{category}/{UNKNOWN_ID}"), &json!({}))
        .await;
    ts.teardown().await;
    assert_eq!(
        seen,
        StatusCode::NOT_FOUND,
        "a board role outside the workspace opens nothing"
    );
    assert_eq!(
        answer(&filed),
        answer(&absent),
        "a board of a workspace the user is not in answers as one that is not there"
    );
}

/// A board of another workspace that was filed under a category before the categories
/// took the boards of their own workspace only does not keep the category from being
/// deleted: the boards of the workspace move to its default category, and that board
/// to the default category of its own workspace, which the user works in.
#[tokio::test]
async fn deleting_a_category_files_a_board_of_another_workspace_in_its_own() {
    let ts = Setup::native().start().await.init_basic().await;
    let user_id = ts.client.me().await.id;
    let ours = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Open).await;
    let theirs = ts.create_board(TEST_WORKSPACE_ID, BoardType::Open).await;
    let category = ts
        .app
        .create_category(fvoci_model::Category {
            name: "Earlier".to_owned(),
            user_id: user_id.clone(),
            workspace_id: ROOT_WORKSPACE_ID.to_owned(),
            ..fvoci_model::Category::default()
        })
        .await
        .expect("the category is made");
    ts.app
        .store()
        .add_update_category_board(
            &user_id,
            &category.id,
            &[ours.id.clone(), theirs.id.clone()],
        )
        .await
        .expect("both boards are filed as they could be before");
    let deleted = ts
        .client
        .delete(&format!("{ROOT_WS}/categories/{}", category.id))
        .await
        .status;
    let mut filed = Vec::new();
    for workspace_id in [ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID] {
        let sidebar: Vec<CategoryBoards> = ts
            .client
            .get(&format!("/w/{workspace_id}/categories"))
            .await
            .ok_json();
        let boards: Vec<String> = sidebar
            .iter()
            .flat_map(|entry| &entry.board_metadata)
            .map(|metadata| metadata.board_id.clone())
            .collect();
        filed.push(boards);
    }
    ts.teardown().await;
    assert_eq!(deleted, StatusCode::NO_CONTENT, "the category is deleted");
    assert!(
        filed[0].contains(&ours.id) && !filed[0].contains(&theirs.id),
        "the workspace's board stayed in its sidebar, the other's left it: {filed:?}"
    );
    assert!(
        filed[1].contains(&theirs.id),
        "the other workspace's board is in its own sidebar: {filed:?}"
    );
}

/// A live link that lets members into the workspace, made by `maker`: its identifier.
async fn a_link_into(ts: &crate::common::TestServer, workspace_id: &str, maker: &str) -> String {
    ts.app
        .store()
        .create_invitation(&fvoci_store::NewLink {
            workspace_id,
            guest_of: None,
            token_hash: &format!("a link into {workspace_id}"),
            created_by: maker,
            now: 0,
            expires_at: i64::MAX,
            max_uses: None,
        })
        .await
        .expect("a link into the workspace")
        .id
}

/// What the routes of [`compare_resource_routes`] are asked about: the things of one
/// workspace that the paths name, and who is named in them.
struct Items<'a> {
    board: &'a Board,
    card: &'a Block,
    file: &'a str,
    upload_id: &'a str,
    category: &'a str,
    /// A user of the items' workspace.
    user: &'a str,
    /// A live link that lets members into the items' workspace.
    invitation: &'a str,
    team: &'a str,
    /// The subscriber of the path, and the one of the same path for nothing: the asker
    /// themselves for both when they are subscribed to the card.
    subscriber: (&'a str, &'a str),
}

/// Asks every operation under `/w/{workspace}` whose path names something (and the
/// subscriptions, whose body names a block) about `items`, and again about nothing:
/// the answers must be the same, and none a success. Where both requests are the same
/// (the asker's own subscriptions), the answer must not name the card. The operations
/// come from the contract, so a new one is compared as it is added.
async fn compare_resource_routes(
    client: &Client,
    workspace: &str,
    items: &Items<'_>,
) -> Vec<String> {
    let Items {
        board,
        card,
        file,
        upload_id,
        category,
        user,
        invitation,
        team,
        subscriber,
    } = items;
    let caller = client.me().await.id;
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract");
    let mut violations = Vec::new();
    for (template, methods) in contract["paths"].as_object().expect("paths") {
        let Some(rest) = template.strip_prefix("/api/v1/w/{workspaceId}") else {
            continue;
        };
        if !(rest.contains('{') || rest.starts_with("/subscriptions")) {
            continue;
        }
        for (method, operation) in methods.as_object().expect("operations") {
            let Some(operation) = operation["operationId"].as_str() else {
                continue;
            };
            // The contract writes methods in lower case, which HTTP does not.
            let method: reqwest::Method = method.to_uppercase().parse().expect("an HTTP method");
            let pick = |absent: bool, there: &str, nothing: &'static str| -> String {
                if absent { nothing } else { there }.to_owned()
            };
            let path = |absent: bool| {
                template
                    .trim_start_matches("/api/v1")
                    .replace("{workspaceId}", workspace)
                    .replace("{boardId}", &pick(absent, &board.id, UNKNOWN_ID))
                    .replace("{cardId}", &pick(absent, &card.id, UNKNOWN_ID))
                    .replace("{blockId}", &pick(absent, &card.id, UNKNOWN_ID))
                    .replace("{fileId}", &pick(absent, file, "missing-file"))
                    .replace("{uploadId}", &pick(absent, upload_id, "missing-upload"))
                    .replace("{userId}", &pick(absent, user, UNKNOWN_ID))
                    .replace("{invitationId}", &pick(absent, invitation, UNKNOWN_ID))
                    .replace("{teamId}", &pick(absent, team, UNKNOWN_ID))
                    .replace(
                        "{subscriberId}",
                        if absent { subscriber.1 } else { subscriber.0 },
                    )
                    .replace("{partNumber}", "1")
                    .replace("{categoryId}", &pick(absent, category, UNKNOWN_ID))
            };
            let block = |absent: bool| pick(absent, &card.id, UNKNOWN_ID);
            let body = |absent: bool| match operation {
                "patchBlocks" => {
                    json!({"blockIds": [block(absent)], "blockPatches": [{"title": "changed"}]})
                }
                "reorderCardProperties" | "createBlocks" => json!([]),
                "createCard" => json!({"title": "new card"}),
                "patchCard" | "patchBoard" | "patchBlock" => json!({"title": "changed"}),
                "createUpload" => json!({"name": "new.bin", "size": 0}),
                "addMember" => json!({"userId": user, "schemeViewer": true}),
                "setSharing" => json!({"enabled": true}),
                "createSubscription" => {
                    json!({"blockId": block(absent), "blockType": "card", "subscriberType": "user", "subscriberId": caller})
                }
                _ => json!({}),
            };
            if path(false).contains('{') {
                violations.push(format!(
                    "{operation} ({method} {template}): a parameter is not filled in"
                ));
                continue;
            }
            let ask = async |absent: bool| {
                Client::send(
                    client
                        .request(method.clone(), &path(absent))
                        .json(&body(absent)),
                )
                .await
            };
            let (hidden, absent) = (ask(false).await, ask(true).await);
            let refused = if path(false) == path(true) && body(false) == body(true) {
                !hidden.text().contains(&card.id)
            } else {
                !hidden.status.is_success()
            };
            if !refused || answer(&hidden) != answer(&absent) {
                violations.push(format!(
                    "{operation} ({method} {template}): hidden={:?}, absent={:?}",
                    answer(&hidden),
                    answer(&absent)
                ));
            }
        }
    }
    violations
}

/// Takes the second user out of the first organization and makes them a member of a
/// workspace of a new one.
async fn move_to_another_organization(ts: &super::common::TestServer) {
    let user = ts.client2.me().await.id;
    for table in ["workspace_members", "organization_members"] {
        ts.app
            .store()
            .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{user}'"))
            .await
            .expect("remove first-org admission");
    }
    let workspace = Workspace {
        id: new_id(),
        ..Workspace::default()
    };
    let org = ts
        .app
        .store()
        .create_organization("ID inventory", &workspace)
        .await
        .expect("another organization");
    ts.app
        .store()
        .for_org(&org)
        .expect("org ID")
        .add_workspace_member(&workspace.id, &user, WorkspaceRole::Member)
        .await
        .expect("admission elsewhere");
}

/// A new user who works in `workspace` and in no other: a user is stored in the
/// default workspace of the organization, the root one, and is taken out of it.
async fn user_of_only(ts: &super::common::TestServer, workspace: &str) -> String {
    let mut user = fvoci_model::User {
        id: new_id(),
        username: format!("only-{workspace}"),
        email: format!("only-{workspace}@sample.com"),
        ..fvoci_model::User::default()
    };
    let store = ts.app.store();
    store
        .create_user(&mut user)
        .await
        .expect("the user is stored");
    store
        .execute_unchecked(&format!(
            "DELETE FROM workspace_members WHERE user_id = '{}' AND workspace_id = '{ROOT_WORKSPACE_ID}'",
            user.id
        ))
        .await
        .expect("the user leaves the root workspace");
    store
        .add_workspace_member(workspace, &user.id, WorkspaceRole::Member)
        .await
        .expect("the user works in the workspace");
    user.id
}

/// A category of the owner in the hidden workspace.
async fn hidden_category(ts: &super::common::TestServer, owner: &str) -> String {
    ts.app
        .create_category(fvoci_model::Category {
            name: "Hidden category".to_owned(),
            user_id: owner.to_owned(),
            workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
            ..fvoci_model::Category::default()
        })
        .await
        .expect("a category of the hidden workspace")
        .id
}

#[tokio::test]
async fn identifier_only_inventory_hides_resources_of_an_unjoined_workspace() {
    let ts = Setup::native().start().await.init_basic().await;
    let owner = user_of_only(&ts, HIDDEN_WORKSPACE_ID).await;
    let mut board = Board {
        id: new_id(),
        workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        title: "Hidden board".to_owned(),
        ..Board::default()
    };
    ts.app
        .store()
        .insert_board_with_admin(&mut board, &owner)
        .await
        .expect("a hidden workspace's board");
    let mut card = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "Hidden card".to_owned(),
        ..Block::default()
    };
    ts.app
        .store()
        .insert_block(&mut card, &owner)
        .await
        .expect("its card");
    let mut data = &b"hidden workspace file"[..];
    let file = ts
        .app
        .save_file(&mut data, &board.id, "hidden.txt")
        .await
        .expect("its file");
    let upload = ts
        .app
        .create_upload(
            &owner,
            &board.id,
            fvoci_server::app::FileToUpload {
                name: "hidden-upload.txt".to_owned(),
                size: 0,
                content_type: String::new(),
            },
        )
        .await
        .expect("a real upload owned by the hidden workspace's member");
    let guest = ts.add_user("id-inventory-guest").await;
    let guest_id = guest.me().await.id;
    ts.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &guest_id, WorkspaceRole::Guest)
        .await
        .expect("a guest who has no board invitation");
    let category = hidden_category(&ts, &owner).await;
    let invitation = a_link_into(&ts, HIDDEN_WORKSPACE_ID, &owner).await;
    let team = a_team_in(&ts, &board.workspace_id, &board.created_by).await;
    let items = Items {
        board: &board,
        card: &card,
        file: &file,
        upload_id: &upload.id,
        category: &category,
        user: &owner,
        invitation: &invitation,
        team: &team,
        subscriber: (&owner, UNKNOWN_ID),
    };
    let mut violations = compare_resource_routes(&ts.client, ROOT_WORKSPACE_ID, &items).await;
    violations.extend(compare_resource_routes(&guest, ROOT_WORKSPACE_ID, &items).await);
    // A role in another organization must not change the answer of any ID route.
    if db_type_of_the_tests() == DbType::Postgres {
        move_to_another_organization(&ts).await;
        violations.extend(compare_resource_routes(&ts.client2, ROOT_WORKSPACE_ID, &items).await);
    }
    ts.teardown().await;
    assert!(
        violations.is_empty(),
        "ID-only route differences:\n{}",
        violations.join("\n")
    );
}

async fn compare_query_targets(client: &Client, own: &Board, hidden: &Board) {
    for (path, absent) in [
        (
            format!(
                "{ROOT_WS}/boards/{}/duplicate?toWorkspace={HIDDEN_WORKSPACE_ID}",
                own.id
            ),
            format!(
                "{ROOT_WS}/boards/{}/duplicate?toWorkspace={UNKNOWN_ID}",
                own.id
            ),
        ),
        (
            format!(
                "{ROOT_WS}/boards/{}/blocks?sourceBoardId={}",
                own.id, hidden.id
            ),
            format!(
                "{ROOT_WS}/boards/{}/blocks?sourceBoardId={UNKNOWN_ID}",
                own.id
            ),
        ),
    ] {
        let body = if path.contains("/blocks?") {
            json!([])
        } else {
            json!({})
        };
        let hidden = client.post(&path, &body).await;
        let missing = client.post(&absent, &body).await;
        assert_eq!(answer(&hidden), answer(&missing), "query target in {path}");
        hidden.check_not_found();
    }
    for method in [reqwest::Method::PATCH, reqwest::Method::DELETE] {
        let body = |board: &str| {
            if method == reqwest::Method::PATCH {
                json!({"boardIds": [board], "boardPatches": [{"title": "changed"}], "blockIds": [], "blockPatches": []})
            } else {
                json!({"boards": [board], "blocks": []})
            }
        };
        let concealed = Client::send(
            client
                .request(method.clone(), &format!("{ROOT_WS}/boards-and-blocks"))
                .json(&body(&hidden.id)),
        )
        .await;
        let missing = Client::send(
            client
                .request(method.clone(), &format!("{ROOT_WS}/boards-and-blocks"))
                .json(&body(UNKNOWN_ID)),
        )
        .await;
        assert_eq!(
            answer(&concealed),
            answer(&missing),
            "{method} bulk board identifiers"
        );
        concealed.check_not_found();
    }
    // For somebody of another organization the root workspace is not there either.
    let search = client
        .get(&format!("{ROOT_WS}/boards/search?q=Hidden%20query"))
        .await;
    if search.status != StatusCode::NOT_FOUND {
        let found: Vec<Board> = search.ok_json();
        assert!(
            found.is_empty(),
            "the search does not find hidden open boards"
        );
    }
}

#[tokio::test]
async fn query_targets_and_bulk_id_bodies_hide_unjoined_workspaces() {
    let ts = Setup::native().start().await.init_basic().await;
    let own = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Private).await;
    let owner = ts.stranger_in(HIDDEN_WORKSPACE_ID).await;
    let mut hidden = Board {
        id: new_id(),
        workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        title: "Hidden query board".to_owned(),
        ..Board::default()
    };
    ts.app
        .store()
        .insert_board_with_admin(&mut hidden, &owner)
        .await
        .expect("the hidden board");
    let guest = ts.add_user("query-guest").await;
    ts.app
        .store()
        .add_workspace_member(
            HIDDEN_WORKSPACE_ID,
            &guest.me().await.id,
            WorkspaceRole::Guest,
        )
        .await
        .expect("a guest actor");
    let mut actors = vec![ts.client.clone(), guest];
    if db_type_of_the_tests() == DbType::Postgres {
        let user = ts.client2.me().await.id;
        for table in ["workspace_members", "organization_members"] {
            ts.app
                .store()
                .execute_unchecked(&format!("DELETE FROM {table} WHERE user_id = '{user}'"))
                .await
                .expect("the actor leaves A");
        }
        let workspace = Workspace {
            id: new_id(),
            ..Workspace::default()
        };
        let org = ts
            .app
            .store()
            .create_organization("Query actor B", &workspace)
            .await
            .expect("B exists");
        let b = ts.app.store().for_org(&org).expect("B's scope");
        b.add_organization_member(&user, fvoci_model::OrgRole::Member)
            .await
            .expect("the actor belongs to B");
        b.add_workspace_member(&workspace.id, &user, WorkspaceRole::Member)
            .await
            .expect("the actor works in B");
        actors.push(ts.client2.clone());
    }
    for actor in actors {
        compare_query_targets(&actor, &own, &hidden).await;
    }
    ts.teardown().await;
}

#[tokio::test]
async fn an_invited_workspace_guest_reads_its_board_without_gaining_editor_rights() {
    let ts = Setup::native().start().await.init_basic().await;
    let user = ts.client2.me().await.id;
    ts.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &user, WorkspaceRole::Guest)
        .await
        .expect("workspace guest");
    let mut board = Board {
        id: new_id(),
        workspace_id: HIDDEN_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        ..Board::default()
    };
    ts.app
        .store()
        .insert_board_with_admin(&mut board, &ts.stranger_in(HIDDEN_WORKSPACE_ID).await)
        .await
        .expect("a hidden board");
    ts.client2
        .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/boards/{}", board.id))
        .await
        .check_not_found();
    ts.app
        .store()
        .save_member(&BoardMember {
            board_id: board.id.clone(),
            user_id: user,
            scheme_viewer: true,
            ..BoardMember::default()
        })
        .await
        .expect("a viewer invitation");
    for suffix in ["", "/blocks", "/cards", "/members"] {
        ts.client2
            .get(&format!(
                "/w/{HIDDEN_WORKSPACE_ID}/boards/{}{suffix}",
                board.id
            ))
            .await
            .check_ok();
    }
    let boards: Vec<Board> = ts
        .client2
        .get(&format!("/w/{HIDDEN_WORKSPACE_ID}/boards"))
        .await
        .ok_json();
    assert_eq!(
        boards
            .iter()
            .map(|board| board.id.as_str())
            .collect::<Vec<_>>(),
        [board.id.as_str()],
        "the workspace list contains the invitation only"
    );
    ts.client2
        .patch(
            &format!("/w/{HIDDEN_WORKSPACE_ID}/boards/{}", board.id),
            &json!({"title": "not permitted"}),
        )
        .await
        .check_forbidden();
    ts.client2
        .post(
            &format!("/w/{HIDDEN_WORKSPACE_ID}/boards/{}/join", board.id),
            &json!({}),
        )
        .await
        .check_not_found();
    ts.teardown().await;
}

/// What was under `/api/v1` before it moved under `/w/{workspaceId}`, and what went
/// with the move (the search of every workspace, the users by identifiers of every
/// workspace): every one of them is a path that does not exist, whoever asks.
#[tokio::test]
async fn the_paths_from_before_the_workspace_are_not_there() {
    let ts = Setup::native().start().await.init_basic().await;
    let gone = WORKSPACE_ROUTES
        .iter()
        .filter(|(_, _, operation)| REACHED_BY_ITEM.contains(operation))
        .map(|(method, suffix, operation)| (*method, fill_with_nothing(suffix), *operation))
        .chain([
            ("POST", "/users".to_owned(), "getUsers"),
            ("GET", "/boards/search?q=secret".to_owned(), "searchBoards"),
        ]);
    let mut violations = Vec::new();
    let mut count = 0;
    for (method, path, operation) in gone {
        count += 1;
        for client in [&ts.client, &ts.anonymous()] {
            let response = Client::send(
                client
                    .request(method.parse().expect("an HTTP method"), &path)
                    .json(&json!({})),
            )
            .await;
            if response.status != StatusCode::NOT_FOUND
                || response.error_message() != "route not found"
            {
                violations.push(format!(
                    "{operation}: {method} {path}: {:?}",
                    answer(&response)
                ));
            }
        }
    }
    ts.teardown().await;
    assert_eq!(
        count, 46,
        "the operations that moved, and the two that went"
    );
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

/// What is of one workspace is not there under another, also for somebody who works in
/// both and may do anything with it in its own: every operation that names something in
/// its path answers about a board, card, file, upload, category and user of the test
/// workspace under the root one as about nothing, and the asker's subscription to the
/// card is not among theirs there; so does a body or a query that names the board.
#[tokio::test]
async fn what_is_of_one_workspace_is_not_there_under_another() {
    let ts = Setup::native().start().await.init_basic().await;
    let me = ts.client.me().await.id;
    let elsewhere = user_of_only(&ts, TEST_WORKSPACE_ID).await;
    let mut board = Board {
        id: new_id(),
        workspace_id: TEST_WORKSPACE_ID.to_owned(),
        board_type: BoardType::Open,
        title: "Sibling board".to_owned(),
        ..Board::default()
    };
    let store = ts.app.store();
    store
        .insert_board_with_admin(&mut board, &me)
        .await
        .expect("a board of the test workspace that the asker administers");
    let mut card = Block {
        id: new_id(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "Sibling card".to_owned(),
        ..Block::default()
    };
    store.insert_block(&mut card, &me).await.expect("its card");
    let mut data = &b"sibling workspace file"[..];
    let file = ts
        .app
        .save_file(&mut data, &board.id, "sibling.txt")
        .await
        .expect("its file");
    let to_upload = fvoci_server::app::FileToUpload {
        name: "sibling-upload.txt".to_owned(),
        size: 0,
        content_type: String::new(),
    };
    let upload = ts
        .app
        .create_upload(&me, &board.id, to_upload)
        .await
        .expect("the asker's upload to it");
    let category = ts
        .app
        .create_category(fvoci_model::Category {
            name: "Sibling category".to_owned(),
            user_id: me.clone(),
            workspace_id: TEST_WORKSPACE_ID.to_owned(),
            ..fvoci_model::Category::default()
        })
        .await
        .expect("the asker's category there")
        .id;
    store
        .create_subscription(fvoci_model::Subscription {
            block_type: BlockType::Card,
            block_id: card.id.clone(),
            subscriber_type: fvoci_model::SubscriberType::User,
            subscriber_id: me.clone(),
            notified_at: 0,
            create_at: 0,
            delete_at: 0,
        })
        .await
        .expect("the asker follows the card");
    // Under its own workspace each of them is there.
    for path in [
        format!("/w/{TEST_WORKSPACE_ID}/boards/{}", board.id),
        format!("/w/{TEST_WORKSPACE_ID}/cards/{}", card.id),
        format!("/w/{TEST_WORKSPACE_ID}/uploads/{}", upload.id),
        format!("/w/{TEST_WORKSPACE_ID}/users/{elsewhere}"),
    ] {
        ts.client.get(&path).await.check_ok();
    }
    let subscriptions = ts
        .client
        .get(&format!("/w/{TEST_WORKSPACE_ID}/subscriptions/{me}"))
        .await;
    assert!(
        subscriptions.text().contains(&card.id),
        "followed in its workspace"
    );
    let invitation = a_link_into(&ts, TEST_WORKSPACE_ID, &me).await;
    let team = a_team_in(&ts, &board.workspace_id, &board.created_by).await;
    let items = Items {
        board: &board,
        card: &card,
        file: &file,
        upload_id: &upload.id,
        category: &category,
        user: &elsewhere,
        invitation: &invitation,
        team: &team,
        subscriber: (&me, &me),
    };
    let violations = compare_resource_routes(&ts.client, ROOT_WORKSPACE_ID, &items).await;
    // Named in a body or a query (the boards of a combined request, the board files are
    // copied from), the board is as one that does not exist too.
    let own = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Open).await;
    compare_query_targets(&ts.client, &own, &board).await;
    ts.teardown().await;
    assert!(
        violations.is_empty(),
        "what is of the test workspace, asked for under the root one:\n{}",
        violations.join("\n")
    );
}

/// A real group for every resource guard, including bodies that do not decode.
async fn a_team_in(ts: &crate::common::TestServer, workspace_id: &str, owner: &str) -> String {
    ts.app.store().execute_unchecked(&format!(
        "UPDATE workspace_members SET role = 'admin' WHERE workspace_id = '{workspace_id}' AND user_id = '{owner}'"
    )).await.expect("the fixture owner administers its workspace");
    ts.app
        .store()
        .create_team(workspace_id, "Resource guard group", owner)
        .await
        .expect("a team in the resource's workspace")
}
