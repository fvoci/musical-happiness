//! A change of a team reaches the rooms of its boards' cards without waiting for the
//! periodic check: whoever reads a board only through a team is put out, or reads
//! only, as soon as the change is answered. What the workspace's subscription is told
//! of the board follows the change too.

use serde_json::{Value, json};
use tokio::time::Instant;

use crate::board_broadcasts::WorkspaceSocket;
use crate::common::{TEST_WORKSPACE_ID, TestServer};
use crate::rooms::{Peer, Table, give, paragraphs, table};
use crate::rooms_changes::{seldom, soon, workspace_admin};

/// A team of the test workspace with the second client's user in it.
async fn team_of_the_second(th: &TestServer, name: &str) -> String {
    let team: Value = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/teams"),
            &json!({ "name": name }),
        )
        .await
        .ok_json();
    let team_id = team["id"].as_str().expect("a team has an id").to_owned();
    let user = th.client2.me().await.id;
    th.client
        .put(
            &team_path(&team_id, &format!("/members/{user}")),
            &json!({}),
        )
        .await
        .check_no_content();
    team_id
}

fn team_path(team_id: &str, rest: &str) -> String {
    format!("/w/{TEST_WORKSPACE_ID}/teams/{team_id}{rest}")
}

/// Retitles the card of `table`, as its board's admin: a change of a block, which is
/// told to the board's members as they are then, and changes nobody's access.
async fn retitle(th: &TestServer, table: &Table, title: &str) {
    th.client
        .patch(
            &format!("/w/{TEST_WORKSPACE_ID}/cards/{}", table.card_id),
            &json!({ "title": title }),
        )
        .await
        .check_ok();
}

fn assignment(table: &Table, team_id: &str) -> String {
    format!(
        "/w/{TEST_WORKSPACE_ID}/boards/{}/teams/{team_id}",
        table.board_id
    )
}

async fn assign(th: &TestServer, table: &Table, team_id: &str, role: &str) {
    th.client
        .put(&assignment(table, team_id), &json!({ "role": role }))
        .await
        .check_no_content();
}

/// The second client's user in the room of a card of a private board that they read
/// only as a member of a team with the role `role` on it, writing if it lets them.
async fn in_a_room_through_a_team(th: &TestServer, role: &str) -> (Table, String, Peer) {
    workspace_admin(th).await;
    let table = table(th, &paragraphs(&["first"])).await;
    let team_id = team_of_the_second(th, "Design").await;
    assign(th, &table, &team_id, role).await;
    let mut member = Peer::connect(&th.client2, 301).await;
    member.joined(TEST_WORKSPACE_ID, &table.card_id).await;
    (table, team_id, member)
}

async fn left_at_once(member: &mut Peer, changed_at: Instant) {
    let left = soon(member, "DOC_LEFT", changed_at).await;
    assert_eq!(
        left,
        json!({"action": "DOC_LEFT", "doc": member.doc, "reason": "not_found"}),
        "the change put the reader out of the room they were in"
    );
}

#[tokio::test]
async fn leaving_a_team_puts_its_reader_out_of_the_room_at_once() {
    let th = seldom().start().await.init_basic().await;
    let (_table, team_id, mut member) = in_a_room_through_a_team(&th, "editor").await;
    let user = th.client2.me().await.id;
    let changed_at = Instant::now();
    th.client
        .delete(&team_path(&team_id, &format!("/members/{user}")))
        .await
        .check_no_content();
    left_at_once(&mut member, changed_at).await;
    th.teardown().await;
}

#[tokio::test]
async fn a_team_role_lowered_then_taken_off_the_board_reaches_the_room_at_once() {
    let th = seldom().start().await.init_basic().await;
    let (table, team_id, mut member) = in_a_room_through_a_team(&th, "editor").await;
    member.write_acked("written through the team").await;

    let changed_at = Instant::now();
    assign(&th, &table, &team_id, "viewer").await;
    assert_eq!(
        soon(&mut member, "DOC_MODE", changed_at).await["mode"],
        json!("read")
    );
    member.write("refused").await;
    assert_eq!(
        member.text("DOC_NACK").await["reason"],
        json!("document_read_only")
    );

    let changed_at = Instant::now();
    th.client
        .delete(&assignment(&table, &team_id))
        .await
        .check_no_content();
    left_at_once(&mut member, changed_at).await;
    th.teardown().await;
}

#[tokio::test]
async fn deleting_a_team_puts_its_readers_out_of_the_room_at_once() {
    let th = seldom().start().await.init_basic().await;
    let (_table, team_id, mut member) = in_a_room_through_a_team(&th, "commenter").await;
    let changed_at = Instant::now();
    th.client
        .delete(&team_path(&team_id, ""))
        .await
        .check_no_content();
    left_at_once(&mut member, changed_at).await;
    th.teardown().await;
}

