//! File upload, download and info.
//!
//! Ports `server/integrationtests/file_test.go`, `TestTeamUploadFile` in
//! `server/integrationtests/user_test.go`, and the real-behaviour parts of
//! `server/app/files_test.go`. The headers and range handling are checked against the
//! oracle.

use fvoci_model::{Block, BlockType, Board, BoardType, IdKind, JsonMap, new_id};
use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use crate::common::{Client, Setup, TEST_TEAM_ID, TestServer};

/// Uploads `data` under `filename` to a board as the given client.
async fn upload(
    client: &Client,
    team_id: &str,
    board_id: &str,
    filename: &str,
    data: &[u8],
) -> crate::common::ApiResponse {
    let part = Part::bytes(data.to_vec())
        .file_name(filename.to_owned())
        .mime_str("application/octet-stream")
        .expect("a valid mime");
    let form = Form::new().part("file", part);
    let request = client
        .request(Method::POST, &format!("/teams/{team_id}/{board_id}/files"))
        .multipart(form);
    Client::send(request).await
}

/// The `fileId` of a successful upload.
fn uploaded_id(response: &crate::common::ApiResponse) -> String {
    response.value()["fileId"]
        .as_str()
        .expect("the upload answers a fileId")
        .to_owned()
}

/// Makes an open board owned by the first client with one image block that refers to an
/// uploaded file, and returns the board and the file identifier.
async fn board_with_image(ts: &TestServer) -> (Board, String) {
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let file_id = uploaded_id(
        &upload(
            &ts.client,
            TEST_TEAM_ID,
            &board.id,
            "pic.png",
            b"hello world, a file\n",
        )
        .await,
    );
    let card_id = new_id(IdKind::Card);
    let mut image_fields = JsonMap::new();
    image_fields.insert("fileId".to_owned(), Value::String(file_id.clone()));
    let blocks = vec![
        Block {
            id: card_id.clone(),
            parent_id: board.id.clone(),
            board_id: board.id.clone(),
            block_type: BlockType::Card,
            create_at: 1,
            update_at: 1,
            ..Block::default()
        },
        Block {
            id: new_id(IdKind::Block),
            parent_id: card_id,
            board_id: board.id.clone(),
            block_type: BlockType::Image,
            fields: image_fields,
            create_at: 1,
            update_at: 1,
            ..Block::default()
        },
    ];
    ts.client.insert_blocks(&board.id, &blocks).await.check_ok();
    (board, file_id)
}

// -- upload ----------------------------------------------------------------------------

/// `TestUploadFile/"a non authenticated user should be rejected"`.
#[tokio::test]
async fn upload_file_a_non_authenticated_user_should_be_rejected() {
    let ts = Setup::native().start().await.init_basic().await;
    let response = upload(
        &ts.anonymous(),
        TEST_TEAM_ID,
        "test-board-id",
        "test.txt",
        b"test",
    )
    .await;
    response.check_unauthorized();
    ts.teardown().await;
}

/// `TestUploadFile/"upload a file ... without permissions"`.
#[tokio::test]
async fn upload_file_without_permissions_is_forbidden() {
    let ts = Setup::native().start().await.init_basic().await;
    let response = upload(
        &ts.client,
        TEST_TEAM_ID,
        "not-valid-board",
        "test.txt",
        b"test",
    )
    .await;
    response.check_forbidden();
    ts.teardown().await;
}

/// `TestUploadFile/"upload a file ... with permissions"`.
#[tokio::test]
async fn upload_file_with_permissions() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = upload(&ts.client, TEST_TEAM_ID, &board.id, "test.txt", b"test").await;
    response.check_ok();
    assert!(!uploaded_id(&response).is_empty(), "the file has an id");
    ts.teardown().await;
}

/// `TestUploadFile/"... reaching the MaxFileLimit"`.
#[tokio::test]
async fn upload_file_reaching_the_max_file_size() {
    let ts = Setup::native()
        .configure(|config| config.max_file_size = 1)
        .start()
        .await
        .init_basic()
        .await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = upload(&ts.client, TEST_TEAM_ID, &board.id, "test.txt", b"test").await;
    response.check_request_entity_too_large();
    ts.teardown().await;
}

