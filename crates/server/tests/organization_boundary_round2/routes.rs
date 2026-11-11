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

use super::common::{
    Client, HIDDEN_WORKSPACE_ID, ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, UNKNOWN_ID,
};
use super::{answer, db_type_of_the_tests};

// Method, path suffix, operation ID: also compared against the published contract.
const WORKSPACE_ROUTES: &[(&str, &str, &str)] = &[
    ("GET", "", "getWorkspace"),
    ("GET", "/archive/export", "exportWorkspaceArchive"),
    ("POST", "/archive/import", "importArchive"),
    ("POST", "/archive/uploads", "createImportUpload"),
    ("GET", "/boards", "getWorkspaceBoards"),
    ("GET", "/boards/search?q=secret", "searchWorkspaceBoards"),
    ("GET", "/categories", "getCategories"),
    ("POST", "/categories", "createCategory"),
    ("DELETE", "/categories/{category}", "deleteCategory"),
    ("PUT", "/categories/{category}", "updateCategory"),
    (
        "PUT",
        "/categories/{category}/board-order",
        "reorderCategoryBoards",
    ),
    (
        "POST",
        "/categories/{category}/boards/{board}",
        "setBoardCategory",
    ),
    (
        "PUT",
        "/categories/{category}/boards/{board}/hide",
        "hideBoard",
    ),
    (
        "PUT",
        "/categories/{category}/boards/{board}/unhide",
        "unhideBoard",
    ),
    ("PUT", "/category-order", "reorderCategories"),
    ("POST", "/onboard", "onboard"),
    ("POST", "/signup-token", "regenerateSignupToken"),
    ("GET", "/templates", "getTemplates"),
    ("GET", "/trash/boards", "getDeletedBoards"),
    ("GET", "/users", "getWorkspaceUsers"),
    ("POST", "/users", "getWorkspaceUsersById"),
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

async fn compare_workspace_routes(client: &Client, user_id: &str, asker: Asker) -> Vec<String> {
    let mut violations = Vec::new();
    for (method, suffix, operation) in WORKSPACE_ROUTES {
        let suffix = suffix
            .replace("{category}", UNKNOWN_ID)
            .replace("{board}", UNKNOWN_ID);
        let body = |workspace: &str| match *operation {
            "createCategory" => {
                json!({"name": "mine", "userId": user_id, "workspaceId": workspace})
            }
            "reorderCategories" | "reorderCategoryBoards" | "getWorkspaceUsersById" => json!([]),
            "importArchive" => json!({"uploadId": "missing-upload"}),
            "createImportUpload" => json!({"name": "archive.zip", "size": 0}),
            _ => json!({}),
        };
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
        } else if *operation == "exportWorkspaceArchive" {
            hidden.status == absent.status
                && hidden.status == StatusCode::OK
                && archive_entries(&hidden.body) == archive_entries(&absent.body)
        } else {
            answer(&hidden) == answer(&absent) && hidden.status == StatusCode::NOT_FOUND
        };
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
                    "/boards-and-blocks",
                    json!({"boards": [board], "blocks": [{"id": "card-ref", "boardId": "board-ref", "type": "card", "createAt": 1, "updateAt": 1}]}),
                )
            } else {
                ("/boards", board)
            };
            client.post(path, &body).await
        };
        let hidden = send(HIDDEN_WORKSPACE_ID).await;
        let absent = send(UNKNOWN_ID).await;
        if hidden.status != StatusCode::BAD_REQUEST || answer(&hidden) != answer(&absent) {
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
/// that does not exist; but `getWorkspaceBoards`, which lets a guest see the boards the
/// guest is on, and `exportWorkspaceArchive`, whose archive of such a workspace has no
/// boards.
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
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/categories"))
        .await
        .ok_json();
    let category = &categories[0].category;
    let hidden = ts
        .client
        .post(
            &format!(
                "/w/{ROOT_WORKSPACE_ID}/categories/{}/boards/{}",
                category.id, board.id
            ),
            &json!({}),
        )
        .await;
    let absent = ts
        .client
        .post(
            &format!(
                "/w/{ROOT_WORKSPACE_ID}/categories/{}/boards/{UNKNOWN_ID}",
                category.id
            ),
            &json!({}),
        )
        .await;
    let after: Vec<CategoryBoards> = ts
        .client
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/categories"))
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
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/categories"))
        .await
        .ok_json();
    let category = format!(
        "/w/{ROOT_WORKSPACE_ID}/categories/{}/boards",
        categories[0].category.id
    );
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
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/categories"))
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
    ts.app
        .store()
        .insert_board_with_admin(&mut board, &user_id)
        .await
        .expect("the user is on a board of a workspace they are not in");
    let categories: Vec<CategoryBoards> = ts
        .client
        .get(&format!("/w/{ROOT_WORKSPACE_ID}/categories"))
        .await
        .ok_json();
    let category = format!(
        "/w/{ROOT_WORKSPACE_ID}/categories/{}/boards",
        categories[0].category.id
    );
    let seen = ts.client.get(&format!("/boards/{}", board.id)).await.status;
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
        .delete(&format!(
            "/w/{ROOT_WORKSPACE_ID}/categories/{}",
            category.id
        ))
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

/// The ID-only inventory comes from the contract, so adding a resource operation
/// automatically includes it in the comparisons. The workspace inventory above spells out
/// the complete workspace list separately, and body/query targets are checked below.
async fn compare_resource_routes(
    client: &Client,
    board: &Board,
    card: &Block,
    file: &str,
    owner: &str,
    upload_id: &str,
) -> Vec<String> {
    let caller = client.me().await.id;
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract");
    let mut violations = Vec::new();
    for (template, methods) in contract["paths"].as_object().expect("paths") {
        if !(template.starts_with("/api/v1/boards/{boardId}")
            || template.starts_with("/api/v1/cards/{cardId}")
            || template.starts_with("/api/v1/uploads/{uploadId}")
            || template.starts_with("/api/v1/subscriptions"))
        {
            continue;
        }
        for (method, operation) in methods.as_object().expect("operations") {
            let Some(operation) = operation["operationId"].as_str() else {
                continue;
            };
            let path = |absent: bool| {
                template
                    .trim_start_matches("/api/v1")
                    .replace("{boardId}", if absent { UNKNOWN_ID } else { &board.id })
                    .replace("{cardId}", if absent { UNKNOWN_ID } else { &card.id })
                    .replace("{blockId}", if absent { UNKNOWN_ID } else { &card.id })
                    .replace("{fileId}", if absent { "missing-file" } else { file })
                    .replace(
                        "{uploadId}",
                        if absent { "missing-upload" } else { upload_id },
                    )
                    .replace("{userId}", if absent { UNKNOWN_ID } else { owner })
                    .replace("{subscriberId}", if absent { UNKNOWN_ID } else { owner })
                    .replace("{partNumber}", "1")
            };
            let body = |absent: bool| match operation {
                "patchBlocks" => {
                    json!({"blockIds": [if absent {UNKNOWN_ID} else {&card.id}], "blockPatches": [{"title": "changed"}]})
                }
                "reorderCardProperties" | "createBlocks" => json!([]),
                "createCard" => json!({"title": "new card"}),
                "patchCard" | "patchBoard" | "patchBlock" => json!({"title": "changed"}),
                "createUpload" => json!({"name": "new.bin", "size": 0}),
                "addMember" => json!({"userId": owner, "schemeViewer": true}),
                "setSharing" => json!({"enabled": true}),
                "createSubscription" => {
                    json!({"blockId": if absent {UNKNOWN_ID} else {&card.id}, "blockType": "card", "subscriberType": "user", "subscriberId": caller})
                }
                _ => json!({}),
            };
            let hidden_body = body(false);
            let missing_body = body(true);
            let hidden = Client::send(
                client
                    .request(method.parse().expect("method"), &path(false))
                    .json(&hidden_body),
            )
            .await;
            let absent = Client::send(
                client
                    .request(method.parse().expect("method"), &path(true))
                    .json(&missing_body),
            )
            .await;
            if hidden.status.is_success() || answer(&hidden) != answer(&absent) {
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

#[tokio::test]
async fn identifier_only_inventory_hides_resources_of_an_unjoined_workspace() {
    let ts = Setup::native().start().await.init_basic().await;
    let owner = ts.stranger().await;
    ts.app
        .store()
        .add_workspace_member(HIDDEN_WORKSPACE_ID, &owner, WorkspaceRole::Member)
        .await
        .expect("the resource owner works in the hidden workspace");
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
    let mut violations =
        compare_resource_routes(&ts.client, &board, &card, &file, &owner, &upload.id).await;
    violations
        .extend(compare_resource_routes(&guest, &board, &card, &file, &owner, &upload.id).await);
    // A role in another organization must not change the answer of any ID route.
    if db_type_of_the_tests() == DbType::Postgres {
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
        violations.extend(
            compare_resource_routes(&ts.client2, &board, &card, &file, &owner, &upload.id).await,
        );
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
                "/boards/{}/duplicate?toWorkspace={HIDDEN_WORKSPACE_ID}",
                own.id
            ),
            format!("/boards/{}/duplicate?toWorkspace={UNKNOWN_ID}", own.id),
        ),
        (
            format!("/boards/{}/blocks?sourceBoardId={}", own.id, hidden.id),
            format!("/boards/{}/blocks?sourceBoardId={UNKNOWN_ID}", own.id),
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
                .request(method.clone(), "/boards-and-blocks")
                .json(&body(&hidden.id)),
        )
        .await;
        let missing = Client::send(
            client
                .request(method.clone(), "/boards-and-blocks")
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
    let found: Vec<Board> = client
        .get("/boards/search?q=Hidden%20query")
        .await
        .ok_json();
    assert!(
        found.is_empty(),
        "global board search does not find hidden open boards"
    );
}

#[tokio::test]
async fn query_targets_and_bulk_id_bodies_hide_unjoined_workspaces() {
    let ts = Setup::native().start().await.init_basic().await;
    let own = ts.create_board(ROOT_WORKSPACE_ID, BoardType::Private).await;
    let owner = ts.stranger().await;
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
        .insert_board_with_admin(&mut board, &ts.stranger().await)
        .await
        .expect("a hidden board");
    ts.client2
        .get(&format!("/boards/{}", board.id))
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
            .get(&format!("/boards/{}{suffix}", board.id))
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
            &format!("/boards/{}", board.id),
            &json!({"title": "not permitted"}),
        )
        .await
        .check_forbidden();
    ts.client2
        .post(&format!("/boards/{}/join", board.id), &json!({}))
        .await
        .check_not_found();
    ts.teardown().await;
}
