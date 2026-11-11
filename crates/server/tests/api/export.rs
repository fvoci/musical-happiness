//! Archive export and import.
//!
//! Ports `server/integrationtests/export_test.go` (`TestExportBoard`) and the behaviour
//! of `server/app/import_test.go`, checked against the oracle.

use std::io::{Cursor, Read as _, Write as _};

use crate::common::{ApiResponse, Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fvoci_model::{Block, BlockType, Board, BoardType, new_id};
use reqwest::{Method, StatusCode};
use sha2::{Digest as _, Sha256};

/// `GET /boards/{id}/archive/export` for a board.
async fn export_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .get(&format!("/boards/{board_id}/archive/export"))
        .await
}

/// Imports archive bytes into a team: opens an upload of the archive, sends its parts
/// and imports it. The first answer that is not a success is returned, else the
/// import's.
pub(crate) async fn import(client: &Client, team_id: &str, bytes: &[u8]) -> ApiResponse {
    let opened = client
        .post(
            &format!("/teams/{team_id}/archive/uploads"),
            &serde_json::json!({"name": "archive.boardarchive", "size": bytes.len()}),
        )
        .await;
    if opened.status != StatusCode::CREATED {
        return opened;
    }
    let upload_id = opened.value()["uploadId"]
        .as_str()
        .expect("an upload has an identifier")
        .to_owned();
    let part_size = opened.value()["partSize"]
        .as_u64()
        .and_then(|size| usize::try_from(size).ok())
        .expect("an upload has a part size");
    for (index, part) in bytes.chunks(part_size).enumerate() {
        let sent = put_part(client, &upload_id, index + 1, part).await;
        if sent.status != StatusCode::NO_CONTENT {
            return sent;
        }
    }
    import_upload(client, team_id, &upload_id).await
}

/// `importArchive` of an upload.
pub(crate) async fn import_upload(client: &Client, team_id: &str, upload_id: &str) -> ApiResponse {
    client
        .post(
            &format!("/teams/{team_id}/archive/import"),
            &serde_json::json!({"uploadId": upload_id}),
        )
        .await
}

/// Sends `bytes` as part `number` of an upload, with their digest.
pub(crate) async fn put_part(
    client: &Client,
    upload_id: &str,
    number: usize,
    bytes: &[u8],
) -> ApiResponse {
    let digest = format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(bytes)));
    let request = client
        .request(Method::PUT, &format!("/uploads/{upload_id}/parts/{number}"))
        .header("Content-Type", "application/octet-stream")
        .header("Content-Digest", digest)
        .body(bytes.to_vec());
    Client::send(request).await
}

/// Makes a board with one card for the first client.
async fn board_with_card(ts: &TestServer, team_id: &str) -> Board {
    let board = ts.create_board(team_id, BoardType::Open).await;
    let card = Block {
        id: new_id(),
        parent_id: board.id.clone(),
        board_id: board.id.clone(),
        block_type: BlockType::Card,
        title: "a card for export".to_owned(),
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    ts.client
        .insert_blocks(&board.id, std::slice::from_ref(&card))
        .await
        .check_created();
    board
}

/// `TestExportBoard/"export single board"`: a board exports, re-imports and keeps its
/// card.
#[tokio::test]
async fn export_single_board_round_trips() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    assert!(!exported.body.is_empty(), "the archive has bytes");

    import(&ts.client, ROOT_TEAM_ID, &exported.body)
        .await
        .check_no_content();

    let user_id = ts.client.me().await.id;
    let boards = ts
        .app
        .boards_for_user_and_team(&user_id, ROOT_TEAM_ID, fvoci_store::PublicBoards::Include)
        .await
        .expect("the imported boards");
    assert!(!boards.is_empty(), "a board was imported into team 0");
    let board_id = boards[0].id.clone();
    let blocks = ts
        .app
        .blocks_for_board(&board_id)
        .await
        .expect("the imported blocks");
    assert!(
        blocks
            .iter()
            .any(|block| block.title == "a card for export"),
        "the imported board keeps its card"
    );
    ts.teardown().await;
}