/// `TestTeamUploadFile/"no permission"`: a board with no members refuses a non-member.
#[tokio::test]
async fn team_upload_file_no_permission() {
    let ts = Setup::native().start().await.init_basic().await;
    let board_id = new_id(IdKind::Board);
    let response = upload(&ts.client, "0", &board_id, "x.bin", &[0_u8; 64]).await;
    response.check_forbidden();
    ts.teardown().await;
}

/// `TestTeamUploadFile/"a board admin should be able to update a file"`.
#[tokio::test]
async fn team_upload_file_a_board_admin_can_upload() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board("0", BoardType::Open).await;
    let response = upload(&ts.client, "0", &board.id, "x.bin", &[1_u8; 64]).await;
    response.check_ok();
    assert!(!uploaded_id(&response).is_empty(), "the file has an id");
    ts.teardown().await;
}

/// `TestTeamUploadFile/"user that doesn't belong to the board ..."`.
#[tokio::test]
async fn team_upload_file_a_non_member_is_forbidden() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board("0", BoardType::Open).await;
    let response = upload(&ts.client2, "0", &board.id, "x.bin", &[1_u8; 64]).await;
    response.check_forbidden();
    ts.teardown().await;
}

/// `TestSaveFile/"... save .jpeg file as jpg ..."`: the stored name's extension is
/// `.jpg`.
#[tokio::test]
async fn upload_maps_jpeg_to_jpg() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let file_id =
        uploaded_id(&upload(&ts.client, TEST_TEAM_ID, &board.id, "photo.JPEG", b"x").await);
    assert_eq!(
        file_id.rsplit_once('.').map(|(_, extension)| extension),
        Some("jpg"),
        "jpeg is stored as jpg: {file_id}"
    );
    ts.teardown().await;
}

/// The upload answers a 27-character generated name plus the extension.
#[tokio::test]
async fn upload_answers_a_generated_name() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let file_id =
        uploaded_id(&upload(&ts.client, TEST_TEAM_ID, &board.id, "picture.png", b"x").await);
    let (stem, extension) = file_id.split_once('.').expect("name has an extension");
    assert_eq!(stem.len(), 27, "a 27-character generated id: {file_id}");
    assert_eq!(extension, "png", "the extension is kept");
    ts.teardown().await;
}

/// A hostile upload name is reduced to its base name before it reaches storage, so it
/// cannot traverse out of the uploads directory.
#[tokio::test]
async fn upload_name_with_path_separators_is_reduced_to_its_base() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = upload(&ts.client, TEST_TEAM_ID, &board.id, "../../evil.sh", b"x").await;
    response.check_ok();
    let file_id = uploaded_id(&response);
    assert_eq!(
        file_id.rsplit_once('.').map(|(_, extension)| extension),
        Some("sh"),
        "the base extension is kept: {file_id}"
    );
    assert!(!file_id.contains('/'), "no separators in the id: {file_id}");
    ts.teardown().await;
}

// -- info ------------------------------------------------------------------------------

/// `TestFileInfo/"Retrieving file info"`: info of an unknown name is `null`, as the
/// original answers (verified on the oracle).
#[tokio::test]
async fn file_info_of_an_unknown_file_is_null() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{}/info",
            board.id, "7unknownunknownunknownunk12.png"
        ))
        .await;
    response.check_ok();
    assert_eq!(response.value(), Value::Null, "unknown file info is null");
    ts.teardown().await;
}

/// The info JSON carries the members the original's JSON has, including `channel_id`,
/// `mini_preview` and `remote_id`, which this server's own model lacks (checked against
/// the oracle).
#[tokio::test]
async fn file_info_has_the_originals_members() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let response = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}/info",
            board.id
        ))
        .await;
    let info = response.ok_json::<Value>();
    for member in [
        "id",
        "user_id",
        "channel_id",
        "create_at",
        "update_at",
        "delete_at",
        "name",
        "extension",
        "size",
        "mime_type",
        "mini_preview",
        "remote_id",
        "archived",
    ] {
        assert!(info.get(member).is_some(), "the info has {member}: {info}");
    }
    assert_eq!(info["name"], json!("pic.png"), "the stored name");
    assert_eq!(info["extension"], json!(".png"), "the extension");
    assert_eq!(info["mini_preview"], Value::Null, "mini_preview is null");
    assert_eq!(info["remote_id"], Value::Null, "remote_id is null");
    ts.teardown().await;
}

