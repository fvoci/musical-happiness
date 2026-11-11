//! The source files the checks read, and how they are named in lists and messages.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Every `.rs` file under `directory`, in the order of their paths.
///
/// # Errors
///
/// Fails if a directory cannot be read.
pub(crate) fn rust_files(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(directory, &mut files)?;
    Ok(files)
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)
        .with_context(|| format!("reading {}", directory.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .with_context(|| format!("reading {}", directory.display()))?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

/// A path below the workspace root as the lists and the messages write it: from the
/// root, with `/` on every system.
pub(crate) fn shown_path(root: &Path, file: &Path) -> String {
    let relative = file.strip_prefix(root).unwrap_or(file);
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.join("/")
}
