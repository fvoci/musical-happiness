//! One visit per connection, coalesced every 250 ms and projected for each reader.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Weak};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};
use tokio::time::Instant;

use super::{Command, Hub, HubInner, ListenerId, Listeners, Outgoing};
use crate::app::{App, Asker};
use crate::auth::secret::SECRET_LENGTH;
use crate::collab::{AccessChange, Wall};

const BATCH: Duration = Duration::from_millis(250);
/// A page renews its visit every 30 seconds; a stopped page disappears too.
const LEASE: Duration = Duration::from_secs(90);
/// A failed permission lookup must not leave a connection present indefinitely.
const LOOKUP_WAIT: Duration = Duration::from_secs(5);
const LONGEST_ID: usize = 64;

#[derive(Clone, PartialEq, Eq)]
struct Visit {
    workspace_id: String,
    board_id: String,
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

    /// The board, as the visits of one roster share it.
    fn board(&self) -> (Wall, String, String) {
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
    pub(super) fn reads_shared_board(&self, board_id: &str) -> bool {
        self.visit
            .as_ref()
            .is_some_and(|visit| visit.board_id == board_id && !visit.read_token.is_empty())
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
        let identity = {
            let mut listeners = self.listeners();
            let Some(listener) = listeners.by_id.get_mut(&id) else {
                return;
            };
            let per_second = app.config().collab.collab_frames_per_second;
            let rate = listener.presence.rate.get_or_insert_with(|| Rate {
                left: u64::from(per_second) * 1000,
                counted: Instant::now(),
            });
            if !rate.admits(per_second) {
                return;
            }
            (listener.user_id.clone(), listener.login_provider.clone())
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
        // Changes made while the lookup runs may not be in what it read.
        let mut meanwhile = app.collab().access_changes();
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
        let access = match access {
            Ok(Ok(Some(access))) => access,
            Ok(Ok(None)) => return,
            Ok(Err(error)) => {
                tracing::warn!(%error, "presence admission could not be read");
                self.clear_presence(id, None);
                return;
            }
            Err(_) => {
                self.clear_presence(id, None);
                return;
            }
        };
        let visit = Visit {
            workspace_id: access.workspace_id,
            board_id: command.board_id,
            card_id: command.card_id,
            read_token: command.read_token,
            wall: access.wall,
        };
        let mut listeners = self.listeners();
        // Drained under the lock: a change raised later is flagged by the watcher
        // (`watch_access`), which takes the same lock, against this visit.
        let read_too_early = changed_meanwhile(&mut meanwhile, &visit);
        let Some(listener) = listeners.by_id.get_mut(&id) else {
            return;
        };
        let state = &mut listener.presence;
        if state.visit.as_ref().is_none_or(|before| {
            before.board_id != visit.board_id || before.read_token != visit.read_token
        }) {
            state.entry = state.entry.wrapping_add(1);
        }
        state.visit = Some(visit);
        state.renewed = Some(Instant::now());
        state.revision = state.revision.wrapping_add(1);
        state.recheck |= read_too_early;
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

    fn clear_presence(&self, id: ListenerId, revision: Option<u64>) {
        if let Some(listener) = self.listeners().by_id.get_mut(&id)
            && revision.is_none_or(|revision| revision == listener.presence.revision)
        {
            listener.presence.visit = None;
            listener.presence.renewed = None;
            listener.presence.revision = listener.presence.revision.wrapping_add(1);
        }
    }

    pub(super) async fn presence_loop(&self, app: &App, id: ListenerId) {
        let mut ticks = tokio::time::interval(BATCH);
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut entry_before = None;
        let mut raw_before = None;
        let mut sent_before = None;
        let mut next_check = Instant::now();
        loop {
            ticks.tick().await;
            let current = self.listeners().by_id.get_mut(&id).and_then(|listener| {
                let state = &mut listener.presence;
                let recheck = std::mem::take(&mut state.recheck);
                state.visit.clone().map(|visit| Batch {
                    visit,
                    revision: state.revision,
                    recheck,
                    entry: state.entry,
                    renewed: state.renewed,
                    user_id: listener.user_id.clone(),
                    provider: listener.login_provider.clone(),
                })
            });
            let Some(Batch {
                visit,
                revision,
                recheck,
                entry,
                renewed,
                user_id,
                provider,
            }) = current
            else {
                raw_before = None;
                sent_before = None;
                continue;
            };
            if renewed.is_none_or(|renewed| renewed.elapsed() >= LEASE) {
                self.clear_presence(id, Some(revision));
                continue;
            }
            if entry_before != Some(entry) {
                entry_before = Some(entry);
                raw_before = None;
                sent_before = None;
            }
            let raw = snapshot(&visit, &self.listeners());
            if raw_before.as_ref() == Some(&raw) && !recheck && Instant::now() < next_check {
                continue;
            }
            let result = tokio::time::timeout(
                LOOKUP_WAIT,
                self.project_presence(app, &visit, &user_id, provider.as_deref(), raw.clone()),
            )
            .await;
            // A later command supersedes a permission lookup that was in flight; a
            // recheck it was for is still owed to the roster's other people.
            let changed_meanwhile = {
                let mut listeners = self.listeners();
                let Some(listener) = listeners.by_id.get_mut(&id) else {
                    continue;
                };
                if listener.presence.revision != revision {
                    listener.presence.recheck |= recheck;
                    continue;
                }
                listener.presence.recheck
            };
            match result {
                // An access change during the lookup may not be in what it read: the
                // roster waits for the next batch, which asks again (still flagged).
                Ok(Ok(Some(_))) if changed_meanwhile => {}
                Ok(Ok(Some(snapshot))) => {
                    if snapshot != sent_before {
                        if let Some(snapshot) = &snapshot {
                            self.send(&[id], snapshot);
                        }
                        sent_before = snapshot;
                    }
                    raw_before = Some(raw);
                    next_check = Instant::now()
                        + Duration::from_secs(app.config().collab.collab_recheck_seconds);
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "presence authority could not be read");
                    self.clear_presence(id, Some(revision));
                }
                Ok(Ok(None)) | Err(_) => self.clear_presence(id, Some(revision)),
            }
        }
    }

    /// Outer None: no board access. Inner None: a token reader, never a recipient.
    async fn project_presence(
        &self,
        app: &App,
        visit: &Visit,
        user_id: &str,
        provider: Option<&str>,
        mut snapshot: Snapshot,
    ) -> Result<Option<Option<Snapshot>>, crate::error::AppError> {
        let asker = Asker {
            user_id,
            made_through: provider,
        };
        let Some(access) = app
            .presence_access(
                asker,
                &visit.workspace_id,
                &visit.board_id,
                &visit.card_id,
                &visit.read_token,
            )
            .await?
        else {
            return Ok(None);
        };
        let Some(role) = access.role else {
            return Ok(Some(None));
        };
        let ids: Vec<String> = snapshot
            .people
            .iter()
            .map(|person| person.user_id.clone())
            .collect();
        let visible = access
            .app
            .users_in_workspace(user_id, &visit.workspace_id, role, &ids)
            .await?;
        snapshot.people.retain(|person| {
            let seen = visible.iter().any(|user| user.id == person.user_id);
            if !seen {
                snapshot.hidden_count += 1;
            }
            seen
        });
        Ok(Some(Some(snapshot)))
    }
}

/// What a batch of a connection's loop reads of its visit, under the lock.
struct Batch {
    visit: Visit,
    revision: u64,
    /// An access change reached the visit since the last batch took this.
    recheck: bool,
    entry: u64,
    renewed: Option<Instant>,
    user_id: String,
    provider: Option<String>,
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
    let boards: HashSet<(Wall, String, String)> = listeners
        .by_id
        .values()
        .filter_map(|listener| {
            let visit = listener.presence.visit.as_ref()?;
            let concerned = change.is_none_or(|change| change.concerns(&listener.user_id));
            (concerned && visit.reached_by(change)).then(|| visit.board())
        })
        .collect();
    if boards.is_empty() {
        return;
    }
    for listener in listeners.by_id.values_mut() {
        let state = &mut listener.presence;
        if let Some(visit) = &state.visit
            && visit.reached_by(change)
            && boards.contains(&visit.board())
        {
            state.recheck = true;
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

fn snapshot(visit: &Visit, listeners: &Listeners) -> Snapshot {
    let mut people: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut anonymous_count = 0;
    for listener in listeners.by_id.values() {
        let state = &listener.presence;
        let Some(other) = &state.visit else { continue };
        if other.board_id != visit.board_id
            || other.workspace_id != visit.workspace_id
            || state
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
        action: "UPDATE_PRESENCE",
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