/// A board without blocks exports and imports as a board without blocks. (The original
/// exports it and then refuses its own archive.)
#[tokio::test]
async fn export_of_a_board_without_blocks_round_trips() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    import(&ts.client, ROOT_TEAM_ID, &exported.body)
        .await
        .check_no_content();

    let user_id = ts.client.me().await.id;
    let boards = ts
        .app
        .boards_for_user_and_team(&user_id, ROOT_TEAM_ID, fvoci_store::PublicBoards::Include)
        .await
        .expect("the imported boards");
    assert_eq!(boards.len(), 1, "the board was imported into team 0");
    let blocks = ts
        .app
        .blocks_for_board(&boards[0].id)
        .await
        .expect("the imported blocks");
    assert!(blocks.is_empty(), "without blocks");
    ts.teardown().await;
}

/// The export headers are the original's.
#[tokio::test]
async fn export_has_the_download_headers() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;
    let response = export_board(&ts.client, &board.id).await;
    response.check_ok();
    assert_eq!(
        response.header("content-type"),
        "application/octet-stream",
        "content type"
    );
    assert!(
        response
            .header("content-disposition")
            .starts_with("attachment; filename=archive-"),
        "disposition: {}",
        response.header("content-disposition")
    );
    assert!(
        response
            .header("content-disposition")
            .ends_with(".boardarchive"),
        "the extension"
    );
    assert_eq!(
        response.header("content-transfer-encoding"),
        "binary",
        "transfer encoding"
    );

    // The body is a ZIP with version 3 and the board's directory.
    let mut zip = zip::ZipArchive::new(Cursor::new(response.body.clone())).expect("a zip");
    let mut version = String::new();
    zip.by_name("version.json")
        .expect("version.json")
        .read_to_string(&mut version)
        .expect("read");
    assert!(version.contains("\"version\":3"), "version 3: {version}");
    assert!(
        (0..zip.len()).any(|index| {
            zip.by_index(index)
                .expect("entry")
                .name()
                .is_ok_and(|name| name.ends_with("board.jsonl"))
        }),
        "a board.jsonl is present"
    );
    ts.teardown().await;
}

/// Export is allowed for members and refused for non-members and anonymous clients.
#[tokio::test]
async fn export_board_access_control() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;

    export_board(&ts.client, &board.id).await.check_ok();
    export_board(&ts.client2, &board.id).await.check_forbidden();
    export_board(&ts.anonymous(), &board.id)
        .await
        .check_unauthorized();
    ts.teardown().await;
}

/// `GET /teams/0/archive/export` is a 200 ZIP (verified on the oracle), and anonymous is
/// refused.
#[tokio::test]
async fn export_team_is_a_zip() {
    let ts = Setup::native().start().await.init_basic().await;
    board_with_card(&ts, ROOT_TEAM_ID).await;
    let response = ts
        .client
        .get(&format!("/teams/{ROOT_TEAM_ID}/archive/export"))
        .await;
    response.check_ok();
    assert!(
        zip::ZipArchive::new(Cursor::new(response.body.clone())).is_ok(),
        "a zip"
    );
    ts.anonymous()
        .get(&format!("/teams/{ROOT_TEAM_ID}/archive/export"))
        .await
        .check_unauthorized();
    ts.teardown().await;
}

/// Any signed-in user may import; an anonymous one may not.
#[tokio::test]
async fn import_is_allowed_for_any_signed_in_user() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, ROOT_TEAM_ID).await;
    let archive = export_board(&ts.client, &board.id).await.body;

    import(&ts.client2, ROOT_TEAM_ID, &archive)
        .await
        .check_no_content();
    import(&ts.anonymous(), ROOT_TEAM_ID, &archive)
        .await
        .check_unauthorized();
    ts.teardown().await;
}

/// `TestApp_ImportArchive`: something that is not an archive is the sender's mistake, a
/// bad request. (The original answers 500.)
#[tokio::test]
async fn import_of_a_non_archive_is_a_bad_request() {
    let ts = Setup::native().start().await.init_basic().await;
    let response = import(&ts.client, ROOT_TEAM_ID, b"this is not a zip file").await;
    response.check_bad_request();
    assert_eq!(response.error_code(), "bad_request", "{}", response.text());
    ts.teardown().await;
}

