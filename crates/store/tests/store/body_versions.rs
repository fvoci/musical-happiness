//! The versions kept of a card's body: which bodies are kept, who is named, how many
//! stay, what they hold of the files, and when they go.

use fvoci_model::{Block, BlockPatch, BlockType, Body, BodyChange, FileInfo, new_id};
use fvoci_store::{BodyToKeep, DEFAULT_BODY_VERSIONS_KEPT, Store};
use serde_json::json;

use crate::common;

/// A body that says `text` and shows the files.
pub(crate) fn body_showing(text: &str, names: &[&str]) -> Option<Body> {
    let mut content =
        vec![json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})];
    content.extend(
        names
            .iter()
            .map(|name| json!({"type": "image", "attrs": {"fileId": name}})),
    );
    Body::parse(json!({"type": "doc", "content": content})).expect("the body may be stored")
}

/// A board of a workspace of the test's organization, with the files `names`.
async fn board_with_files(store: &Store, user_id: &str, names: &[&str]) -> String {
    let workspace_id = common::workspace(store).await;
    let boards = common::create_test_boards(store, &workspace_id, user_id, 1).await;
    let board_id = boards[0].id.clone();
    for name in names {
        store
            .save_file_info(&FileInfo {
                id: new_id(),
                board_id: board_id.clone(),
                stored_name: (*name).to_owned(),
                create_at: 1,
                path: format!("boards/{board_id}/{name}"),
                name: "upload.png".to_owned(),
                extension: ".png".to_owned(),
                size: 7,
            })
            .await
            .expect("a file of the board is stored");
    }
    board_id
}

/// A card of the board with the body, made by the user.
async fn card_with(store: &Store, board_id: &str, body: Option<Body>, user_id: &str) -> String {
    let mut card = Block {
        id: new_id(),
        board_id: board_id.to_owned(),
        block_type: BlockType::Card,
        body,
        ..Block::default()
    };
    common::insert_blocks(store, std::slice::from_mut(&mut card), user_id).await;
    card.id
}

/// Writes the card's body whole, by the user.
async fn write_body(store: &Store, card_id: &str, body: Option<Body>, user_id: &str) {
    let patch = BlockPatch {
        body: body.map_or(BodyChange::Clear, BodyChange::Set),
        ..BlockPatch::default()
    };
    store
        .patch_block(card_id, &patch, user_id)
        .await
        .expect("the body is written");
}

/// Keeps the card's body as a version, its digest the body's text (`-` for none):
/// the store takes the digest as it is given it.
pub(crate) async fn keep(store: &Store, card_id: &str, written_by: &str) -> Option<i64> {
    let block = store.get_block(card_id).await.expect("the card");
    let digest = block
        .body
        .as_ref()
        .map_or_else(|| "-".to_owned(), Body::text);
    let kept = store
        .keep_body_versions(&[BodyToKeep {
            block: &block,
            digest: digest.as_bytes(),
            written_by,
        }])
        .await
        .expect("kept");
    kept[0]
}

/// The numbers and texts of the versions of a card, the newest first.
async fn texts(store: &Store, card_id: &str) -> Vec<(i64, String)> {
    let mut texts = Vec::new();
    for kept in store.body_versions(card_id).await.expect("the versions") {
        let (_, body) = store
            .body_version(card_id, kept.version)
            .await
            .expect("read")
            .expect("a listed version is there");
        texts.push((
            kept.version,
            body.map(|body| body.text()).unwrap_or_default(),
        ));
    }
    texts
}

/// A body is kept once: the same body again is no version, a body that changed is the
/// next, and no body is a version only of a card that has one to go back from.
#[tokio::test]
async fn a_body_is_kept_when_it_is_not_the_newest_version() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let card_id = card_with(store, &board_id, None, &author).await;

    assert_eq!(keep(store, &card_id, "").await, None, "no body, no version");
    assert_eq!(texts(store, &card_id).await, []);

    write_body(store, &card_id, body_showing("one", &[]), &author).await;
    assert_eq!(keep(store, &card_id, "").await, Some(1));
    assert_eq!(keep(store, &card_id, "").await, None, "kept already");
    write_body(store, &card_id, body_showing("two", &[]), &author).await;
    assert_eq!(keep(store, &card_id, "").await, Some(2));
    write_body(store, &card_id, None, &author).await;
    assert_eq!(keep(store, &card_id, "").await, Some(3), "cleared");
    write_body(store, &card_id, body_showing("one", &[]), &author).await;
    assert_eq!(
        keep(store, &card_id, "").await,
        Some(4),
        "an earlier body again is a version: only the newest is compared"
    );
    assert_eq!(
        texts(store, &card_id).await,
        [
            (4, "one".to_owned()),
            (3, String::new()),
            (2, "two".to_owned()),
            (1, "one".to_owned())
        ]
    );
    assert!(
        store
            .body_version(&card_id, 9)
            .await
            .expect("read")
            .is_none(),
        "no such version"
    );
    test.teardown().await;
}

