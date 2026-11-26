//! Integration, regression, and 50×200 open-board delivery tests for `server-authority`:
//! - Atomic `duplicateCard` pre-copy rollback on storage copy failure (`0` orphan/original-file blocks)
//! - Caller-scoped `my-permissions` isolation across `containerChanged`, `containerMemberChanged`,
//!   and `workspaceRoleChanged`
//! - Immediate Collab room and WebSocket eviction when an open board switches to `private`
//! - Fractional-indexed board views, multi-card `card-moves`, `batch-delete`/`batch-restore`,
//!   formal card comments, verified card attachments, cross-workspace copy (`allowCopyOut`,
//!   external `[[` & `@mention` rewriting, `container_copied_out` audit), and `'Today'` hub
//! - 50 open boards × 200 WebSocket connections card-edit broadcast benchmark

use std::time::{Duration, Instant};

use fvoci_model::{Container, ContainerVisibility, OrgKind, UserWorkspace, WorkspaceRole};
use fvoci_server::app::tend_notifications;
use fvoci_server::storage::FaultyOp;
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::board_broadcasts::WorkspaceSocket;
use crate::common::{Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer};
use crate::files::{upload, uploaded_id};
use crate::org_admin::org_of;
use crate::rooms::{Incoming, Peer, QUIET, paragraphs};
use crate::rooms_changes::seldom;

fn text_doc(text: &str) -> Value {
    paragraphs(&[text])
}

/// 1) Mandatory regression test: if storage `Copy` fails while duplicating a card with
/// an attached file and an embedded file block, `POST .../cards/{cardId}/duplicate`
/// returns 500, leaves **zero** cloned blocks referencing the original `fileId` (and
/// zero extra cards on the board), and leaves no unsettled `Writing` loose objects.
#[tokio::test]
async fn atomic_card_duplicate_rolls_back_when_file_copy_fails() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerVisibility::Private)
        .await;
    let up = upload(&ts.client, &board.id, "spec.txt", b"important bytes").await;
    up.check_created();
    let orig_file_id = uploaded_id(&up);

    let up2 = upload(&ts.client, &board.id, "attach.txt", b"attachment bytes").await;
    up2.check_created();
    let second_file_id = uploaded_id(&up2);

    let body = json!({
        "type": "doc",
        "content": [{
            "type": "file",
            "attrs": { "fileId": orig_file_id }
        }]
    });
    let card: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Original", "body": body}),
        )
        .await
        .created_json();
    let card_id = card["id"].as_str().expect("card id");
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/cards/{card_id}/attachments", board.id),
            &json!({"fileId": second_file_id}),
        )
        .await
        .check_created();

    // First verify happy-path atomic duplication remaps both body fileId and attachment fileId.
    let copy_ok: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards/{card_id}/duplicate", board.id),
            &json!({}),
        )
        .await
        .created_json();
    let copy_id = copy_ok["id"].as_str().expect("copied card id");
    let copy_detail: Value = ts
        .client
        .get(&format!("{TEST_WS}/cards/{copy_id}"))
        .await
        .ok_json();
    let copied_body_file = copy_detail["body"]["content"][0]["attrs"]["fileId"]
        .as_str()
        .expect("copied body fileId");
    assert_ne!(
        copied_body_file, orig_file_id,
        "copied body uses new fileId"
    );

    // Inject storage Copy failure on the second file copy (attachment after body file).
    let faults = ts.app.files().faults().expect("faults enabled");
    faults.fail_after(FaultyOp::Copy, 1, 1);

    let cards_before: Vec<Value> = ts
        .client
        .get(&format!("{TEST_WS}/boards/{}/cards", board.id))
        .await
        .ok_json();
    let failed = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards/{card_id}/duplicate", board.id),
            &json!({}),
        )
        .await;
    assert_eq!(failed.status, StatusCode::INTERNAL_SERVER_ERROR);

    let cards_after: Vec<Value> = ts
        .client
        .get(&format!("{TEST_WS}/boards/{}/cards", board.id))
        .await
        .ok_json();
    assert_eq!(
        cards_after.len(),
        cards_before.len(),
        "no partial duplicate card committed"
    );

    let all_blocks: Vec<Value> = ts
        .client
        .get(&format!("{TEST_WS}/boards/{}/blocks?all=true", board.id))
        .await
        .ok_json();
    let refs_to_orig: Vec<&Value> = all_blocks
        .iter()
        .filter(|b| {
            let id = b["id"].as_str().unwrap_or_default();
            let parent = b["parentId"].as_str().unwrap_or_default();
            id != card_id && parent != card_id && b.to_string().contains(orig_file_id.as_str())
        })
        .collect();
    assert!(
        refs_to_orig.is_empty(),
        "0 cloned blocks may reference original fileId: {refs_to_orig:?}"
    );
    ts.teardown().await;
}

