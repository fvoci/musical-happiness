//! A permission change asks again only what it reaches: a connection coming into a
//! room is admitted again, and a periodic answer dropped, for a change of its room's
//! board and its user, not for one of another board.

use std::time::Duration;

use fvoci_model::{BoardMember, BoardType};
use serde_json::json;
use tokio::time::Instant;

use crate::common::{Setup, TEST_WORKSPACE_ID, TestServer};
use crate::rooms::{Peer, give, paragraphs, table, two_in_a_room};

/// What the tests allow for something that waits for nothing slow.
const PROMPT: Duration = Duration::from_secs(1);

/// The periodic check comes when a test asks for it, not by itself.
fn seldom() -> Setup {
    Setup::native().configure(|config| config.collab.collab_recheck_seconds = 3600)
}

/// Comes into the room of `card_id` while `change` is made between the admission and
/// the room: whether the connection was admitted once more for it.
async fn admitted_again_for(
    th: &TestServer,
    peer: &mut Peer,
    card_id: &str,
    change: impl Future<Output = ()>,
) -> bool {
    let (admitted, go) = th.app.collab().pause_next_admission();
    let joining = peer.join(TEST_WORKSPACE_ID, card_id);
    let changing = async {
        admitted.notified().await;
        change.await;
        let (again, go_again) = th.app.collab().pause_next_admission();
        go.notify_one();
        let again = tokio::time::timeout(PROMPT, again.notified()).await.is_ok();
        go_again.notify_one();
        again
    };
    let (answer, again) = tokio::join!(joining, changing);
    assert_eq!(answer["action"], json!("DOC_JOINED"), "{answer}");
    // Not admitted again: the pause set for that is taken by no one; take it away.
    drop(th.app.collab().pause_next_admission());
    again
}

/// Changes go on on another board of the organization while a member comes into a
/// room: the member is let in at once, never admitted again for them (three changes
/// would refuse them). A change of the room's board for that member does admit
/// them again.
#[tokio::test]
async fn a_change_of_another_board_does_not_admit_a_joining_member_again() {
    let th = seldom().start().await.init_basic().await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let elsewhere = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let user = th.client2.me().await.id;
    give(&th, &t.board_id, &user, "editor").await;
    let mut peer = Peer::connect(&th.client2, 102).await;
    for role in ["viewer", "editor", "commenter"] {
        let again = admitted_again_for(&th, &mut peer, &t.card_id, async {
            give(&th, &elsewhere.id, &user, role).await;
        })
        .await;
        assert!(
            !again,
            "admitted again for a change of another board ({role})"
        );
    }
    let again = admitted_again_for(&th, &mut peer, &t.card_id, async {
        give(&th, &t.board_id, &user, "editor").await;
    })
    .await;
    assert!(
        again,
        "a change of the room's board for its member admits them again"
    );
    th.teardown().await;
}

/// A member made a reader where the store alone says so (no immediate check), and a
/// change of another board for the same member between the periodic check's reads
/// and its answers: the periodic answer is applied all the same.
#[tokio::test]
async fn a_periodic_answer_is_applied_though_another_board_changed_meanwhile() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 3)
        .start()
        .await
        .init_basic()
        .await;
    let (t, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let elsewhere = th.create_board(TEST_WORKSPACE_ID, BoardType::Private).await;
    let user = th.client2.me().await.id;
    // A pass that read before the store changed: let go without reading again.
    let (read, go) = th.app.collab().pause_next_recheck();
    tokio::time::timeout(Duration::from_secs(10), read.notified())
        .await
        .expect("the periodic check reads");
    th.app
        .store()
        .save_member(&BoardMember {
            board_id: t.board_id.clone(),
            user_id: user.clone(),
            scheme_viewer: true,
            ..BoardMember::default()
        })
        .await
        .expect("the member is made a reader in the store alone");
    go.notify_one();
    // The next pass reads the reader's role; another board changes before it answers.
    let (read, go) = th.app.collab().pause_next_recheck();
    tokio::time::timeout(Duration::from_secs(10), read.notified())
        .await
        .expect("the periodic check reads again");
    give(&th, &elsewhere.id, &user, "editor").await;
    let released = Instant::now();
    go.notify_one();
    let mode = tokio::time::timeout(PROMPT, member.text("DOC_MODE"))
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the periodic answer was not applied within {PROMPT:?} of its release \
                 (waited {:?}): it was dropped for a change of another board",
                released.elapsed()
            )
        });
    assert_eq!(mode["mode"], json!("read"));
    th.teardown().await;
}