/// An empty ZIP is no archive either.
#[tokio::test]
async fn import_of_an_empty_zip_is_a_bad_request() {
    let ts = Setup::native().start().await.init_basic().await;
    let empty = zip::ZipWriter::new(Cursor::new(Vec::new()))
        .finish()
        .expect("an empty zip")
        .into_inner();
    let response = import(&ts.client, ROOT_TEAM_ID, &empty).await;
    response.check_bad_request();
    assert_eq!(response.error_code(), "bad_request", "{}", response.text());
    ts.teardown().await;
}

/// Nor is an archive of a version this server does not read.
#[tokio::test]
async fn import_of_an_unsupported_version_is_a_bad_request() {
    let ts = Setup::native().start().await.init_basic().await;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("version.json", zip::write::SimpleFileOptions::default())
        .expect("start");
    writer
        .write_all(br#"{"version":99,"date":1}"#)
        .expect("write");
    let bytes = writer.finish().expect("finish").into_inner();
    let response = import(&ts.client, ROOT_TEAM_ID, &bytes).await;
    response.check_bad_request();
    assert_eq!(response.error_code(), "bad_request", "{}", response.text());
    ts.teardown().await;
}

/// A hostile ZIP whose entry name escapes its directory is imported harmlessly: the file
/// name is reduced to its base, so nothing is written outside the uploads root, and the
/// orphan image (no board) is skipped.
#[tokio::test]
async fn import_of_a_traversing_entry_is_harmless() {
    let ts = Setup::native().start().await.init_basic().await;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    writer
        .start_file("board1/../../../evil.txt", options)
        .expect("start file");
    writer.write_all(b"pwned").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    // No board directory matches the orphan file, so it is skipped and the import
    // succeeds.
    import(&ts.client, ROOT_TEAM_ID, &bytes)
        .await
        .check_no_content();
    ts.teardown().await;
}

/// Two blocks that name the same file: the original writes the file into the archive
/// twice under one name; the port writes it once. Found when an export of such a board
/// failed. A third block's own file is exported too.
#[tokio::test]
async fn export_of_a_board_whose_blocks_share_a_file() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;
    let attachment = |file: &str| {
        let mut fields = fvoci_model::JsonMap::new();
        fields.insert(
            "fileId".to_owned(),
            serde_json::Value::String(file.to_owned()),
        );
        fvoci_model::Block {
            id: fvoci_model::new_id(),
            board_id: board.id.clone(),
            parent_id: board.id.clone(),
            block_type: fvoci_model::BlockType::Attachment,
            fields,
            create_at: 1,
            update_at: 1,
            ..fvoci_model::Block::default()
        }
    };
    ts.client
        .insert_blocks(&board.id, &[attachment("7aaaaaaaaaaaaaaaaaaaaaaaaaa.png")])
        .await
        .check_created();
    // Another block shares the file, and a third has one that only it shows.
    let mut others = [
        attachment("7aaaaaaaaaaaaaaaaaaaaaaaaaa.png"),
        attachment("7bbbbbbbbbbbbbbbbbbbbbbbbbb.png"),
    ];
    ts.app
        .insert_blocks(
            &mut others,
            &ts.stranger().await,
            fvoci_server::app::ChangeNotification::Send,
        )
        .await
        .expect("the blocks are stored");

    let export = export_board(&ts.client, &board.id).await;
    export.check_ok();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(export.body.clone()))
        .expect("the export is an archive");
    let mut files: Vec<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter(|name| name.ends_with(".png"))
        .map(|name| name.rsplit('/').next().unwrap_or_default().to_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "7aaaaaaaaaaaaaaaaaaaaaaaaaa.png",
            "7bbbbbbbbbbbbbbbbbbbbbbbbbb.png"
        ],
        "the shared file has one entry, and the third block's own file one too"
    );
    ts.teardown().await;
}

/// The boards of a team that the first client sees.
pub(crate) async fn boards_of(ts: &TestServer) -> Vec<Board> {
    ts.client
        .get(&format!("/teams/{TEST_TEAM_ID}/boards"))
        .await
        .ok_json()
}

