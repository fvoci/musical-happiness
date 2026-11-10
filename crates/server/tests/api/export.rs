//! Archive export and import.
//!
//! Ports `server/integrationtests/export_test.go` (`TestExportBoard`) and the behaviour
//! of `server/app/import_test.go`, checked against the oracle.

use std::io::{Cursor, Read as _, Write as _};

use crate::common::{ApiResponse, Client, ROOT_TEAM_ID, Setup, TEST_TEAM_ID, TestServer};
use fvoci_model::{Block, BlockType, Board, BoardType, new_id};
use reqwest::Method;
use reqwest::multipart::{Form, Part};

/// `GET /boards/{id}/archive/export` for a board.
async fn export_board(client: &Client, board_id: &str) -> ApiResponse {
    client
        .get(&format!("/boards/{board_id}/archive/export"))
        .await
}

/// Imports archive bytes into a team.
pub(crate) async fn import(client: &Client, team_id: &str, bytes: &[u8]) -> ApiResponse {
    let part = Part::bytes(bytes.to_vec()).file_name("archive.boardarchive".to_owned());
    let form = Form::new().part("file", part);
    let request = client
        .request(Method::POST, &format!("/teams/{team_id}/archive/import"))
        .multipart(form);
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
        .check_ok();
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
        .check_ok();

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
        .check_ok();

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

    // The body is a ZIP with version 2 and the board's directory.
    let mut zip = zip::ZipArchive::new(Cursor::new(response.body.clone())).expect("a zip");
    let mut version = String::new();
    zip.by_name("version.json")
        .expect("version.json")
        .read_to_string(&mut version)
        .expect("read");
    assert!(version.contains("\"version\":2"), "version 2: {version}");
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

    import(&ts.client2, ROOT_TEAM_ID, &archive).await.check_ok();
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
        .write_all(br#"{"version":2,"date":1}"#)
        .expect("write");
    writer
        .start_file("board1/../../../evil.txt", options)
        .expect("start file");
    writer.write_all(b"pwned").expect("write");
    let bytes = writer.finish().expect("finish").into_inner();

    // No board directory matches the orphan file, so it is skipped and the import is a
    // clean 200.
    import(&ts.client, ROOT_TEAM_ID, &bytes).await.check_ok();
    ts.teardown().await;
}

/// Two blocks that name the same file: the original writes the file into the archive
/// twice under one name; the port writes it once. Found when an export of such a board
/// failed.
#[tokio::test]
async fn export_of_a_board_whose_blocks_share_a_file() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = board_with_card(&ts, TEST_TEAM_ID).await;
    let image = |file: &str| {
        let mut fields = fvoci_model::JsonMap::new();
        fields.insert(
            "fileId".to_owned(),
            serde_json::Value::String(file.to_owned()),
        );
        fvoci_model::Block {
            id: fvoci_model::new_id(),
            board_id: board.id.clone(),
            parent_id: board.id.clone(),
            block_type: fvoci_model::BlockType::Image,
            fields,
            create_at: 1,
            update_at: 1,
            ..fvoci_model::Block::default()
        }
    };
    let blocks = vec![
        image("7aaaaaaaaaaaaaaaaaaaaaaaaaa.png"),
        image("7aaaaaaaaaaaaaaaaaaaaaaaaaa.png"),
    ];
    ts.client.insert_blocks(&board.id, &blocks).await.check_ok();

    let export = export_board(&ts.client, &board.id).await;
    export.check_ok();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(export.body.clone()))
        .expect("the export is an archive");
    let files = archive
        .file_names()
        .filter(|name| name.as_ref().is_ok_and(|name| name.ends_with(".png")))
        .count();
    assert_eq!(files, 1, "the shared file has one entry");
    ts.teardown().await;
}
