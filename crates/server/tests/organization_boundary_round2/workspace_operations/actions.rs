//! Successful operations on real objects, with destructive cases after their reads.

use serde_json::json;

use super::{Case, files};

pub(super) async fn exercise(case: &mut Case) {
    let workspace = case.workspace.clone();
    case.present("getWorkspace", &[], &workspace).await;
    let made = case
        .run(
            "createBoard",
            &[],
            json!({"title": format!("secret {} board", case.label), "type": "O", "cardProperties": [{"id": "first", "name": "First", "type": "text"}, {"id": "second", "name": "Second", "type": "text"}]}),
        )
        .await
        .value();
    let board = case.remember(&made, "id");
    boards(case, &board).await;
    let card = cards(case, &board).await;
    categories(case, &board).await;
    let audited = people(case, &board).await;
    subscriptions(case, &card).await;
    files::exercise(case, &board).await;
    bulk(case).await;
    templates(case).await;
    case.run("deleteBoard", &[("boardId", &board)], json!({}))
        .await;
    let trash = case.run("getDeletedBoards", &[], json!({})).await;
    assert!(
        trash.text().contains(&board),
        "the deleted board is in this founder's trash"
    );
    case.run("undeleteBoard", &[("boardId", &board)], json!({}))
        .await;
    // Keep private fixtures behind for the next founder's template and trash reads.
    for template in [true, false] {
        let made = case
            .run(
                "createBoard",
                &[],
                json!({"title": format!("{} private fixture", case.label), "type": "P", "isTemplate": template}),
            )
            .await
            .value();
        let fixture = case.remember(&made, "id");
        if !template {
            case.run("deleteBoard", &[("boardId", &fixture)], json!({}))
                .await;
        }
    }
    case.present("getWorkspaceAudit", &[], &audited).await;
}

async fn boards(case: &mut Case, board: &str) {
    let at = &[("boardId", board)];
    case.present("getWorkspaceBoards", &[], board).await;
    case.present("getBoard", at, board).await;
    case.run(
        "patchBoard",
        at,
        json!({"title": format!("secret {} renamed", case.label)}),
    )
    .await;
    let found = case.run("searchWorkspaceBoards", &[], json!({})).await;
    assert!(
        found.text().contains(board),
        "search finds the founder's board"
    );
    let order = case
        .run("reorderCardProperties", at, json!(["second", "first"]))
        .await
        .value();
    assert_eq!(
        order,
        json!(["second", "first"]),
        "nonempty properties are reordered"
    );
    case.run("setSharing", at, json!({"enabled": true})).await;
    let sharing = case.run("getSharing", at, json!({})).await.value();
    assert_eq!(sharing["enabled"], true, "sharing was enabled");
}

