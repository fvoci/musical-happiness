//! The pause of a document's admission at a stage, for the tests of what happens
//! meanwhile.

use std::sync::Arc;

/// The admission stage a deterministic test pauses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionStage {
    /// The document is asked for, and permissions have not been read.
    BeforeCheck,
    /// Access was checked, and the document has not been opened.
    AfterCheck,
}

#[derive(Debug)]
pub(super) struct AdmissionPause {
    stage: AdmissionStage,
    reached: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}

impl super::Collab {
    /// Pauses the next admitted document before the document or room is opened.
    ///
    /// # Panics
    ///
    /// Panics if a previous panic poisoned the test pause lock.
    pub fn pause_next_admission(
        &self,
        stage: AdmissionStage,
    ) -> (Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let pause = AdmissionPause {
            stage,
            reached: Arc::default(),
            resume: Arc::default(),
        };
        let handle = (Arc::clone(&pause.reached), Arc::clone(&pause.resume));
        *self
            .inner
            .admission_pause
            .lock()
            .expect("the admission pause lock") = Some(pause);
        handle
    }

    pub(super) async fn paused_admission(&self, stage: AdmissionStage) {
        let pause = self
            .inner
            .admission_pause
            .lock()
            .expect("the admission pause lock")
            .take_if(|pause| pause.stage == stage);
        if let Some(pause) = pause {
            pause.reached.notify_one();
            pause.resume.notified().await;
        }
    }
}