#[tokio::test]
async fn joining_a_team_that_edits_lets_a_reader_write_at_once() {
    let th = seldom().start().await.init_basic().await;
    workspace_admin(&th).await;
    let table = table(&th, &paragraphs(&["first"])).await;
    let user = th.client2.me().await.id;
    give(&th, &table.board_id, &user, "viewer").await;
    let team: Value = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/teams"),
            &json!({ "name": "Editors" }),
        )
        .await
        .ok_json();
    let team_id = team["id"].as_str().expect("a team has an id");
    assign(&th, &table, team_id, "editor").await;
    let mut member = Peer::connect(&th.client2, 302).await;
    let joined = member.joined(TEST_WORKSPACE_ID, &table.card_id).await;
    assert_eq!(joined["mode"], json!("read"), "{joined}");

    let changed_at = Instant::now();
    th.client
        .put(&team_path(team_id, &format!("/members/{user}")), &json!({}))
        .await
        .check_no_content();
    assert_eq!(
        soon(&mut member, "DOC_MODE", changed_at).await["mode"],
        json!("write")
    );
    member.write_acked("written once in the team").await;
    th.teardown().await;
}

#[tokio::test]
async fn deleting_a_team_reaches_every_board_it_was_on_and_leaves_the_admin_writing() {
    let th = seldom().start().await.init_basic().await;
    workspace_admin(&th).await;
    let team_id = team_of_the_second(&th, "Both boards").await;
    let mut rooms = Vec::new();
    for (index, text) in [(0, "first board"), (1, "second board")] {
        let table = table(&th, &paragraphs(&[text])).await;
        assign(&th, &table, &team_id, "editor").await;
        let mut owner = Peer::connect(&th.client, 310 + index).await;
        owner.joined(TEST_WORKSPACE_ID, &table.card_id).await;
        let mut member = Peer::connect(&th.client2, 320 + index).await;
        member.joined(TEST_WORKSPACE_ID, &table.card_id).await;
        rooms.push((owner, member));
    }

    let changed_at = Instant::now();
    th.client
        .delete(&team_path(&team_id, ""))
        .await
        .check_no_content();
    for (owner, member) in &mut rooms {
        left_at_once(member, changed_at).await;
        owner.write_acked("the owner of the board writes on").await;
        owner.settle().await;
        assert!(
            owner
                .unread()
                .iter()
                .all(|message| !["DOC_LEFT", "DOC_MODE"]
                    .contains(&message["action"].as_str().unwrap_or_default())),
            "the board's own admin is not told anything: {:?}",
            owner.unread()
        );
    }
    th.teardown().await;
}

#[tokio::test]
async fn a_board_is_told_to_whoever_reads_it_through_a_team_until_they_leave_it() {
    let th = seldom().start().await.init_basic().await;
    workspace_admin(&th).await;
    let table = table(&th, &paragraphs(&["first"])).await;
    let team_id = team_of_the_second(&th, "Listeners").await;
    assign(&th, &table, &team_id, "viewer").await;
    let user = th.client2.me().await.id;
    let mut socket = WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &user).await;

    retitle(&th, &table, "Told").await;
    socket
        .expect("UPDATE_BLOCK", "/block/id", &table.card_id)
        .await;
    th.client
        .delete(&team_path(&team_id, &format!("/members/{user}")))
        .await
        .check_no_content();
    socket
        .expect("DELETE_MEMBER", "/member/boardId", &table.board_id)
        .await;
    retitle(&th, &table, "Not told").await;
    let heard = socket.drain().await;
    assert!(
        heard
            .iter()
            .all(|message| message["action"] != json!("UPDATE_BLOCK")),
        "who left the team hears no more of the board: {heard:?}"
    );
    th.teardown().await;
}

#[tokio::test]
async fn a_board_is_told_to_whoever_joins_a_team_on_it_from_then_on() {
    let th = seldom().start().await.init_basic().await;
    workspace_admin(&th).await;
    let table = table(&th, &paragraphs(&["first"])).await;
    let made: Value = th
        .client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/teams"),
            &json!({ "name": "Joined later" }),
        )
        .await
        .ok_json();
    let team_id = made["id"].as_str().expect("a team has an id").to_owned();
    assign(&th, &table, &team_id, "viewer").await;
    let user = th.client2.me().await.id;
    let mut socket = WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &user).await;

    // Told to the board's members as they are before the person joins the team.
    retitle(&th, &table, "Not told yet").await;
    let heard = socket.drain().await;
    assert!(
        heard
            .iter()
            .all(|message| message["action"] != json!("UPDATE_BLOCK")),
        "who is in no team of the board hears nothing of it: {heard:?}"
    );
    th.client
        .put(
            &team_path(&team_id, &format!("/members/{user}")),
            &json!({}),
        )
        .await
        .check_no_content();
    retitle(&th, &table, "Told").await;
    let told = socket
        .expect("UPDATE_BLOCK", "/block/id", &table.card_id)
        .await;
    assert_eq!(told["block"]["title"], json!("Told"), "{told}");
    th.teardown().await;
}
