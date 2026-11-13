//! Pending admissions and joined documents share the connection's room lifetime.

use std::sync::{Arc, Mutex};

use fvoci_model::WorkspaceRole;

use super::rooms::Joined;

/// The document workspaces a connection is entering or currently holds.
#[derive(Debug, Clone, Default)]
pub struct Participation {
    joined: Arc<Mutex<Joined>>,
}

#[derive(Debug)]
pub(super) struct Watch {
    workspace: String,
    role: Option<WorkspaceRole>,
}

impl Participation {
    pub(super) fn new(joined: Arc<Mutex<Joined>>) -> Self {
        Self { joined }
    }

    /// Whether the connection holds or is entering a document in this workspace.
    ///
    /// # Panics
    ///
    /// Panics if a previous panic poisoned the connection's room lock.
    pub fn contains_workspace(&self, workspace: &str) -> bool {
        self.joined
            .lock()
            .expect("the connection's rooms lock")
            .watched
            .values()
            .any(|watch| watch.workspace == workspace)
    }

    /// Workspaces and their admitted roles; `None` means admission is still checking.
    ///
    /// # Panics
    ///
    /// Panics if a previous panic poisoned the connection's room lock.
    pub fn workspaces(&self) -> Vec<(String, Option<WorkspaceRole>)> {
        self.joined
            .lock()
            .expect("the connection's rooms lock")
            .watched
            .values()
            .map(|watch| (watch.workspace.clone(), watch.role))
            .collect()
    }

    pub(super) fn admitting(self, doc: u32, workspace: &str) -> Admission {
        self.joined
            .lock()
            .expect("the connection's rooms lock")
            .watched
            .insert(
                doc,
                Watch {
                    workspace: workspace.to_owned(),
                    role: None,
                },
            );
        Admission {
            participation: self,
            doc,
            finished: false,
        }
    }
}

/// Failed or cancelled admission removes its watch; a joined room owns it thereafter.
pub(super) struct Admission {
    participation: Participation,
    doc: u32,
    finished: bool,
}

impl Admission {
    pub(super) fn set_role(&mut self, role: WorkspaceRole) {
        if let Some(watch) = self
            .participation
            .joined
            .lock()
            .expect("the connection's rooms lock")
            .watched
            .get_mut(&self.doc)
        {
            watch.role = Some(role);
        }
    }

    pub(super) fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        if !self.finished {
            self.participation
                .joined
                .lock()
                .expect("the connection's rooms lock")
                .watched
                .remove(&self.doc);
        }
    }
}

/// The admission stage a deterministic test pauses.
#[cfg(feature = "test-util")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionStage {
    /// Pending participation is visible but permissions have not been read.
    BeforeCheck,
    /// Permissions are published but the document has not been opened.
    AfterCheck,
}

#[cfg(feature = "test-util")]
#[derive(Debug)]
pub(super) struct AdmissionPause {
    stage: AdmissionStage,
    reached: Arc<tokio::sync::Notify>,
    resume: Arc<tokio::sync::Notify>,
}

#[cfg(feature = "test-util")]
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
