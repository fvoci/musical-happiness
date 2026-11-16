//! The file block of a body: a file of the card's board or the page's wiki, to download.
//! It goes every way an image goes (the tests of images run for it too: `bodies.rs`,
//! `collab_images.rs`, the copies, the archives, the versions); these are the ways an
//! image has no test of its own for, and what only a file block is held to.

use fvoci_model::{ContainerType, ContainersAndBlocks};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer};
use crate::files::{upload, uploaded_id};
use crate::wiki_copies::{page, wiki, write};

/// What the file of the tests holds: a page that would run a script if it were opened as
/// one.
const PAGE: &[u8] = b"<html><script>alert(1)</script></html>\n";

/// A document of a line and a file block naming the file of that stored name.
fn with_file(file_id: &str) -> Value {
    json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": "Minutes"}]},
        {"type": "file", "attrs": {"fileId": file_id}},
    ]})
}

/// The stored name the file block of a body names.
fn file_of(body: &Value) -> String {
    body["content"][1]["attrs"]["fileId"]
        .as_str()
        .unwrap_or_else(|| panic!("a file block: {body}"))
        .to_owned()
}

/// A comment's body has no file block, on any route that writes a comment; an image it
/// may have.
#[tokio::test]
async fn a_comment_has_no_file_block() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerType::Private)
        .await;
    let file_id = uploaded_id(&upload(&ts.client, &board.id, "minutes.pdf", b"%PDF").await);
    let card: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Card", "body": with_file(&file_id)}),
        )
        .await
        .created_json();
    let blocks = format!("{TEST_WS}/boards/{}/blocks", board.id);
    let comment = |body: &Value| {
        json!([{"containerId": board.id, "parentId": card["id"], "type": "comment",
                "createAt": 1, "updateAt": 1, "body": body}])
    };
    let refused = ts
        .client
        .post(&blocks, &comment(&with_file(&file_id)))
        .await;
    check_refused_body(&refused, "0/body");

    let image =
        json!({"type": "doc", "content": [{"type": "image", "attrs": {"fileId": file_id}}]});
    let made: Vec<Value> = ts
        .client
        .post(&blocks, &comment(&image))
        .await
        .created_json();
    let comment_id = made[0]["id"].as_str().expect("an id");
    let refused = ts
        .client
        .patch(
            &format!("{blocks}/{comment_id}"),
            &json!({"body": with_file(&file_id), "readAt": made[0]["updateAt"]}),
        )
        .await;
    check_refused_body(&refused, "body");

    let wiki_id = wiki(&ts.client, "Handbook").await;
    let page_id = page(&ts.client, &wiki_id, "", "Minutes").await;
    let wiki_file = uploaded_id(&upload(&ts.client, &wiki_id, "minutes.pdf", b"%PDF").await);
    let refused = ts
        .client
        .post(
            &format!("{TEST_WS}/wikis/{wiki_id}/pages/{page_id}/comments"),
            &json!({"body": with_file(&wiki_file)}),
        )
        .await;
    check_refused_body(&refused, "body");
    ts.teardown().await;
}

#[track_caller]
fn check_refused_body(response: &ApiResponse, member: &str) {
    response.check_status(StatusCode::BAD_REQUEST);
    let text = response.text();
    assert!(
        text.contains("file block") && text.contains(member),
        "refused as {member}: {text}"
    );
}

/// A template made of a board, and a board made of that template, each name files of
/// their own in their file blocks, with the bytes of the original's.
#[tokio::test]
async fn a_template_and_a_board_made_of_it_have_files_of_their_own() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerType::Private)
        .await;
    let file_id = uploaded_id(&upload(&ts.client, &board.id, "minutes.html", PAGE).await);
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Card", "body": with_file(&file_id)}),
        )
        .await
        .check_status(StatusCode::CREATED);

    let mut from = (board.id.clone(), file_id);
    for query in ["?asTemplate=true", "?asTemplate=false"] {
        let made: ContainersAndBlocks = ts
            .client
            .post(
                &format!("{TEST_WS}/containers/{}/duplicate{query}", from.0),
                &json!({}),
            )
            .await
            .created_json();
        let copy = made.containers[0].id.clone();
        let cards: Vec<Value> = ts
            .client
            .get(&format!("{TEST_WS}/boards/{copy}/blocks?type=card"))
            .await
            .ok_json();
        let file = file_of(&cards[0]["body"]);
        assert_ne!(file, from.1, "{query}: a file of its own");
        let read = ts
            .client
            .get(&format!("{TEST_WS}/containers/{copy}/files/{file}"))
            .await;
        read.check_ok();
        assert_eq!(read.body, PAGE, "{query}: the same bytes");
        from = (copy, file);
    }
    ts.teardown().await;
}