/// A version names who wrote the body since the one before: the editors whose updates
/// were logged, and who wrote it whole; nobody known, who changed the card last, when
/// they did. A body that is kept already takes the names with it.
#[tokio::test]
async fn a_version_names_who_wrote_the_body_since_the_one_before() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let editor = common::user(store, "kelly.kapoor").await;
    let other = common::user(store, "oscar.martinez").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let card_id = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let made = store.get_block(&card_id).await.expect("the card");

    // Nobody known: who changed the card last, when they did.
    assert_eq!(keep(store, &card_id, "").await, Some(1));
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(versions[0].editors, std::slice::from_ref(&author));
    assert_eq!(versions[0].kept_at, made.update_at);

    // The editors whose updates were logged since, each once, and who wrote it whole.
    store
        .create_body_doc(&card_id, b"a state", b"a digest")
        .await
        .expect("a document");
    for by in [&editor, &other, &editor] {
        store
            .append_body_updates(
                &card_id,
                &[(b"an update", by.as_str())],
                &[],
                &[by.as_str()],
            )
            .await
            .expect("logged");
    }
    write_body(store, &card_id, body_showing("two", &[]), &author).await;
    let before = crate::blocks::now_millis();
    assert_eq!(keep(store, &card_id, &author).await, Some(2));
    let versions = store.body_versions(&card_id).await.expect("the versions");
    let mut named = vec![author.clone(), editor.clone(), other.clone()];
    named.sort();
    assert_eq!(
        versions[0].editors, named,
        "in the order of the identifiers"
    );
    assert!(versions[0].kept_at >= before, "when it was kept");
    let (read, _) = store
        .body_version(&card_id, 2)
        .await
        .expect("read")
        .expect("there");
    assert_eq!(read, versions[0], "one version reads as the list has it");

    // An update whose body is the newest version's leaves nobody for the next.
    store
        .append_body_updates(&card_id, &[(b"an update", &editor)], &[], &[&editor])
        .await
        .expect("logged");
    assert_eq!(keep(store, &card_id, "").await, None);
    write_body(store, &card_id, body_showing("three", &[]), &other).await;
    assert_eq!(keep(store, &card_id, &other).await, Some(3));
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(versions[0].editors, std::slice::from_ref(&other));
    test.teardown().await;
}

/// The newest versions are kept, as many as set: the oldest go as new ones come, the
/// numbers go on, and a file only a version that went showed is collected.
#[tokio::test]
async fn the_oldest_versions_go_and_their_files_with_them() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let board_id = board_with_files(store, &author, &["7first.png", "7second.png"]).await;
    let first = body_showing("v1", &["7first.png"]);
    let card_id = card_with(store, &board_id, first, &author).await;
    store.set_body_versions_kept(3);
    assert_eq!(keep(store, &card_id, &author).await, Some(1));
    for (text, files) in [("v2", &["7second.png"][..]), ("v3", &[]), ("v4", &[])] {
        write_body(store, &card_id, body_showing(text, files), &author).await;
        keep(store, &card_id, &author).await.expect("a version");
    }
    assert_eq!(
        texts(store, &card_id).await,
        [
            (4, "v4".to_owned()),
            (3, "v3".to_owned()),
            (2, "v2".to_owned())
        ],
        "the fourth takes the first out"
    );
    let later = crate::blocks::now_millis() + 3_600_000;
    let collected: Vec<String> = store
        .collect_unreferenced_files(later)
        .await
        .expect("collected")
        .into_iter()
        .map(|file| file.stored_name)
        .collect();
    assert_eq!(
        collected,
        ["7first.png"],
        "the second version still shows its file, which the card does not"
    );
    for table in ["body_versions", "body_version_editors"] {
        assert_eq!(
            store.count_rows(table).await.expect("counted"),
            3,
            "{table}"
        );
    }
    store.set_body_versions_kept(DEFAULT_BODY_VERSIONS_KEPT);
    test.teardown().await;
}

/// The versions stay while the card is in the trash, come back with it, and go when it
/// is gone for good, with who wrote them and who was put down as writing since.
#[tokio::test]
async fn versions_stay_in_the_trash_and_go_with_the_card() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let card_id = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let stays = card_with(store, &board_id, body_showing("other", &[]), &author).await;
    for id in [&card_id, &stays] {
        assert_eq!(keep(store, id, &author).await, Some(1));
        store
            .create_body_doc(id, b"a state", b"a digest")
            .await
            .expect("a document");
        store
            .append_body_updates(id, &[(b"an update", &author)], &[], &[&author])
            .await
            .expect("logged");
    }

    store
        .delete_block(&card_id, &author)
        .await
        .expect("the card is deleted");
    assert_eq!(texts(store, &card_id).await, [(1, "one".to_owned())]);
    store
        .undelete_block(&card_id, &author)
        .await
        .expect("the card is restored");
    assert_eq!(texts(store, &card_id).await, [(1, "one".to_owned())]);

    store
        .delete_block(&card_id, &author)
        .await
        .expect("the card is deleted again");
    store
        .purge_trash(i64::MAX)
        .await
        .expect("the trash is purged");
    assert_eq!(texts(store, &card_id).await, []);
    assert_eq!(texts(store, &stays).await, [(1, "other".to_owned())]);
    assert_eq!(store.count_rows("body_versions").await.expect("counted"), 1);
    assert_eq!(
        store
            .count_rows("body_version_editors")
            .await
            .expect("counted"),
        2,
        "the other card's: of its version, and who wrote since"
    );
    test.teardown().await;
}