/// The archives of the original, the line-based version 1 and the ZIP of version 2, and
/// a ZIP without `version.json` are refused, and nothing of them is stored
/// (`docs/s6-design.md` 3.10).
#[tokio::test]
async fn archives_of_the_former_formats_are_refused() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = serde_json::json!({"type": "board", "data": {
        "id": "b1", "teamId": TEST_TEAM_ID, "type": "P", "title": "Former", "createAt": 1, "updateAt": 1,
    }});
    let zip_of = |entries: &[(&str, String)]| {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("start");
            writer.write_all(content.as_bytes()).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    };
    let version_two = zip_of(&[
        ("version.json", r#"{"version":2,"date":1}"#.to_owned()),
        ("b1/board.jsonl", format!("{board}\n")),
    ]);
    let without_version = zip_of(&[("b1/board.jsonl", format!("{board}\n"))]);
    let legacy = format!(
        "{{\"version\":1,\"date\":1}}\n{}\n",
        serde_json::json!({"type": "block", "data": {
            "id": "b1", "type": "board", "title": "Former", "createAt": 1, "updateAt": 1,
        }})
    );
    let before = boards_of(&ts).await.len();
    for (what, bytes) in [
        ("version 2", version_two),
        ("no version.json", without_version),
        ("version 1", legacy.into_bytes()),
    ] {
        let response = import(&ts.client, TEST_TEAM_ID, &bytes).await;
        response.check_bad_request();
        assert_eq!(
            response.error_code(),
            "bad_request",
            "{what}: {}",
            response.text()
        );
    }
    assert_eq!(boards_of(&ts).await.len(), before, "no board was made");
    ts.teardown().await;
}

/// An archive carries the files that the images of bodies show, and an import points
/// the bodies at the board's new files. An image block without a file, which an export
/// used to answer with 500, is exported as it is.
#[tokio::test]
async fn an_archive_carries_the_images_of_bodies() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let file_id = crate::files::uploaded_id(
        &crate::files::upload(&ts.client, &board.id, "picture.png", b"png").await,
    );
    let body = serde_json::json!({"type": "doc", "content": [
        {"type": "image", "attrs": {"fileId": file_id}},
    ]});
    ts.client
        .post(
            &format!("/boards/{}/cards", board.id),
            &serde_json::json!({"title": "Card", "body": body}),
        )
        .await
        .check_status(reqwest::StatusCode::CREATED);
    let without_file = Block {
        id: new_id(),
        parent_id: board.id.clone(),
        board_id: board.id.clone(),
        block_type: BlockType::Attachment,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    ts.client
        .insert_blocks(&board.id, std::slice::from_ref(&without_file))
        .await
        .check_created();

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    let archive = zip::ZipArchive::new(Cursor::new(exported.body.clone())).expect("a zip");
    let entry = format!("{}/{file_id}", board.id);
    assert!(
        archive
            .file_names()
            .any(|name| name.is_ok_and(|name| name == entry.as_str())),
        "the body's image is in the archive: {:?}",
        archive.file_names().collect::<Vec<_>>()
    );

    import(&ts.client, TEST_TEAM_ID, &exported.body)
        .await
        .check_no_content();
    let imported = boards_of(&ts)
        .await
        .into_iter()
        .find(|other| other.id != board.id)
        .expect("the imported board");
    let blocks: Vec<serde_json::Value> = ts
        .client
        .get(&format!("/boards/{}/blocks?type=card", imported.id))
        .await
        .ok_json();
    let new_file = blocks[0]["body"]["content"][0]["attrs"]["fileId"]
        .as_str()
        .expect("the image names a file")
        .to_owned();
    assert_ne!(new_file, file_id, "a file of the new board");
    let file = ts
        .client
        .get(&format!("/boards/{}/files/{new_file}", imported.id))
        .await;
    file.check_ok();
    assert_eq!(
        file.body.as_slice(),
        b"png",
        "with the content of the old one"
    );
    ts.teardown().await;
}