// -- download --------------------------------------------------------------------------

/// A plain download carries the original's headers and content.
#[tokio::test]
async fn download_has_the_originals_headers() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let response = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}",
            board.id
        ))
        .await;
    response.check_ok();
    assert_eq!(response.body, b"hello world, a file\n", "the content");
    assert_eq!(
        response.header("content-type"),
        "application/octet-stream",
        "the content type"
    );
    assert_eq!(
        response.header("cache-control"),
        "private, no-cache",
        "cache control"
    );
    assert_eq!(
        response.header("x-content-type-options"),
        "nosniff",
        "nosniff"
    );
    assert_eq!(response.header("x-frame-options"), "DENY", "frame options");
    assert_eq!(
        response.header("content-security-policy"),
        "Frame-ancestors 'none'",
        "csp"
    );
    assert_eq!(response.header("accept-ranges"), "bytes", "accept ranges");
    assert_eq!(response.header("content-length"), "20", "content length");
    // The original builds the disposition from the name in the URL, which is the file id.
    assert_eq!(
        response.header("content-disposition"),
        format!("attachment;filename=\"{file_id}\"; filename*=UTF-8''{file_id}"),
        "the disposition names the file id"
    );
    assert!(
        !response.header("last-modified").is_empty(),
        "last modified is set"
    );
    ts.teardown().await;
}

/// A `Range: bytes=0-7` download is a 206 with the first eight bytes (verified on the
/// oracle).
#[tokio::test]
async fn download_with_a_range_is_partial_content() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let request = ts
        .client
        .request(
            Method::GET,
            &format!("/files/teams/{TEST_TEAM_ID}/{}/{file_id}", board.id),
        )
        .header("Range", "bytes=0-7");
    let response = Client::send(request).await;
    response.check_status(reqwest::StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.body, b"hello wo", "the first eight bytes");
    assert_eq!(
        response.header("content-range"),
        "bytes 0-7/20",
        "the content range"
    );
    assert_eq!(response.header("content-length"), "8", "the range length");
    ts.teardown().await;
}

/// A range past the end is a 416 with a `Content-Range` of `bytes */size` and a body
/// shorter than its `Content-Length` (verified on the oracle).
#[tokio::test]
async fn download_with_a_range_past_the_end_is_416() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let request = ts
        .client
        .request(
            Method::GET,
            &format!("/files/teams/{TEST_TEAM_ID}/{}/{file_id}", board.id),
        )
        .header("Range", "bytes=1000-2000");
    let response = Client::send(request).await;
    response.check_status(reqwest::StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        response.header("content-range"),
        "bytes */20",
        "the content range"
    );
    assert_eq!(
        response.header("content-type"),
        "text/plain; charset=utf-8",
        "the 416 content type"
    );
    assert_eq!(
        response.text(),
        "invalid range: failed to overlap\n",
        "the message"
    );
    ts.teardown().await;
}

/// A future `If-Modified-Since` gets a 304 (verified on the oracle).
#[tokio::test]
async fn download_if_modified_since_in_the_future_is_304() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let request = ts
        .client
        .request(
            Method::GET,
            &format!("/files/teams/{TEST_TEAM_ID}/{}/{file_id}", board.id),
        )
        .header("If-Modified-Since", "Wed, 10 Nov 2060 05:00:00 GMT");
    let response = Client::send(request).await;
    response.check_status(reqwest::StatusCode::NOT_MODIFIED);
    ts.teardown().await;
}

/// Downloading an unknown file is a 500, as the original's handler answers before its
/// legacy fallback (verified on the oracle).
#[tokio::test]
async fn download_of_an_unknown_file_is_500() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{}",
            board.id, "7unknownunknownunknownunk12.png"
        ))
        .await;
    response.check_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
    ts.teardown().await;
}

// -- access control --------------------------------------------------------------------

/// An anonymous request for a file is refused.
#[tokio::test]
async fn download_without_a_session_is_refused() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let response = ts
        .anonymous()
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}",
            board.id
        ))
        .await;
    response.check_unauthorized();
    ts.teardown().await;
}

