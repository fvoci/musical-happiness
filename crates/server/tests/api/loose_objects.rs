//! The objects of the file storage whose record went: removed by the housekeeping,
//! tried again when the removal fails, and finished when a run was cut off. And what
//! keeps an object: a record of any board or organization.

use fvoci_model::{BoardType, FileInfo, Team, Upload, UploadPurpose, new_id};
use fvoci_server::app::RemovedObjects;
use fvoci_server::auth::now_millis;
use fvoci_server::auth::secret::new_secret;
use fvoci_server::storage::FaultyOp;
use fvoci_store::testing::db_type_of_the_tests;
use fvoci_store::{DbType, LooseState, Store};

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
            failed: 1,
            kept: 0,
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
            failed: 0,
            kept: 0,
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
            failed: 0,
            kept: 0,
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

/// Writes a file of `board_id` and records it through `owner`, and returns its place.
async fn recorded_file(ts: &TestServer, owner: &Store, board_id: &str) -> String {
    let stored_name = format!("{}.txt", new_secret());
    let path = format!("boards/20261112/{board_id}/{stored_name}");
    ts.app
        .files()
        .write(&path, &mut &b"recorded"[..])
        .await
        .expect("the file is written");
    owner
        .save_file_info(&FileInfo {
            id: new_id(),
            board_id: board_id.to_owned(),
            stored_name,
            path: path.clone(),
            size: 8,
            create_at: 1,
            ..FileInfo::default()
        })
        .await
        .expect("the file is recorded");
    path
}

/// An upload session of the second user, on a board of the test's organization, that
/// makes its file at `path`: as only a wrong call of the store could store one.
async fn misbound_upload(ts: &TestServer, path: &str) -> Upload {
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let upload = Upload {
        id: new_secret(),
        user_id: ts.client2.me().await.id,
        purpose: UploadPurpose::File,
        board_id: board.id,
        name: "misbound.txt".to_owned(),
        size: 8,
        part_size: 8,
        create_at: now_millis(),
        expires_at: now_millis() + 60_000,
        multipart_id: "left-over-multipart".to_owned(),
        path: path.to_owned(),
        ..Upload::default()
    };
    ts.app
        .store()
        .create_upload(&upload)
        .await
        .expect("the session is stored");
    upload
}

/// Abandons a session that makes its file at the place of a recorded file of another
/// board, and checks that the file is still there.
async fn abandoning_it_keeps_the_file(ts: &TestServer, path: &str) {
    let upload = misbound_upload(ts, path).await;
    ts.client2
        .delete(&format!("/uploads/{}", upload.id))
        .await
        .check_no_content();
    assert!(
        exists(ts, path).await,
        "the recorded file is still there after the session that named its place went"
    );
}

/// A session that names the place of another board's recorded file does not take the
/// file with it when it is abandoned: whether an object may go is asked of every
/// record, whatever board the session names.
#[tokio::test]
async fn abandoning_a_misbound_upload_keeps_another_boards_file() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let path = recorded_file(&ts, ts.app.store(), &board.id).await;
    abandoning_it_keeps_the_file(&ts, &path).await;
    ts.teardown().await;
}

/// The same across organizations: a session of one organization that names the place
/// of another organization's file does not take it with it (an object's place is one
/// across organizations).
#[tokio::test]
async fn abandoning_a_misbound_upload_keeps_another_organizations_file() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native().start().await.init_basic().await;
    let (theirs, board_id) = another_organizations_board(&ts).await;
    let path = recorded_file(&ts, &theirs, &board_id).await;
    abandoning_it_keeps_the_file(&ts, &path).await;
    ts.teardown().await;
}

/// A second organization, a board of it, and the handle that reaches its rows.
async fn another_organizations_board(ts: &TestServer) -> (Store, String) {
    let workspace = Team {
        id: new_id(),
        signup_token: new_secret(),
        ..Team::default()
    };
    let org_id = ts
        .app
        .store()
        .create_organization("another", &workspace)
        .await
        .expect("a second organization is made");
    let theirs = ts.app.store().for_org(&org_id).expect("an identifier");
    let mut board = fvoci_model::Board {
        id: new_id(),
        team_id: workspace.id.clone(),
        board_type: BoardType::Open,
        ..fvoci_model::Board::default()
    };
    theirs
        .insert_board(&mut board, &ts.client.me().await.id)
        .await
        .expect("their board is stored");
    (theirs, board.id)
}

/// The test's organization dooms the place of a recorded file, as a wrong call of the
/// store could (a mark of writing at the place, gone stale): the housekeeping asks of
/// every organization before it removes anything, keeps the file, takes the row off the
/// list and counts it as an error.
async fn a_doomed_object_that_is_held_is_kept(ts: &TestServer, path: &str) {
    ts.app
        .store()
        .mark_writing(path, 0)
        .await
        .expect("the place is marked");
    assert_eq!(
        remove_doomed(ts, now_millis()).await,
        RemovedObjects {
            removed: 0,
            failed: 0,
            kept: 1,
        },
        "the held object is kept, as an error"
    );
    assert!(exists(ts, path).await, "the recorded file is still there");
    assert_eq!(
        ts.app
            .housekeeping()
            .store()
            .loose_objects()
            .await
            .expect("listed"),
        [],
        "and taken off the list"
    );
}

#[tokio::test]
async fn a_doomed_object_that_another_board_holds_is_kept() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let path = recorded_file(&ts, ts.app.store(), &board.id).await;
    a_doomed_object_that_is_held_is_kept(&ts, &path).await;
    ts.teardown().await;
}

#[tokio::test]
async fn a_doomed_object_that_another_organization_holds_is_kept() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let ts = Setup::native().start().await.init_basic().await;
    let (theirs, board_id) = another_organizations_board(&ts).await;
    let path = recorded_file(&ts, &theirs, &board_id).await;
    a_doomed_object_that_is_held_is_kept(&ts, &path).await;
    ts.teardown().await;
}