/// 2) Mandatory regression test: `Container` JSON never embeds `myPermissions`, and two
/// connections with different roles (`admin` vs `viewer` vs unjoined `member`) retain
/// their own `my-permissions` across `containerChanged`, `containerMemberChanged`, and
/// `workspaceRoleChanged`.
#[tokio::test]
async fn caller_scoped_my_permissions_never_bleed_across_websocket_broadcasts() {
    let ts = Setup::native().start().await.init_basic().await;
    let admin_id = ts.client.me().await.id;
    let viewer_id = ts.client2.me().await.id;
    let unjoined_client = ts.add_user("unjoined.reader").await;
    let unjoined_id = unjoined_client.me().await.id;

    let board: Container = ts
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"visibility": "open", "title": "Shared Open"}),
        )
        .await
        .created_json();
    ts.client
        .post(
            &format!("{TEST_WS}/containers/{}/members", board.id),
            &json!({"userId": viewer_id, "role": "viewer"}),
        )
        .await
        .check_ok();

    let mut admin_ws =
        WorkspaceSocket::subscribe(&ts, &ts.client, TEST_WORKSPACE_ID, &admin_id).await;
    let mut viewer_ws =
        WorkspaceSocket::subscribe(&ts, &ts.client2, TEST_WORKSPACE_ID, &viewer_id).await;
    let mut unjoined_ws =
        WorkspaceSocket::subscribe(&ts, &unjoined_client, TEST_WORKSPACE_ID, &unjoined_id).await;

    let raw_container: Value = ts
        .client
        .get(&format!("{TEST_WS}/containers/{}", board.id))
        .await
        .ok_json();
    assert!(
        raw_container.get("myPermissions").is_none(),
        "Container struct must never carry myPermissions"
    );

    let perms_path = format!("{TEST_WS}/containers/{}/my-permissions", board.id);
    let admin_p: Value = ts.client.get(&perms_path).await.ok_json();
    let viewer_p: Value = ts.client2.get(&perms_path).await.ok_json();
    let unjoined_p: Value = unjoined_client.get(&perms_path).await.ok_json();
    assert_eq!(admin_p["canEdit"], json!(true));
    assert_eq!(admin_p["canManageRoles"], json!(true));
    assert_eq!(viewer_p["canEdit"], json!(false));
    assert_eq!(viewer_p["canComment"], json!(false));
    assert_eq!(unjoined_p["canEdit"], json!(false));
    assert_eq!(unjoined_p["canComment"], json!(false));

    // Admin patches board title -> broadcasts containerChanged to all three sockets.
    ts.client
        .patch(
            &format!("{TEST_WS}/containers/{}", board.id),
            &json!({"title": "Updated Title"}),
        )
        .await
        .check_ok();
    for sock in [&mut admin_ws, &mut viewer_ws, &mut unjoined_ws] {
        let msg = sock
            .expect("containerChanged", "/container/id", &board.id)
            .await;
        assert!(
            msg["container"].get("myPermissions").is_none(),
            "containerChanged payload must not embed caller permissions: {msg}"
        );
    }
    let viewer_after: Value = ts.client2.get(&perms_path).await.ok_json();
    let unjoined_after: Value = unjoined_client.get(&perms_path).await.ok_json();
    assert_eq!(viewer_after["canEdit"], json!(false));
    assert_eq!(unjoined_after["canEdit"], json!(false));

    // Promote viewer to editor -> containerMemberChanged + updated my-permissions.
    ts.client
        .put(
            &format!("{TEST_WS}/containers/{}/members/{viewer_id}", board.id),
            &json!({"role": "editor"}),
        )
        .await
        .check_ok();
    viewer_ws
        .expect("containerMemberChanged", "/member/userId", &viewer_id)
        .await;
    let promoted: Value = ts.client2.get(&perms_path).await.ok_json();
    assert_eq!(promoted["canEdit"], json!(true));

    // Workspace admin promotes unjoined member to workspace admin -> workspaceRoleChanged!
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' WHERE CAST(user_id AS TEXT) = '{admin_id}' \
             AND CAST(workspace_id AS TEXT) = '{TEST_WORKSPACE_ID}'",
            WorkspaceRole::Admin.as_str()
        ))
        .await
        .expect("grant test workspace admin");
    ts.client
        .put(
            &format!("{TEST_WS}/members/{unjoined_id}"),
            &json!({"role": "admin"}),
        )
        .await
        .check_ok();
    let role_msg = unjoined_ws
        .expect("workspaceRoleChanged", "/userId", &unjoined_id)
        .await;
    assert_eq!(role_msg["role"], json!("admin"));
    let ws_admin_p: Value = unjoined_client.get(&perms_path).await.ok_json();
    assert_eq!(ws_admin_p["canManageRoles"], json!(true));
    ts.teardown().await;
}