/// A non-member is refused.
#[tokio::test]
async fn download_by_a_non_member_is_forbidden() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let response = ts
        .client2
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}",
            board.id
        ))
        .await;
    response.check_forbidden();
    ts.teardown().await;
}

/// A board's valid read token lets an anonymous client read its files.
#[tokio::test]
async fn download_with_a_valid_read_token() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;
    let sharing = json!({"id": board.id, "enabled": true, "token": "sharetoken"});
    ts.client
        .post(&format!("/boards/{}/sharing", board.id), &sharing)
        .await
        .check_ok();

    let right = ts
        .anonymous()
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}?read_token=sharetoken",
            board.id
        ))
        .await;
    right.check_ok();
    let wrong = ts
        .anonymous()
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{file_id}?read_token=nope",
            board.id
        ))
        .await;
    wrong.check_unauthorized();
    ts.teardown().await;
}

/// In single-user mode, file routes still end at the no-session refusal without a valid
/// token, and succeed with the right token (the security fix on `OptionalSession`).
#[tokio::test]
async fn single_user_mode_needs_the_token_for_files() {
    let ts = Setup::single_user().start().await;
    // The single-user client is already authorised; make a board and a file.
    let board = ts.create_board("0", BoardType::Open).await;
    let file_id = uploaded_id(&upload(&ts.client, "0", &board.id, "f.bin", b"secret-bytes").await);
    let path = format!("/files/teams/0/{}/{file_id}", board.id);

    ts.client.get(&path).await.check_ok();
    ts.anonymous().get(&path).await.check_unauthorized();
    ts.client_with_token("wrong-token")
        .get(&path)
        .await
        .check_unauthorized();
    ts.teardown().await;
}

// -- copying files on duplication ------------------------------------------------------

/// `TestCopyAndUpdateCardFiles`: duplicating a board copies its image file to a new name
/// and the copy is downloadable.
#[tokio::test]
async fn duplicating_a_board_copies_its_files() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, file_id) = board_with_image(&ts).await;

    let duplicate = ts
        .client
        .post(&format!("/boards/{}/duplicate", board.id), &json!({}))
        .await;
    let value = duplicate.ok_json::<Value>();
    let new_board_id = value["boards"][0]["id"]
        .as_str()
        .expect("a new board")
        .to_owned();

    let blocks = ts
        .client
        .get(&format!("/boards/{new_board_id}/blocks"))
        .await
        .ok_json::<Vec<Block>>();
    let image = blocks
        .iter()
        .find(|block| block.block_type == BlockType::Image)
        .expect("the copy has an image block");
    let new_file_id = image
        .fields
        .get("fileId")
        .and_then(Value::as_str)
        .expect("a copied file id");
    assert_ne!(new_file_id, file_id, "the copy has a new file id");

    let download = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{new_board_id}/{new_file_id}"
        ))
        .await;
    download.check_ok();
    assert_eq!(
        download.body, b"hello world, a file\n",
        "the copy has the content"
    );
    ts.teardown().await;
}

/// `TestCopyCard`: duplicating a single card copies the files its image blocks refer to.
#[tokio::test]
async fn duplicating_a_card_copies_its_files() {
    let ts = Setup::native().start().await.init_basic().await;
    let (board, _file_id) = board_with_image(&ts).await;
    let card_id = ts
        .client
        .get(&format!("/boards/{}/blocks", board.id))
        .await
        .ok_json::<Vec<Block>>()
        .into_iter()
        .find(|block| block.block_type == BlockType::Card)
        .expect("the board has a card")
        .id;

    let copies = ts
        .client
        .post(
            &format!("/boards/{}/blocks/{card_id}/duplicate", board.id),
            &json!({}),
        )
        .await
        .ok_json::<Vec<Block>>();
    let image = copies
        .iter()
        .find(|block| block.block_type == BlockType::Image)
        .expect("a copied image block");
    let new_file_id = image
        .fields
        .get("fileId")
        .and_then(Value::as_str)
        .expect("a copied file id");
    let download = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{}/{new_file_id}",
            board.id
        ))
        .await;
    download.check_ok();
    ts.teardown().await;
}

// -- the legacy reader fallback (ported but unreachable from the route) -----------------

