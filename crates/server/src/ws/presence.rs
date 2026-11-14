//! One visit per connection, indexed by board, coalesced every 250 ms and projected
//! for each reader (`batch`).

mod batch;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};
use tokio::time::Instant;

use super::readers::ReaderLimits;
use super::{Command, Hub, HubInner, ListenerId, Listeners, Outgoing};
use crate::app::{App, Asker};
use crate::auth::secret::SECRET_LENGTH;
use crate::collab::{AccessChange, Wall};

/// A page renews its visit every 30 seconds; a stopped page disappears too.
const LEASE: Duration = Duration::from_secs(90);
/// A failed permission lookup must not leave a connection present indefinitely.
const LOOKUP_WAIT: Duration = Duration::from_secs(5);
const LONGEST_ID: usize = 64;
/// The action of the roster a reader is sent.
const UPDATE_PRESENCE: &str = "UPDATE_PRESENCE";

/// A board as the visits of one roster share it: its organization, workspace and ID.
type BoardKey = (Wall, String, String);

#[derive(Clone, PartialEq, Eq)]
struct Visit {
    workspace_id: String,
    board_id: String,
    /// The open card, checked to be one of the board's; empty if none.
    card_id: String,
    read_token: String,
    /// The board's organization, as access changes are told apart by.
    wall: Wall,
}

impl Visit {
    /// Whether `change` may have changed who is let in here or who sees whom; a
    /// change not told (`None`, the listener lagged) may have.
    fn reached_by(&self, change: Option<&AccessChange>) -> bool {
        change.is_none_or(|change| {
            change.reaches(self.wall, &self.workspace_id, &self.board_id, &self.card_id)
        })
    }

    fn board(&self) -> BoardKey {
        (self.wall, self.workspace_id.clone(), self.board_id.clone())
    }
}

impl std::fmt::Debug for Visit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Visit")
            .field("workspace_id", &self.workspace_id)
            .field("board_id", &self.board_id)
            .field("card_id", &self.card_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
pub(super) struct State {
    /// Changed only through [`Listeners::place`], which keeps the boards' index.
    visit: Option<Visit>,
    renewed: Option<Instant>,
    revision: u64,
    entry: u64,
    rate: Option<Rate>,
    /// An access change reached the visit since its roster was last projected: the
    /// next batch asks again whatever the interval of the periodic check.
    recheck: bool,
}

impl State {
    /// Whether the visit is renewed within its lease: a lapsed one is on no roster.
    pub(super) fn is_current(&self) -> bool {
        self.visit.is_some()
            && self
                .renewed
                .is_some_and(|renewed| renewed.elapsed() < LEASE)
    }

    pub(super) fn reads_shared_board(&self, board_id: &str) -> bool {
        self.visit
            .as_ref()
            .is_some_and(|visit| visit.board_id == board_id && !visit.read_token.is_empty())
    }
}

/// The visits of each board, and when they last changed: a reader reads its board's
/// roster again only when it did (or when its own access is asked again).
#[derive(Debug, Default)]
pub(super) struct Boards {
    by_board: HashMap<BoardKey, OnBoard>,
    /// How many changes the visits of every board had, numbering them.
    changes: u64,
}

#[derive(Debug, Default)]
struct OnBoard {
    visitors: BTreeSet<ListenerId>,
    /// The number of the board's latest change.
    changed: u64,
}

impl Boards {
    /// Forgets a connection that went away with `state`.
    pub(super) fn forget(&mut self, id: ListenerId, state: &State) {
        if let Some(visit) = &state.visit {
            self.leave(&visit.board(), id);
        }
    }

    /// The number of the latest change of `board`'s visits.
    fn changed(&self, board: &BoardKey) -> Option<u64> {
        self.by_board.get(board).map(|on| on.changed)
    }

    fn enter(&mut self, board: BoardKey, id: ListenerId) {
        self.changes += 1;
        let on = self.by_board.entry(board).or_default();
        on.visitors.insert(id);
        on.changed = self.changes;
    }

    /// Counts a change of `board`'s roster that changed no visit (a lapsed visit
    /// renewed: the rosters built meanwhile left it out).
    fn touch(&mut self, board: &BoardKey) {
        self.changes += 1;
        if let Some(on) = self.by_board.get_mut(board) {
            on.changed = self.changes;
        }
    }

    fn leave(&mut self, board: &BoardKey, id: ListenerId) {
        self.changes += 1;
        if let Some(on) = self.by_board.get_mut(board) {
            on.visitors.remove(&id);
            on.changed = self.changes;
            if on.visitors.is_empty() {
                self.by_board.remove(board);
            }
        }
    }
}

impl Listeners {
    /// Ends the visits made by a read token of `board_id`: the token stops opening the
    /// board (sharing ended, a new token, the board deleted).
    pub(super) fn end_shared_visits(&mut self, board_id: &str) {
        let ids: Vec<ListenerId> = self
            .by_id
            .iter()
            .filter(|(_, listener)| listener.presence.reads_shared_board(board_id))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.end_visit(id);
        }
    }

    /// Ends the visit of `id`, whatever command it came with.
    fn end_visit(&mut self, id: ListenerId) {
        let Some(listener) = self.by_id.get_mut(&id) else {
            return;
        };
        let state = &mut listener.presence;
        state.renewed = None;
        state.revision = state.revision.wrapping_add(1);
        self.place(id, None);
    }

    /// Sets the visit of `id` (`None`: it left the board), keeping the boards' index.
    fn place(&mut self, id: ListenerId, visit: Option<Visit>) {
        let Some(listener) = self.by_id.get_mut(&id) else {
            return;
        };
        let state = &mut listener.presence;
        if state.visit == visit {
            return;
        }
        if let Some(before) = state.visit.take() {
            self.visits.leave(&before.board(), id);
        }
        if let Some(visit) = &visit {
            self.visits.enter(visit.board(), id);
        }
        state.visit = visit;
    }
}