/// 3) Mandatory regression test: an unjoined workspace member reading an open board and
/// its card Collab room is immediately evicted (`docLeft` + `containerChanged` deletion
/// tombstone + zero subsequent `blockChanged`) the instant the board switches to `private`.
#[tokio::test]
async fn open_board_switched_to_private_immediately_evicts_unjoined_readers() {
    let ts = seldom().start().await.init_basic().await;
    let reader_id = ts.client2.me().await.id;
    let board: Container = ts
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"visibility": "open", "title": "Open Roadmap"}),
        )
        .await
        .created_json();
    let card: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Card 1", "body": text_doc("visible while open")}),
        )
        .await
        .created_json();
    let card_id = card["id"].as_str().expect("card id").to_owned();

    let mut reader_ws =
        WorkspaceSocket::subscribe(&ts, &ts.client2, TEST_WORKSPACE_ID, &reader_id).await;
    let mut reader_room = Peer::connect(&ts.client2, 2).await;
    let joined = reader_room.join(TEST_WORKSPACE_ID, &card_id).await;
    assert_eq!(joined["type"], json!("docJoined"), "{joined}");
    assert_eq!(joined["mode"], json!("read"));

    // Switch open board to private -> immediate Collab room eviction + WS tombstone!
    ts.client
        .patch(
            &format!("{TEST_WS}/containers/{}", board.id),
            &json!({"visibility": "private"}),
        )
        .await
        .check_ok();

    let left = reader_room.text("docLeft").await;
    assert_eq!(left["reason"], json!("not_found"), "{left}");

    let gone = reader_ws
        .expect("containerChanged", "/container/id", &board.id)
        .await;
    assert!(
        gone["container"]["deletedAt"].is_number(),
        "unjoined reader receives deletion tombstone on open->private: {gone}"
    );

    // Subsequent card edits must NOT leak to the evicted reader over WS or Collab.
    ts.client
        .patch(
            &format!("{TEST_WS}/cards/{card_id}"),
            &json!({"title": "Top Secret"}),
        )
        .await
        .check_ok();
    assert_eq!(reader_room.incoming(QUIET).await, Incoming::Nothing);
    let leaked = reader_ws.drain().await;
    assert!(
        !leaked.iter().any(|m| m["type"] == json!("blockChanged")),
        "no blockChanged may leak after board becomes private: {leaked:?}"
    );
    ts.teardown().await;
}