// -- files are tied to their board --------------------------------------------------------

/// The private board of the first user with an uploaded file, and a private board of
/// the second user who knows that file's name and has put it into an image block and an
/// attachment block of their own board.
struct ForeignFile {
    owner_board: Board,
    file_id: String,
    intruder_board: Board,
}

const SECRET: &[u8] = b"the first user's file\n";

async fn foreign_file(ts: &TestServer) -> ForeignFile {
    let owner_board = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let file_id = uploaded_id(
        &upload(
            &ts.client,
            TEST_TEAM_ID,
            &owner_board.id,
            "secret.png",
            SECRET,
        )
        .await,
    );

    let board = Board {
        team_id: TEST_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        ..Board::default()
    };
    let intruder_board: Board = ts.client2.create_board(&board).await.ok_json();
    let card_id = new_id(IdKind::Card);
    let named = |key: &str| {
        let mut fields = JsonMap::new();
        fields.insert(key.to_owned(), Value::String(file_id.clone()));
        fields
    };
    let block = |id: String, parent: &str, block_type: BlockType, fields: JsonMap| Block {
        id,
        parent_id: parent.to_owned(),
        board_id: intruder_board.id.clone(),
        block_type,
        fields,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    let blocks = vec![
        block(
            card_id.clone(),
            &intruder_board.id,
            BlockType::Card,
            JsonMap::new(),
        ),
        block(
            new_id(IdKind::Block),
            &card_id,
            BlockType::Image,
            named("fileId"),
        ),
        block(
            new_id(IdKind::Block),
            &card_id,
            BlockType::Attachment,
            named("attachmentId"),
        ),
    ];
    ts.client2
        .insert_blocks(&intruder_board.id, &blocks)
        .await
        .check_ok();
    ForeignFile {
        owner_board,
        file_id,
        intruder_board,
    }
}

/// Security fix. On the original, a member of any board fetches any file whose name
/// they know through their own board's path (verified: a user who gets 403 for the file
/// through its board's path gets 200 through the path of a board of their own). A file
/// this server stored is given out through its own board only.
#[tokio::test]
async fn a_file_is_not_given_out_through_another_board() {
    let ts = Setup::native().start().await.init_basic().await;
    let fixture = foreign_file(&ts).await;
    let through_own_board = format!(
        "/files/teams/{TEST_TEAM_ID}/{}/{}",
        fixture.intruder_board.id, fixture.file_id
    );

    let download = ts.client2.get(&through_own_board).await;
    download.check_forbidden();
    assert_eq!(
        download.error_message(),
        "file does not belong to the specified board",
        "the download is refused"
    );
    let info = ts.client2.get(&format!("{through_own_board}/info")).await;
    info.check_forbidden();
    assert_eq!(
        info.error_message(),
        "access denied to file",
        "so is the info"
    );

    // The owner cannot be tricked into it either: the file is the other board's.
    ts.client.get(&through_own_board).await.check_forbidden();
    ts.teardown().await;
}

/// The legitimate reads: through the file's own board, by a member and with the read
/// token of the shared board.
#[tokio::test]
async fn a_file_is_given_out_through_its_own_board() {
    let ts = Setup::local_mode().start().await.init_basic().await;
    let fixture = foreign_file(&ts).await;
    let path = format!(
        "/files/teams/{TEST_TEAM_ID}/{}/{}",
        fixture.owner_board.id, fixture.file_id
    );

    let download = ts.client.get(&path).await;
    download.check_ok();
    assert_eq!(download.body, SECRET, "the owner reads the file");
    let info = ts.client.get(&format!("{path}/info")).await.value();
    assert_eq!(info["name"], json!("secret.png"), "and its info: {info}");

    // The second user is no member of that board.
    ts.client2.get(&path).await.check_forbidden();

    ts.client
        .post(
            &format!("/boards/{}/sharing", fixture.owner_board.id),
            &json!({"id": fixture.owner_board.id, "enabled": true, "token": "read-me"}),
        )
        .await
        .check_ok();
    let shared = ts
        .anonymous()
        .get(&format!("{path}?read_token=read-me"))
        .await;
    shared.check_ok();
    assert_eq!(
        shared.body, SECRET,
        "a reader of the shared board reads the file"
    );
    ts.teardown().await;
}

/// The same tie holds where the server itself reads a file by name: duplicating a block
/// or a board does not copy another board's file into the duplicate, and an export does
/// not put it into the archive. On the original, the duplicate gets a copy of the file.
#[tokio::test]
async fn a_file_of_another_board_is_neither_copied_nor_exported() {
    let ts = Setup::native().start().await.init_basic().await;
    let fixture = foreign_file(&ts).await;
    let board_id = &fixture.intruder_board.id;

    let duplicate = ts
        .client2
        .post(&format!("/boards/{board_id}/duplicate"), &json!({}))
        .await
        .ok_json::<Value>();
    let copy_id = duplicate["boards"][0]["id"]
        .as_str()
        .expect("a copy")
        .to_owned();
    let blocks: Vec<Block> = ts
        .client2
        .get(&format!("/boards/{copy_id}/blocks"))
        .await
        .ok_json();
    for block in blocks
        .iter()
        .filter(|block| matches!(block.block_type, BlockType::Image | BlockType::Attachment))
    {
        for key in ["fileId", "attachmentId"] {
            let Some(name) = block.fields.get(key).and_then(Value::as_str) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let response = ts
                .client2
                .get(&format!("/files/teams/{TEST_TEAM_ID}/{copy_id}/{name}"))
                .await;
            assert_ne!(
                response.body, SECRET,
                "the duplicate's {key} {name} does not give the file out"
            );
        }
    }

    let export = ts
        .client2
        .get(&format!("/boards/{board_id}/archive/export"))
        .await;
    export.check_ok();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(export.body.clone()))
        .expect("the export is an archive");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("an entry");
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut content).expect("the entry is read");
        assert_ne!(
            content, SECRET,
            "the entry {index} is not the other board's file"
        );
    }
    ts.teardown().await;
}

