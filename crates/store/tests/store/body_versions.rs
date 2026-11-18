//! The versions kept of a card's body: which bodies are kept, who is named, how many
//! stay, what they hold of the files, and when they go.

use fvoci_model::{
    Block, BlockPatch, BlockType, Body, BodyChange, Container, ContainerKind, ContainerType,
    FileInfo, NewPage, new_id,
};
use fvoci_store::{BodyBefore, DEFAULT_BODY_VERSIONS_KEPT, Store};
use serde_json::json;

use crate::boards::new_board;
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
pub(crate) async fn board_with_files(store: &Store, user_id: &str, names: &[&str]) -> String {
    let workspace_id = common::workspace(store).await;
    let boards = common::create_test_boards(store, &workspace_id, user_id, 1).await;
    let board_id = boards[0].id.clone();
    for name in names {
        store
            .save_file_info(&FileInfo {
                id: new_id(),
                container_id: board_id.clone(),
                stored_name: (*name).to_owned(),
                create_at: 1,
                path: format!("boards/{board_id}/{name}"),
                name: "upload.png".to_owned(),
                extension: ".png".to_owned(),
                size: 7,
                ..FileInfo::default()
            })
            .await
            .expect("a file of the board is stored");
    }
    board_id
}

/// A card of the board with the body, made by the user.
pub(crate) async fn card_with(
    store: &Store,
    board_id: &str,
    body: Option<Body>,
    user_id: &str,
) -> String {
    let mut card = Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        block_type: BlockType::Card,
        body,
        ..Block::default()
    };
    common::insert_blocks(store, std::slice::from_mut(&mut card), user_id).await;
    card.id
}

/// Writes the card's body whole, by the user: the store keeps its versions itself.
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

/// Keeps the body the card holds as a version, as the server does with what editors
/// typed.
pub(crate) async fn keep(store: &Store, card_id: &str) -> Option<i64> {
    store.keep_body_version(card_id).await.expect("kept")
}

/// Has the card's document hold `body` and writes it to the card, as the server does
/// with what editors typed: by `user_id`, and no version of its own.
pub(crate) async fn type_body(store: &Store, card_id: &str, body: Option<Body>, user_id: &str) {
    if store.get_body_doc(card_id).await.expect("read").is_none() {
        store
            .create_body_doc(card_id, b"a state", b"a digest")
            .await
            .expect("a document");
    }
    let as_read = crate::body_docs::as_read(store, card_id).await;
    let seqs = store
        .append_body_updates(
            card_id,
            &[(b"an update", user_id)],
            as_read,
            &[],
            &[user_id],
            BodyBefore::Leave,
        )
        .await
        .expect("logged");
    let read_at = store.get_block(card_id).await.expect("the card").update_at;
    let digest = fvoci_store::body_digest(body.as_ref());
    let as_read = crate::body_docs::as_read(store, card_id).await;
    store
        .project_body(
            card_id,
            Some(body),
            read_at,
            seqs[0],
            &digest,
            user_id,
            as_read,
        )
        .await
        .expect("written to the card");
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

/// A body written whole is kept within its write: the body before, if it was no version
/// yet, by nobody and as of the card's last change, and the body after, by its writer.
/// The same body again and a change of something else keep nothing, and only the
/// newest version is compared.
#[tokio::test]
async fn a_body_written_whole_keeps_the_body_before_and_the_body_after() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let editor = common::user(store, "kelly.kapoor").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let card_id = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    assert_eq!(texts(store, &card_id).await, [], "a card as it was made");
    // Renamed by another: who wrote the body the card was made with is not known.
    let renamed = BlockPatch {
        title: Some("named".to_owned()),
        ..BlockPatch::default()
    };
    store
        .patch_block(&card_id, &renamed, &editor)
        .await
        .expect("renamed");
    let made = store.get_block(&card_id).await.expect("the card");

    write_body(store, &card_id, body_showing("two", &[]), &editor).await;
    assert_eq!(
        texts(store, &card_id).await,
        [(2, "two".to_owned()), (1, "one".to_owned())]
    );
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(versions[1].editors, Vec::<String>::new(), "nobody known");
    assert_eq!(
        versions[1].kept_at, made.update_at,
        "as of the card's change"
    );
    assert_eq!(versions[0].editors, std::slice::from_ref(&editor));
    let (read, _) = store
        .body_version(&card_id, 2)
        .await
        .expect("read")
        .expect("there");
    assert_eq!(read, versions[0], "one version reads as the list has it");

    write_body(store, &card_id, body_showing("two", &[]), &author).await;
    let renamed = BlockPatch {
        title: Some("renamed".to_owned()),
        ..BlockPatch::default()
    };
    store
        .patch_block(&card_id, &renamed, &author)
        .await
        .expect("renamed");
    assert_eq!(texts(store, &card_id).await.len(), 2, "no body changed");

    write_body(store, &card_id, None, &author).await;
    write_body(store, &card_id, body_showing("one", &[]), &author).await;
    assert_eq!(
        texts(store, &card_id).await,
        [
            (4, "one".to_owned()),
            (3, String::new()),
            (2, "two".to_owned()),
            (1, "one".to_owned())
        ],
        "no body is a version, and an earlier body again is one"
    );
    assert!(
        store
            .body_version(&card_id, 9)
            .await
            .expect("read")
            .is_none(),
        "no such version"
    );

    // No body is no first version: there is nothing in it to go back to.
    let empty = card_with(store, &board_id, None, &author).await;
    assert_eq!(keep(store, &empty).await, None);
    assert_eq!(keep(store, &new_id()).await, None, "no such card");
    write_body(store, &empty, body_showing("first", &[]), &editor).await;
    assert_eq!(texts(store, &empty).await, [(1, "first".to_owned())]);
    test.teardown().await;
}