/// Whoever reads a board or a wiki by its public link reads the file block as it is
/// stored and downloads its file, as an attachment that is never sniffed, whole or in
/// part; a file of another board or wiki is not there by that link.
#[tokio::test]
async fn a_public_link_reader_downloads_the_file_of_a_file_block() {
    // Public links are a switch of the organization, on in local mode.
    let ts = Setup::local_mode().start().await.init_basic().await;
    let board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerType::Open)
        .await;
    let board_file = uploaded_id(&upload(&ts.client, &board.id, "minutes.html", PAGE).await);
    ts.client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Card", "body": with_file(&board_file)}),
        )
        .await
        .check_status(StatusCode::CREATED);
    let wiki_id = wiki(&ts.client, "Handbook").await;
    let page_id = page(&ts.client, &wiki_id, "", "Minutes").await;
    let wiki_file = uploaded_id(&upload(&ts.client, &wiki_id, "minutes.html", PAGE).await);
    write(&ts.client, &wiki_id, &page_id, &with_file(&wiki_file)).await;

    let reader = ts.anonymous();
    let board_token = ts.share(&board.id).await;
    let blocks: Vec<Value> = reader
        .get(&format!(
            "/shared/{}/blocks?readToken={board_token}",
            board.id
        ))
        .await
        .ok_json();
    let card = blocks
        .iter()
        .find(|block| block["type"] == json!("card"))
        .expect("the card");
    assert_eq!(
        file_of(&card["body"]),
        board_file,
        "the file block as it is"
    );
    let wiki_token = ts.share(&wiki_id).await;
    let read: Value = reader
        .get(&format!(
            "/shared/{wiki_id}/pages/{page_id}?readToken={wiki_token}"
        ))
        .await
        .ok_json();
    assert_eq!(file_of(&read["body"]), wiki_file, "the file block as it is");

    for (container, file, token, elsewhere) in [
        (&board.id, &board_file, &board_token, &wiki_file),
        (&wiki_id, &wiki_file, &wiki_token, &board_file),
    ] {
        let at = |rest: &str| format!("/shared/{container}/files/{rest}?readToken={token}");
        check_downloaded(&reader, &at(file), file).await;
        let info: Value = reader.get(&at(&format!("{file}/info"))).await.ok_json();
        assert_eq!(
            (&info["name"], &info["size"]),
            (&json!("minutes.html"), &json!(PAGE.len())),
            "{info}"
        );
        assert_eq!(info.get("createdBy"), None, "nobody named: {info}");
        reader
            .get(&at(elsewhere))
            .await
            .check_status(StatusCode::NOT_FOUND);
        reader
            .get(&at(&format!("{elsewhere}/info")))
            .await
            .check_status(StatusCode::NOT_FOUND);
    }
    ts.teardown().await;
}

/// Downloads the file at `path`, whole and its first bytes, and checks that each is an
/// attachment of no kind that the browser may not sniff or run.
async fn check_downloaded(client: &Client, path: &str, file_id: &str) {
    let whole = client.get(path).await;
    whole.check_ok();
    assert_eq!(whole.body, PAGE, "the bytes of the file");
    let first = Client::send(
        client
            .bare_request(Method::GET, path)
            .header("Range", "bytes=0-5"),
    )
    .await;
    first.check_status(StatusCode::PARTIAL_CONTENT);
    assert_eq!(first.body, &PAGE[..6], "the range asked for");
    for response in [&whole, &first] {
        assert!(
            response
                .header("content-disposition")
                .starts_with(&format!("attachment;filename=\"{file_id}\"")),
            "an attachment: {}",
            response.header("content-disposition")
        );
        assert_eq!(
            response.header("x-content-type-options"),
            "nosniff",
            "the headers of a download"
        );
        assert_eq!(
            response.header("content-type"),
            "application/octet-stream",
            "the headers of a download"
        );
        assert!(
            response
                .header("content-security-policy")
                .starts_with("sandbox"),
            "opened as a document, it runs nothing"
        );
    }
}

/// A file block keeps its file through the trash: neither the card in the trash nor the
/// page in it lets its file be collected, and once they are gone for good the files are.
#[tokio::test]
async fn a_file_block_in_the_trash_keeps_its_file_until_it_is_gone_for_good() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts
        .create_board(TEST_WORKSPACE_ID, ContainerType::Private)
        .await;
    let file_id = uploaded_id(&upload(&ts.client, &board.id, "minutes.pdf", b"%PDF").await);
    let card: Value = ts
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "Card", "body": with_file(&file_id)}),
        )
        .await
        .created_json();
    let wiki_id = wiki(&ts.client, "Handbook").await;
    let page_id = page(&ts.client, &wiki_id, "", "Minutes").await;
    let wiki_file = uploaded_id(&upload(&ts.client, &wiki_id, "minutes.pdf", b"%PDF").await);
    write(&ts.client, &wiki_id, &page_id, &with_file(&wiki_file)).await;
    let card_id = card["id"].as_str().expect("an id");
    ts.client
        .delete(&format!("{TEST_WS}/boards/{}/blocks/{card_id}", board.id))
        .await
        .check_no_content();
    ts.client
        .delete(&format!("{TEST_WS}/wikis/{wiki_id}/pages/{page_id}"))
        .await
        .check_no_content();

    let collected = collected_names(&ts).await;
    assert!(
        !collected.contains(&file_id) && !collected.contains(&wiki_file),
        "kept while in the trash: {collected:?}"
    );
    ts.app
        .store()
        .purge_trash(i64::MAX / 2)
        .await
        .expect("the trash is emptied");
    let collected = collected_names(&ts).await;
    assert!(
        collected.contains(&file_id) && collected.contains(&wiki_file),
        "collected once gone for good: {collected:?}"
    );
    ts.teardown().await;
}

/// The stored names of the files collected now, however recently they were last shown.
async fn collected_names(ts: &TestServer) -> Vec<String> {
    ts.app
        .store()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("collected")
        .into_iter()
        .map(|file| file.stored_name)
        .collect()
}
