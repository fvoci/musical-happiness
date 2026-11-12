//! Independent PostgreSQL regressions for changes of admission during pending writes.

#[path = "api/common.rs"]
mod common;

use std::time::Duration;

use fvoci_model::{Board, Invitation, new_id};
use fvoci_store::testing::db_type_of_the_tests;
use fvoci_store::{DbType, Store, StoreError};
use reqwest::StatusCode;
use serde_json::json;
use tokio::task::JoinHandle;

use common::{Client, PASSWORD, ROOT_WORKSPACE_ID, Setup};

/// Waits for a database-observable synchronization point, rather than guessing how
/// long another request takes. Queries below concern only this test's own gate PID.
async fn wait_for_row(store: &Store, query: &str) -> String {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(found) = store
                .texts_unchecked(query)
                .await
                .expect("the synchronization point is inspected")
                .into_iter()
                .next()
            {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the request reaches the synchronization point")
}

/// Holds one row while another request reaches its lock. Cancellation releases this
/// test's gate, not the request being tested. The sleep bounds an abandoned gate.
struct RowGate {
    store: Store,
    pid: String,
    task: JoinHandle<Result<Vec<String>, StoreError>>,
}

impl RowGate {
    async fn hold(store: &Store, table: &str, id: &str) -> Self {
        let store = store.system();
        let marker = format!("review_gate_{}", new_id());
        let query = format!(
            "SELECT pg_sleep(30)::text FROM \
             (SELECT id FROM {table} WHERE id = '{id}' FOR NO KEY UPDATE) AS held \
             /* {marker} */"
        );
        let connection = store.clone();
        let task = tokio::spawn(async move { connection.texts_unchecked(&query).await });
        let pid = wait_for_row(
            &store,
            &format!(
                "SELECT pid::text FROM pg_stat_activity \
                 WHERE wait_event = 'PgSleep' AND query LIKE '%{marker}%' \
                 AND pid <> pg_backend_pid()"
            ),
        )
        .await;
        Self { store, pid, task }
    }

    async fn wait_for_waiter(&self) {
        wait_for_row(
            &self.store,
            &format!(
                "SELECT pid::text FROM pg_stat_activity \
                 WHERE {} = ANY(pg_blocking_pids(pid))",
                self.pid
            ),
        )
        .await;
    }

    async fn release(self) {
        let cancelled = self
            .store
            .texts_unchecked(&format!("SELECT pg_cancel_backend({})::text", self.pid))
            .await
            .expect("this test's gate query is cancelled");
        assert_eq!(cancelled, ["true"], "the gate was still held");
        assert!(
            self.task.await.expect("the gate task completes").is_err(),
            "cancellation rolls back the gate transaction and releases its row"
        );
    }
}

fn member_path(user_id: &str) -> String {
    format!("/w/{ROOT_WORKSPACE_ID}/members/{user_id}")
}

async fn private_board(client: &Client, title: &str) -> Board {
    client
        .post(
            &format!("/w/{}/boards", ROOT_WORKSPACE_ID),
            &json!({"workspaceId": ROOT_WORKSPACE_ID, "type": "P", "title": title}),
        )
        .await
        .created_json()
}

#[tokio::test]
async fn acceptance_rechecks_the_inviter_after_waiting_for_its_link() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let th = Setup::native().start().await.init_basic().await;
    let maker = th.client2.me().await.id;
    th.client
        .put(&member_path(&maker), &json!({"role": "admin"}))
        .await
        .check_ok();
    let link: Invitation = th
        .client2
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/invitations"),
            &json!({"role": "member"}),
        )
        .await
        .created_json();
    let token = link.token.expect("a token is handed out once");
    let gate = RowGate::hold(th.app.store(), "invitations", &link.id).await;
    let nobody = th.anonymous();
    let accepting_token = token.clone();
    let accepting = tokio::spawn(async move {
        nobody
            .post(
                "/invitations/accept",
                &json!({
                    "token": accepting_token,
                    "account": {
                        "username": "after-removal",
                        "email": "after-removal@example.com",
                        "password": PASSWORD,
                    },
                }),
            )
            .await
    });
    gate.wait_for_waiter().await;
    let removed = th.client.delete(&member_path(&maker)).await;
    let preview = th
        .anonymous()
        .post("/invitations/preview", &json!({"token": token}))
        .await;
    gate.release().await;
    let accepted = accepting.await.expect("the acceptance request completes");
    let memberships = th
        .app
        .workspace_members(ROOT_WORKSPACE_ID)
        .await
        .expect("members");
    th.teardown().await;

    removed.check_no_content();
    preview.check_status(StatusCode::NOT_FOUND);
    assert_eq!(
        accepted.status,
        StatusCode::NOT_FOUND,
        "a removed inviter's link must not admit a new account: {memberships:?}"
    );
    assert!(
        memberships
            .iter()
            .all(|member| member.username != "after-removal"),
        "the refused invitation must leave no new membership"
    );
}

#[derive(Debug, Clone, Copy)]
enum MembershipLoss {
    Removed,
    MadeGuest,
}

async fn a_new_board_keeps_an_admin_during(loss: MembershipLoss) {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let th = Setup::native().start().await.init_basic().await;
    let admin = th.client.me().await.id;
    let leaving = th.client2.me().await.id;
    let existing = private_board(&th.client2, "Existing board").await;
    let gate = RowGate::hold(th.app.store(), "boards", &existing.id).await;
    let manager = th.client.clone();
    let changing = tokio::spawn(async move {
        match loss {
            MembershipLoss::Removed => manager.delete(&member_path(&leaving)).await,
            MembershipLoss::MadeGuest => {
                manager
                    .put(&member_path(&leaving), &json!({"role": "guest"}))
                    .await
            }
        }
    });
    // The change holds the workspace and has already enumerated the user's boards.
    gate.wait_for_waiter().await;
    let created = private_board(&th.client2, "Created during the membership change").await;
    gate.release().await;
    let changed = changing.await.expect("the membership change completes");
    let members = th
        .app
        .store()
        .get_members_for_board(&created.id)
        .await
        .expect("the new board's roles are read");
    th.teardown().await;

    changed.check_status(match loss {
        MembershipLoss::Removed => StatusCode::NO_CONTENT,
        MembershipLoss::MadeGuest => StatusCode::OK,
    });
    assert!(
        members
            .iter()
            .any(|member| member.user_id == admin && member.scheme_admin),
        "the workspace administrator must take over the new board during {loss:?}: {members:?}"
    );
}

#[tokio::test]
async fn a_board_created_during_member_removal_is_handed_over() {
    a_new_board_keeps_an_admin_during(MembershipLoss::Removed).await;
}

#[tokio::test]
async fn a_board_created_during_guest_demotion_is_handed_over() {
    a_new_board_keeps_an_admin_during(MembershipLoss::MadeGuest).await;
}