/// A card whose editors typed what no version holds is one a server that starts sees
/// to, until its body is kept; a block that is a card no more is not, though the
/// names of its editors stay put down.
#[tokio::test]
async fn a_card_with_what_no_version_holds_is_seen_to_and_no_other_block() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "erin.hannon").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let kept = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let changed = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let unfinished = async || -> Vec<String> {
        let docs = store.unfinished_body_docs().await.expect("read");
        docs.into_iter().map(|(block_id, _)| block_id).collect()
    };
    assert_eq!(unfinished().await, Vec::<String>::new());

    for card_id in [&kept, &changed] {
        type_body(store, card_id, body_showing("two", &[]), &author).await;
    }
    let mut both = vec![kept.clone(), changed.clone()];
    both.sort();
    assert_eq!(unfinished().await, both);

    assert!(keep(store, &kept).await.is_some());
    let mut view = store.get_block(&changed).await.expect("the card");
    view.block_type = BlockType::View;
    view.body = None;
    common::insert_blocks(store, std::slice::from_mut(&mut view), &author).await;
    assert_eq!(keep(store, &changed).await, None, "no card, no version");
    assert_eq!(unfinished().await, Vec::<String>::new());
    test.teardown().await;
}

/// What editors typed is a version when the server keeps it, once: it names the users
/// whose updates were logged since the version before, and nobody if none is known.
/// A body written whole over what they typed keeps theirs first.
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

    // Nobody known: as of the card's last change.
    assert_eq!(keep(store, &card_id).await, Some(1));
    assert_eq!(keep(store, &card_id).await, None, "kept already");
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(versions[0].editors, Vec::<String>::new());
    assert_eq!(versions[0].kept_at, made.update_at);

    // The editors whose updates were logged since, each once; no version until kept.
    for by in [&editor, &other, &editor] {
        type_body(store, &card_id, body_showing("two", &[]), by).await;
    }
    assert_eq!(texts(store, &card_id).await.len(), 1, "typed, not kept yet");
    let stored = store.get_body_doc(&card_id).await.expect("read");
    let mut typists = vec![editor.clone(), other.clone()];
    typists.sort();
    assert_eq!(
        stored.expect("a document").unkept_editors,
        typists,
        "put down with the log"
    );
    let before = crate::blocks::now_millis();
    assert_eq!(keep(store, &card_id).await, Some(2));
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(
        versions[0].editors, typists,
        "in the order of the identifiers"
    );
    assert!(versions[0].kept_at >= before, "when it was kept");
    let stored = store.get_body_doc(&card_id).await.expect("read");
    assert_eq!(
        stored.expect("a document").unkept_editors,
        Vec::<String>::new()
    );

    // Typed and written whole over: theirs is kept before the writer's.
    type_body(store, &card_id, body_showing("three", &[]), &other).await;
    write_body(store, &card_id, body_showing("four", &[]), &author).await;
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(
        versions
            .iter()
            .map(|kept| (kept.version, kept.editors.as_slice()))
            .collect::<Vec<_>>()[..2],
        [
            (4, std::slice::from_ref(&author)),
            (3, std::slice::from_ref(&other))
        ]
    );
    assert_eq!(texts(store, &card_id).await[1], (3, "three".to_owned()));
    test.teardown().await;
}

