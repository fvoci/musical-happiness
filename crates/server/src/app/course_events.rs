//! Course study meetings, shared attendance, exams, strictly personal exam scores,
//! and upcoming-meeting notifications.
//!
//! Personal exam score writes intentionally emit no websocket broadcasts, no search index
//! updates, no webhooks, no notifications, and no audit log entries so a learner's private
//! score and note never leave the caller's own session.

use std::collections::BTreeSet;

use fvoci_model::{
    AttendanceStatus, CourseExam, CourseExamChange, CourseMeeting, CourseMeetingChange,
    MeetingAttendance, MyExamScore, RefusalReason, Refusals, ValidatedMyExamScore,
    ValidatedNewExam, ValidatedNewMeeting,
};
use fvoci_store::ExamScoreOutcome;

use super::App;
use crate::error::AppError;

/// Lead time before a meeting's start during which an upcoming reminder is sent (24 hours).
pub const MEETING_REMINDER_WINDOW_MILLIS: i64 = 24 * 60 * 60 * 1000;

impl App {
    /// Lists all study meetings of `course_id`, ordered chronologically, each with its
    /// attendance records.
    ///
    /// # Errors
    ///
    /// Returns the store's error if meetings cannot be read.
    pub async fn list_course_meetings(
        &self,
        course_id: &str,
    ) -> Result<Vec<CourseMeeting>, AppError> {
        Ok(self.store().list_course_meetings(course_id).await?)
    }

    /// Creates a new study meeting on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course does not exist or an assigned presenter/host is not an
    /// active enrolled course member, and the store's error on failure.
    pub async fn create_course_meeting(
        &self,
        course_id: &str,
        new: &ValidatedNewMeeting,
    ) -> Result<CourseMeeting, AppError> {
        Ok(self.store().create_course_meeting(course_id, new).await?)
    }

    /// Updates a study meeting on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or meeting does not exist or an assigned presenter/host
    /// is not an active enrolled course member, and the store's error on failure.
    pub async fn patch_course_meeting(
        &self,
        course_id: &str,
        meeting_id: &str,
        change: &CourseMeetingChange,
    ) -> Result<CourseMeeting, AppError> {
        Ok(self
            .store()
            .patch_course_meeting(course_id, meeting_id, change)
            .await?)
    }

    /// Permanently deletes a study meeting on `course_id` along with its attendance rows.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or meeting does not exist, and the store's error on
    /// failure.
    pub async fn delete_course_meeting(
        &self,
        course_id: &str,
        meeting_id: &str,
    ) -> Result<(), AppError> {
        Ok(self
            .store()
            .delete_course_meeting(course_id, meeting_id)
            .await?)
    }

    /// Records or updates `target_user_id`'s attendance on `meeting_id` of `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or meeting does not exist or `target_user_id` is not an
    /// active enrolled member of the course, and the store's error on failure.
    pub async fn put_meeting_attendance(
        &self,
        course_id: &str,
        meeting_id: &str,
        target_user_id: &str,
        status: AttendanceStatus,
    ) -> Result<MeetingAttendance, AppError> {
        Ok(self
            .store()
            .upsert_meeting_attendance(course_id, meeting_id, target_user_id, status)
            .await?)
    }

    /// Lists all exams of `course_id` with `caller_user_id`'s own score (if enrolled) and
    /// aggregate averages/counts across active enrolled course members.
    ///
    /// # Errors
    ///
    /// Returns the store's error if exams cannot be read.
    pub async fn list_course_exams(
        &self,
        course_id: &str,
        caller_user_id: Option<&str>,
    ) -> Result<Vec<CourseExam>, AppError> {
        Ok(self
            .store()
            .list_course_exams(course_id, caller_user_id)
            .await?)
    }

    /// Creates a new exam on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course does not exist, and the store's error on failure.
    pub async fn create_course_exam(
        &self,
        course_id: &str,
        new: &ValidatedNewExam,
    ) -> Result<CourseExam, AppError> {
        Ok(self.store().create_course_exam(course_id, new).await?)
    }

    /// Updates an exam on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or exam does not exist, and the store's error on failure.
    pub async fn patch_course_exam(
        &self,
        course_id: &str,
        exam_id: &str,
        caller_user_id: Option<&str>,
        change: &CourseExamChange,
    ) -> Result<CourseExam, AppError> {
        Ok(self
            .store()
            .patch_course_exam(course_id, exam_id, caller_user_id, change)
            .await?)
    }

