//! Twenty connections in one room: how long an update takes to be answered and to
//! reach everyone, and what the server and its worker hold meanwhile.

use std::time::{Duration, Instant};

use serde_json::json;

use crate::common::{Setup, TEST_WORKSPACE_ID};
use crate::rooms::{Incoming, PATIENCE, Peer, UPDATE, card_holds, paragraphs, table};

/// The resident memory of a process, in KiB, as `/proc` has it; `None` where there
/// is no `/proc`.
fn resident_kib(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn millis(duration: Duration) -> String {
    format!("{:.1}", duration.as_secs_f64() * 1000.0)
}

/// The middle, the ninety-fifth of a hundred and the longest of `times`.
fn spread(times: &mut [Duration]) -> String {
    times.sort_unstable();
    let at = |hundredth: usize| times[(times.len() - 1) * hundredth / 100];
    format!(
        "median {} ms, p95 {} ms, max {} ms",
        millis(at(50)),
        millis(at(95)),
        millis(at(100))
    )
}

#[expect(clippy::print_stderr, reason = "the measurements are for the report")]
fn report(line: &str) {
    let profile = if cfg!(debug_assertions) {
        "debug build"
    } else {
        "release build"
    };
    eprintln!("measured ({profile}): {line}");
}

/// Twenty connections in the room of one card write in turn, and then all at once:
/// every one holds the same document in the end, and so does the card. Prints how
/// long an update took until its sender was answered and until the last of the other
/// nineteen had it, and the memory of the server's process (which is this test's)
/// and of its worker.
#[tokio::test]
async fn twenty_connections_edit_one_body() {
    const PEERS: usize = 20;
    const ROUNDS: usize = 5;
    let th = Setup::native()
        // All twenty are one user's.
        .configure(|config| config.collab.collab_connections_per_user = 20)
        .start()
        .await
        .init_basic()
        .await;
    let t = table(&th, &paragraphs(&["first"])).await;
    let before = resident_kib(std::process::id());
    let mut peers = Vec::new();
    for client in 0..PEERS {
        let mut peer = Peer::connect(&th.client, 100 + u64::try_from(client).expect("few")).await;
        peer.joined(TEST_WORKSPACE_ID, &t.card_id).await;
        peers.push(peer);
    }

    // In turn: each update is answered and has reached everyone before the next.
    let (mut answered, mut reached) = (Vec::new(), Vec::new());
    for round in 0..ROUNDS {
        for writer in 0..PEERS {
            let began = Instant::now();
            peers[writer].write(&format!("{round}.{writer}")).await;
            let ack = peers[writer].text("DOC_ACK").await;
            assert_eq!(ack["epoch"], json!(1), "{ack}");
            answered.push(began.elapsed());
            for (reader, peer) in peers.iter_mut().enumerate() {
                if reader == writer {
                    continue;
                }
                match peer.incoming(PATIENCE).await {
                    Incoming::Frame(frame) => {
                        assert_eq!(frame.sync, UPDATE, "{frame:?}");
                        peer.editor().receive(&frame.payload);
                    }
                    other => panic!("the update reaches everyone: {other:?}"),
                }
            }
            reached.push(began.elapsed());
        }
    }
    report(&format!(
        "{PEERS} connections in one room, {} updates in turn: answered (DOC_ACK) {}; \
         at the last of the other {} {}",
        answered.len(),
        spread(&mut answered),
        PEERS - 1,
        spread(&mut reached),
    ));

    // All at once: each writes before it has the others'.
    let began = Instant::now();
    for (writer, peer) in peers.iter_mut().enumerate() {
        peer.write(&format!("at once {writer}")).await;
    }
    for peer in &mut peers {
        peer.text("DOC_ACK").await;
    }
    let all_answered = began.elapsed();
    for peer in &mut peers {
        peer.settle().await;
    }
    let body = peers[0].body().expect("a body");
    assert_eq!(
        peers[0].lines().len(),
        1 + ROUNDS * PEERS + PEERS,
        "every update is in it"
    );
    for peer in &peers {
        assert_eq!(peer.body().as_ref(), Some(&body), "all hold the same");
    }
    let worker = th.app.collab().worker_pids().await[0];
    report(&format!(
        "{PEERS} updates at once: all answered after {} ms; resident memory of the \
         server's process (with the test and its {PEERS} editors) {} KiB before the \
         connections and {} KiB after, of the worker {} KiB",
        millis(all_answered),
        before.map_or_else(|| "?".to_owned(), |kib| kib.to_string()),
        resident_kib(std::process::id()).map_or_else(|| "?".to_owned(), |kib| kib.to_string()),
        worker
            .and_then(resident_kib)
            .map_or_else(|| "?".to_owned(), |kib| kib.to_string()),
    ));
    drop(peers);
    card_holds(&th.client, &t.card_id, &body).await;
    th.teardown().await;
}
