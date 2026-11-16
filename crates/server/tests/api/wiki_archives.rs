//! Wikis in archives: exported with their pages, the comments on them and the images of
//! their bodies, by the route of a board and with the workspace; imported as wikis, the
//! tree, bodies and files as they were, what a body named among the archive's blocks
//! named by the copies. What the rules of a wiki's tree refuse stores nothing of the
//! archive.

use std::io::{Cursor, Write as _};

use fvoci_model::{BoardKind, BoardMember, PageNode};
use serde_json::{Value, json};

use crate::boards::another_workspace;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer};
use crate::export::import;
use crate::files::{upload, uploaded_id};
use crate::references::made;
use crate::references_shared::references_in;
use crate::references_to::{body_naming, card_names};
use crate::wiki_copies::{body_of, page, titled, titled_id, tree_of, wiki, write};

/// A wiki of "Welcome" with "Getting started" and "Second steps" below it, "Policies"
/// after it and "Old" in the trash. "Getting started" shows a picture, "Welcome" names
/// "Getting started" and `outside`, and has a comment.
struct Handbook {
    wiki: String,
    file: String,
}

async fn handbook(client: &Client, outside: &str) -> Handbook {
    let wiki = wiki(client, "Handbook").await;
    let a = page(client, &wiki, "", "Welcome").await;
    let b = page(client, &wiki, &a, "Getting started").await;
    page(client, &wiki, &a, "Second steps").await;
    page(client, &wiki, "", "Policies").await;
    let old = page(client, &wiki, "", "Old").await;
    client
        .delete(&format!("{TEST_WS}/wikis/{wiki}/pages/{old}"))
        .await
        .check_no_content();
    let file = uploaded_id(&upload(client, &wiki, "pic.png", b"a picture\n").await);
    let image = json!({"type": "doc", "content": [{"type": "image", "attrs": {"fileId": file}}]});
    write(client, &wiki, &b, &image).await;
    write(client, &wiki, &a, &body_naming("see", &[&b, outside])).await;
    client
        .post(
            &format!("{TEST_WS}/wikis/{wiki}/pages/{a}/comments"),
            &json!({"body": body_naming("a comment", &[])}),
        )
        .await
        .check_created();
    Handbook { wiki, file }
}

/// The wikis of the workspace at `at`.
async fn wikis_at(client: &Client, at: &str) -> Vec<fvoci_model::Board> {
    client.get(&format!("{at}/wikis")).await.ok_json()
}

/// Checks that the wiki `copy` of the workspace at `at` is the handbook again: its tree
/// without the trash, "Welcome" naming its own "Getting started" and `outside`, the
/// picture a file of the copy's with the same bytes, the comment, no versions, and the
/// importer its admin.
async fn check_handbook(ts: &TestServer, at: &str, copy: &str, source: &Handbook, outside: &str) {
    let client = &ts.client;
    let source_tree = tree_of(client, TEST_WS, &source.wiki).await;
    let copy_tree: Vec<PageNode> = tree_of(client, at, copy).await;
    assert_eq!(titled(&copy_tree), titled(&source_tree), "the same tree");
    assert_eq!(copy_tree.len(), 4, "the page in the trash stays behind");
    let (a, b) = (
        titled_id(&copy_tree, "Welcome"),
        titled_id(&copy_tree, "Getting started"),
    );
    assert!(
        !source_tree.iter().any(|page| page.id == a || page.id == b),
        "the pages are new"
    );
    let named = references_in(&body_of(client, at, copy, &a).await);
    assert_eq!(
        named,
        [b.clone(), outside.to_owned()],
        "its own page, and what is outside"
    );
    let body = body_of(client, at, copy, &b).await;
    let file = body["content"][0]["attrs"]["fileId"]
        .as_str()
        .expect("the copy shows its picture")
        .to_owned();
    assert_ne!(file, source.file, "a file of the copy's own");
    let read = client
        .get(&format!("{at}/boards/{copy}/files/{file}"))
        .await;
    read.check_ok();
    assert_eq!(read.body, b"a picture\n", "the same bytes");
    let comments: Vec<Value> = client
        .get(&format!("{at}/wikis/{copy}/pages/{a}/comments"))
        .await
        .ok_json();
    assert_eq!(comments.len(), 1, "the comments come as a board's do");
    for page in &copy_tree {
        let kept = ts.app.store().body_versions(&page.id).await;
        assert!(kept.expect("the versions").is_empty(), "no versions come");
    }
    let members: Vec<BoardMember> = client
        .get(&format!("{at}/boards/{copy}/members"))
        .await
        .ok_json();
    let me = client.me().await.id;
    assert!(
        members
            .iter()
            .any(|member| member.user_id == me && member.scheme_admin),
        "who imports administers it"
    );
}

