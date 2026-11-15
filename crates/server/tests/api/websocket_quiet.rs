//! A connection that has died without a word: the page asks whether its connection
//! still carries (`PING`, answered at once with `PONG`), and the server closes one
//! that has sent nothing for the quiet limit, which takes it out of its rooms.
//!
//! The limit is ninety seconds (`ws/inbound.rs`, whose tests hold it to that); these
//! tests shorten it once what they need is set up.

use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::common::{Setup, TestServer};
use crate::rooms::{Incoming, PATIENCE, Peer, awareness, paragraphs, two_in_a_room};
use crate::websocket::{Event, Socket};

/// The quiet limit of these tests.
const LIMIT: Duration = Duration::from_secs(3);

/// More than a loaded machine takes to close a connection once its limit has passed.
const LEEWAY: Duration = Duration::from_secs(10);

async fn server() -> TestServer {
    Setup::native().start().await.init_basic().await
}

/// Reads until a `PONG` comes, passing over what else does; how long it took.
async fn pong(peer: &mut Peer) -> Duration {
    let began = Instant::now();
    peer.send_json(json!({"action": "PING"})).await;
    loop {
        match peer.incoming(PATIENCE).await {
            Incoming::Text(message) if message["action"] == json!("PONG") => {
                assert_eq!(message, json!({"action": "PONG"}), "a PONG carries nothing");
                return began.elapsed();
            }
            Incoming::Closed | Incoming::Nothing => panic!("no PONG came"),
            _ => {}
        }
    }
}

/// A `PING` is answered with a `PONG` alone, on a connection with a session and on
/// one without; it is no command of anything else, and costs nothing of the rate of
/// the rooms' frames.
#[tokio::test]
async fn a_ping_is_answered_with_a_pong() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_frames_per_second = 10)
        .start()
        .await
        .init_basic()
        .await;
    let mut anonymous = Socket::connect(&th).await;
    anonymous.send(json!({"action": "PING"})).await;
    assert_eq!(
        anonymous.next_json(PATIENCE).await,
        Some(json!({"action": "PONG"}))
    );
    let (_table, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    // Far past the rooms' rate: none is counted.
    for _ in 0..50 {
        pong(&mut one).await;
    }
    let seq = one.write_acked("written after the pings").await;
    assert!(seq > 0, "the room takes what is written");
    two.settle().await;
    assert_eq!(two.lines(), ["first", "written after the pings"]);
    th.teardown().await;
}

/// A connection in a room that sends nothing is closed once the quiet limit has
/// passed, not before, and its caret goes from the room's others then; one that
/// pings stays, as long as it does.
#[tokio::test]
async fn a_connection_that_sends_nothing_is_closed_and_goes_from_its_room() {
    let th = server().await;
    let (_table, mut quiet, mut other) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    // From here on: the room was made under the server's own limit.
    th.app.ws().set_quiet_limit(LIMIT);
    let place = |clock: u64| json!({"type": null, "tname": null, "item": {"client": 7, "clock": clock}, "assoc": 0});
    let caret = json!({"user": {}, "cursor": {"anchor": place(1), "head": place(4)}});
    // Before the last thing it sends: the server hears it after this.
    let went_quiet = Instant::now();
    quiet
        .send_caret(&awareness(quiet.client(), 1, &caret))
        .await;
    let shown = other.caret().await;

    // The other pings at a third of the limit for three limits at least: it is
    // heard, and stays; it hears the quiet one's caret go.
    let mut gone = None;
    while went_quiet.elapsed() < LIMIT * 3
        || (gone.is_none() && went_quiet.elapsed() < LIMIT + LEEWAY)
    {
        pong(&mut other).await;
        let wait = LIMIT / 3;
        let began = Instant::now();
        while began.elapsed() < wait {
            if !other.step(wait.saturating_sub(began.elapsed())).await {
                break;
            }
        }
        if gone.is_none()
            && let Some(told) = other.carets.pop_front()
        {
            gone = Some((went_quiet.elapsed(), told));
        }
    }
    let (after, told) = gone.expect("the quiet connection's caret went");
    assert_eq!(
        (told.number, &told.state),
        (shown.number, &Value::Null),
        "the caret of the quiet connection goes"
    );
    assert!(
        after >= LIMIT && after < LIMIT + LEEWAY,
        "gone once the limit passed: after {after:?}"
    );
    assert!(
        matches!(quiet.incoming(PATIENCE).await, Incoming::Closed),
        "the quiet connection is closed"
    );
    assert!(pong(&mut other).await < PATIENCE, "the other stays");
    th.teardown().await;
}

/// The websocket's own Pong is heard as the page's `PING` is: a connection that
/// answers the server's Ping stays.
#[tokio::test]
async fn a_pong_to_the_servers_ping_keeps_a_connection() {
    let th = Setup::native()
        .configure(|config| config.websocket_ping_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    th.app.ws().set_quiet_limit(LIMIT);
    let mut socket = Socket::connect_as(&th.client).await;
    let began = Instant::now();
    // Reading answers the server's Pings: nothing else is sent.
    while began.elapsed() < LIMIT * 3 {
        let event = socket.next(LIMIT).await;
        assert!(matches!(event, Event::Ping), "only pings come: {event:?}");
    }
    socket.send(json!({"action": "PING"})).await;
    assert_eq!(
        socket.next_json(PATIENCE).await,
        Some(json!({"action": "PONG"}))
    );
    th.teardown().await;
}

/// A member whose change waits for an immediate check of their access is answered a
/// `PING` meanwhile, and is not closed for the quiet limit however long the check
/// takes: the server reads on while it works for the connection.
#[tokio::test]
async fn a_member_held_by_a_check_is_answered_and_kept() {
    let th = server().await;
    let (table, _owner, mut member) = two_in_a_room(&th, &paragraphs(&["first"])).await;
    th.app.ws().set_quiet_limit(LIMIT);
    let user = th.client2.me().await.id;
    let (reached, go) = th.app.collab().pause_next_access_read();
    let demote = async {
        crate::rooms::give(&th, &table.board_id, &user, "commenter").await;
    };
    let held = async {
        // The member pings while the change of their role makes its way to the
        // check, however long a loaded machine takes.
        let reached = reached.notified();
        tokio::pin!(reached);
        loop {
            tokio::select! {
                () = &mut reached => break,
                () = tokio::time::sleep(LIMIT / 3) => {
                    pong(&mut member).await;
                }
            }
        }
        // Held: the change waits for the check, and what is sent after it too.
        member.write("while held").await;
        let answered = pong(&mut member).await;
        // Past the quiet limit with nothing else sent: the connection works for the
        // member, it is not waiting for them.
        tokio::time::sleep(LIMIT * 2).await;
        go.notify_one();
        answered
    };
    let ((), answered) = tokio::join!(demote, held);
    assert!(answered < PATIENCE, "answered while held: {answered:?}");
    let refused = member.text("DOC_NACK").await;
    assert_eq!(refused["reason"], json!("document_read_only"), "{refused}");
    assert!(pong(&mut member).await < PATIENCE, "still open");
    th.teardown().await;
}
