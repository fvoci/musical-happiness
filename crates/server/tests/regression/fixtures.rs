//! Request bodies and setup steps the scenarios share.

use serde_json::{Value, json};

use crate::common::ROOT_WORKSPACE_ID;
use crate::harness::ROOT_API;
use crate::harness::{Session, string_member};

/// A one-pixel PNG.
pub(crate) const PNG: [u8; 67] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x00, 0x00, 0x02,
    0x00, 0x00, 0x01, 0xe2, 0x21, 0xbc, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

/// Whether a file name ends in exactly this extension, in this case.
pub(crate) fn has_extension(name: &str, extension: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, found)| found == extension)
}

/// The `id` member of a JSON object.
#[track_caller]
pub(crate) fn id_of(value: &Value) -> String {
    string_member(value, "id")
}

/// `base` with the members of `changes` set on it. Both must be JSON objects.
#[track_caller]
pub(crate) fn merged(base: &Value, changes: Value) -> Value {
    let mut result = base.clone();
    let members = result.as_object_mut().expect("the base is a JSON object");
    let Value::Object(changes) = changes else {
        panic!("the changes are no JSON object: {changes}");
    };
    members.extend(changes);
    result
}

/// A block as a client sends it. An empty `parent` means the board itself; `fields` is
/// the block's `fields` object.
pub(crate) fn block(
    id: &str,
    board: &str,
    block_type: &str,
    title: &str,
    parent: &str,
    fields: Value,
) -> Value {
    let mut block = json!({
        "id": id,
        "boardId": board,
        "parentId": if parent.is_empty() { board } else { parent },
        "type": block_type,
        "title": title,
        "schema": 1,
        "createAt": 1,
        "updateAt": 1,
        "deleteAt": 0,
    });
    block["fields"] = fields;
    block
}

/// The role flags of a board member.
#[track_caller]
pub(crate) fn role_flags(role: &str) -> Value {
    match role {
        "admin" => json!({"schemeAdmin": true}),
        "editor" => json!({"schemeEditor": true}),
        "commenter" => json!({"schemeCommenter": true}),
        "viewer" => json!({"schemeViewer": true}),
        other => panic!("no role {other:?}"),
    }
}

/// Creates a board as `user`, recorded as `create board <title>`, and returns its
/// identifier. `extra` holds further members of the request body.
pub(crate) async fn new_board(
    s: &mut Session,
    user: &str,
    title: &str,
    board_type: &str,
    extra: Value,
) -> String {
    let body = merged(
        &json!({
            "workspaceId": ROOT_WORKSPACE_ID,
            "type": board_type,
            "title": title,
            "properties": {},
            "cardProperties": [],
        }),
        extra,
    );
    s.post(
        &format!("create board {title}"),
        &format!("{ROOT_API}/boards"),
    )
    .json(body)
    .user(user)
    .await
    .string("id")
}

/// Inserts blocks as `user`, recorded as `name`, and returns the created blocks.
pub(crate) async fn new_blocks(
    s: &mut Session,
    user: &str,
    board: &str,
    blocks: Value,
    name: &str,
) -> Value {
    s.post(name, &format!("{ROOT_API}/boards/{board}/blocks"))
        .json(blocks)
        .user(user)
        .await
        .json()
        .clone()
}

/// Adds `user` to the board with a role, as `admin`. Recorded as `add <role>`.
pub(crate) async fn add_member(s: &mut Session, admin: &str, board: &str, user: &str, role: &str) {
    let body = merged(
        &json!({"boardId": board, "userId": s.user_id(user).await}),
        role_flags(role),
    );
    s.post(
        &format!("add {role}"),
        &format!("{ROOT_API}/boards/{board}/members"),
    )
    .json(body)
    .user(admin)
    .await;
}