/// An archive with an entry that cannot be read (here its checksum does not match) is
/// refused before anything of it is stored.
#[tokio::test]
async fn an_archive_with_a_broken_entry_stores_nothing() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = serde_json::json!({"type": "board", "data": {
        "id": "b1", "teamId": TEST_TEAM_ID, "type": "P", "title": "Broken", "createAt": 1, "updateAt": 1,
    }});
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("version.json", stored).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    writer.start_file("b1/board.jsonl", stored).expect("start");
    writer
        .write_all(format!("{board}\n").as_bytes())
        .expect("write");
    writer.start_file("b1/picture.png", stored).expect("start");
    writer.write_all(b"the bytes of a picture").expect("write");
    let mut bytes = writer.finish().expect("finish").into_inner();
    let at = bytes
        .windows(5)
        .position(|window| window == b"bytes")
        .expect("the stored content");
    bytes[at] = b'B';

    let before = boards_of(&ts).await.len();
    let response = import(&ts.client, TEST_TEAM_ID, &bytes).await;
    response.check_bad_request();
    assert_eq!(boards_of(&ts).await.len(), before, "no board was made");
    ts.teardown().await;
}

/// An attachment block may name its file as a client wrote it. A name that cannot be a
/// stored name gets no entry, so the archive is exported and read back.
#[tokio::test]
async fn an_archive_reads_back_whatever_an_attachment_names() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;
    let image = |file: &str| {
        let mut fields = fvoci_model::JsonMap::new();
        fields.insert(
            "fileId".to_owned(),
            serde_json::Value::String(file.to_owned()),
        );
        Block {
            id: new_id(),
            board_id: board.id.clone(),
            parent_id: board.id.clone(),
            block_type: BlockType::Attachment,
            fields,
            create_at: 1,
            update_at: 1,
            ..Block::default()
        }
    };
    ts.client
        .insert_blocks(
            &board.id,
            &[
                image("board.jsonl"),
                image("x/board.jsonl"),
                image("version.json"),
            ],
        )
        .await
        .check_created();

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    let before = boards_of(&ts).await.len();
    import(&ts.client, TEST_TEAM_ID, &exported.body)
        .await
        .check_no_content();
    assert_eq!(boards_of(&ts).await.len(), before + 1, "one board was made");
    ts.teardown().await;
}

/// A team's archive of two boards: the first is fine, the second has a card and the
/// given blocks below it.
fn two_board_archive(second_blocks: &[serde_json::Value]) -> Vec<u8> {
    let board = |id: &str| {
        serde_json::json!({"type": "board", "data": {
            "id": id, "teamId": TEST_TEAM_ID, "type": "P", "title": id, "createAt": 1,
            "updateAt": 1,
        }})
    };
    let card = |id: &str, board_id: &str| {
        serde_json::json!({"type": "block", "data": {
            "id": id, "boardId": board_id, "type": "card", "title": "card", "createAt": 1,
            "updateAt": 1,
        }})
    };
    let first = vec![board("b1"), card("c1", "b1")];
    let mut second = vec![board("b2"), card("c2", "b2")];
    second.extend(second_blocks.iter().cloned());
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    for (dir, lines) in [("b1", &first), ("b2", &second)] {
        writer
            .start_file(format!("{dir}/board.jsonl"), options)
            .expect("start");
        for line in lines {
            writer
                .write_all(format!("{line}\n").as_bytes())
                .expect("write");
        }
    }
    writer.finish().expect("finish").into_inner()
}

/// An archive whose second board the server refuses stores neither board: every board
/// is read and checked before the first is stored. The second board holds a block of a
/// type no new block has, as an export of a board that an older server stored can, or a
/// card whose fields are over the limit of a card, as a card stored before the limit
/// can be.
#[tokio::test]
async fn an_archive_with_a_refused_board_stores_nothing() {
    let ts = Setup::native().start().await.init_basic().await;
    let older_type = serde_json::json!({"type": "block", "data": {
        "id": "t2", "boardId": "b2", "parentId": "c2", "type": "text", "title": "text",
        "createAt": 1, "updateAt": 1,
    }});
    let oversized = serde_json::json!({"type": "block", "data": {
        "id": "c3", "boardId": "b2", "type": "card", "title": "big", "createAt": 1,
        "updateAt": 1, "fields": {"note": "x".repeat(70_000)},
    }});
    for (block, says) in [
        (older_type, "\"text\""),
        (oversized, "block fields size limit exceeded"),
    ] {
        let before = boards_of(&ts).await.len();
        let response = import(&ts.client, TEST_TEAM_ID, &two_board_archive(&[block])).await;
        response.check_bad_request();
        assert!(
            response.value()["message"]
                .as_str()
                .is_some_and(|message| message.contains(says)),
            "the refusal says {says}: {}",
            response.text()
        );
        assert_eq!(boards_of(&ts).await.len(), before, "no board was made");
    }
    ts.teardown().await;
}

