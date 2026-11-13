use super::*;
use crate::ws::{Listener, Listeners};
use std::io::Write as _;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

fn visit(board: &str, card: &str, token: &str) -> Visit {
    Visit {
        workspace_id: "workspace".to_owned(),
        board_id: board.to_owned(),
        card_id: card.to_owned(),
        read_token: token.to_owned(),
    }
}

fn add(listeners: &mut Listeners, user: &str, visit: Visit, age: Duration) {
    let (outbox, _) = mpsc::channel(1);
    listeners.next_id += 1;
    listeners.by_id.insert(
        listeners.next_id,
        Listener {
            user_id: user.to_owned(),
            session_id: String::new(),
            login_provider: None,
            outbox,
            closed: CancellationToken::new(),
            closed_because: Arc::default(),
            workspaces: Vec::new(),
            blocks: Vec::new(),
            shared_boards: Vec::new(),
            presence: State {
                visit: Some(visit),
                renewed: Some(Instant::now() - age),
                ..State::default()
            },
        },
    );
}

#[test]
fn a_stalled_connection_expires_without_a_socket_close_and_tabs_union_their_cards() {
    let mut listeners = Listeners::default();
    let here = visit("board", "", "");
    for (user, card, token, age) in [
        ("one", "a", "", Duration::ZERO),
        ("one", "b", "", Duration::ZERO),
        ("two", "c", "", LEASE),
        ("signed-in", "secret-card", "token", Duration::ZERO),
    ] {
        add(&mut listeners, user, visit("board", card, token), age);
    }
    add(
        &mut listeners,
        "elsewhere",
        visit("other-board", "d", ""),
        Duration::ZERO,
    );
    let roster = snapshot(&here, &listeners);
    assert_eq!(
        roster.people,
        vec![Person {
            user_id: "one".to_owned(),
            card_ids: BTreeSet::from(["a".to_owned(), "b".to_owned()])
        }],
        "stalled and other-board connections have no place; live tabs union their cards"
    );
    assert_eq!(
        roster.anonymous_count, 1,
        "a token never identifies its signed-in user"
    );
}

#[tokio::test(start_paused = true)]
async fn commands_have_a_bounded_burst_and_refill_at_the_configured_rate() {
    let mut rate = Rate {
        left: 2000,
        counted: Instant::now(),
    };
    assert!(rate.admits(2), "first command of the burst");
    assert!(rate.admits(2), "second command of the burst");
    assert!(!rate.admits(2), "the burst is exhausted");
    tokio::time::advance(Duration::from_millis(499)).await;
    assert!(!rate.admits(2), "no whole command has refilled yet");
    tokio::time::advance(Duration::from_millis(1)).await;
    assert!(
        rate.admits(2),
        "half a second buys one command at two per second"
    );
    assert!(!rate.admits(2), "only one refilled");
}

#[test]
fn fifty_visits_hold_only_bounded_identifiers_in_the_connection_table() {
    let id = "01930000-0000-7000-8000-000000000001";
    let mut listeners = Listeners::default();
    for _ in 0..50 {
        let mut place = visit(id, id, "");
        place.workspace_id = id.to_owned();
        add(&mut listeners, id, place, Duration::ZERO);
    }
    let bytes: usize = listeners
        .by_id
        .values()
        .map(|listener| {
            let place = listener.presence.visit.as_ref().expect("a visit");
            std::mem::size_of::<State>()
                + place.workspace_id.capacity()
                + place.board_id.capacity()
                + place.card_id.capacity()
                + place.read_token.capacity()
        })
        .sum();
    assert!(
        bytes < 20_000,
        "fifty active visit states hold {bytes} bytes before allocator overhead"
    );
    writeln!(std::io::stdout(), "presence table: 50 visits, {bytes} bytes (State plus owned identifier capacities; excludes existing sockets, allocator overhead and outgoing snapshot caches)").expect("the measurement is written");
}
