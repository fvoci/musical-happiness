//! Transactions that write the links of invitations, and those that delete what the
//! links hang from, wait for each other instead of each holding what the other waits
//! for (`crates/store/src/link_locks.rs`). On PostgreSQL each test stops one
//! transaction at its first lock of a link or of a workspace, starts the other, lets
//! the first go once the other waits for it, and asks that both end as they should; on
//! SQLite, whose writers take turns, the two run one after the other.

use fvoci_model::{BoardRole, DeleteBoardsAndBlocks, GuestInvitationsChange, Invitation};
use fvoci_store::{DbType, MailedTo, NewLink};

use crate::common::{self, TestStore};
use crate::invitations::{LATER, NOW, link};
use crate::lock_gate::{Gate, ended};

const OFF: GuestInvitationsChange = GuestInvitationsChange { enabled: false };

/// The link made like `parent` for one recipient of its mail, with the token's digest
/// `token_hash`.
fn mailed_like<'a>(
    workspace_id: &'a str,
    parent: &'a Invitation,
    token_hash: &'a str,
) -> NewLink<'a> {
    NewLink {
        workspace_id,
        guest_of: parent.board_id.as_deref().map(|id| (id, BoardRole::Viewer)),
        token_hash,
        created_by: &parent.created_by,
        now: NOW,
        expires_at: LATER,
        max_uses: Some(1),
        mailed: Some(MailedTo {
            parent_id: &parent.id,
            recipient: "someone@example.test",
        }),
    }
}

/// Whether the row of the link `first` comes before that of `second` in its table, as
/// a scan of the whole table meets them.
async fn comes_before(test: &TestStore, first: &str, second: &str) -> bool {
    let mut conn = common::connect_past_the_store(&test.dbconfig()).await;
    sqlx::query_scalar(
        "SELECT (SELECT ctid FROM invitations WHERE id = $1::uuid) \
         < (SELECT ctid FROM invitations WHERE id = $2::uuid)",
    )
    .bind(first)
    .bind(second)
    .fetch_one(&mut conn)
    .await
    .expect("the rows are found")
}

/// The digests of the tokens of the links that are there and not revoked.
async fn unrevoked(test: &TestStore) -> Vec<String> {
    let mut conn = common::connect_past_the_store(&test.dbconfig()).await;
    sqlx::query_scalar("SELECT token_hash FROM invitations WHERE revoked_at = 0 ORDER BY 1")
        .fetch_all(&mut conn)
        .await
        .expect("the links are read")
}

/// Revoking a mailed link holds it and then its children; deleting its board deletes
/// them in the order the table holds them, here the child first: the link was used
/// after it was mailed, and its row moved behind the child's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn revoking_a_link_and_deleting_its_board_wait_for_each_other() {
    let test = common::setup().await;
    let admin = common::user(&test, "admin").await;
    let boards = common::create_test_boards(&test, &test.workspace_id, &admin, 1).await;
    let board_id = boards[0].id.clone();
    let guest_of = Some((board_id.as_str(), BoardRole::Viewer));
    let parent = link(&test, &admin, "parent", guest_of, None).await;
    let child = test
        .create_invitation(&mailed_like(&test.workspace_id, &parent, "child"))
        .await
        .expect("the child is stored");
    if common::db_type() != DbType::Postgres {
        let revoked = test.revoke_invitation(&test.workspace_id, &parent.id, None, NOW);
        assert!(revoked.await.expect("revoked"));
        test.delete_board(&board_id, &admin).await.expect("deleted");
        let child = test.unended_invitation("child", NOW).await.expect("asked");
        assert_eq!(child, None, "the child went with its board");
        test.teardown().await;
        return;
    }
    let used = format!(
        "UPDATE invitations SET uses = uses + 1 WHERE id = '{}'",
        parent.id
    );
    common::execute_raw(&test.dbconfig(), &used).await;
    assert!(
        comes_before(&test, &child.id, &parent.id).await,
        "the deletion meets the child first"
    );

    let mut gate = Gate::before(
        &test,
        "UPDATE",
        "invitations",
        "OLD.parent_id IS NULL AND OLD.revoked_at = 0 AND NEW.revoked_at <> 0",
    )
    .await;
    let deleter = gate.second_store(&test).await;
    let revoker = test.store.clone();
    let (workspace_id, parent_id) = (test.workspace_id.clone(), parent.id.clone());
    let revoke = tokio::spawn(async move {
        revoker
            .revoke_invitation(&workspace_id, &parent_id, None, NOW)
            .await
    });
    let revoking = gate.stopped(&revoke).await;
    let by = admin.clone();
    let delete = tokio::spawn(async move { deleter.delete_board(&board_id, &by).await });
    gate.waits_for(revoking, &delete).await;
    gate.open().await;

    assert!(ended(revoke).await.expect("the link is revoked"));
    ended(delete).await.expect("the board is deleted");
    assert_eq!(
        test.unended_invitation("child", NOW).await.expect("asked"),
        None,
        "the child went with its board"
    );
    test.teardown().await;
}

/// Turning a workspace's guest links off holds them in the order the table holds them;
/// deleting two boards at once deletes the links of each in the order the boards are
/// asked for, here the other way round.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn turning_guest_links_off_and_deleting_their_boards_wait_for_each_other() {
    let test = common::setup().await;
    let admin = common::user(&test, "admin").await;
    let boards = common::create_test_boards(&test, &test.workspace_id, &admin, 2).await;
    let on_board = |at: usize| Some((boards[at].id.as_str(), BoardRole::Viewer));
    let first = link(&test, &admin, "first", on_board(0), None).await;
    let second = link(&test, &admin, "second", on_board(1), None).await;
    let both = DeleteBoardsAndBlocks {
        boards: vec![boards[1].id.clone(), boards[0].id.clone()],
        blocks: Vec::new(),
    };
    if common::db_type() != DbType::Postgres {
        let turned = test.set_guest_invitations(&test.workspace_id, OFF, &admin);
        turned.await.expect("turned off");
        let deleted = test.delete_boards_and_blocks(&both, &admin);
        deleted.await.expect("deleted");
        test.teardown().await;
        return;
    }
    assert!(
        comes_before(&test, &first.id, &second.id).await,
        "turning off meets the first link first"
    );

    let mut gate = Gate::before(
        &test,
        "UPDATE",
        "invitations",
        "OLD.revoked_at = 0 AND NEW.revoked_at <> 0",
    )
    .await;
    let deleter = gate.second_store(&test).await;
    let turner = test.store.clone();
    let (workspace_id, by) = (test.workspace_id.clone(), admin.clone());
    let turn_off =
        tokio::spawn(async move { turner.set_guest_invitations(&workspace_id, OFF, &by).await });
    let turning = gate.stopped(&turn_off).await;
    let by = admin.clone();
    let delete = tokio::spawn(async move { deleter.delete_boards_and_blocks(&both, &by).await });
    gate.waits_for(turning, &delete).await;
    gate.open().await;

    let workspace = ended(turn_off).await.expect("turned off");
    assert!(!workspace.guest_invitations);
    ended(delete).await.expect("the boards are deleted");
    assert_eq!(unrevoked(&test).await, Vec::<String>::new());
    test.teardown().await;
}
