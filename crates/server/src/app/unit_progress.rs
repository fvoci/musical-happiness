//! Personal unit progress and course progress summaries.
//!
//! Personal progress writes intentionally emit no websocket broadcasts, no search index
//! updates, no webhooks, and no audit log entries so a learner's private per-unit state
//! never leaves the caller's own session.

use fvoci_model::{CompletionChange, CourseProgressSummary, UnitProgress, UnitProgressChange};

use super::App;
use crate::error::AppError;
use crate::metrics::UnitCompletionAction;

impl App {
    /// Returns the caller's personal progress rows for live units of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if progress cannot be read.
    pub async fn my_course_progress(
        &self,
        course_id: &str,
        user_id: &str,
    ) -> Result<Vec<UnitProgress>, AppError> {
        Ok(self
            .store()
            .get_my_course_progress(course_id, user_id)
            .await?)
    }

    /// Creates or updates the caller's personal progress on a live unit of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if `unit_id` is not a live unit of `course_id`, and the store's error
    /// if the write fails.
    pub async fn put_unit_progress(
        &self,
        course_id: &str,
        unit_id: &str,
        user_id: &str,
        change: &UnitProgressChange,
    ) -> Result<UnitProgress, AppError> {
        let progress = self
            .store()
            .upsert_unit_progress(course_id, unit_id, user_id, change)
            .await?;
        match change.completion {
            CompletionChange::Complete => {
                self.metrics()
                    .course_unit_completion(UnitCompletionAction::Completed);
            }
            CompletionChange::Incomplete => {
                self.metrics()
                    .course_unit_completion(UnitCompletionAction::Uncompleted);
            }
            CompletionChange::Keep => {}
        }
        Ok(progress)
    }

    /// Returns the aggregate completion summary across active members of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the summary cannot be computed.
    pub async fn course_progress_summary(
        &self,
        course_id: &str,
    ) -> Result<CourseProgressSummary, AppError> {
        Ok(self.store().get_course_progress_summary(course_id).await?)
    }
}