#[tokio::test]
async fn a_wiki_exported_by_its_route_is_imported_into_another_workspace_as_it_was() {
    let ts = Setup::native().start().await.init_basic().await;
    let client = &ts.client;
    let elsewhere = made(client, TEST_WS, "O", "elsewhere").await;
    let source = handbook(client, &elsewhere.card).await;
    let exported = client
        .get(&format!("{TEST_WS}/boards/{}/archive/export", source.wiki))
        .await;
    exported.check_ok();

    let me = client.me().await.id;
    let other = another_workspace(&ts, &me).await;
    let at = format!("/w/{other}");
    import(client, &other, &exported.body).await.check_ok();
    let wikis = wikis_at(client, &at).await;
    assert_eq!(wikis.len(), 1, "made as a wiki: {wikis:?}");
    let copy = &wikis[0];
    assert_eq!(
        (copy.kind, copy.title.as_str()),
        (BoardKind::Wiki, "Handbook")
    );
    assert_eq!(copy.workspace_id, other);
    check_handbook(&ts, &at, &copy.id, &source, &elsewhere.card).await;
    ts.teardown().await;
}

#[tokio::test]
async fn the_workspace_archive_has_its_wikis_and_a_card_names_the_imported_page() {
    let ts = Setup::native().start().await.init_basic().await;
    let client = &ts.client;
    let elsewhere = made(client, TEST_WS, "O", "elsewhere").await;
    let source = handbook(client, &elsewhere.card).await;
    // The card of another board of the archive names a page of the wiki.
    let welcome = titled_id(&tree_of(client, TEST_WS, &source.wiki).await, "Welcome");
    card_names(client, TEST_WS, &elsewhere.card, &[&welcome]).await;
    let exported = client.get(&format!("{TEST_WS}/archive/export")).await;
    exported.check_ok();

    let me = client.me().await.id;
    let other = another_workspace(&ts, &me).await;
    let at = format!("/w/{other}");
    import(client, &other, &exported.body).await.check_ok();
    let wikis = wikis_at(client, &at).await;
    let copy = wikis
        .iter()
        .find(|wiki| wiki.title == "Handbook")
        .expect("the handbook is imported");
    let copy_tree = tree_of(client, &at, &copy.id).await;
    let copy_welcome = titled_id(&copy_tree, "Welcome");
    let boards: Vec<fvoci_model::Board> = client.get(&format!("{at}/boards")).await.ok_json();
    let copied_board = boards
        .iter()
        .find(|board| board.title == "elsewhere (its container)")
        .expect("the board is imported beside it");
    let cards: Vec<Value> = client
        .get(&format!("{at}/boards/{}/cards", copied_board.id))
        .await
        .ok_json();
    let card_copy = cards[0]["id"].as_str().expect("an id").to_owned();
    let card = client.get(&format!("{at}/cards/{card_copy}")).await;
    let named = references_in(&card.ok_json::<Value>()["body"]);
    assert_eq!(
        named,
        [copy_welcome.as_str()],
        "the card names the imported page"
    );
    // "Welcome" named the card: in the copy, the imported card.
    let welcome_named = references_in(&body_of(client, &at, &copy.id, &copy_welcome).await);
    assert_eq!(welcome_named[1], card_copy);
    ts.teardown().await;
}