/// A token bucket, using the rooms' configured per-second rate and one-second burst.
#[derive(Debug)]
struct Rate {
    left: u64,
    counted: Instant,
}

impl Rate {
    fn admits(&mut self, per_second: u32) -> bool {
        let now = Instant::now();
        let millis =
            u64::try_from(now.duration_since(self.counted).as_millis()).unwrap_or(u64::MAX);
        self.counted += Duration::from_millis(millis);
        let rate = u64::from(per_second);
        self.left = self
            .left
            .saturating_add(millis.saturating_mul(rate))
            .min(rate * 1000);
        if self.left < 1000 {
            return false;
        }
        self.left -= 1000;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Person {
    user_id: String,
    card_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    action: &'static str,
    workspace_id: String,
    board_id: String,
    people: Vec<Person>,
    anonymous_count: usize,
    hidden_count: usize,
}

impl Outgoing for Snapshot {
    fn action(&self) -> &'static str {
        self.action
    }
}

impl Hub {
    pub(super) async fn set_presence(&self, app: &App, id: ListenerId, command: Command) {
        // Leaving is always allowed, as LEAVE_DOC is, even after a burst.
        if command.board_id.is_empty() {
            self.clear_presence(id, None);
            return;
        }
        let Some(identity) = self.rated_identity(app, id) else {
            return;
        };
        if [&command.workspace_id, &command.board_id, &command.card_id]
            .iter()
            .any(|id| id.len() > LONGEST_ID)
            || (!command.read_token.is_empty() && command.read_token.len() != SECRET_LENGTH)
        {
            return;
        }
        let asker = Asker {
            user_id: &identity.0,
            made_through: identity.1.as_deref(),
        };
        // Changes made while the lookup runs may not be in what it read: access changes
        // of the rooms, and links taken back (`end_readers_of_board`).
        let mut meanwhile = app.collab().access_changes();
        let since = self.listeners().revoked.now();
        let access = tokio::time::timeout(
            LOOKUP_WAIT,
            app.presence_access(
                asker,
                &command.workspace_id,
                &command.board_id,
                &command.card_id,
                &command.read_token,
            ),
        )
        .await;
        // Whatever is refused, the visit asked before is not kept in its place.
        let access = match access {
            Ok(Ok(Some(access))) => access,
            Ok(Ok(None)) | Err(_) => {
                self.clear_presence(id, None);
                return;
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "presence admission could not be read");
                self.clear_presence(id, None);
                return;
            }
        };
        let visit = Visit {
            workspace_id: access.workspace_id,
            board_id: command.board_id,
            card_id: access.card_id,
            read_token: command.read_token,
            wall: access.wall,
        };
        let limits = ReaderLimits::of(app);
        self.place_visit(id, visit, &mut meanwhile, since, limits);
    }

    /// The connection's user and the provider of its session, if the rate of its
    /// commands takes one more.
    fn rated_identity(&self, app: &App, id: ListenerId) -> Option<(String, Option<String>)> {
        let mut listeners = self.listeners();
        let listener = listeners.by_id.get_mut(&id)?;
        let per_second = app.config().collab.collab_frames_per_second;
        let rate = listener.presence.rate.get_or_insert_with(|| Rate {
            left: u64::from(per_second) * 1000,
            counted: Instant::now(),
        });
        rate.admits(per_second).then(|| {
            (
                listener.user_id.clone(),
                listener.login_provider().map(str::to_owned),
            )
        })
    }

    /// Places a visit read before now: not if the link it came by was taken back since
    /// `since`, or the board has as many readers by its link as `limits` take (the
    /// visit before ends then too).
    fn place_visit(
        &self,
        id: ListenerId,
        visit: Visit,
        meanwhile: &mut broadcast::Receiver<AccessChange>,
        since: u64,
        limits: ReaderLimits,
    ) {
        let mut listeners = self.listeners();
        // Drained under the lock: a change raised later is flagged by the watcher
        // (`watch_access`), which takes the same lock, against this visit.
        let read_too_early = changed_meanwhile(meanwhile, &visit);
        let by_link = !visit.read_token.is_empty();
        let user_id = listeners
            .by_id
            .get(&id)
            .map_or_else(String::new, |listener| listener.user_id.clone());
        if by_link
            && (listeners
                .revoked
                .of_board_since(&user_id, &visit.board_id, since)
                || !listeners.admits_reader(id, &visit.board_id, limits))
        {
            listeners.end_visit(id);
            return;
        }
        let Some(listener) = listeners.by_id.get_mut(&id) else {
            return;
        };
        let state = &mut listener.presence;
        if state.visit.as_ref().is_none_or(|before| {
            before.board_id != visit.board_id || before.read_token != visit.read_token
        }) {
            state.entry = state.entry.wrapping_add(1);
        }
        let lapsed = state
            .renewed
            .is_some_and(|renewed| renewed.elapsed() >= LEASE);
        state.renewed = Some(Instant::now());
        state.revision = state.revision.wrapping_add(1);
        state.recheck |= read_too_early;
        let board = visit.board();
        listeners.place(id, Some(visit));
        if lapsed {
            listeners.visits.touch(&board);
        }
    }