/// Exercises fractional-indexed views, `card-moves` (validation, WS order, and assignee
/// notification), `batch-delete` / `batch-restore`, formal card comments, and verified
/// card attachments.
#[tokio::test]
async fn board_views_card_moves_batch_ops_comments_and_attachments() {
    let ts = Setup::native().start().await.init_basic().await;
    let admin_id = ts.client.me().await.id;
    let member_id = ts.client2.me().await.id;
    let board: Container = ts
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"visibility": "open", "title": "Ops", "scaffold": true}),
        )
        .await
        .created_json();
    ts.client
        .post(
            &format!("{TEST_WS}/containers/{}/members", board.id),
            &json!({"userId": member_id, "role": "editor"}),
        )
        .await
        .check_ok();

    let assignee_prop = board
        .card_properties
        .iter()
        .find(|p| p.property_type == fvoci_model::PropertyType::Person)
        .expect("scaffold person property")
        .id
        .clone();

    // Fractional-indexed views: create, duplicate, move.
    let v_created: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/views", board.id),
            &json!({"title": "Table", "viewType": "table", "fields": {"viewType": "table"}}),
        )
        .await
        .created_json();
    let v_id = v_created["view"]["id"].as_str().expect("view id");
    let v_dup: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/views/{v_id}/duplicate", board.id),
            &json!({"title": "Table Copy", "after": v_id}),
        )
        .await
        .created_json();
    let dup_id = v_dup["view"]["id"].as_str().expect("dup view id");
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/views/{dup_id}/move", board.id),
            &json!({"after": ""}),
        )
        .await
        .check_ok();

    let c1: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "One"}),
        )
        .await
        .created_json();
    let c2: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Two"}),
        )
        .await
        .created_json();
    let (c1_id, c2_id) = (
        c1["id"].as_str().expect("c1").to_owned(),
        c2["id"].as_str().expect("c2").to_owned(),
    );

    let mut member_ws =
        WorkspaceSocket::subscribe(&ts, &ts.client2, TEST_WORKSPACE_ID, &member_id).await;
    let _ = member_ws.drain().await;

    // Duplicate cardIds -> 400.
    let dup_bad = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/views/{v_id}/card-moves", board.id),
            &json!({"cardIds": [c1_id, c1_id]}),
        )
        .await;
    assert_eq!(dup_bad.status, StatusCode::BAD_REQUEST);

    // Valid card-moves with person assignment -> WS sends view BEFORE cards, plus notification!
    let moved: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/views/{v_id}/card-moves", board.id),
            &json!({
                "cardIds": [c2_id, c1_id],
                "afterCardId": "",
                "groupPropertyId": assignee_prop,
                "groupValue": member_id,
            }),
        )
        .await
        .ok_json();
    assert_eq!(moved["cards"].as_array().map(Vec::len), Some(2));
    let events = member_ws.drain().await;
    let block_events: Vec<&str> = events
        .iter()
        .filter(|m| m["type"] == json!("blockChanged"))
        .filter_map(|m| m["block"]["type"].as_str())
        .collect();
    assert_eq!(
        block_events.first().copied(),
        Some("view"),
        "view blockChanged must precede card blockChanged: {block_events:?}"
    );
    tend_notifications(&ts.app).await;
    let unread: Value = ts.client2.get("/notifications/unread").await.ok_json();
    assert!(
        unread["count"].as_i64().unwrap_or(0) >= 1,
        "assignee notified"
    );

    // Batch delete & batch restore.
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/cards/batch-delete", board.id),
            &json!({"cardIds": [c1_id, c2_id]}),
        )
        .await
        .check_no_content();
    let restored: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards/batch-restore", board.id),
            &json!({"cardIds": [c1_id, c2_id]}),
        )
        .await
        .ok_json();
    assert_eq!(restored["cards"].as_array().map(Vec::len), Some(2));

    verify_card_comments_and_attachments(&ts, &board.id, &c1_id, &admin_id).await;
    ts.teardown().await;
}