/// An archive of version 3 of the given `board.jsonl`s, each its lines, in that order.
fn archive_of(boards: &[&[Value]]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer.start_file("version.json", options).expect("start");
    writer
        .write_all(br#"{"version":3,"date":1}"#)
        .expect("write");
    for (at, lines) in boards.iter().enumerate() {
        writer
            .start_file(format!("b{at}/board.jsonl"), options)
            .expect("start");
        for line in *lines {
            writer
                .write_all(format!("{line}\n").as_bytes())
                .expect("write");
        }
    }
    writer.finish().expect("finish").into_inner()
}

fn board_line(id: &str, kind: &str, title: &str) -> Value {
    json!({"type": "board", "data": {
        "id": id, "workspaceId": TEST_WORKSPACE_ID, "type": "P", "kind": kind,
        "title": title, "createAt": 1, "updateAt": 1,
    }})
}

fn block_line(id: &str, board: &str, parent: &str, block_type: &str) -> Value {
    json!({"type": "block", "data": {
        "id": id, "boardId": board, "parentId": parent, "type": block_type,
        "title": id, "createAt": 1, "updateAt": 1,
    }})
}

#[tokio::test]
async fn what_the_rules_of_a_wiki_refuse_stores_nothing_of_the_archive() {
    let ts = Setup::native()
        .configure(|config| {
            config.wiki.wiki_page_depth = 2;
            config.wiki.wiki_pages = 3;
        })
        .start()
        .await
        .init_basic()
        .await;
    let client = &ts.client;
    let board = [board_line("b", "board", "Archived board")];
    let refused = |blocks: Vec<Value>, reason: Option<&'static str>| {
        let mut wiki = vec![board_line("w", "wiki", "Archived wiki")];
        wiki.extend(blocks);
        let bytes = archive_of(&[&board, &wiki]);
        async move {
            let answer = import(client, TEST_WORKSPACE_ID, &bytes).await;
            answer.check_bad_request();
            assert_eq!(answer.refused_place(), None, "{}", answer.text());
            if let Some(reason) = reason {
                assert_eq!(answer.value()["reason"], json!(reason), "{}", answer.text());
            }
        }
    };
    let page = |id: &str, parent: &str| block_line(id, "w", parent, "page");
    refused(vec![block_line("c", "w", "", "card")], None).await;
    refused(vec![page("a", "gone")], None).await;
    refused(vec![page("a", "b"), page("b", "a")], None).await;
    refused(
        vec![
            page("a", ""),
            block_line("x", "w", "a", "comment"),
            page("p", "x"),
        ],
        None,
    )
    .await;
    refused(
        vec![page("a", ""), page("b", "a"), page("c", "b")],
        Some("page_too_deep"),
    )
    .await;
    refused(
        vec![page("a", ""), page("b", ""), page("c", ""), page("d", "")],
        Some("too_many_pages"),
    )
    .await;
    // A page on a board, before a wiki that would do.
    let board_with_page = [
        board_line("b", "board", "Archived board"),
        block_line("p", "b", "", "page"),
    ];
    let wiki = [board_line("w", "wiki", "Archived wiki"), page("a", "")];
    let answer = import(
        client,
        TEST_WORKSPACE_ID,
        &archive_of(&[&wiki, &board_with_page]),
    )
    .await;
    answer.check_bad_request();

    let boards: Vec<fvoci_model::Board> = client.get(&format!("{TEST_WS}/boards")).await.ok_json();
    assert!(
        !boards
            .iter()
            .any(|board| board.title.starts_with("Archived"))
    );
    assert!(
        wikis_at(client, TEST_WS).await.is_empty(),
        "nothing is stored"
    );

    // Within the rules, the board and the wiki are made.
    let wiki = [
        board_line("w", "wiki", "Archived wiki"),
        page("a", "w"),
        page("b", "a"),
        block_line("x", "w", "b", "comment"),
    ];
    import(client, TEST_WORKSPACE_ID, &archive_of(&[&board, &wiki]))
        .await
        .check_ok();
    let wikis = wikis_at(client, TEST_WS).await;
    assert_eq!(wikis.len(), 1);
    let tree = tree_of(client, TEST_WS, &wikis[0].id).await;
    assert_eq!(
        titled(&tree),
        [
            ("a".to_owned(), String::new()),
            ("b".to_owned(), "a".to_owned())
        ]
    );
    ts.teardown().await;
}
