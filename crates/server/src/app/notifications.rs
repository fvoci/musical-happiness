//! What a person is told inside the application: their list, as they read it now, and
//! the working off of whom a change may be told to.
//!
//! A notification is made, and listed, only for somebody who reads what it is about
//! at that moment ([`App::places_read_by`]): once when the list of the store is worked
//! off ([`tend`]), and again each time its reader lists it ([`App::notifications`]).
//! What they do not read then, because it is gone, in the trash, another's now or
//! behind a sign-in their session does not open, is listed as something they cannot
//! see: no title, no place, no name.

use std::collections::{HashMap, HashSet};

use fvoci_model::{Notification, NotificationSubject, Session};
use fvoci_store::{NotificationEvent, NotificationLimits, NotificationPlace, StoredNotification};

use super::sso::Asker;
use super::{App, ListPage};
use crate::auth::now_millis;
use crate::error::{AppError, Reason};
use crate::scheduler::RecurringTask;

/// The bounds a notification is made within: an hour in which the same (a mention
/// taken out and put back, a card given again) tells nobody twice, and ten thousand
/// to a person.
const LIMITS: NotificationLimits = NotificationLimits {
    same_within_millis: 60 * 60 * 1000,
    most_of_a_person: 10_000,
};
/// How long a notification is kept after it was read: ninety days.
const READ_KEPT_MILLIS: i64 = 90 * 24 * 60 * 60 * 1000;
/// How often the list of whom to tell is worked off.
const UPKEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
/// How many rows of the list one round reads, and how many rounds one run works off
/// at most: the rest is the next run's, a second later.
const ROUND: i64 = 200;
/// See [`ROUND`].
const ROUNDS_A_RUN: usize = 5;
/// How long a row that could not be worked off waits for its next try, at first and
/// at most; doubled with every failure in a row.
const FIRST_RETRY_MILLIS: i64 = 5_000;
/// See [`FIRST_RETRY_MILLIS`].
const LONGEST_RETRY_MILLIS: i64 = 10 * 60 * 1000;

/// What a notification, or a row of the list, names.
#[derive(Debug, Clone, Copy)]
struct Named<'a> {
    /// The card or the page it is about.
    block_id: &'a str,
    /// Where it happened: that block, or a comment on it.
    source_id: &'a str,
}

/// On whose word a person is let to what is an organization's.
#[derive(Debug, Clone, Copy)]
enum Entry<'a> {
    /// A session's: its user enters each workspace as any request of it does, and one
    /// its sign-in does not open is closed to it.
    Session(Asker<'a>),
    /// Nobody's: what the user is there, whatever session they may come with later.
    Standing,
}

