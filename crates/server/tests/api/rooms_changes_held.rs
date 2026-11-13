//! What the room sends a member while an immediate permission check holds them: nothing
//! of the others' changes until the store answered of them. Put out, they were sent
//! none of what was written after the change; kept, they are sent all of it at once.

use std::time::Duration;

use serde_json::json;

use crate::common::{Setup, TEST_WORKSPACE_ID, TestServer};
use crate::rooms::{Peer, STEP_1, STEP_2, frame, give, paragraphs, two_in_a_room};

/// How much longer than the store every immediate permission read takes: the time the
/// member is held, long enough for the owner to write twice meanwhile.
const SLOW_READ: Duration = Duration::from_secs(3);

/// When the owner begins to write after the change is asked for: the check holds the
/// member long before, and answers long after the owner is acknowledged.
const WRITE_AFTER: Duration = Duration::from_millis(400);

/// How long the member reads while held, after the owner was acknowledged.
const READ_WHILE_HELD: Duration = Duration::from_millis(400);

/// A store read slower than the writes, and well within its deadline (one interval of
/// the periodic check).
async fn slow_store() -> TestServer {
    let th = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 10)
        .start()
        .await
        .init_basic()
        .await;
    th.app.collab().slow_access_reads(SLOW_READ);
    th
}

/// The owner writes `lines` while the member is held, each acknowledged, and then the
/// member reads for a while: the texts of what the member holds by then.
async fn written_while_held(owner: &mut Peer, member: &mut Peer, lines: &[&str]) -> Vec<String> {
    tokio::time::sleep(WRITE_AFTER).await;
    for line in lines {
        owner.write_acked(line).await;
    }
    while member.step(READ_WHILE_HELD).await {}
    member.lines()
}

/// The member is taken off the board, and the store is slow to say so; the owner
/// writes meanwhile, after the removal was stored. The member is put out, and was
/// sent nothing of what was written after they could no longer read the card.
#[tokio::test]
async fn a_member_taken_off_the_board_is_sent_nothing_written_after() {
    let th = slow_store().await;
    let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let remove = async {
        th.client
            .delete(&format!(
                "/w/{TEST_WORKSPACE_ID}/boards/{}/members/{user}",
                table.board_id
            ))
            .await
            .check_no_content();
    };
    let ((), held) = tokio::join!(
        remove,
        written_while_held(&mut owner, &mut member, &["after the removal"])
    );
    assert_eq!(
        held,
        ["first"],
        "the held member was sent the owner's update"
    );
    assert_eq!(member.text("DOC_LEFT").await["reason"], json!("not_found"));
    member.settle().await;
    assert_eq!(member.lines(), ["first"]);
    th.teardown().await;
}

/// The member's role goes down to viewer, and the store is slow to say so; the owner
/// writes twice meanwhile. The member is sent none of it while held, and once kept
/// (reading from then on) all of it in one update, after `DOC_MODE`.
#[tokio::test]
async fn a_member_kept_is_sent_at_once_what_was_held_from_them() {
    let th = slow_store().await;
    let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let frames_before = member.taken.len();
    let ((), held) = tokio::join!(
        give(&th, &table.board_id, &user, "viewer"),
        written_while_held(&mut owner, &mut member, &["one", "two"])
    );
    assert_eq!(
        held,
        ["first"],
        "the held member was sent the owner's updates"
    );
    assert_eq!(member.text("DOC_MODE").await["mode"], json!("read"));
    member.settle().await;
    assert_eq!(member.lines(), ["first", "one", "two"]);
    assert_eq!(
        member.taken.len() - frames_before,
        1,
        "what was held is sent in one update: {:?}",
        &member.taken[frames_before..]
    );
    // Kept: what the owner writes next comes as it always does.
    owner.write_acked("three").await;
    member.settle().await;
    assert_eq!(member.lines(), ["first", "one", "two", "three"]);
    th.teardown().await;
}

/// A member held across the document being made anew (the card's body written whole
/// over HTTP) is sent the new document once kept, and not before.
#[tokio::test]
async fn a_member_kept_after_the_document_was_made_anew_is_sent_it_then() {
    let th = slow_store().await;
    let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let epoch_before = member.epoch;
    let replace = async {
        tokio::time::sleep(WRITE_AFTER).await;
        crate::rooms::write_whole(&th.client, &table.card_id, &paragraphs(&["replaced"])).await;
        owner.settle().await;
        while member.step(READ_WHILE_HELD).await {}
        (owner.lines(), member.lines(), member.epoch)
    };
    let ((), (owner_lines, held, held_epoch)) =
        tokio::join!(give(&th, &table.board_id, &user, "commenter"), replace);
    assert_eq!(owner_lines, ["replaced"]);
    assert_eq!(
        (held, held_epoch),
        (vec!["first".to_owned()], epoch_before),
        "the held member was sent the new document"
    );
    assert_eq!(member.text("DOC_MODE").await["mode"], json!("read"));
    member.text("DOC_RESET").await;
    member.settle().await;
    assert_eq!(member.lines(), ["replaced"]);
    assert!(member.epoch > epoch_before);
    th.teardown().await;
}

/// A held member asks what they are missing (a sync step 1): answered once the check
/// keeps them, after `DOC_MODE`, and not before.
#[tokio::test]
async fn a_held_member_asking_what_they_miss_is_answered_once_kept() {
    let th = slow_store().await;
    let (table, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let frames_before = member.taken.len();
    let ask = async {
        tokio::time::sleep(WRITE_AFTER).await;
        member
            .send_binary(frame(member.doc, member.epoch, STEP_1, &[0]))
            .await;
        while member.step(READ_WHILE_HELD).await {}
        member.taken.len()
    };
    let ((), frames_while_held) = tokio::join!(give(&th, &table.board_id, &user, "viewer"), ask);
    assert_eq!(
        frames_while_held, frames_before,
        "the held member's sync step 1 was answered while held"
    );
    assert_eq!(member.text("DOC_MODE").await["mode"], json!("read"));
    member.settle().await;
    let answered: Vec<u64> = member.taken[frames_before..]
        .iter()
        .map(|frame| frame.sync)
        .collect();
    assert_eq!(answered, [STEP_2]);
    assert_eq!(member.lines(), ["first"]);
    th.teardown().await;
}