/// An archive is written whole to a temporary file before it is sent, so the answer
/// says its length; nothing of the file is left once it is sent.
#[tokio::test]
async fn an_export_is_sent_with_its_length_and_leaves_nothing() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    assert_eq!(
        exported.header("Content-Length"),
        exported.body.len().to_string(),
        "the length of the archive"
    );
    let exports = ts.app.config().tmp_dir().join("exports");
    let left = std::fs::read_dir(&exports).map_or(0, Iterator::count);
    assert_eq!(left, 0, "no temporary archive is left");

    // What a stopped server left is removed when it starts.
    std::fs::create_dir_all(&exports).expect("the directory is made");
    std::fs::write(exports.join("stopped.boardarchive"), b"half").expect("written");
    let removed = ts
        .app
        .sweep_temporary_files()
        .await
        .expect("the sweep runs");
    assert_eq!(removed, 1);
    let left = std::fs::read_dir(&exports).map_or(0, Iterator::count);
    assert_eq!(left, 0, "the left-over archive is gone");
}

/// A file larger than what the archive writer gathers at a time is written across
/// several pieces of its entry, and comes back whole.
#[tokio::test]
async fn a_large_file_is_whole_in_the_archive() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let data: Vec<u8> = (0..5 * 1024 * 1024 / 2)
        .map(|index: usize| u8::try_from(index % 251).expect("below 251"))
        .collect();
    let response = crate::files::upload(&ts.client, &board.id, "large.bin", &data).await;
    response.check_created();
    let file_id = crate::files::uploaded_id(&response);
    let attachment = Block {
        id: new_id(),
        parent_id: board.id.clone(),
        board_id: board.id.clone(),
        block_type: BlockType::Attachment,
        fields: serde_json::json!({"fileId": file_id})
            .as_object()
            .expect("an object")
            .clone(),
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    ts.client
        .insert_blocks(&board.id, std::slice::from_ref(&attachment))
        .await
        .check_created();

    let exported = export_board(&ts.client, &board.id).await;
    exported.check_ok();
    let mut zip = zip::ZipArchive::new(Cursor::new(exported.body)).expect("the export is a ZIP");
    let mut entry = zip
        .by_name(&format!("{}/{file_id}", board.id))
        .expect("the file has its entry");
    let mut content = Vec::new();
    entry.read_to_end(&mut content).expect("the entry is read");
    assert!(content == data, "the file is whole in the archive");
}

/// Not in the original (user decision 2026-11-11 14:25 KST). A user runs one export at a
/// time and the server `max_concurrent_exports` (2): over either, an export of a board or
/// of a team is 429 with a reason. An export holds its place until it is sent
/// (`a_download_that_takes_nothing_is_given_up`).
#[tokio::test]
async fn exports_run_one_per_user_and_two_on_the_server() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;
    let me = ts.client.me().await;
    let team_export = format!("/teams/{TEST_TEAM_ID}/archive/export");
    let refused = |response: &ApiResponse, reason: &str| {
        response.check_status(StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.error_reason(), reason, "{}", response.text());
    };

    let mine = ts.app.start_export(&me.id).expect("a place for user1");
    refused(&export_board(&ts.client, &board.id).await, "export_running");
    refused(&ts.client.get(&team_export).await, "export_running");
    let other = ts.app.start_export(&new_id()).expect("a place for another");
    refused(&ts.client2.get(&team_export).await, "server_exports_busy");
    // Who may not export the board is told so first: a full server says nothing of it.
    let outsider = export_board(&ts.client2, &board.id).await;
    assert!(
        [StatusCode::FORBIDDEN, StatusCode::NOT_FOUND].contains(&outsider.status),
        "{}",
        outsider.text()
    );

    drop(mine);
    drop(other);
    export_board(&ts.client, &board.id).await.check_ok();
    // The place of a sent export is given back.
    export_board(&ts.client, &board.id).await.check_ok();
    ts.client2.get(&team_export).await.check_ok();
}

