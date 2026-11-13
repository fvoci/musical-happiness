//! Who is in a room is asked again while they are there: a role that was lowered
//! reads from then on, and who may no longer read is put out.

use serde_json::json;

use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Incoming, QUIET, UPDATE, frame, give, no_rooms, paragraphs, two_in_a_room};

/// A server that asks every second whether who is in a room may be there.
fn asking_often() -> Setup {
    Setup::native().configure(|config| config.collab.collab_recheck_seconds = 1)
}

/// An editor in a room is made a viewer: at the next asking it is told that it reads,
/// and what it writes is refused; made an editor again, it writes. Taken off the
/// board, it is put out, as for a card that is not there, and the room goes on
/// without it. Who stays is told nothing.
#[tokio::test]
async fn a_lowered_role_reads_and_who_is_taken_off_the_board_is_put_out() {
    let th = asking_often().start().await.init_basic().await;
    let (t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    two.write_acked("as an editor").await;

    give(&th, &t.board_id, &user2, "viewer").await;
    let mode = two.text("DOC_MODE").await;
    assert_eq!(
        mode,
        json!({"action": "DOC_MODE", "doc": two.doc, "mode": "read"})
    );
    two.write("as a viewer").await;
    let nack = two.text("DOC_NACK").await;
    assert_eq!(nack["reason"], json!("document_read_only"), "{nack}");
    one.settle().await;
    assert_eq!(one.lines(), ["first", "as an editor"]);
    // It still reads.
    one.write_acked("to a viewer").await;
    two.settle().await;
    assert!(two.lines().contains(&"to a viewer".to_owned()));

    // An editor again: it comes in anew to drop what was refused, and writes.
    give(&th, &t.board_id, &user2, "editor").await;
    let mode = two.text("DOC_MODE").await;
    assert_eq!(mode["mode"], json!("write"), "{mode}");
    two.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    two.write_acked("as an editor again").await;
    one.settle().await;
    assert_eq!(one.body(), two.body());

    // Off the board: out of the room, and what it sends reaches nobody.
    th.client
        .delete(&format!(
            "/w/{TEST_WORKSPACE_ID}/boards/{}/members/{user2}",
            t.board_id
        ))
        .await
        .check_no_content();
    let left = two.text("DOC_LEFT").await;
    assert_eq!(
        left,
        json!({"action": "DOC_LEFT", "doc": two.doc, "reason": "not_found"})
    );
    let late = two.editor().append_paragraph("after it was put out");
    two.send_binary(frame(two.doc, two.epoch, UPDATE, &late))
        .await;
    assert_eq!(two.incoming(QUIET).await, Incoming::Nothing, "not answered");
    let refused = two.join(TEST_WORKSPACE_ID, &t.card_id).await;
    assert_eq!(refused["reason"], json!("not_found"), "{refused}");
    one.write_acked("without it").await;
    assert_eq!(
        one.lines(),
        [
            "first",
            "as an editor",
            "to a viewer",
            "as an editor again",
            "without it"
        ]
    );
    assert!(one.unread().is_empty(), "{:?}", one.unread());
    assert_eq!(th.app.collab().rooms(), 1);
    th.teardown().await;
}

/// Who leaves the card's workspace, with a role left on the board, is put out too.
#[tokio::test]
async fn who_is_taken_out_of_the_workspace_is_put_out_of_the_room() {
    let th = asking_often().start().await.init_basic().await;
    let (_t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let user2 = th.client2.me().await.id;
    crate::workspace_removal::remove_from_the_test_workspace(&th, &user2).await;
    let left = two.text("DOC_LEFT").await;
    assert_eq!(left["reason"], json!("not_found"), "{left}");
    one.write_acked("goes on").await;
    assert!(one.unread().is_empty(), "{:?}", one.unread());
    th.teardown().await;
}

/// A card that is deleted while its room is open: who is in the room is put out at
/// the next asking, as for a card that is not there.
#[tokio::test]
async fn the_room_of_a_deleted_card_is_emptied() {
    let th = asking_often().start().await.init_basic().await;
    let (t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    th.client
        .delete(&format!(
            "/w/{TEST_WORKSPACE_ID}/boards/{}/blocks/{}",
            t.board_id, t.card_id
        ))
        .await
        .check_no_content();
    for peer in [&mut one, &mut two] {
        let left = peer.text("DOC_LEFT").await;
        assert_eq!(
            left,
            json!({"action": "DOC_LEFT", "doc": peer.doc, "reason": "not_found"})
        );
    }
    crate::rooms::no_rooms(&th).await;
    th.teardown().await;
}

/// The store cannot be asked about who is in a room: asked once more two seconds
/// later, it answers, and nobody is put out; when it does not answer then either,
/// everybody is, with the reason it gave. The memberships of the boards are out of the
/// store's reach meanwhile: their table has another name.
#[tokio::test]
async fn a_store_that_fails_for_a_moment_empties_no_room_and_one_that_stays_failed_does() {
    let th = asking_often().start().await.init_basic().await;
    let (t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let rename = async |from: &str, to: &str| {
        th.app
            .store()
            .execute_unchecked(&format!("ALTER TABLE {from} RENAME TO {to}"))
            .await
            .expect("the table of the memberships is renamed");
    };
    // For longer than the time between two askings, and less than until the second.
    rename("board_members", "board_members_away").await;
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    rename("board_members_away", "board_members").await;
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    for peer in [&mut one, &mut two] {
        assert_eq!(peer.incoming(QUIET).await, Incoming::Nothing, "stays in");
    }
    one.write_acked("after the store came back").await;
    two.settle().await;
    assert_eq!(two.lines(), ["first", "after the store came back"]);

    rename("board_members", "board_members_away").await;
    for peer in [&mut one, &mut two] {
        let left = peer.text("DOC_LEFT").await;
        assert_eq!(
            left,
            json!({"action": "DOC_LEFT", "doc": peer.doc, "reason": "internal"})
        );
    }
    rename("board_members_away", "board_members").await;
    no_rooms(&th).await;
    one.joined(TEST_WORKSPACE_ID, &t.card_id).await;
    assert_eq!(one.lines(), ["first", "after the store came back"]);
    th.teardown().await;
}

/// Removing the direct role keeps a collaborator who reads through a team writing in
/// the room, and their leaving the team puts them out before it is answered: the
/// periodic check is too seldom to do either.
#[tokio::test]
async fn losing_the_team_role_puts_the_member_out_of_the_room() {
    use fvoci_model::{Team, WorkspaceRole};
    let th = crate::rooms_changes::seldom()
        .start()
        .await
        .init_basic()
        .await;
    let (t, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    let owner = th.client.me().await.id;
    let user = th.client2.me().await.id;
    th.app
        .store()
        .change_workspace_role(
            TEST_WORKSPACE_ID,
            &owner,
            (WorkspaceRole::Member, WorkspaceRole::Admin),
            &owner,
        )
        .await
        .unwrap();
    let team: Team = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/teams"),
            &json!({"name":"Collaborators"}),
        )
        .await
        .ok_json();
    let link = format!("/w/{TEST_WORKSPACE_ID}/teams/{}/members/{user}", team.id);
    th.client.put(&link, &json!({})).await.check_no_content();
    th.client
        .put(
            &format!(
                "/w/{TEST_WORKSPACE_ID}/boards/{}/teams/{}",
                t.board_id, team.id
            ),
            &json!({"role":"editor"}),
        )
        .await
        .check_no_content();
    th.client
        .delete(&format!(
            "/w/{TEST_WORKSPACE_ID}/boards/{}/members/{user}",
            t.board_id
        ))
        .await
        .check_no_content();
    two.write_acked("through a team").await;
    let changed_at = tokio::time::Instant::now();
    th.client.delete(&link).await.check_no_content();
    let left = crate::rooms_changes::soon(&mut two, "DOC_LEFT", changed_at).await;
    assert_eq!(
        left,
        json!({"action": "DOC_LEFT", "doc": two.doc, "reason": "not_found"})
    );
    let refused = two.join(TEST_WORKSPACE_ID, &t.card_id).await;
    assert_eq!(refused["reason"], json!("not_found"), "{refused}");
    one.write_acked("after the team member left").await;
    th.teardown().await;
}
