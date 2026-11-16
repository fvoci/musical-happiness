//! File uploads and full archive round trips in the founder's own workspace.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use reqwest::multipart::{Form, Part};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::super::routes::archive_entries;
use super::Case;

pub(super) async fn exercise(case: &mut Case, board: &str) {
    let at = &[("boardId", board)];
    let payload = format!("{} private file", case.label).into_bytes();
    let filename = format!("{}-private.txt", case.label);
    let request = case.request("uploadFile", at).multipart(Form::new().part(
        "file",
        Part::bytes(payload.clone()).file_name(filename.clone()),
    ));
    let uploaded = case.send("uploadFile", request).await.value();
    let file = case.remember(&uploaded, "fileId");
    let file_at = &[("boardId", board), ("fileId", file.as_str())];
    let received = case.run("getFile", file_at, json!({})).await;
    assert_eq!(
        received.body, payload,
        "the file remains in its organization"
    );
    let info = case
        .present("getFileInfo", file_at, &filename)
        .await
        .value();
    assert_eq!(
        info["size"],
        payload.len(),
        "file metadata matches its contents"
    );
    let payload = format!("{} private multipart file", case.label).into_bytes();
    let upload = case
        .run(
            "createUpload",
            at,
            json!({"name": "parts.txt", "size": payload.len()}),
        )
        .await
        .value();
    let upload = case.remember(&upload, "uploadId");
    let upload_at = &[("uploadId", upload.as_str())];
    let state = case.present("getUpload", upload_at, &upload).await.value();
    assert_eq!(state["containerId"], board, "upload belongs to this board");
    part(case, &upload, &payload).await;
    let completed = case
        .run("completeUpload", upload_at, json!({}))
        .await
        .value();
    let file = case.remember(&completed, "fileId");
    let received = case
        .run(
            "getFile",
            &[("boardId", board), ("fileId", &file)],
            json!({}),
        )
        .await;
    assert_eq!(
        received.body, payload,
        "completed parts remain in their organization"
    );
    let abandoned = case
        .run(
            "createUpload",
            at,
            json!({"name": "abandoned.txt", "size": 0}),
        )
        .await
        .value();
    let abandoned = case.remember(&abandoned, "uploadId");
    case.run("deleteUpload", &[("uploadId", &abandoned)], json!({}))
        .await;
    archives(case, board).await;
}

async fn part(case: &mut Case, upload: &str, bytes: &[u8]) {
    let digest = format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(bytes)));
    let request = case
        .request(
            "putUploadPart",
            &[("uploadId", upload), ("partNumber", "1")],
        )
        .header("Content-Type", "application/octet-stream")
        .header("Content-Digest", digest)
        .body(bytes.to_vec());
    case.send("putUploadPart", request).await;
}

async fn archives(case: &mut Case, board: &str) {
    let archive = case
        .run("exportContainerArchive", &[("boardId", board)], json!({}))
        .await;
    assert!(
        archive.body.starts_with(b"PK"),
        "a board export is an archive"
    );
    let all = case.run("exportWorkspaceArchive", &[], json!({})).await;
    assert!(
        all.body.starts_with(b"PK"),
        "a workspace export is an archive"
    );
    check_archive(case, &archive.body);
    check_archive(case, &all.body);
    let upload = case
        .run(
            "createImportUpload",
            &[],
            json!({"name": "parity.boardarchive", "size": archive.body.len()}),
        )
        .await
        .value();
    let upload = case.remember(&upload, "uploadId");
    part(case, &upload, &archive.body).await;
    case.run("importArchive", &[], json!({"uploadId": upload}))
        .await;
    let boards = case.run("getWorkspaceBoards", &[], json!({})).await.value();
    let copies = boards
        .as_array()
        .expect("boards")
        .iter()
        .filter(|candidate| candidate["title"] == format!("secret {} renamed", case.label))
        .count();
    assert_eq!(copies, 2, "import makes a second copy in this workspace");
}

fn check_archive(case: &Case, bytes: &[u8]) {
    let entries = archive_entries(bytes);
    for (_, contents) in &entries {
        case.assert_is_ours("exported archive", contents);
    }
    assert!(
        entries
            .iter()
            .any(|(_, contents)| String::from_utf8_lossy(contents).contains(&case.label)),
        "archive contains this organization's board"
    );
}