async fn verify_card_comments_and_attachments(
    ts: &TestServer,
    board_id: &str,
    card_id: &str,
    _admin_id: &str,
) {
    let comments_url = format!("{TEST_WS}/boards/{board_id}/cards/{card_id}/comments");
    let created: Value = ts
        .client2
        .post(&comments_url, &json!({"body": text_doc("hello card")}))
        .await
        .created_json();
    let comment_id = created["id"].as_str().expect("comment id");
    let patched: Value = ts
        .client2
        .patch(
            &format!("{comments_url}/{comment_id}"),
            &json!({"body": text_doc("edited card"), "readAt": created["updatedAt"]}),
        )
        .await
        .ok_json();
    assert!(patched["editedAt"].as_i64().unwrap_or(0) > 0);
    let versions: Vec<Value> = ts
        .client2
        .get(&format!("{comments_url}/{comment_id}/versions"))
        .await
        .ok_json();
    assert_eq!(versions.len(), 1);
    ts.client2
        .delete(&format!("{comments_url}/{comment_id}"))
        .await
        .check_no_content();
    ts.client2
        .post(&format!("{comments_url}/{comment_id}/restore"), &json!({}))
        .await
        .check_ok();

    // Verified attachment rejects file uploaded to a different board.
    let other_board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerVisibility::Private)
        .await;
    let foreign_up = upload(&ts.client, &other_board.id, "other.txt", b"foreign").await;
    let foreign_file = uploaded_id(&foreign_up);
    let rejected = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{board_id}/cards/{card_id}/attachments"),
            &json!({"fileId": foreign_file}),
        )
        .await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
}

/// 4) Cross-workspace container copy (`allowCopyOut`, non-member 403, external `[[` and
/// absent `@mention` sanitization, `container_copied_out` audit log) and `'Today'` hub.
#[tokio::test]
async fn cross_workspace_copy_and_today_hub_enforce_boundaries() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let user2_id = ts.client2.me().await.id;
    let workspaces: Vec<UserWorkspace> = ts.client.get("/users/me/workspaces").await.ok_json();
    let personal_ws = workspaces
        .iter()
        .find(|w| w.kind == OrgKind::Personal)
        .expect("personal workspace")
        .id
        .clone();

    let other_board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerVisibility::Open)
        .await;
    let external_card: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", other_board.id),
            &json!({"title": "External Target"}),
        )
        .await
        .created_json();
    let ext_id = external_card["id"].as_str().expect("ext id");

    let source_board: Container = ts
        .client
        .post(
            &format!("{TEST_WS}/boards"),
            &json!({"visibility": "open", "title": "Team Specs", "scaffold": true}),
        )
        .await
        .created_json();
    let rich_body = json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [
                {"type": "reference", "attrs": {"id": ext_id}},
                {"type": "text", "text": " "},
                {"type": "mention", "attrs": {"userId": user2_id}}
            ]
        }]
    });
    let today_iso = "2026-01-01";
    let due_prop = source_board.due_date_property_id.clone().expect("due prop");
    let assignee_prop = source_board
        .card_properties
        .iter()
        .find(|p| p.property_type == fvoci_model::PropertyType::Person)
        .expect("person prop")
        .id
        .clone();
    let admin_id = ts.client.me().await.id;
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", source_board.id),
            &json!({
                "title": "Due Task",
                "body": rich_body,
                "properties": {
                    due_prop: json!({"from": today_iso}),
                    assignee_prop: admin_id,
                }
            }),
        )
        .await
        .check_created();

    // Non-member on open board cannot copy-to-workspace -> 403!
    let user2_wss: Vec<UserWorkspace> = ts.client2.get("/users/me/workspaces").await.ok_json();
    let user2_personal = user2_wss
        .iter()
        .find(|w| w.kind == OrgKind::Personal)
        .expect("user2 personal")
        .id
        .clone();
    let non_member_copy = ts
        .client2
        .post(
            &format!("{TEST_WS}/containers/{}/copy-to-workspace", source_board.id),
            &json!({"targetWorkspaceId": user2_personal}),
        )
        .await;
    assert_eq!(non_member_copy.status, StatusCode::FORBIDDEN);

    // Member copies to personal space -> external cardLink & absent user2 mention become plain text!
    let copied: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/containers/{}/copy-to-workspace", source_board.id),
            &json!({"targetWorkspaceId": personal_ws, "title": "My Personal Copy"}),
        )
        .await
        .created_json();
    let copied_board_id = copied["container"]["id"].as_str().expect("copied id");
    let copied_cards: Vec<Value> = ts
        .client
        .get(&format!("/w/{personal_ws}/boards/{copied_board_id}/cards"))
        .await
        .ok_json();
    let copied_card_id = copied_cards[0]["id"].as_str().expect("copied card id");
    let copied_detail: Value = ts
        .client
        .get(&format!("/w/{personal_ws}/cards/{copied_card_id}"))
        .await
        .ok_json();
    let body_str = copied_detail["body"].to_string();
    assert!(
        !body_str.contains("\"reference\"") && !body_str.contains("\"mention\""),
        "external reference and non-target mention must be sanitized to text: {body_str}"
    );

    // Audit log records container_copied_out, and disabling allowCopyOut returns 403 copy_out_disabled.
    let org_id = org_of(&ts).await;
    let audit: Value = ts
        .client
        .get(&format!("/org/{org_id}/audit"))
        .await
        .ok_json();
    assert!(
        audit.to_string().contains("container_copied_out"),
        "audit log records container_copied_out: {audit}"
    );
    ts.client
        .patch(&format!("/org/{org_id}"), &json!({"allowCopyOut": false}))
        .await
        .check_ok();
    let disabled = ts
        .client
        .post(
            &format!("{TEST_WS}/containers/{}/copy-to-workspace", source_board.id),
            &json!({"targetWorkspaceId": personal_ws}),
        )
        .await;
    assert_eq!(disabled.status, StatusCode::FORBIDDEN);
    assert_eq!(disabled.problem().1["reason"], json!("copy_out_disabled"));

    // Account-wide and workspace-scoped Today hub return the overdue assigned card!
    let me_today: Value = ts.client.get("/users/me/today").await.ok_json();
    assert_eq!(me_today["lockedWorkspaceCount"], json!(0));
    assert!(
        me_today["cards"]
            .as_array()
            .is_some_and(|cards| cards.len() >= 2),
        "today hub includes overdue cards from shared and personal workspaces: {me_today}"
    );
    ts.teardown().await;
}