impl App {
    /// Where each of `named` is now, for a user who reads it now; `None` for what the
    /// user does not read. For a handle behind the wall of the organization the rows
    /// are of.
    ///
    /// What is read is a card that is no template of its board, or a page, that is
    /// not in the trash, on a board or in a wiki that is no template, in a workspace
    /// the user enters (`entry`), which the user reads there ([`App::container_access`],
    /// asked once a container); and where it happened in a comment, the comment is
    /// there, on that card or page.
    async fn places_read_by(
        &self,
        user_id: &str,
        entry: Entry<'_>,
        named: &[Named<'_>],
    ) -> Result<Vec<Option<NotificationPlace>>, AppError> {
        let block_ids: Vec<&str> = named.iter().map(|named| named.block_id).collect();
        let places: HashMap<String, NotificationPlace> = self
            .store()
            .notification_places(&block_ids)
            .await?
            .into_iter()
            .map(|place| (place.block_id.clone(), place))
            .collect();
        let comment_ids: Vec<&str> = named
            .iter()
            .filter(|named| named.source_id != named.block_id)
            .map(|named| named.source_id)
            .collect();
        let comments: HashMap<String, String> = self
            .store()
            .comments_with_parents(&comment_ids)
            .await?
            .into_iter()
            .collect();
        let mut entered: HashMap<&str, Option<Self>> = HashMap::new();
        let mut reads: HashMap<&str, bool> = HashMap::new();
        let mut read = Vec::with_capacity(named.len());
        for named in named {
            let place = places.get(named.block_id).filter(|_| {
                named.source_id == named.block_id
                    || comments.get(named.source_id).map(String::as_str) == Some(named.block_id)
            });
            let Some(place) = place else {
                read.push(None);
                continue;
            };
            let workspace_id = place.workspace_id.as_str();
            if !entered.contains_key(workspace_id) {
                let app = self.entered_workspace(entry, workspace_id).await?;
                entered.insert(workspace_id, app);
            }
            let Some(app) = entered.get(workspace_id).and_then(Option::as_ref) else {
                read.push(None);
                continue;
            };
            let container_id = place.container_id.as_str();
            let reads_it = if let Some(reads_it) = reads.get(container_id) {
                *reads_it
            } else {
                let reads_it = app
                    .container_access(user_id, container_id)
                    .await?
                    .is_some_and(|access| {
                        access.grant.reads()
                            && !access.container.is_template
                            && access.container.workspace_id == workspace_id
                    });
                reads.insert(container_id, reads_it);
                reads_it
            };
            read.push(reads_it.then(|| place.clone()));
        }
        Ok(read)
    }

    /// This handle for what is in the workspace, as `entry` lets its user in; `None`
    /// for a workspace a session does not enter: one its user is no member of, and
    /// one its sign-in does not open.
    async fn entered_workspace(
        &self,
        entry: Entry<'_>,
        workspace_id: &str,
    ) -> Result<Option<Self>, AppError> {
        let Entry::Session(asker) = entry else {
            return Ok(Some(self.clone()));
        };
        match self.enter_workspace_as(asker, workspace_id).await {
            Ok(entered) => Ok(entered.map(|entered| entered.app)),
            Err(error)
                if matches!(
                    error.reason(),
                    Some(Reason::SsoRequired | Reason::PasswordSignInRequired)
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// A page of what the session's user was told, in every organization, the newest
    /// first. Each is asked again whether the user reads what it is about, as the
    /// session opens it: one they do not read now has no subject.
    ///
    /// # Errors
    ///
    /// Returns the store's error if a fact cannot be read: that is no "cannot see".
    pub async fn notifications(
        &self,
        session: &Session,
        page: ListPage,
    ) -> Result<Vec<Notification>, AppError> {
        let user_id = session.user_id.as_str();
        let stored = self
            .store()
            .notifications_of_user(user_id, page.paging())
            .await?;
        let mut places: HashMap<&str, NotificationPlace> = HashMap::new();
        let org_ids: HashSet<&str> = stored.iter().map(|told| told.org_id.as_str()).collect();
        for org_id in org_ids {
            let of_org: Vec<&StoredNotification> =
                stored.iter().filter(|told| told.org_id == org_id).collect();
            let named: Vec<Named<'_>> = of_org
                .iter()
                .map(|told| Named {
                    block_id: &told.block_id,
                    source_id: &told.source_id,
                })
                .collect();
            let read = self
                .in_organization(org_id)?
                .places_read_by(user_id, Entry::Session(session.into()), &named)
                .await?;
            for (told, place) in of_org.into_iter().zip(read) {
                if let Some(place) = place {
                    places.insert(told.id.as_str(), place);
                }
            }
        }
        Ok(stored
            .iter()
            .map(|told| Notification {
                id: told.id.clone(),
                create_at: told.create_at,
                read_at: told.read_at,
                subject: places
                    .remove(told.id.as_str())
                    .map(|place| subject(told, place)),
            })
            .collect())
    }

    /// How many notifications the user has not read, in every organization: those
    /// they cannot see any more among them, until they are read.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the query fails.
    pub async fn unread_notifications(&self, user_id: &str) -> Result<i64, AppError> {
        Ok(self.store().unread_notifications_of_user(user_id).await?)
    }

    /// Marks a notification of the user as read.
    ///
    /// # Errors
    ///
    /// Returns a not-found error for a notification that is not the user's, as for
    /// one that is not there.
    pub async fn read_notification(
        &self,
        user_id: &str,
        notification_id: &str,
    ) -> Result<(), AppError> {
        let theirs = self
            .store()
            .read_notification(user_id, notification_id, now_millis())
            .await?;
        if theirs {
            Ok(())
        } else {
            Err(AppError::not_found("notification"))
        }
    }

    /// Marks as read every notification of the user made up to `made_by`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the statement fails.
    pub async fn read_notifications(&self, user_id: &str, made_by: i64) -> Result<(), AppError> {
        self.store()
            .read_notifications_of_user(user_id, made_by, now_millis())
            .await?;
        Ok(())
    }

    /// Forgets the notifications of every organization that were read more than
    /// ninety days ago. It takes the housekeeping's reach itself; what fails is
    /// logged, and left for the next run.
    pub async fn forget_old_notifications(&self) {
        let app = self.housekeeping();
        if let Err(error) = app.require_server_lock() {
            tracing::warn!(%error, "the notifications read long ago are not forgotten");
            return;
        }
        let read_before = now_millis().saturating_sub(READ_KEPT_MILLIS);
        match app.store().forget_read_notifications(read_before).await {
            Ok(0) => {}
            Ok(forgotten) => tracing::info!(forgotten, "forgot notifications read long ago"),
            Err(error) => tracing::error!(%error, "unable to forget notifications read long ago"),
        }
    }
}

/// What a notification is about, for a reader who reads `place` now.
fn subject(told: &StoredNotification, place: NotificationPlace) -> NotificationSubject {
    let comment_id = if told.source_id == told.block_id {
        String::new()
    } else {
        told.source_id.clone()
    };
    NotificationSubject {
        kind: told.kind,
        actor_id: told.actor_id.clone(),
        workspace_id: place.workspace_id,
        container_id: place.container_id,
        container_kind: place.container_kind,
        block_id: place.block_id,
        block_kind: place.block_kind,
        title: place.title,
        comment_id,
    }
}

/// When a row that failed for the `attempts`-th time in a row is tried again.
fn retry_at(now: i64, attempts: i64) -> i64 {
    let doublings = u32::try_from(attempts.clamp(0, 16)).unwrap_or(16);
    let wait = FIRST_RETRY_MILLIS
        .saturating_mul(2_i64.saturating_pow(doublings))
        .min(LONGEST_RETRY_MILLIS);
    now.saturating_add(wait)
}

/// Works off one row of the list: the person is told if somebody else did it and
/// they read what it is about now, and the row is dropped otherwise. `app` reaches
/// every organization.
async fn settle(app: &App, event: &NotificationEvent) -> Result<(), AppError> {
    let named = Named {
        block_id: &event.block_id,
        source_id: &event.source_id,
    };
    let told = event.actor_id != event.recipient_id
        && app
            .in_organization(&event.org_id)?
            .places_read_by(&event.recipient_id, Entry::Standing, &[named])
            .await?
            .first()
            .is_some_and(Option::is_some);
    if told {
        app.store().tell_of(event, now_millis(), LIMITS).await?;
    } else {
        app.store().drop_notification_event(event).await?;
    }
    Ok(())
}

/// Works off the rows that are due, a few rounds of them. A row that fails is tried
/// again later and later, and the others are worked off all the same.
async fn work_off(app: &App) -> Result<(), AppError> {
    for _ in 0..ROUNDS_A_RUN {
        let now = now_millis();
        let events = app.store().due_notification_events(now, ROUND).await?;
        if events.is_empty() {
            break;
        }
        for event in &events {
            if let Err(error) = settle(app, event).await {
                tracing::warn!(
                    %error,
                    attempts = event.attempts.saturating_add(1),
                    "a person could not be told of a change; it is tried again later"
                );
                app.store()
                    .notification_event_failed(event, retry_at(now, event.attempts))
                    .await?;
            }
        }
    }
    Ok(())
}

/// Works off whom the changes so far may be told to, once, now: what the server's
/// task does every interval, for a server that holds the lock. One run at a time: a
/// second one waits for the first. The server does it by itself while it serves;
/// this is for its tests.
pub async fn tend(app: &App) {
    let _turn = app.inner.telling.lock().await;
    let app = app.housekeeping();
    if !app.holds_server_lock() {
        return;
    }
    if let Err(error) = work_off(&app).await {
        tracing::error!(%error, "the list of whom to tell could not be worked off");
    }
}

/// Starts the task that works off whom to tell ([`tend`]).
pub(crate) fn start_upkeep(app: &App) -> RecurringTask {
    let app = app.clone();
    RecurringTask::spawn_starting_now("tellPeople", UPKEEP_INTERVAL, move || {
        let app = app.clone();
        async move { tend(&app).await }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_that_failed_waits_twice_as_long_each_time_up_to_ten_minutes() {
        assert_eq!(retry_at(1000, 0), 6000);
        assert_eq!(retry_at(1000, 1), 11_000);
        assert_eq!(retry_at(0, 20), LONGEST_RETRY_MILLIS);
        assert_eq!(retry_at(0, -1), FIRST_RETRY_MILLIS);
    }
}