async fn cards(case: &mut Case, board: &str) -> String {
    let at = &[("boardId", board)];
    let made = case
        .run(
            "createCard",
            at,
            json!({"title": "parity card", "properties": {}}),
        )
        .await
        .value();
    let card = case.remember(&made, "id");
    case.present("getCards", at, &card).await;
    case.present("getCard", &[("cardId", &card)], &card).await;
    case.run(
        "patchCard",
        &[("cardId", &card)],
        json!({"title": "changed card"}),
    )
    .await;
    let created = case
        .run(
            "createBlocks",
            at,
            json!([{
                "id": "comment-reference", "boardId": board, "parentId": card,
                "type": "comment", "body": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "first comment"}]}]}, "createAt": 1, "updateAt": 1
            }]),
        )
        .await
        .value();
    let comment = case.remember(&created[0], "id");
    let comment_at = &[("boardId", board), ("blockId", comment.as_str())];
    case.present("getBlocks", at, &comment).await;
    case.run("patchBlock", comment_at, json!({"readAt": created[0]["updateAt"], "body": {"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "edited comment"}]}]}}))
        .await;
    case.present("getCommentVersions", comment_at, "first comment")
        .await;
    case.run(
        "patchBlocks",
        at,
        json!({"blockIds": [card], "blockPatches": [{"title": "batch change"}]}),
    )
    .await;
    let card_at = &[("boardId", board), ("blockId", card.as_str())];
    case.run("duplicateBlock", card_at, json!({})).await;
    case.run("deleteBlock", card_at, json!({})).await;
    let trash = case.run("getDeletedCards", at, json!({})).await;
    assert!(
        trash.text().contains(&card),
        "deleted card is visible in its trash"
    );
    case.run("undeleteBlock", card_at, json!({})).await;
    card
}

async fn categories(case: &mut Case, board: &str) {
    let created = case
        .run(
            "createCategory",
            &[],
            json!({
                "name": "Parity category", "userId": case.user, "workspaceId": case.workspace
            }),
        )
        .await
        .value();
    let category = case.remember(&created, "id");
    let at = &[("categoryId", category.as_str())];
    case.run("updateCategory", at, json!({"id": category, "name": "Renamed category", "userId": case.user, "workspaceId": case.workspace})).await;
    let board_at = &[("categoryId", category.as_str()), ("boardId", board)];
    case.run("setBoardCategory", board_at, json!({})).await;
    case.run("hideBoard", board_at, json!({})).await;
    case.run("unhideBoard", board_at, json!({})).await;
    case.run("reorderCategoryBoards", at, json!([board])).await;
    let listed = case.present("getCategories", &[], &category).await.value();
    let mut order: Vec<_> = listed
        .as_array()
        .expect("categories")
        .iter()
        .map(|entry| entry["id"].clone())
        .collect();
    order.reverse();
    let reordered = case
        .run("reorderCategories", &[], json!(order))
        .await
        .value();
    assert_eq!(reordered, json!(order), "categories are reordered");
    case.run("deleteCategory", at, json!({})).await;
}

async fn people(case: &mut Case, board: &str) -> String {
    let user = case.user.clone();
    let peer = case.peer.clone();
    case.present("getWorkspaceUsers", &[], &peer).await;
    let users = case
        .run("getWorkspaceUsersById", &[], json!([user, peer]))
        .await;
    assert!(
        users.text().contains(&user) && users.text().contains(&peer),
        "requested users are returned"
    );
    case.present("getWorkspaceUser", &[("userId", &peer)], &peer)
        .await;
    case.present("getWorkspaceMembers", &[], &peer).await;
    case.run(
        "setWorkspaceRole",
        &[("user", &peer)],
        json!({"role": "admin"}),
    )
    .await;
    case.run(
        "setWorkspaceRole",
        &[("user", &peer)],
        json!({"role": "member"}),
    )
    .await;
    let at = &[("boardId", board)];
    case.run(
        "addMember",
        at,
        json!({"boardId": board, "userId": peer, "schemeEditor": true}),
    )
    .await;
    case.present("getMembers", at, &peer).await;
    let member_at = &[("boardId", board), ("userId", peer.as_str())];
    case.run(
        "updateMember",
        member_at,
        json!({"boardId": board, "userId": peer, "schemeViewer": true}),
    )
    .await;
    case.run("removeMember", member_at, json!({})).await;
    teams(case, board).await;
    joining(case).await;
    let audited = audit_admission(case).await;
    case.run("setGuestInvitations", &[], json!({"enabled": true}))
        .await;
    let invitation = case
        .run("createInvitation", &[], json!({"role": "member"}))
        .await
        .value();
    let invitation = case.remember(&invitation, "id");
    case.present("getInvitations", &[], &invitation).await;
    case.run(
        "revokeInvitation",
        &[("invitation", &invitation)],
        json!({}),
    )
    .await;
    case.run("removeWorkspaceMember", &[("user", &peer)], json!({}))
        .await;
    audited
}

/// A team of the workspace with the peer in it and a role on the board, renamed and
/// then taken apart.
async fn teams(case: &mut Case, board: &str) {
    let peer = case.peer.clone();
    let made = case
        .run("createTeam", &[], json!({"name": "Design"}))
        .await
        .value();
    let team = case.remember(&made, "id");
    let at = &[("teamId", team.as_str())];
    case.present("getTeams", &[], &team).await;
    case.present("getTeam", at, &team).await;
    case.run("renameTeam", at, json!({"name": "Designers"}))
        .await;
    let member_at = &[("teamId", team.as_str()), ("userId", peer.as_str())];
    case.run("addTeamMember", member_at, json!({})).await;
    let on_board = &[("boardId", board), ("teamId", team.as_str())];
    case.run("setBoardTeam", on_board, json!({"role": "editor"}))
        .await;
    case.present("getBoardTeams", &[("boardId", board)], &team)
        .await;
    case.run("removeBoardTeam", on_board, json!({})).await;
    case.run("removeTeamMember", member_at, json!({})).await;
    case.run("deleteTeam", at, json!({})).await;
}

async fn joining(case: &mut Case) {
    let made = case
        .run(
            "createBoard",
            &[],
            json!({"title": "Join and leave", "type": "O"}),
        )
        .await
        .value();
    let board = case.remember(&made, "id");
    let at = &[("boardId", board.as_str())];
    case.run(
        "addMember",
        at,
        json!({"boardId": board, "userId": case.peer, "schemeAdmin": true}),
    )
    .await;
    case.run("leaveBoard", at, json!({})).await;
    case.run("joinBoard", at, json!({})).await;
}

async fn audit_admission(case: &mut Case) -> String {
    let made = case
        .run(
            "createBoard",
            &[],
            json!({"title": "Audit admission", "type": "P"}),
        )
        .await
        .value();
    let board = case.remember(&made, "id");
    let at = &[("boardId", board.as_str())];
    case.run(
        "addMember",
        at,
        json!({"boardId": board, "userId": case.peer, "schemeAdmin": true}),
    )
    .await;
    case.run("leaveBoard", at, json!({})).await;
    // A workspace administrator who cannot read a private board records gaining a role.
    case.run(
        "addMember",
        at,
        json!({"boardId": board, "userId": case.user, "schemeAdmin": true}),
    )
    .await;
    board
}

async fn subscriptions(case: &mut Case, card: &str) {
    let user = case.user.clone();
    case.run("createSubscription", &[], json!({"blockId": card, "blockType": "card", "subscriberId": user, "subscriberType": "user"})).await;
    let subscriptions = case
        .run("getSubscriptions", &[("subscriberId", &user)], json!({}))
        .await;
    assert!(
        subscriptions.text().contains(card),
        "the subscription is listed"
    );
    case.run(
        "deleteSubscription",
        &[("subscriberId", &user), ("blockId", card)],
        json!({}),
    )
    .await;
}

async fn bulk(case: &mut Case) {
    let made = case.run("createBoardsAndBlocks", &[], json!({
        "boards": [{"id": "board-reference", "title": "Bulk board", "type": "P", "workspaceId": case.workspace}],
        "blocks": [{"id": "card-reference", "boardId": "board-reference", "title": "Bulk card", "type": "card", "createAt": 1, "updateAt": 1}]
    })).await.value();
    let board = case.remember(&made["boards"][0], "id");
    let card = case.remember(&made["blocks"][0], "id");
    case.run("patchBoardsAndBlocks", &[], json!({"boardIds": [board], "boardPatches": [{"title": "Bulk changed"}], "blockIds": [card], "blockPatches": [{"title": "Bulk card changed"}]})).await;
    case.run(
        "deleteBoardsAndBlocks",
        &[],
        json!({"boards": [board], "blocks": [card]}),
    )
    .await;
}

async fn templates(case: &mut Case) {
    let made = case
        .run(
            "createBoard",
            &[],
            json!({"title": format!("{} template", case.label), "type": "P", "isTemplate": true}),
        )
        .await
        .value();
    let own = case.remember(&made, "id");
    let templates = case.present("getTemplates", &[], &own).await.value();
    let template = templates
        .as_array()
        .expect("templates")
        .iter()
        .find(|entry| entry["id"] != own)
        .expect("a built-in template")["id"]
        .as_str()
        .expect("template ID");
    let copied = case
        .run("duplicateBoard", &[("boardId", template)], json!({}))
        .await
        .value();
    case.remember(&copied["boards"][0], "id");
    let toured = case.run("onboard", &[], json!({})).await.value();
    assert_eq!(
        toured["workspaceId"], case.workspace,
        "onboarding uses this workspace"
    );
}
