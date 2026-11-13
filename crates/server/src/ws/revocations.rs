//! What was taken back, kept so that a subscription checked meanwhile is checked again
//! when, and only when, something that concerns it was: another user's logout, or a
//! change in another organization's workspace, does not overtake it.

use std::collections::HashMap;

/// When each thing a revocation concerns was last taken back, numbered in one
/// sequence: a check notes the number before it reads the store ([`Self::now`]), and
/// asks as it registers whether what concerns it was taken back after that. One
/// sequence, so that a check that learns its board only as it reads can still be
/// measured against that board.
#[derive(Debug, Default)]
pub(super) struct Revocations {
    /// The number of the latest revocation.
    now: u64,
    /// What may end anybody's access in any workspace: an organization whose
    /// workspaces could not be read beginning to take no password.
    everyone: u64,
    /// What ended a user's access somewhere, or closed a connection of theirs.
    users: HashMap<String, u64>,
    /// A workspace that went to the trash, or whose organization began to take no
    /// password.
    workspaces: HashMap<String, u64>,
    /// A board whose read token stopped opening it.
    boards: HashMap<String, u64>,
}

impl Revocations {
    /// The number of the latest revocation: what is read from now on is measured
    /// against it.
    pub(super) fn now(&self) -> u64 {
        self.now
    }

    /// Whether what concerns `user_id` following `workspace_id` was taken back after
    /// `since`.
    pub(super) fn of_workspace_since(&self, user_id: &str, workspace_id: &str, since: u64) -> bool {
        self.everyone > since
            || after(&self.users, user_id, since)
            || after(&self.workspaces, workspace_id, since)
    }

    /// Whether what concerns a connection of `user_id` (empty without a session)
    /// following blocks of `board_id` by its read token was taken back after `since`.
    pub(super) fn of_board_since(&self, user_id: &str, board_id: &str, since: u64) -> bool {
        self.everyone > since
            || after(&self.users, user_id, since)
            || after(&self.boards, board_id, since)
    }

    pub(super) fn everyone(&mut self) {
        self.everyone = self.next();
    }

    pub(super) fn user(&mut self, user_id: &str) {
        if !user_id.is_empty() {
            let now = self.next();
            self.users.insert(user_id.to_owned(), now);
        }
    }

    pub(super) fn workspace(&mut self, workspace_id: &str) {
        let now = self.next();
        self.workspaces.insert(workspace_id.to_owned(), now);
    }

    pub(super) fn board(&mut self, board_id: &str) {
        let now = self.next();
        self.boards.insert(board_id.to_owned(), now);
    }

    fn next(&mut self) -> u64 {
        self.now += 1;
        self.now
    }
}

fn after(last: &HashMap<String, u64>, key: &str, since: u64) -> bool {
    last.get(key).is_some_and(|at| *at > since)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_revocation_concerns_its_user_its_workspace_and_its_board_alone() {
        let mut revoked = Revocations::default();
        let before = revoked.now();
        revoked.user("one");
        revoked.workspace("w1");
        revoked.board("b1");
        assert!(revoked.of_workspace_since("one", "w2", before));
        assert!(revoked.of_workspace_since("two", "w1", before));
        assert!(!revoked.of_workspace_since("two", "w2", before));
        assert!(revoked.of_board_since("", "b1", before));
        assert!(!revoked.of_board_since("", "b2", before));
        let after = revoked.now();
        assert!(!revoked.of_workspace_since("one", "w1", after));
        revoked.everyone();
        assert!(revoked.of_workspace_since("two", "w2", after));
        assert!(revoked.of_board_since("", "b2", after));
    }
}
