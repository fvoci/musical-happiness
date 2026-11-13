//! What was taken back, kept so that a subscription checked meanwhile is checked again
//! when, and only when, something that concerns it was: another user's logout, or a
//! change in another organization or workspace, does not overtake it.

use std::collections::HashMap;

/// When each thing a revocation concerns was last taken back, numbered in one
/// sequence: a check notes the number before it reads the store ([`Self::now`]), and
/// asks as it registers whether what concerns it was taken back after that. One
/// sequence, so that a check that learns its board or its organization only as it
/// reads can still be measured against it.
#[derive(Debug, Default)]
pub(super) struct Revocations {
    /// The number of the latest revocation.
    now: u64,
    /// What may end anybody's access in any workspace: an organization whose
    /// workspaces could not be read beginning to take no password.
    everyone: u64,
    /// A user whose connections were closed: a session of theirs ended.
    users: HashMap<String, u64>,
    /// A user whose access to one workspace ended or changed, by user and workspace.
    users_in_workspaces: HashMap<String, HashMap<String, u64>>,
    /// A user whose access was taken back in an organization (an administrator's
    /// joining, who runs the server), by user and organization.
    users_in_orgs: HashMap<String, HashMap<String, u64>>,
    /// A workspace that went to the trash, or whose organization began to take no
    /// password.
    workspaces: HashMap<String, u64>,
    /// A board whose read token stopped opening it.
    boards: HashMap<String, u64>,
}

/// What a workspace subscription is of, to be measured against what was taken back.
#[derive(Debug, Clone, Copy)]
pub(super) struct Followed<'a> {
    pub(super) user: &'a str,
    pub(super) workspace: &'a str,
    pub(super) org: &'a str,
}

impl Revocations {
    /// The number of the latest revocation: what is read from now on is measured
    /// against it.
    pub(super) fn now(&self) -> u64 {
        self.now
    }

    /// Whether what concerns the subscription was taken back after `since`.
    pub(super) fn of_workspace_since(&self, followed: Followed<'_>, since: u64) -> bool {
        self.everyone > since
            || after(&self.users, followed.user, since)
            || after_of(
                &self.users_in_workspaces,
                followed.user,
                followed.workspace,
                since,
            )
            || after_of(&self.users_in_orgs, followed.user, followed.org, since)
            || after(&self.workspaces, followed.workspace, since)
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

    pub(super) fn user_in_workspace(&mut self, user_id: &str, workspace_id: &str) {
        let now = self.next();
        self.users_in_workspaces
            .entry(user_id.to_owned())
            .or_default()
            .insert(workspace_id.to_owned(), now);
    }

    pub(super) fn user_in_org(&mut self, user_id: &str, org_id: &str) {
        let now = self.next();
        self.users_in_orgs
            .entry(user_id.to_owned())
            .or_default()
            .insert(org_id.to_owned(), now);
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

fn after_of(
    last: &HashMap<String, HashMap<String, u64>>,
    user: &str,
    of: &str,
    since: u64,
) -> bool {
    last.get(user)
        .is_some_and(|of_user| after(of_user, of, since))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn followed<'a>(user_id: &'a str, workspace_id: &'a str, org_id: &'a str) -> Followed<'a> {
        Followed {
            user: user_id,
            workspace: workspace_id,
            org: org_id,
        }
    }

    #[test]
    fn a_revocation_concerns_what_it_names_alone() {
        let mut revoked = Revocations::default();
        let before = revoked.now();
        revoked.user_in_workspace("one", "w1");
        revoked.user_in_org("two", "o2");
        revoked.workspace("w3");
        revoked.board("b1");
        assert!(revoked.of_workspace_since(followed("one", "w1", "o1"), before));
        assert!(
            !revoked.of_workspace_since(followed("one", "w2", "o1"), before),
            "another workspace of the same user"
        );
        assert!(revoked.of_workspace_since(followed("two", "w2", "o2"), before));
        assert!(
            !revoked.of_workspace_since(followed("two", "w1", "o1"), before),
            "the same user in another organization"
        );
        assert!(revoked.of_workspace_since(followed("three", "w3", "o1"), before));
        assert!(revoked.of_board_since("", "b1", before));
        assert!(!revoked.of_board_since("", "b2", before));
        let after = revoked.now();
        assert!(!revoked.of_workspace_since(followed("one", "w1", "o1"), after));
        revoked.user("one");
        assert!(revoked.of_workspace_since(followed("one", "w2", "o2"), after));
        assert!(revoked.of_board_since("one", "b2", after));
        revoked.everyone();
        assert!(revoked.of_workspace_since(followed("four", "w4", "o4"), after));
        assert!(revoked.of_board_since("", "b2", after));
    }
}
