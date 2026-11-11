//! Failures to make a file backend have, for the tests of what the server does when the
//! file storage fails (the feature `test-util`).

use std::sync::Mutex;

use super::StorageError;

/// An operation of a file backend that can be made to fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultyOp {
    /// [`FileBackend::write`](super::FileBackend::write), failing before it writes.
    Write,
    /// [`FileBackend::write`](super::FileBackend::write), failing after it wrote: as
    /// when the object store took the object and its answer was lost.
    WriteAnswerLost,
    /// [`FileBackend::copy`](super::FileBackend::copy).
    Copy,
    /// [`FileBackend::delete`](super::FileBackend::delete).
    Delete,
}

/// The failures a test asks for: how many of the next calls of each operation fail.
/// None at first.
#[derive(Debug, Default)]
pub struct Faults {
    left: Mutex<Vec<(FaultyOp, u32)>>,
}

impl Faults {
    /// Makes the next `times` calls of `op` fail, in place of what was asked before.
    ///
    /// # Panics
    ///
    /// Panics if a test panicked while it held the faults.
    pub fn fail(&self, op: FaultyOp, times: u32) {
        let mut left = self.left.lock().expect("the faults are not poisoned");
        left.retain(|(other, _)| *other != op);
        left.push((op, times));
    }

    /// Fails a call of `op` if one is asked for, counting it.
    pub(super) fn check(&self, op: FaultyOp, path: &str) -> Result<(), StorageError> {
        let mut left = self.left.lock().expect("the faults are not poisoned");
        match left
            .iter_mut()
            .find(|(other, times)| *other == op && *times > 0)
        {
            Some((_, times)) => {
                *times -= 1;
                Err(StorageError::Io(std::io::Error::other(format!(
                    "a failure the test asked for: {op:?} {path}"
                ))))
            }
            None => Ok(()),
        }
    }
}