    /// Starts, once for the hub, the task that flags the visits access changes reach
    /// (`watch_access`). `app` is any application of the server: they share the rooms
    /// that raise the changes.
    pub(super) fn watch_access_changes(&self, app: &App) {
        self.inner.watching_access.call_once(|| {
            let changes = app.collab().access_changes();
            tokio::spawn(watch_access(Arc::downgrade(&self.inner), changes));
        });
    }

    /// Ends the visit of `id`, unless a command came since `revision` (if given).
    fn clear_presence(&self, id: ListenerId, revision: Option<u64>) {
        let mut listeners = self.listeners();
        let Some(listener) = listeners.by_id.get_mut(&id) else {
            return;
        };
        if revision.is_none_or(|revision| revision == listener.presence.revision) {
            listeners.end_visit(id);
        }
    }
}

/// Flags, for each access change, the visits whose roster it may have changed, until
/// nothing raises changes any more or the hub is gone.
async fn watch_access(hub: Weak<HubInner>, mut changes: broadcast::Receiver<AccessChange>) {
    loop {
        let change = match changes.recv().await {
            Ok(change) => Some(change),
            Err(RecvError::Lagged(_)) => None,
            Err(RecvError::Closed) => return,
        };
        let Some(hub) = hub.upgrade() else { return };
        let mut listeners = hub
            .listeners
            .lock()
            .expect("the listener table lock is never held across a panic");
        flag_reached(&mut listeners, change.as_ref());
    }
}

/// Flags the visits `change` reached (all of them if it is `None`: changes were
/// missed) on the boards where somebody it concerns is: a roster is the reader's own
/// access and whom the reader may see of those present, so a change of somebody's
/// facts elsewhere changes no roster here.
fn flag_reached(listeners: &mut Listeners, change: Option<&AccessChange>) {
    let boards: HashSet<BoardKey> = listeners
        .by_id
        .values()
        .filter_map(|listener| {
            let visit = listener.presence.visit.as_ref()?;
            let concerned = change.is_none_or(|change| change.concerns(&listener.user_id));
            (concerned && visit.reached_by(change)).then(|| visit.board())
        })
        .collect();
    for board in &boards {
        let Some(on) = listeners.visits.by_board.get(board) else {
            continue;
        };
        for id in &on.visitors {
            if let Some(listener) = listeners.by_id.get_mut(id)
                && listener
                    .presence
                    .visit
                    .as_ref()
                    .is_some_and(|visit| visit.reached_by(change))
            {
                listener.presence.recheck = true;
            }
        }
    }
}

/// Whether an access change told to `changes` may have reached `visit`.
fn changed_meanwhile(changes: &mut broadcast::Receiver<AccessChange>, visit: &Visit) -> bool {
    loop {
        match changes.try_recv() {
            Ok(change) if visit.reached_by(Some(&change)) => return true,
            Ok(_) => {}
            Err(TryRecvError::Lagged(_)) => return true,
            Err(TryRecvError::Empty | TryRecvError::Closed) => return false,
        }
    }
}

/// Everybody on `visit`'s board, before the reader's sight is applied.
fn snapshot(visit: &Visit, listeners: &Listeners) -> Snapshot {
    let mut people: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut anonymous_count = 0;
    let visitors = listeners
        .visits
        .by_board
        .get(&visit.board())
        .map(|on| &on.visitors);
    for id in visitors.into_iter().flatten() {
        let Some(listener) = listeners.by_id.get(id) else {
            continue;
        };
        let state = &listener.presence;
        let Some(other) = &state.visit else { continue };
        if state
            .renewed
            .is_none_or(|renewed| renewed.elapsed() >= LEASE)
        {
            continue;
        }
        if other.read_token.is_empty() {
            let cards = people.entry(listener.user_id.clone()).or_default();
            if !other.card_id.is_empty() {
                cards.insert(other.card_id.clone());
            }
        } else {
            anonymous_count += 1;
        }
    }
    Snapshot {
        action: UPDATE_PRESENCE,
        workspace_id: visit.workspace_id.clone(),
        board_id: visit.board_id.clone(),
        anonymous_count,
        hidden_count: 0,
        people: people
            .into_iter()
            .map(|(user_id, card_ids)| Person { user_id, card_ids })
            .collect(),
    }
}

#[cfg(test)]
mod tests;