/// Files in the original's layout name no board in their path and are served as the
/// original serves them, through any board the requester may read. Recorded as an open
/// question; this pins what happens.
#[tokio::test]
async fn a_file_in_the_originals_layout_is_served_as_the_original_does() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let other = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;

    let name = format!("{}.txt", new_id(IdKind::None));
    let path = format!("boards/20230101/{name}");
    ts.app
        .files()
        .write(&path, &mut &b"an old file"[..])
        .await
        .expect("the file is stored");
    let info = fvoci_model::FileInfo {
        id: name[1..name.len() - 4].to_owned(),
        name: "old.txt".to_owned(),
        extension: ".txt".to_owned(),
        size: 11,
        path,
        ..fvoci_model::FileInfo::default()
    };
    ts.app
        .store()
        .save_file_info(&info)
        .await
        .expect("the record is stored");

    for board_id in [&board.id, &other.id] {
        let download = ts
            .client
            .get(&format!("/files/teams/{TEST_TEAM_ID}/{board_id}/{name}"))
            .await;
        download.check_ok();
        assert_eq!(download.body, b"an old file", "through board {board_id}");
    }
    ts.teardown().await;
}

/// A block that names no file does not stop a duplication: the original logs the failed
/// copy and carries on (`server/app/files.go:186-190`; verified on the original, which
/// answers 200 and keeps the empty `fileId`).
#[tokio::test]
async fn duplicating_a_board_with_an_empty_file_reference_works() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let card_id = new_id(IdKind::Card);
    let mut empty = JsonMap::new();
    empty.insert("fileId".to_owned(), Value::String(String::new()));
    let block = |id: String, parent: &str, block_type: BlockType, fields: JsonMap| Block {
        id,
        parent_id: parent.to_owned(),
        board_id: board.id.clone(),
        block_type,
        fields,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    let blocks = vec![
        block(card_id.clone(), &board.id, BlockType::Card, JsonMap::new()),
        block(new_id(IdKind::Block), &card_id, BlockType::Image, empty),
        block(
            new_id(IdKind::Block),
            &card_id,
            BlockType::Attachment,
            JsonMap::new(),
        ),
    ];
    ts.client.insert_blocks(&board.id, &blocks).await.check_ok();

    let duplicate = ts
        .client
        .post(&format!("/boards/{}/duplicate", board.id), &json!({}))
        .await
        .ok_json::<Value>();
    let copies = duplicate["blocks"].as_array().expect("the copied blocks");
    assert_eq!(copies.len(), 3, "all three blocks are copied: {duplicate}");
    let image = copies
        .iter()
        .find(|block| block["type"] == json!("image"))
        .expect("the image block is copied");
    assert_eq!(
        image["fields"]["fileId"],
        json!(""),
        "and still names no file"
    );
    ts.teardown().await;
}

