//! Imports through an upload in parts (`docs/s6-design.md` 4.2, 5.6, 9): the archive
//! is sent as a file is, imported by naming the upload, held to the limits of an
//! archive, and nothing of the upload stays behind.

use std::io::{Cursor, Write as _};
use std::path::PathBuf;

use fvoci_model::{Block, BlockType, BoardType, new_id};
use reqwest::StatusCode;
use serde_json::json;

use crate::common::{Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer};
use crate::export::{boards_of, import, import_upload, put_part};

/// The smallest part size, so that an archive of two parts stays small.
const PART: usize = 5 * 1024 * 1024;

/// The directory under which the parts of uploads are kept.
fn uploads_dir(ts: &TestServer) -> PathBuf {
    ts.app.config().tmp_dir().join("uploads")
}

/// What is in a directory, by name; nothing if it is not there.
fn entries(directory: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(directory)
        .map(|read| {
            read.map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
        })
        .unwrap_or_default()
}

/// Opens an upload of an archive of `size` bytes into `team_id`.
async fn open_import(client: &Client, team_id: &str, size: usize) -> crate::common::ApiResponse {
    client
        .post(
            &format!("/teams/{team_id}/archive/uploads"),
            &json!({"name": "archive.boardarchive", "size": size}),
        )
        .await
}

/// The identifier of an upload that was opened.
fn upload_id(response: &crate::common::ApiResponse) -> String {
    response.check_created();
    response.value()["uploadId"]
        .as_str()
        .expect("an upload has an identifier")
        .to_owned()
}

/// Bytes that do not compress, so that an archive holding them is as large as they are.
fn incompressible(size: usize) -> Vec<u8> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    (0..size)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// An archive of version 3 with one board and the given entries beside its
/// `board.jsonl`, each written with `method`.
fn archive_with(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
    let board = json!({"type": "board", "data": {
        "id": "b1", "teamId": TEST_TEAM_ID, "type": "P", "title": "Limits", "createAt": 1, "updateAt": 1,
    }});
    let options = zip::write::SimpleFileOptions::default().compression_method(method);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    writer.start_file("b1/board.jsonl", options).expect("start");
    writer
        .write_all(format!("{board}\n").as_bytes())
        .expect("write");
    for (name, content) in entries {
        writer.start_file(*name, options).expect("start");
        writer.write_all(content).expect("write");
    }
    writer.finish().expect("finish").into_inner()
}

/// An archive of two parts is imported whole, and its upload leaves nothing on the
/// disk.
#[tokio::test]
async fn an_archive_in_parts_is_imported_and_leaves_nothing() {
    let ts = Setup::native()
        .configure(|config| config.upload_part_bytes = 5 * 1024 * 1024)
        .start()
        .await
        .init_basic()
        .await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    // A file is at most a part when sent whole: two of them make an archive of two.
    let data = incompressible(PART);
    let mut attachments = Vec::new();
    for half in data.chunks(PART / 2 + 1024) {
        let response = crate::files::upload(&ts.client, &board.id, "half.bin", half).await;
        response.check_created();
        let file_id = crate::files::uploaded_id(&response);
        attachments.push(Block {
            id: new_id(),
            parent_id: board.id.clone(),
            board_id: board.id.clone(),
            block_type: BlockType::Attachment,
            fields: json!({"fileId": file_id})
                .as_object()
                .expect("an object")
                .clone(),
            create_at: 1,
            update_at: 1,
            ..Block::default()
        });
    }
    ts.client
        .insert_blocks(&board.id, &attachments)
        .await
        .check_created();
    let archive = ts
        .client
        .get(&format!("/boards/{}/archive/export", board.id))
        .await
        .body;
    assert!(archive.len() > PART, "the archive comes in two parts");

    let before = boards_of(&ts).await.len();
    import(&ts.client, TEST_TEAM_ID, &archive)
        .await
        .check_no_content();
    assert_eq!(boards_of(&ts).await.len(), before + 1, "the board is made");
    assert_eq!(
        entries(&uploads_dir(&ts)),
        Vec::<String>::new(),
        "nothing stays"
    );

    // An import that fails leaves nothing either.
    import(&ts.client, TEST_TEAM_ID, b"not an archive")
        .await
        .check_bad_request();
    assert_eq!(
        entries(&uploads_dir(&ts)),
        Vec::<String>::new(),
        "nothing stays"
    );
    ts.teardown().await;
}

