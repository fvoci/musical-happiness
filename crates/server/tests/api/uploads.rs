//! Uploads in parts (`docs/s6-design.md` 5.6): open, send parts, resume, complete,
//! abandon, and what is refused. The parts kept on the disk are looked at too: nothing
//! of an upload stays behind once it is completed, abandoned or refused.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use fvoci_model::BoardType;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID, TestServer};

/// The smallest part size, which the tests use so that a file of two parts stays small.
const PART: usize = 5 * 1024 * 1024;

/// A server whose uploads come in parts of [`PART`] bytes.
async fn server() -> TestServer {
    Setup::native()
        .configure(|config| config.upload_part_bytes = 5 * 1024 * 1024)
        .start()
        .await
        .init_basic()
        .await
}

/// `size` bytes that differ from part to part, so that parts in the wrong order show.
fn file_of(size: usize) -> Vec<u8> {
    (0..size)
        .map(|index| u8::try_from(index % 251).expect("below 251"))
        .collect()
}

/// The `Content-Digest` of `bytes`.
fn digest(bytes: &[u8]) -> String {
    format!("sha-256=:{}:", STANDARD.encode(Sha256::digest(bytes)))
}

async fn open(client: &Client, board_id: &str, name: &str, size: usize) -> ApiResponse {
    client
        .post(
            &format!("/boards/{board_id}/uploads"),
            &json!({"name": name, "size": size}),
        )
        .await
}

/// Opens an upload and returns its identifier.
async fn opened(client: &Client, board_id: &str, name: &str, size: usize) -> String {
    let response = open(client, board_id, name, size).await;
    response.check_created();
    response.value()["uploadId"]
        .as_str()
        .expect("an upload has an identifier")
        .to_owned()
}

/// Sends `bytes` as part `number` with the digest `digest`.
async fn put_part_with(
    client: &Client,
    upload_id: &str,
    number: &str,
    bytes: &[u8],
    digest: &str,
) -> ApiResponse {
    let request = client
        .request(Method::PUT, &format!("/uploads/{upload_id}/parts/{number}"))
        .header("Content-Type", "application/octet-stream")
        .header("Content-Digest", digest)
        .body(bytes.to_vec());
    Client::send(request).await
}

/// Sends `bytes` as part `number`, with their digest.
async fn put_part(client: &Client, upload_id: &str, number: usize, bytes: &[u8]) -> ApiResponse {
    put_part_with(
        client,
        upload_id,
        &number.to_string(),
        bytes,
        &digest(bytes),
    )
    .await
}

/// The directory that holds the parts of an upload.
fn parts_dir(ts: &TestServer, upload_id: &str) -> PathBuf {
    ts.app.config().tmp_dir().join("uploads").join(upload_id)
}

