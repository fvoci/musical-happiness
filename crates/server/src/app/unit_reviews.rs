//! Personal spaced-repetition reviews, personal unit notes, and due-review notifications.
//!
//! Personal review and note writes intentionally emit no websocket broadcasts, no search
//! index updates, no webhooks, and no audit log entries so a learner's private study
//! state never leaks beyond their own session. Due-review notifications are delivered
//! solely to the owning learner's own notification inbox.

use std::collections::HashMap;

use fvoci_model::{
    CompletedUnitReview, ContainerKinds, NewUnitNote, NewUnitReview, ReviewStatusFilter, TimeZone,
    TodayReviewItem, UnitNote, UnitNoteChange, UnitNoteKind, UnitReview, add_days, day_at,
};

use super::App;
use crate::auth::now_millis;
use crate::error::AppError;

/// Maximum candidate reviews read per round in [`App::notify_due_reviews`].
const REVIEW_NOTIFY_ROUND: i64 = 200;

/// Maximum rounds per [`App::notify_due_reviews`] invocation.
const REVIEW_NOTIFY_MAX_ROUNDS: usize = 10;

/// Maximum notifications kept per person (matches `notifications.rs`).
const MOST_NOTIFICATIONS_PER_USER: i64 = 10_000;

impl App {
    /// Returns the calendar day (`YYYY-MM-DD`) at `now` in `user_id`'s configured time
    /// zone, falling back to UTC when the user leaves time zone on `auto`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if settings cannot be read, or an internal error if
    /// `now` falls outside calendar years 1 to 9999.
    pub async fn user_today(&self, user_id: &str, now: i64) -> Result<String, AppError> {
        let settings = self.store().get_user_settings(user_id).await?;
        day_at(now, &settings.time_zone)
            .or_else(|| day_at(now, &TimeZone::default()))
            .ok_or_else(|| AppError::internal("current timestamp is outside supported years"))
    }

    /// Lists the caller's personal reviews on live units of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if reviews cannot be read.
    pub async fn list_my_course_reviews(
        &self,
        course_id: &str,
        user_id: &str,
        status: ReviewStatusFilter,
        unit_id: Option<&str>,
    ) -> Result<Vec<UnitReview>, AppError> {
        Ok(self
            .store()
            .list_my_course_reviews(course_id, user_id, status, unit_id)
            .await?)
    }

    /// Schedules a personal review on a live unit of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `unit_id` is not a live unit of `course_id`, and the store's error
    /// if the write fails.
    pub async fn schedule_unit_review(
        &self,
        course_id: &str,
        unit_id: &str,
        user_id: &str,
        new: &NewUnitReview,
    ) -> Result<UnitReview, AppError> {
        Ok(self
            .store()
            .schedule_unit_review(course_id, unit_id, user_id, new)
            .await?)
    }

    /// Completes the caller's review `review_id` in `course_id`, computing the suggested
    /// next interval and optionally scheduling a follow-up review atomically.
    ///
    /// # Errors
    ///
    /// Returns 404 if `review_id` does not belong to `user_id` in `course_id` or its
    /// unit is no longer live, and the store's error if the write fails.
    pub async fn complete_unit_review(
        &self,
        course_id: &str,
        review_id: &str,
        user_id: &str,
        intervals: &[u32],
        next: Option<&NewUnitReview>,
    ) -> Result<CompletedUnitReview, AppError> {
        Ok(self
            .store()
            .complete_unit_review(course_id, review_id, user_id, intervals, next)
            .await?)
    }

    /// Deletes the caller's review `review_id` in `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `review_id` does not belong to `user_id` in `course_id`, and the
    /// store's error if the write fails.
    pub async fn delete_unit_review(
        &self,
        course_id: &str,
        review_id: &str,
        user_id: &str,
    ) -> Result<(), AppError> {
        Ok(self
            .store()
            .delete_unit_review(course_id, review_id, user_id)
            .await?)
    }

    /// Lists the caller's pending reviews due today or overdue in `workspace_id`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if reviews cannot be read.
    pub async fn workspace_today_reviews(
        &self,
        workspace_id: &str,
        user_id: &str,
        today: &str,
    ) -> Result<Vec<TodayReviewItem>, AppError> {
        Ok(self
            .store()
            .list_workspace_today_reviews(workspace_id, user_id, today)
            .await?)
    }

    /// Lists the caller's personal notes on live unit `unit_id` of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `unit_id` is not a live unit of `course_id`, and the store's error
    /// if notes cannot be read.
    pub async fn list_my_unit_notes(
        &self,
        course_id: &str,
        unit_id: &str,
        user_id: &str,
        kind: Option<UnitNoteKind>,
    ) -> Result<Vec<UnitNote>, AppError> {
        Ok(self
            .store()
            .list_my_unit_notes(course_id, unit_id, user_id, kind)
            .await?)
    }