/// An upload of an archive is held to what an import takes, and is imported only into
/// its team, by `importArchive`; one of a file is completed only by `completeUpload`.
#[tokio::test]
async fn an_upload_of_an_archive_is_kept_to_its_purpose() {
    let ts = Setup::native()
        .configure(|config| config.max_import_bytes = 1000)
        .start()
        .await
        .init_basic()
        .await;
    let config = ts.client.get("/client-config").await;
    assert_eq!(config.value()["maxImportSize"], json!(1000));

    let response = open_import(&ts.client, TEST_TEAM_ID, 1001).await;
    response.check_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.value()["limit"], json!(1000));

    let archive = archive_with(&[], zip::CompressionMethod::Stored);
    let id = upload_id(&open_import(&ts.client, TEST_TEAM_ID, archive.len()).await);
    let state = ts.client.get(&format!("/uploads/{id}")).await;
    assert_eq!(state.value()["purpose"], json!("import"));
    assert_eq!(state.value()["teamId"], json!(TEST_TEAM_ID));
    assert_eq!(state.value()["boardId"], json!(""));

    // A part missing: refused, and the upload stays open for it.
    let response = import_upload(&ts.client, TEST_TEAM_ID, &id).await;
    response.check_bad_request();
    put_part(&ts.client, &id, 1, &archive)
        .await
        .check_status(StatusCode::NO_CONTENT);
    let response = ts
        .client
        .post(&format!("/uploads/{id}/complete"), &json!({}))
        .await;
    response.check_bad_request();
    assert_eq!(response.value()["field"], json!("uploadId"));
    let response = import_upload(&ts.client, ROOT_TEAM_ID, &id).await;
    response.check_bad_request();
    assert_eq!(response.value()["field"], json!("uploadId"));
    import_upload(&ts.client2, TEST_TEAM_ID, &id)
        .await
        .check_status(StatusCode::NOT_FOUND);

    import_upload(&ts.client, TEST_TEAM_ID, &id)
        .await
        .check_no_content();
    import_upload(&ts.client, TEST_TEAM_ID, &id)
        .await
        .check_status(StatusCode::NOT_FOUND);

    // An upload of a file is not imported.
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = ts
        .client
        .post(
            &format!("/boards/{}/uploads", board.id),
            &json!({"name": "a.bin", "size": 3}),
        )
        .await;
    let file_upload = upload_id(&response);
    let response = import_upload(&ts.client, TEST_TEAM_ID, &file_upload).await;
    response.check_bad_request();
    assert_eq!(response.value()["field"], json!("uploadId"));
    ts.teardown().await;
}

/// An archive over a limit of what it unpacks to is 413 with the limit, and nothing of
/// it is stored.
#[tokio::test]
async fn an_archive_over_its_limits_stores_nothing() {
    let ts = Setup::native()
        .configure(|config| config.max_file_size = 1000)
        .start()
        .await
        .init_basic()
        .await;
    let before = boards_of(&ts).await.len();

    let file = vec![7; 1001];
    let too_large_file = archive_with(&[("b1/large.bin", &file)], zip::CompressionMethod::Deflated);
    let response = import(&ts.client, TEST_TEAM_ID, &too_large_file).await;
    response.check_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.value()["limit"], json!(1000));

    let names: Vec<String> = (0..10_000).map(|index| format!("b1/{index}.bin")).collect();
    let many: Vec<(&str, &[u8])> = names.iter().map(|name| (name.as_str(), &b""[..])).collect();
    let too_many = archive_with(&many, zip::CompressionMethod::Stored);
    let response = import(&ts.client, TEST_TEAM_ID, &too_many).await;
    response.check_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.value()["limit"], json!(10_000));

    let jsonl = archive_with(&[], zip::CompressionMethod::Deflated);
    let mut zip = zip::ZipArchive::new(Cursor::new(jsonl)).expect("a ZIP");
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .raw_copy_file(zip.by_name("version.json").expect("the version"))
        .expect("copy");
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file("b1/board.jsonl", deflated)
        .expect("start");
    writer
        .write_all(&vec![b' '; 64 * 1024 * 1024 + 1])
        .expect("write");
    let large_jsonl = writer.finish().expect("finish").into_inner();
    let response = import(&ts.client, TEST_TEAM_ID, &large_jsonl).await;
    response.check_status(StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.value()["limit"], json!(64 * 1024 * 1024));

    assert_eq!(boards_of(&ts).await.len(), before, "no board was made");
    assert_eq!(
        entries(&uploads_dir(&ts)),
        Vec::<String>::new(),
        "nothing stays"
    );
    ts.teardown().await;
}