    /// Permanently deletes an exam on `course_id` along with all associated scores.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or exam does not exist, and the store's error on failure.
    pub async fn delete_course_exam(&self, course_id: &str, exam_id: &str) -> Result<(), AppError> {
        Ok(self.store().delete_course_exam(course_id, exam_id).await?)
    }

    /// Publishes or republishes a coarse exam result snapshot on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or exam does not exist, 409 if fewer than 5 active
    /// enrolled members have scored or fewer than 3 net scorers changed since the previous
    /// publication, and the store's error on failure.
    pub async fn publish_course_exam_results(
        &self,
        course_id: &str,
        exam_id: &str,
        caller_user_id: Option<&str>,
    ) -> Result<CourseExam, AppError> {
        match self
            .store()
            .publish_course_exam_results(course_id, exam_id, caller_user_id)
            .await?
        {
            Ok(exam) => Ok(exam),
            Err(refused) => Err(AppError::conflict(refused)),
        }
    }

    /// Unpublishes the exam result snapshot on `course_id`.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or exam does not exist, and the store's error on failure.
    pub async fn unpublish_course_exam_results(
        &self,
        course_id: &str,
        exam_id: &str,
        caller_user_id: Option<&str>,
    ) -> Result<CourseExam, AppError> {
        Ok(self
            .store()
            .unpublish_course_exam_results(course_id, exam_id, caller_user_id)
            .await?)
    }

    /// Creates or updates the calling learner's own personal exam score.
    ///
    /// Emits no websocket broadcasts, search index updates, webhooks, notifications, or
    /// audit log entries.
    ///
    /// # Errors
    ///
    /// Returns 400 if `score > exam.max_score`, 404 if the course or exam does not exist,
    /// and the store's error on failure.
    pub async fn put_my_exam_score(
        &self,
        course_id: &str,
        exam_id: &str,
        user_id: &str,
        input: &ValidatedMyExamScore,
    ) -> Result<MyExamScore, AppError> {
        match self
            .store()
            .upsert_my_exam_score(course_id, exam_id, user_id, input)
            .await?
        {
            ExamScoreOutcome::Saved(score) => Ok(score),
            ExamScoreOutcome::ExceedsMaxScore { max_score } => {
                let mut refused = Refusals::new();
                refused.refuse(
                    "score",
                    RefusalReason::Invalid,
                    format!("score must be at most {max_score}"),
                );
                Err(AppError::from(refused))
            }
        }
    }

    /// Deletes the calling learner's own personal exam score.
    ///
    /// Emits no websocket broadcasts, search index updates, webhooks, notifications, or
    /// audit log entries.
    ///
    /// # Errors
    ///
    /// Returns 404 if the course or exam does not exist, and the store's error on failure.
    pub async fn delete_my_exam_score(
        &self,
        course_id: &str,
        exam_id: &str,
        user_id: &str,
    ) -> Result<(), AppError> {
        Ok(self
            .store()
            .delete_my_exam_score(course_id, exam_id, user_id)
            .await?)
    }

    /// Sends one-shot `meeting_upcoming` notifications for unnotified course meetings
    /// starting within 24 hours of `now` across all shared organizations.
    ///
    /// Takes the housekeeping reach itself and requires the server lock. Returns the
    /// number of notifications created.
    ///
    /// # Errors
    ///
    /// Returns an error if the server lock is not held or if the database query fails.
    pub async fn notify_upcoming_meetings(&self, now: i64) -> Result<u64, AppError> {
        let app = self.housekeeping();
        app.require_server_lock()?;
        let sent = app
            .store()
            .notify_upcoming_meetings(now, MEETING_REMINDER_WINDOW_MILLIS)
            .await?;
        let count = u64::try_from(sent.len()).unwrap_or(u64::MAX);
        let mut notify_ws: BTreeSet<(&str, &str)> = BTreeSet::new();
        for item in &sent {
            notify_ws.insert((item.recipient_id.as_str(), item.org_id.as_str()));
        }
        for (user_id, org_id) in notify_ws {
            app.ws().notifications_changed(user_id, &[org_id]).await;
        }
        Ok(count)
    }
}
