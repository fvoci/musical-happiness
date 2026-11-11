//! Temporary files and directories that go however the work that made them ends.
//!
//! A request whose client goes away is dropped where it waits; the code after that
//! point, which would have removed what it wrote, never runs. A [`Doomed`] path is
//! removed when it is dropped, off the runtime's worker threads.

use std::path::{Path, PathBuf};

/// A temporary file or directory, removed when this is dropped.
#[derive(Debug)]
pub(crate) struct Doomed {
    path: PathBuf,
    kind: Kind,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    File,
    Directory,
}

impl Doomed {
    /// A file to be removed.
    pub(crate) fn file(path: PathBuf) -> Self {
        Self {
            path,
            kind: Kind::File,
        }
    }

    /// A directory to be removed with what it holds.
    pub(crate) fn directory(path: PathBuf) -> Self {
        Self {
            path,
            kind: Kind::Directory,
        }
    }

    /// The path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Doomed {
    fn drop(&mut self) {
        let path = std::mem::take(&mut self.path);
        let kind = self.kind;
        let remove = move || {
            let removed = match kind {
                Kind::File => std::fs::remove_file(&path),
                Kind::Directory => std::fs::remove_dir_all(&path),
            };
            match removed {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "a temporary file could not be removed");
                }
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn_blocking(remove);
            }
            Err(_) => remove(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_dropped_path_is_removed() {
        let directory = std::env::temp_dir().join(format!("doomed-{}", std::process::id()));
        std::fs::create_dir_all(directory.join("inside")).expect("a directory");
        let file = directory.join("file");
        std::fs::write(&file, b"x").expect("a file");

        drop(Doomed::file(file.clone()));
        drop(Doomed::directory(directory.join("inside")));
        for _ in 0..100 {
            if !file.exists() && !directory.join("inside").exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(!file.exists(), "the file went");
        assert!(!directory.join("inside").exists(), "the directory went");
        std::fs::remove_dir_all(&directory).expect("cleaned up");
    }
}