/// A file name that begins with a character of several bytes: the original drops the
/// first byte of a name to find its record, which then finds nothing, and falls back to
/// the path built from the team, the board and the name. Template boards keep the
/// client's file name, so such names occur.
#[tokio::test]
async fn a_template_file_whose_name_begins_with_a_wide_character_is_served() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = Board {
        team_id: TEST_TEAM_ID.to_owned(),
        board_type: BoardType::Private,
        is_template: true,
        ..Board::default()
    };
    let board: Board = ts.client.create_board(&board).await.ok_json();
    let name = "사진.png";
    let file_id = uploaded_id(&upload(&ts.client, TEST_TEAM_ID, &board.id, name, b"wide").await);
    assert_eq!(file_id, name, "a template upload keeps its name");

    let encoded = "%EC%82%AC%EC%A7%84.png";
    let path = format!("/files/teams/{TEST_TEAM_ID}/{}/{encoded}", board.id);
    let download = ts.client.get(&path).await;
    download.check_ok();
    assert_eq!(download.body, b"wide", "the file is served");
    let info = ts.client.get(&format!("{path}/info")).await;
    info.check_ok();
    assert_eq!(
        info.value(),
        Value::Null,
        "there is no record under such a name"
    );

    // An image block that names it: the board exports and duplicates.
    let mut fields = JsonMap::new();
    fields.insert("fileId".to_owned(), Value::String(name.to_owned()));
    let card_id = new_id(IdKind::Card);
    let block = |id: String, parent: &str, block_type: BlockType, fields: JsonMap| Block {
        id,
        parent_id: parent.to_owned(),
        board_id: board.id.clone(),
        block_type,
        fields,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    let blocks = vec![
        block(card_id.clone(), &board.id, BlockType::Card, JsonMap::new()),
        block(new_id(IdKind::Block), &card_id, BlockType::Image, fields),
    ];
    ts.client.insert_blocks(&board.id, &blocks).await.check_ok();
    ts.client
        .get(&format!("/boards/{}/archive/export", board.id))
        .await
        .check_ok();
    let duplicate = ts
        .client
        .post(&format!("/boards/{}/duplicate", board.id), &json!({}))
        .await
        .ok_json::<Value>();
    let copy_id = duplicate["boards"][0]["id"]
        .as_str()
        .expect("a copy")
        .to_owned();
    let copied: Vec<Block> = ts
        .client
        .get(&format!("/boards/{copy_id}/blocks"))
        .await
        .ok_json();
    let image = copied
        .iter()
        .find(|block| block.block_type == BlockType::Image)
        .expect("the image block is copied");
    let copy_name = image.fields["fileId"].as_str().expect("a file name");
    assert!(!copy_name.is_empty(), "the copy names a file of its own");
    let copy = ts
        .client
        .get(&format!(
            "/files/teams/{TEST_TEAM_ID}/{copy_id}/{copy_name}"
        ))
        .await;
    copy.check_ok();
    assert_eq!(copy.body, b"wide", "with the same content");
    ts.teardown().await;
}

/// A range on an empty file: the whole, empty file is the answer, as in the original.
#[tokio::test]
async fn a_range_on_an_empty_file_is_ignored() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Private).await;
    let file_id = uploaded_id(&upload(&ts.client, TEST_TEAM_ID, &board.id, "empty.txt", b"").await);
    let path = format!("/files/teams/{TEST_TEAM_ID}/{}/{file_id}", board.id);

    for range in ["bytes=-5", "bytes=0-", "bytes=0-10", "bytes=3-"] {
        let request = ts.client.request(Method::GET, &path).header("Range", range);
        let response = Client::send(request).await;
        response.check_ok();
        assert!(response.body.is_empty(), "{range}: the empty file");
    }
    ts.teardown().await;
}
