//! Read-token HTTP paths use the token's board and never reveal people.

use reqwest::multipart::{Form, Part};
use serde_json::json;

use super::Case;

pub(super) async fn exercise(case: &mut Case) {
    let made = case
        .run(
            "createBoard",
            &[],
            json!({"title": format!("{} shared", case.label), "type": "P"}),
        )
        .await
        .value();
    let board = case.remember(&made, "id");
    let at = &[("boardId", board.as_str())];
    let made = case
        .run(
            "createCard",
            at,
            json!({"title": format!("{} shared card", case.label)}),
        )
        .await
        .value();
    let card = case.remember(&made, "id");
    let sharing = case
        .run("setSharing", at, json!({"enabled": true}))
        .await
        .value();
    let token = sharing["token"].as_str().expect("read token");
    for (operation, expected) in [
        ("getSharedBoard", &board),
        ("getSharedBlocks", &card),
        ("getSharedCards", &card),
    ] {
        let request = super::with_query(case.request(operation, at), "readToken", token);
        let response = case.send(operation, request).await;
        assert!(
            response.text().contains(expected),
            "{operation} returns its own content"
        );
        assert!(
            !response.text().contains(&case.user),
            "{operation} hides user IDs even with a session"
        );
        assert!(
            !response.text().contains(&case.workspace),
            "{operation} hides the workspace"
        );
    }
    let payload = format!("{} shared file", case.label).into_bytes();
    let name = format!("{}-shared.txt", case.label);
    let request = case
        .request("uploadFile", at)
        .multipart(Form::new().part("file", Part::bytes(payload.clone()).file_name(name.clone())));
    let uploaded = case.send("uploadFile", request).await.value();
    let file = case.remember(&uploaded, "fileId");
    let at = &[("boardId", board.as_str()), ("fileId", file.as_str())];
    let request = super::with_query(case.request("getSharedFile", at), "readToken", token);
    let read = case.send("getSharedFile", request).await;
    assert_eq!(
        read.body, payload,
        "public file belongs to this organization"
    );
    let request = super::with_query(case.request("getSharedFileInfo", at), "readToken", token);
    let info = case.send("getSharedFileInfo", request).await.value();
    assert_eq!(info["name"], name, "public file metadata names this upload");
}