/// 5) Benchmark: 50 open boards × 200 concurrent WebSocket connections in one workspace.
/// Measures single-card-edit fanout latency across all 200 connections.
#[tokio::test]
async fn open_board_50_boards_200_connections_delivery_benchmark() {
    let ts = Setup::native().start().await.init_basic().await;
    let user2_id = ts.client2.me().await.id;

    let mut target_card_id = String::new();
    for idx in 0..50 {
        let board: Container = ts
            .client
            .post(
                &format!("{TEST_WS}/boards"),
                &json!({"visibility": "open", "title": format!("Open {idx}")}),
            )
            .await
            .created_json();
        let card: Value = ts
            .client
            .post(
                &format!("{TEST_WS}/boards/{}/cards", board.id),
                &json!({"title": format!("Card {idx}")}),
            )
            .await
            .created_json();
        if idx == 25 {
            target_card_id = card["id"].as_str().expect("card id").to_owned();
        }
    }

    let mut sockets = Vec::with_capacity(200);
    for _ in 0..200 {
        sockets
            .push(WorkspaceSocket::subscribe(&ts, &ts.client2, TEST_WORKSPACE_ID, &user2_id).await);
    }

    let started = Instant::now();
    ts.client
        .patch(
            &format!("{TEST_WS}/cards/{target_card_id}"),
            &json!({"title": "Benchmarked Update"}),
        )
        .await
        .check_ok();
    for sock in &mut sockets {
        sock.expect("blockChanged", "/block/id", &target_card_id)
            .await;
    }
    let elapsed = started.elapsed();
    eprintln!(
        "[server-authority benchmark] 50 open boards x 200 WS connections: card edit + 200-way delivery completed in {:.2} ms",
        elapsed.as_secs_f64() * 1000.0
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "50x200 open-board WS fanout took too long: {elapsed:?}"
    );
    ts.teardown().await;
}
