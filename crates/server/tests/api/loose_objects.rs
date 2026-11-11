//! The objects of the file storage whose record went: removed by the housekeeping,
//! tried again when the removal fails, and finished when a run was cut off.

use fvoci_model::BoardType;
use fvoci_server::app::RemovedObjects;
use fvoci_server::auth::now_millis;
use fvoci_server::storage::FaultyOp;
use fvoci_store::LooseState;

use crate::common::{Setup, TEST_TEAM_ID, TestServer};
use crate::files::{upload, uploaded_id};
use crate::uploads::s3_server;

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;

/// Uploads a file to a new board and returns the board and the file's place in the
/// file storage.
async fn uploaded(ts: &TestServer, data: &[u8]) -> (String, String) {
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = upload(&ts.client, &board.id, "a.txt", data).await;
    response.check_created();
    let info = ts
        .app
        .store()
        .get_file_info(&board.id, &uploaded_id(&response))
        .await
        .expect("the file is recorded");
    (board.id, info.path)
}

async fn exists(ts: &TestServer, path: &str) -> bool {
    ts.app
        .files()
        .exists(path)
        .await
        .expect("the storage is read")
}

async fn remove_doomed(ts: &TestServer, now: i64) -> RemovedObjects {
    ts.app
        .housekeeping()
        .remove_doomed_objects(now)
        .await
        .expect("the doomed objects are read")
}

/// The objects of the records that the collection of unshown files and the purge of the
/// trash remove are removed by the housekeeping; one whose removal fails stays listed,
/// with the failure noted, until it is due again and goes.
async fn doomed_objects_are_removed_and_tried_again(ts: &TestServer) {
    let housekeeping = ts.app.housekeeping();
    let (_, unshown) = uploaded(ts, b"shown by no block").await;
    let (board_id, purged) = uploaded(ts, b"of a purged board").await;
    ts.client
        .delete(&format!("/boards/{board_id}"))
        .await
        .check_no_content();
    housekeeping
        .store()
        .purge_trash(i64::MAX)
        .await
        .expect("the trash is purged");
    housekeeping
        .store()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("the files no block shows are collected");
    assert!(
        exists(ts, &unshown).await && exists(ts, &purged).await,
        "the objects stay until the housekeeping removes them"
    );

    let faults = ts
        .app
        .files()
        .faults()
        .expect("a test's storage takes faults");
    faults.fail(FaultyOp::Delete, 1);
    let now = now_millis();
    let first = remove_doomed(ts, now).await;
    assert_eq!(
        first,
        RemovedObjects {
            removed: 1,
            failed: 1
        },
        "one removal fails"
    );
    let listed = housekeeping.store().loose_objects().await.expect("listed");
    assert_eq!(listed.len(), 1, "{listed:?}");
    let failed = &listed[0];
    assert!(exists(ts, &failed.path).await, "its object is there");
    assert_eq!(
        (failed.state, failed.attempts, failed.last_error.as_str()),
        (LooseState::Doomed, 1, "io"),
        "the failure is noted"
    );
    assert!(failed.retry_at > now, "tried again later: {failed:?}");

    assert_eq!(
        remove_doomed(ts, now).await,
        RemovedObjects::default(),
        "not before it is due"
    );
    assert_eq!(
        remove_doomed(ts, now + DAY_MILLIS).await,
        RemovedObjects {
            removed: 1,
            failed: 0
        },
        "and then it goes"
    );
    assert!(
        !exists(ts, &unshown).await && !exists(ts, &purged).await,
        "both objects are gone"
    );
    assert_eq!(
        housekeeping.store().loose_objects().await.expect("listed"),
        [],
        "nothing is left listed"
    );
}

/// A run cut off between the removal of an object and taking it off the list leaves it
/// listed; the next run finds it gone and takes it off.
async fn a_run_cut_off_after_the_removal_is_finished_by_the_next(ts: &TestServer) {
    let housekeeping = ts.app.housekeeping();
    let (_, path) = uploaded(ts, b"removed, still listed").await;
    housekeeping
        .store()
        .collect_unreferenced_files(i64::MAX)
        .await
        .expect("collected");
    ts.app
        .files()
        .delete(&path)
        .await
        .expect("the removal of the run that was cut off");
    assert_eq!(
        remove_doomed(ts, now_millis()).await,
        RemovedObjects {
            removed: 1,
            failed: 0
        },
        "the object is found gone"
    );
    assert_eq!(
        housekeeping.store().loose_objects().await.expect("listed"),
        [],
        "and taken off the list"
    );
}

#[tokio::test]
async fn doomed_objects_on_the_disk_are_removed_and_tried_again() {
    let ts = Setup::native().start().await.init_basic().await;
    doomed_objects_are_removed_and_tried_again(&ts).await;
    a_run_cut_off_after_the_removal_is_finished_by_the_next(&ts).await;
    ts.teardown().await;
}

#[tokio::test]
#[ignore = "needs an S3-compatible server; see tests/storage.rs"]
async fn with_s3_doomed_objects_are_removed_and_tried_again() {
    let ts = s3_server().await;
    doomed_objects_are_removed_and_tried_again(&ts).await;
    a_run_cut_off_after_the_removal_is_finished_by_the_next(&ts).await;
    ts.teardown().await;
}
