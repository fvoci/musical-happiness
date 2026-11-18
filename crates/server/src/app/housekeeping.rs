//! The reach of the server's housekeeping: the one place of the server that asks for
//! the rows of every organization; and the sweep of the temporary directories at the
//! start.

use std::io;
use std::path::{Path, PathBuf};

use fvoci_store::Tenant;
use tokio::fs;

use super::App;
use crate::auth::now_millis;
use crate::error::AppError;
use crate::storage::WRITING_DIR;

impl App {
    /// The server as its housekeeping uses it: reaching the rows of every organization.
    /// What the server throws away on nobody's behalf (sessions, uploads and temporary
    /// files that ended, what was in the trash too long) is looked for in all of them;
    /// as one organization, a clean-up would take what it does not see of the others
    /// for gone.
    ///
    /// Each job of the housekeeping takes this reach itself, whatever handle it is
    /// started with. Nothing that answers a request works through this.
    #[must_use]
    pub fn housekeeping(&self) -> Self {
        self.scoped(Tenant::System)
    }

    /// Removes the former providers whose time ran out (`oidc_former_provider_days`),
    /// of every organization, each recorded in its organization's audit log as the
    /// server's own doing. Until this runs such a one is read as none already. It
    /// takes the housekeeping's reach itself; what fails is logged, and left for the
    /// next run.
    pub async fn end_lapsed_former_providers(&self) {
        let app = self.housekeeping();
        if let Err(error) = app.require_server_lock() {
            tracing::warn!(%error, "the former providers that lapsed are not removed");
            return;
        }
        match app
            .store()
            .end_lapsed_oidc_former_providers(now_millis())
            .await
        {
            Ok(0) => {}
            Ok(ended) => tracing::info!(ended, "removed former providers whose time ran out"),
            Err(error) => {
                tracing::error!(%error, "unable to remove the former providers that lapsed");
            }
        }
    }

    /// Removes for good the workspaces of every organization that have been in the
    /// trash since before `deleted_before`, each in its organization, with what is in
    /// them. Returns how many boards and blocks went, how many files were doomed, and
    /// how many workspaces could not be removed: one that fails is logged and left for
    /// the next run, and the others go. For the housekeeping, whose reach this is.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the workspaces to remove cannot be read.
    pub async fn purge_workspaces(
        &self,
        deleted_before: i64,
    ) -> Result<WorkspacesPurged, AppError> {
        let mut done = WorkspacesPurged::default();
        for (org_id, workspace_id) in self.store().workspaces_to_purge(deleted_before).await? {
            let purged = match self.store().for_org(&org_id) {
                Ok(store) => store.purge_workspace(&workspace_id, deleted_before).await,
                Err(error) => Err(error),
            };
            match purged {
                Ok(purged) => {
                    done.records += purged.records;
                    done.files += purged.files.len();
                }
                Err(error) => {
                    tracing::error!(%error, org_id, workspace_id, "unable to purge a workspace");
                    done.failed += 1;
                }
            }
        }
        Ok(done)
    }

    /// Removes what the temporary directories hold that nothing will use: the parts of
    /// sessions that are gone, the directories of completions that were cut off, the
    /// archives being exported, and the files the local storage was writing. Only for
    /// when no request is under way, as at the start, and only by the process that holds
    /// the lock of the server, with one server to the files directory and the
    /// temporary directory (`docs/configuration.md`). Returns how many it removed.
    ///
    /// # Errors
    ///
    /// Returns a 503 error if this process does not hold the lock of the server, and
    /// the store's error if the sessions cannot be read.
    pub async fn sweep_temporary_files(&self) -> Result<usize, AppError> {
        self.require_server_lock()?;
        let sessions: std::collections::HashSet<String> =
            self.store().upload_ids().await?.into_iter().collect();
        let mut removed = 0;
        let uploads = self.config().tmp_dir().join("uploads");
        for (name, path) in entries(&uploads).await {
            // A session's directory is named by its identifier; anything else, such as a
            // directory a completion moved aside, is left over.
            self.require_server_lock()?;
            if !sessions.contains(&name) && remove_entry(&path).await {
                removed += 1;
            }
        }
        // Archives being written or sent when the server stopped.
        let exports = self.config().tmp_dir().join(super::export::EXPORTS_DIR);
        for (_, path) in entries(&exports).await {
            self.require_server_lock()?;
            if remove_entry(&path).await {
                removed += 1;
            }
        }
        if self.config().file_storage == "local" {
            let writing = Path::new(&self.config().file_storage_path).join(WRITING_DIR);
            for (_, path) in entries(&writing).await {
                self.require_server_lock()?;
                if remove_entry(&path).await {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

/// The entries of a directory with their names; none if it cannot be read.
async fn entries(directory: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut listing = match fs::read_dir(directory).await {
        Ok(listing) => listing,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return found,
        Err(error) => {
            tracing::warn!(%error, "a temporary directory could not be read");
            return found;
        }
    };
    loop {
        match listing.next_entry().await {
            Ok(Some(entry)) => {
                found.push((
                    entry.file_name().to_string_lossy().into_owned(),
                    entry.path(),
                ));
            }
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(%error, "a temporary directory could not be read to its end");
                break;
            }
        }
    }
    found
}

/// Removes a file or a directory that is left over, with what is in it. Returns whether
/// it was removed.
async fn remove_entry(path: &Path) -> bool {
    let removed = match fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path).await,
        Ok(_) => fs::remove_file(path).await,
        Err(error) => Err(error),
    };
    match removed {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => {
            tracing::warn!(%error, "a left-over temporary file could not be removed");
            false
        }
    }
}

/// What a purge of the workspaces in the trash came to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspacesPurged {
    /// The boards and blocks that went for good.
    pub records: u64,
    /// The files whose objects were doomed.
    pub files: usize,
    /// The workspaces that could not be removed, left for the next run.
    pub failed: usize,
}