/// A board with three attachments of 6 MiB that do not compress (a request takes 8 MiB at
/// most), so that its archive is larger than what the connection's buffers take in.
async fn board_with_large_archive(ts: &TestServer) -> Board {
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    let mut attachments = Vec::new();
    for index in 0..3 {
        let data: Vec<u8> = (0..6 * 1024 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state.to_le_bytes()[0]
            })
            .collect();
        let response =
            crate::files::upload(&ts.client, &board.id, &format!("noise{index}.bin"), &data).await;
        response.check_created();
        attachments.push(Block {
            id: new_id(),
            parent_id: board.id.clone(),
            board_id: board.id.clone(),
            block_type: BlockType::Attachment,
            fields: serde_json::json!({"fileId": crate::files::uploaded_id(&response)})
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
    board
}

/// Starts the download of a board's archive and returns it with its length, unread.
async fn start_download(ts: &TestServer, board_id: &str) -> (reqwest::Response, usize) {
    let response = ts
        .client
        .request(Method::GET, &format!("/boards/{board_id}/archive/export"))
        .send()
        .await
        .expect("the export is answered");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the export is let through"
    );
    let length = response
        .content_length()
        .and_then(|length| usize::try_from(length).ok())
        .expect("the archive has its length");
    (response, length)
}

/// The temporary archives in the exports directory.
fn temporary_archives(ts: &TestServer) -> usize {
    std::fs::read_dir(ts.app.config().tmp_dir().join("exports")).map_or(0, Iterator::count)
}

/// Not in the original (coordinator's decision 2026-11-11 16:20 KST). An export holds its
/// place and its temporary file while it is sent; a client that takes nothing for
/// `export_stall_seconds` has both taken away, and the rest of its download is broken.
#[tokio::test]
async fn a_download_that_takes_nothing_is_given_up() {
    let ts = Setup::native()
        .configure(|config| config.export_stall_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let board = board_with_large_archive(&ts).await;
    let me = ts.client.me().await;

    let (mut stalled, length) = start_download(&ts, &board.id).await;
    assert!(
        ts.app.start_export(&me.id).is_err(),
        "the place is held while it is sent"
    );
    assert_eq!(temporary_archives(&ts), 1, "and so is the file");

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while ts.app.start_export(&me.id).is_err() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the place is given back"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    while temporary_archives(&ts) > 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the file is removed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let mut received = 0;
    let broken = loop {
        match stalled.chunk().await {
            Ok(Some(piece)) => received += piece.len(),
            Ok(None) => break false,
            Err(_) => break true,
        }
    };
    assert!(
        broken && received < length,
        "{received} of {length} bytes, broken: {broken}"
    );
}

/// Not in the original (coordinator's decision 2026-11-11 16:20 KST). A client that takes
/// its download slowly, with pauses shorter than `export_stall_seconds`, gets it whole
/// however long it takes; then the place and the file are given back.
#[tokio::test]
async fn a_download_taken_slowly_is_not_given_up() {
    let ts = Setup::native()
        .configure(|config| config.export_stall_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let board = board_with_large_archive(&ts).await;
    let me = ts.client.me().await;
    let started = tokio::time::Instant::now();

    let (mut slow, length) = start_download(&ts, &board.id).await;
    let mut received = 0;
    let mut next_pause = 0;
    while let Some(piece) = slow.chunk().await.expect("the download is not broken") {
        received += piece.len();
        if received >= next_pause {
            next_pause += 2 * 1024 * 1024;
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        }
    }
    assert_eq!(received, length, "the archive is whole");
    assert!(
        started.elapsed() > std::time::Duration::from_secs(2),
        "the download took longer than the stall time"
    );
    drop(
        ts.app
            .start_export(&me.id)
            .expect("the place is given back"),
    );
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while temporary_archives(&ts) > 0 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the file is removed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