/// The names of the files in a directory, sorted; none if it is not there.
fn files_in(directory: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| {
            entry
                .expect("an entry is read")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// A file of two parts, sent out of order and once again, completes into the file it
/// was, which downloads whole; the upload and its parts are gone after.
#[tokio::test]
async fn a_file_sent_in_parts_is_stored_whole() {
    let ts = server().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let data = file_of(PART + 1000);

    let response = open(&ts.client, &board.id, "Report.PDF", data.len()).await;
    response.check_created();
    let opened = response.value();
    assert_eq!(opened["partSize"], PART, "the server's part size");
    assert_eq!(opened["partCount"], 2, "a full part and the rest");
    let upload_id = opened["uploadId"]
        .as_str()
        .expect("an identifier")
        .to_owned();
    assert_eq!(upload_id.len(), 43, "256 bits in URL-safe base64");

    put_part(&ts.client, &upload_id, 2, &data[PART..])
        .await
        .check_no_content();
    // What a client that lost its place sees.
    let state = ts.client.get(&format!("/uploads/{upload_id}")).await;
    state.check_ok();
    let state = state.value();
    assert_eq!(state["boardId"], board.id.as_str());
    assert_eq!(state["name"], "Report.PDF");
    assert_eq!(state["size"], data.len());
    assert_eq!(state["parts"], json!([{"number": 2, "size": 1000}]));

    // Part 1 is sent wrong, then right: the second replaces the first.
    let mut wrong = data[..PART].to_vec();
    wrong[0] ^= 1;
    put_part(&ts.client, &upload_id, 1, &wrong)
        .await
        .check_no_content();
    put_part(&ts.client, &upload_id, 1, &data[..PART])
        .await
        .check_no_content();
    assert_eq!(files_in(&parts_dir(&ts, &upload_id)), ["1", "2"]);

    let response = ts
        .client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await;
    response.check_created();
    let file_id = response.value()["fileId"]
        .as_str()
        .expect("a file identifier")
        .to_owned();
    assert_eq!(
        Path::new(&file_id).extension(),
        Some("pdf".as_ref()),
        "the extension is kept, in lower case"
    );
    let download = ts
        .client
        .get(&format!("/boards/{}/files/{file_id}", board.id))
        .await;
    download.check_ok();
    assert!(download.body == data, "the file is the parts in order");

    assert!(
        !parts_dir(&ts, &upload_id).exists(),
        "the parts are gone once the file is made"
    );
    ts.client
        .get(&format!("/uploads/{upload_id}"))
        .await
        .check_not_found();
    ts.client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await
        .check_not_found();
}

/// An empty file has no part, and completes at once.
#[tokio::test]
async fn an_empty_file_has_no_part() {
    let ts = server().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = open(&ts.client, &board.id, "empty.txt", 0).await;
    response.check_created();
    assert_eq!(response.value()["partCount"], 0);
    let upload_id = response.value()["uploadId"]
        .as_str()
        .expect("an identifier")
        .to_owned();
    put_part(&ts.client, &upload_id, 1, b"")
        .await
        .check_bad_request();
    let response = ts
        .client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await;
    response.check_created();
    let file_id = response.value()["fileId"]
        .as_str()
        .expect("an id")
        .to_owned();
    let download = ts
        .client
        .get(&format!("/boards/{}/files/{file_id}", board.id))
        .await;
    download.check_ok();
    assert_eq!(download.body, b"");
}

/// A part of the wrong length, with a wrong or missing digest, or of a number the
/// upload has no part of is 400, and nothing of it is kept on the disk.
#[tokio::test]
async fn a_part_that_fails_a_check_is_not_kept() {
    let ts = server().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let data = file_of(PART + 10);
    let upload_id = opened(&ts.client, &board.id, "a.bin", data.len()).await;
    let last = &data[PART..];

    let refusals = [
        ("2", &last[..9], digest(&last[..9]), "too short"),
        ("1", last, digest(last), "the size of another part"),
        ("2", last, digest(b"something else"), "another digest"),
        ("2", last, "md5=:AAAA:".to_owned(), "no sha-256 digest"),
        ("0", last, digest(last), "no part 0"),
        ("3", last, digest(last), "no part 3"),
        ("two", last, digest(last), "no number"),
    ];
    for (number, bytes, digest, why) in refusals {
        let response = put_part_with(&ts.client, &upload_id, number, bytes, &digest).await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "{why}");
        assert!(
            files_in(&parts_dir(&ts, &upload_id)).is_empty(),
            "nothing is kept of a part that is {why}"
        );
    }
    let state = ts.client.get(&format!("/uploads/{upload_id}")).await;
    assert_eq!(state.value()["parts"], json!([]), "no part is recorded");

    // A part without a digest header at all.
    let request = ts
        .client
        .request(Method::PUT, &format!("/uploads/{upload_id}/parts/2"))
        .body(last.to_vec());
    let response = Client::send(request).await;
    response.check_bad_request();
    assert_eq!(response.value()["field"], "Content-Digest");

    // Completing with a part missing names it, and the upload stays open for it.
    put_part(&ts.client, &upload_id, 2, last)
        .await
        .check_no_content();
    let response = ts
        .client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await;
    response.check_bad_request();
    assert_eq!(response.error_message(), "parts missing: 1");
    put_part(&ts.client, &upload_id, 1, &data[..PART])
        .await
        .check_no_content();
    ts.client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await
        .check_created();
}

/// An upload is its user's: to anyone else it is not there, as one that does not exist.
#[tokio::test]
async fn another_users_upload_is_not_found() {
    let ts = server().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let upload_id = opened(&ts.client, &board.id, "a.txt", 3).await;

    for id in [upload_id.as_str(), "no-such-upload"] {
        let other = &ts.client2;
        other.get(&format!("/uploads/{id}")).await.check_not_found();
        put_part(other, id, 1, b"abc").await.check_not_found();
        other
            .post_empty(&format!("/uploads/{id}/complete"))
            .await
            .check_not_found();
        other
            .delete(&format!("/uploads/{id}"))
            .await
            .check_not_found();
    }
    // Still there for its user.
    ts.client
        .get(&format!("/uploads/{upload_id}"))
        .await
        .check_ok();
}

/// An abandoned upload is gone, its parts with it.
#[tokio::test]
async fn an_abandoned_upload_leaves_nothing() {
    let ts = server().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let upload_id = opened(&ts.client, &board.id, "a.txt", 3).await;
    put_part(&ts.client, &upload_id, 1, b"abc")
        .await
        .check_no_content();
    assert_eq!(files_in(&parts_dir(&ts, &upload_id)), ["1"]);

    ts.client
        .delete(&format!("/uploads/{upload_id}"))
        .await
        .check_no_content();
    assert!(!parts_dir(&ts, &upload_id).exists(), "the parts are gone");
    ts.client
        .get(&format!("/uploads/{upload_id}"))
        .await
        .check_not_found();
    ts.client
        .post_empty(&format!("/uploads/{upload_id}/complete"))
        .await
        .check_not_found();
}

/// Opening an upload takes the permission of `uploadFile`, a name, and a size within
/// `maxfilesize`.
#[tokio::test]
async fn opening_an_upload_is_checked() {
    let ts = Setup::native()
        .configure(|config| config.max_file_size = 1000)
        .start()
        .await
        .init_basic()
        .await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;

    open(&ts.client, &board.id, "a.txt", 1000)
        .await
        .check_created();
    let response = open(&ts.client, &board.id, "a.txt", 1001).await;
    response.check_request_entity_too_large();

    let refusals: [(Value, &str); 5] = [
        (json!({"name": "", "size": 1}), "name"),
        (json!({"name": "a".repeat(256), "size": 1}), "name"),
        (json!({"name": "a.txt", "size": -1}), "size"),
        (
            json!({"name": "a.txt", "size": 1, "contentType": "a".repeat(256)}),
            "contentType",
        ),
        (json!({"name": "a.txt", "size": 1, "fileId": "x"}), ""),
    ];
    for (body, field) in refusals {
        let response = ts
            .client
            .post(&format!("/boards/{}/uploads", board.id), &body)
            .await;
        response.check_bad_request();
        if !field.is_empty() {
            assert_eq!(response.value()["field"], field, "{body}");
        }
    }

    // Not a member of the board.
    let response = open(&ts.client2, &board.id, "a.txt", 3).await;
    assert!(
        matches!(
            response.status,
            StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
        ),
        "{}",
        response.status
    );
}