    /// Creates a personal note on live unit `unit_id` of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `unit_id` is not a live unit of `course_id`, and the store's error
    /// if the write fails.
    pub async fn create_unit_note(
        &self,
        course_id: &str,
        unit_id: &str,
        user_id: &str,
        new: &NewUnitNote,
    ) -> Result<UnitNote, AppError> {
        Ok(self
            .store()
            .create_unit_note(course_id, unit_id, user_id, new)
            .await?)
    }

    /// Updates the caller's personal note `note_id` in `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `note_id` does not belong to `user_id` in `course_id` or its unit
    /// is no longer live, and the store's error if the write fails.
    pub async fn patch_unit_note(
        &self,
        course_id: &str,
        note_id: &str,
        user_id: &str,
        change: &UnitNoteChange,
    ) -> Result<UnitNote, AppError> {
        Ok(self
            .store()
            .patch_unit_note(course_id, note_id, user_id, change)
            .await?)
    }

    /// Deletes the caller's personal note `note_id` in `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `note_id` does not belong to `user_id` in `course_id`, and the
    /// store's error if the write fails.
    pub async fn delete_unit_note(
        &self,
        course_id: &str,
        note_id: &str,
        user_id: &str,
    ) -> Result<(), AppError> {
        Ok(self
            .store()
            .delete_unit_note(course_id, note_id, user_id)
            .await?)
    }

    /// Creates `review_due` notifications for pending, un-notified reviews whose
    /// `scheduled_date` has reached each learner's local calendar day at `now`, marking
    /// `notified_at = now` atomically so each review notifies its owner at most once.
    ///
    /// Returns the number of notifications created. Runs under housekeeping's system
    /// scope and requires the server lock.
    ///
    /// # Errors
    ///
    /// Returns an error if the server lock is not held or a database query fails.
    pub async fn notify_due_reviews(&self, now: i64) -> Result<u64, AppError> {
        let app = self.housekeeping();
        app.require_server_lock()?;
        let utc_day = day_at(now, &TimeZone::default())
            .ok_or_else(|| AppError::internal("current timestamp is outside supported years"))?;
        let max_date = add_days(&utc_day, 1).unwrap_or(utc_day);

        let mut user_days: HashMap<String, String> = HashMap::new();
        let mut cursor: Option<(String, i64, String)> = None;
        let mut sent = 0_u64;

        for _ in 0..REVIEW_NOTIFY_MAX_ROUNDS {
            let after = cursor
                .as_ref()
                .map(|(date, create_at, id)| (date.as_str(), *create_at, id.as_str()));
            let batch = app
                .store()
                .due_unnotified_reviews(&max_date, after, REVIEW_NOTIFY_ROUND)
                .await?;
            if batch.is_empty() {
                break;
            }
            let reached_limit =
                i64::try_from(batch.len()).unwrap_or(i64::MAX) >= REVIEW_NOTIFY_ROUND;
            for candidate in &batch {
                let user_today = if let Some(day) = user_days.get(&candidate.user_id) {
                    day.as_str()
                } else {
                    let computed = app.user_today(&candidate.user_id, now).await?;
                    user_days
                        .entry(candidate.user_id.clone())
                        .or_insert(computed)
                        .as_str()
                };
                if candidate.scheduled_date.as_str() > user_today {
                    continue;
                }
                let enrolled = app
                    .in_organization(&candidate.org_id)?
                    .in_workspace(&candidate.workspace_id)
                    .reaching(ContainerKinds::COURSES)
                    .container_access(&candidate.user_id, &candidate.container_id)
                    .await?
                    .is_some_and(|access| {
                        access.has_membership()
                            && access.grant.reads()
                            && !access.container.is_template
                    });
                if !enrolled {
                    continue;
                }
                if app
                    .store()
                    .notify_due_review(candidate, now, MOST_NOTIFICATIONS_PER_USER)
                    .await?
                {
                    sent += 1;
                    app.ws()
                        .notifications_changed(&candidate.user_id, &[&candidate.org_id])
                        .await;
                }
            }
            if !reached_limit {
                break;
            }
            if let Some(last) = batch.last() {
                cursor = Some((last.scheduled_date.clone(), last.create_at, last.id.clone()));
            }
        }
        Ok(sent)
    }

    /// Housekeeping entry point for [`App::notify_due_reviews`]: logs failures and leaves
    /// remaining work for the next run.
    pub async fn notify_due_reviews_upkeep(&self) {
        if self.config().notifications_by_hand || !self.holds_server_lock() {
            return;
        }
        match self.notify_due_reviews(now_millis()).await {
            Ok(0) => {}
            Ok(sent) => tracing::info!(sent, "sent due unit review notifications"),
            Err(error) => tracing::error!(%error, "unable to send due unit review notifications"),
        }
    }
}