/// The body a card holds before updates of its document are logged is kept within
/// that write when asked: by nobody, with the sender put down as writing since, and
/// not at all when the updates are refused.
#[tokio::test]
async fn the_body_before_an_update_is_kept_with_its_log() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "meredith.palmer").await;
    let editor = common::user(store, "kelly.kapoor").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let card_id = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    store
        .create_body_doc(&card_id, b"a state", b"a digest")
        .await
        .expect("a document");
    let update: &[(&[u8], &str)] = &[(b"an update", &editor)];

    let no_file = ["7nothing.png".to_owned()];
    let as_read = crate::body_docs::as_read(store, &card_id).await;
    store
        .append_body_updates(
            &card_id,
            update,
            as_read,
            &no_file,
            &[&editor],
            BodyBefore::Keep,
        )
        .await
        .expect_err("the board has no such file");
    assert_eq!(
        texts(store, &card_id).await,
        [],
        "nothing logged, nothing kept"
    );

    for _ in 0..2 {
        let as_read = crate::body_docs::as_read(store, &card_id).await;
        store
            .append_body_updates(&card_id, update, as_read, &[], &[&editor], BodyBefore::Keep)
            .await
            .expect("logged");
    }
    assert_eq!(
        texts(store, &card_id).await,
        [(1, "one".to_owned())],
        "once"
    );
    let versions = store.body_versions(&card_id).await.expect("the versions");
    assert_eq!(versions[0].editors, Vec::<String>::new());
    let stored = store.get_body_doc(&card_id).await.expect("read");
    assert_eq!(
        stored.expect("a document").unkept_editors,
        std::slice::from_ref(&editor),
        "the sender wrote since"
    );
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
    for (text, files) in [("v2", &["7second.png"][..]), ("v3", &[]), ("v4", &[])] {
        write_body(store, &card_id, body_showing(text, files), &author).await;
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
    let collected = async || -> Vec<String> {
        store
            .collect_unreferenced_files(later)
            .await
            .expect("collected")
            .into_iter()
            .map(|file| file.stored_name)
            .collect()
    };
    assert_eq!(
        collected().await,
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

    // The same when the server keeps what editors typed.
    type_body(store, &card_id, body_showing("v5", &[]), &author).await;
    assert_eq!(keep(store, &card_id).await, Some(5));
    assert_eq!(texts(store, &card_id).await.len(), 3);
    assert_eq!(collected().await, ["7second.png"]);
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
    for (id, text) in [(&card_id, "one"), (&stays, "other")] {
        type_body(store, id, body_showing(text, &[]), &author).await;
        assert_eq!(keep(store, id).await, Some(1));
        let as_read = crate::body_docs::as_read(store, id).await;
        store
            .append_body_updates(
                id,
                &[(b"an update", &author)],
                as_read,
                &[],
                &[&author],
                BodyBefore::Leave,
            )
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

/// The body of a page is kept as that of a card: written whole it keeps the body
/// after, and before it what its editors typed; what they typed is seen to until the
/// server keeps it.
#[tokio::test]
async fn the_body_of_a_page_is_kept_as_that_of_a_card() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "pam.beesly").await;
    let editor = common::user(store, "jim.halpert").await;
    let workspace_id = common::workspace_of(store, &[&author, &editor]).await;
    let mut wiki = Container {
        kind: ContainerKind::Wiki,
        ..new_board(&new_id(), &workspace_id, ContainerType::Private)
    };
    store
        .insert_container_with_admin(&mut wiki, &author)
        .await
        .expect("the wiki is stored");
    let page_id = store
        .create_page(
            &wiki.id,
            &NewPage::default(),
            fvoci_model::PageLimits::default(),
            &author,
        )
        .await
        .expect("the page is made")
        .page
        .id;
    assert_eq!(texts(store, &page_id).await, [], "a page as it was made");

    write_body(store, &page_id, body_showing("one", &[]), &author).await;
    write_body(store, &page_id, body_showing("two", &[]), &author).await;
    assert_eq!(
        texts(store, &page_id).await,
        [(2, "two".to_owned()), (1, "one".to_owned())]
    );

    type_body(store, &page_id, body_showing("three", &[]), &author).await;
    let unfinished = store.unfinished_body_docs().await.expect("read");
    let unfinished: Vec<&str> = unfinished.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(unfinished, [page_id.as_str()], "typed and not kept yet");
    // Written whole over what was typed: what was typed is kept first, by who typed it.
    write_body(store, &page_id, body_showing("four", &[]), &editor).await;
    assert_eq!(
        texts(store, &page_id).await[..2],
        [(4, "four".to_owned()), (3, "three".to_owned())]
    );
    let versions = store.body_versions(&page_id).await.expect("the versions");
    assert_eq!(versions[1].editors, std::slice::from_ref(&author));
    assert_eq!(store.unfinished_body_docs().await.expect("read"), []);

    type_body(store, &page_id, body_showing("five", &[]), &author).await;
    assert_eq!(
        keep(store, &page_id).await,
        Some(5),
        "kept as the server asks"
    );
    assert_eq!(store.unfinished_body_docs().await.expect("read"), []);
    test.teardown().await;
}

/// A document whose log holds updates its state does not is one a server that starts
/// folds, written to its card and kept or not; folded, it is not, and neither is the
/// document of a card in the trash.
#[tokio::test]
async fn a_document_with_a_log_its_state_does_not_hold_is_folded_at_the_start() {
    let test = common::setup().await;
    let store = &test.store;
    let author = common::user(store, "kelly.kapoor").await;
    let board_id = board_with_files(store, &author, &[]).await;
    let folded = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let logged = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let trashed = card_with(store, &board_id, body_showing("one", &[]), &author).await;
    let unfolded = async || -> Vec<String> {
        let docs = store.unfolded_body_docs().await.expect("read");
        docs.into_iter().map(|(block_id, _)| block_id).collect()
    };
    assert_eq!(unfolded().await, Vec::<String>::new(), "no documents yet");
    for card_id in [&folded, &logged, &trashed] {
        type_body(store, card_id, body_showing("two", &[]), &author).await;
        keep(store, card_id).await;
    }
    assert_eq!(
        store.unfinished_body_docs().await.expect("read"),
        [],
        "written and kept"
    );
    let as_read = store
        .get_body_doc(&folded)
        .await
        .expect("read")
        .expect("a document")
        .as_read();
    let log_end = as_read.log_end;
    store
        .fold_body_doc(&folded, b"the state with it", log_end, as_read)
        .await
        .expect("folded");
    store
        .delete_block(&trashed, &author)
        .await
        .expect("to the trash");
    assert_eq!(unfolded().await, std::slice::from_ref(&logged));
    test.teardown().await;
}
