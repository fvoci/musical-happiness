//! One visit per connection, coalesced every 250 ms and projected for each reader.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;

use super::{Command, Hub, ListenerId, Outgoing};
use crate::app::{App, Asker};
use crate::auth::secret::SECRET_LENGTH;

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
        let visit = Visit {
            workspace_id: command.workspace_id,
            board_id: command.board_id,
            card_id: command.card_id,
            read_token: command.read_token,
        };
        let asker = Asker {
            user_id: &identity.0,
            made_through: identity.1.as_deref(),
        };
        let access = tokio::time::timeout(
            LOOKUP_WAIT,
            app.presence_access(
                asker,
                &visit.workspace_id,
                &visit.board_id,
                &visit.card_id,
                &visit.read_token,
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
        let mut listeners = self.listeners();
        let Some(listener) = listeners.by_id.get_mut(&id) else {
            return;
        };
        if listener.presence.visit.as_ref().is_none_or(|before| {
            before.board_id != visit.board_id || before.read_token != visit.read_token
        }) {
            listener.presence.entry = listener.presence.entry.wrapping_add(1);
        }
        listener.presence.visit = Some(Visit {
            workspace_id: access.workspace_id,
            ..visit
        });
        listener.presence.renewed = Some(Instant::now());
        listener.presence.revision = listener.presence.revision.wrapping_add(1);
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
            let current = self.listeners().by_id.get(&id).and_then(|listener| {
                let state = &listener.presence;
                state.visit.clone().map(|visit| {
                    (
                        visit,
                        state.revision,
                        state.entry,
                        state.renewed,
                        listener.user_id.clone(),
                        listener.login_provider.clone(),
                    )
                })
            });
            let Some((visit, revision, entry, renewed, user_id, provider)) = current else {
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
            if raw_before.as_ref() == Some(&raw) && Instant::now() < next_check {
                continue;
            }
            let result = tokio::time::timeout(
                LOOKUP_WAIT,
                self.project_presence(app, &visit, &user_id, provider.as_deref(), raw.clone()),
            )
            .await;
            // A later command supersedes a permission lookup that was in flight.
            if self
                .listeners()
                .by_id
                .get(&id)
                .is_none_or(|listener| listener.presence.revision != revision)
            {
                continue;
            }
            match result {
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

fn snapshot(visit: &Visit, listeners: &super::Listeners) -> Snapshot {
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
