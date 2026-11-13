//! An immediate permission check whose store read is slow, as seen by everybody it does
//! not concern: the other members of the room it is reading for, and the other
//! permission changes of the same organization.

use std::time::Duration;

use tokio::time::Instant;

use crate::common::{Setup, TEST_WORKSPACE_ID};
use serde_json::json;

use crate::rooms::{Peer, give, paragraphs, two_in_a_room};

/// How long the immediate check may wait for the store: one interval of the periodic
/// check (`collab_recheck_seconds`).
const RECHECK_SECONDS: u64 = 10;

/// What the tests allow for an answer that waits for nothing slow.
const PROMPT: Duration = Duration::from_secs(2);

fn slow_store_deadline() -> Setup {
    Setup::native().configure(|config| config.collab.collab_recheck_seconds = RECHECK_SECONDS)
}

/// The member's role is changed and the store does not answer for them; the owner,
/// whom the change does not concern, types meanwhile in the same room. Their update
/// waits for the room's turn, which the immediate check holds across its store read
/// until the deadline (one recheck interval: 30 seconds by default).
#[tokio::test]
async fn a_slow_permission_read_for_one_member_does_not_hold_the_others_typing() {
    let th = slow_store_deadline().start().await.init_basic().await;
    let (table, mut owner, _member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    let waiting = th.app.collab().stall_next_access_check();
    let demote = give(&th, &table.board_id, &user, "viewer");
    let typing = async {
        waiting.notified().await;
        let started = Instant::now();
        owner.write("typed while another member is checked").await;
        let acked = tokio::time::timeout(PROMPT, owner.text("DOC_ACK"))
            .await
            .is_ok();
        (acked, started.elapsed())
    };
    let ((), (acked, waited)) = tokio::join!(demote, typing);
    assert!(
        acked,
        "the owner's update was not acknowledged within {PROMPT:?} (waited {waited:?}): \
         the immediate check of another member held the room's turn while the store was slow"
    );
    th.teardown().await;
}

/// The same slow read, and a change on another board of the organization that no room
/// is open on: its request waits for the organization's turn of immediate checks, held
/// by the slow one until its deadline.
#[tokio::test]
async fn a_slow_permission_read_does_not_hold_other_changes_of_the_organization() {
    let th = slow_store_deadline().start().await.init_basic().await;
    let (table, _owner, _member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let elsewhere = th
        .create_board(TEST_WORKSPACE_ID, fvoci_model::BoardType::Private)
        .await;
    let user = th.client2.me().await.id;
    let waiting = th.app.collab().stall_next_access_check();
    let demote = give(&th, &table.board_id, &user, "viewer");
    let other_change = async {
        waiting.notified().await;
        let started = Instant::now();
        let answered = tokio::time::timeout(PROMPT, give(&th, &elsewhere.id, &user, "editor"))
            .await
            .is_ok();
        (answered, started.elapsed())
    };
    let ((), (answered, waited)) = tokio::join!(demote, other_change);
    assert!(
        answered,
        "adding a member to a board with no open room did not answer within {PROMPT:?} \
         (waited {waited:?}): it waited for another board's slow immediate check"
    );
    th.teardown().await;
}

/// The `DOC_LEFT` messages a peer is sent until nothing comes for a while.
async fn left(peer: &mut Peer) -> Vec<serde_json::Value> {
    peer.settle().await;
    peer.unread()
        .into_iter()
        .filter(|message| message["action"] == json!("DOC_LEFT"))
        .collect()
}

/// A change of the whole board (its minimum role) that leaves both members' roles as
/// they are, and the store slow for one of them only. The read that is slow passes the
/// pass's one deadline; every read after it in the pass has no time left and its
/// member is put out with `document_unavailable` although the store would have
/// answered at once that they may stay: here the board's admin, in their own card.
#[tokio::test]
async fn a_slow_permission_read_puts_out_nobody_whose_read_was_not_slow() {
    let th = slow_store_deadline().start().await.init_basic().await;
    let (table, mut owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let _stalled = th.app.collab().stall_next_access_check();
    th.client
        .patch(
            &format!("/w/{TEST_WORKSPACE_ID}/boards/{}", table.board_id),
            &json!({"minimumRole": "viewer"}),
        )
        .await
        .check_ok();
    let (owner_left, member_left) = (left(&mut owner).await, left(&mut member).await);
    assert!(
        owner_left.is_empty() || member_left.is_empty(),
        "both members were put out, though only one read was slow and both keep their \
         roles: owner {owner_left:?}, member {member_left:?}"
    );
    th.teardown().await;
}
