//! What a store tells its observer about its calls.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use fvoci_model::new_id;
use fvoci_store::{Access, Call, DbType, Observer, Outcome};

use crate::common;

/// What an observer was told.
#[derive(Debug, Default)]
struct Told {
    calls: Vec<Call>,
    waits: Vec<(DbType, Duration)>,
}

/// An observer that keeps what it is told.
#[derive(Debug, Clone, Default)]
struct Recorder(Arc<Mutex<Told>>);

impl Recorder {
    /// The calls told since the last time, and how many waits.
    fn take(&self) -> (Vec<Call>, usize) {
        let mut told = self.0.lock().expect("lock poisoned");
        let calls = std::mem::take(&mut told.calls);
        let waits = std::mem::take(&mut told.waits).len();
        (calls, waits)
    }
}

impl Observer for Recorder {
    fn called(&self, call: Call) {
        self.0.lock().expect("lock poisoned").calls.push(call);
    }

    fn waited_for_connection(&self, db: DbType, waited: Duration) {
        self.0
            .lock()
            .expect("lock poisoned")
            .waits
            .push((db, waited));
    }
}

/// The access and the outcome of each call.
fn shapes(calls: &[Call]) -> Vec<(Access, Outcome)> {
    calls
        .iter()
        .map(|call| (call.access, call.outcome))
        .collect()
}

#[tokio::test]
async fn every_call_is_told_with_whether_it_wrote_and_how_it_ended() {
    let store = common::setup().await;
    let recorder = Recorder::default();
    assert!(
        store.observe(Box::new(recorder.clone())),
        "the first observer"
    );
    assert!(
        !store.observe(Box::new(Recorder::default())),
        "a store has one observer"
    );
    let db = common::db_type();

    // A read that finds what it asks for, and one that does not.
    store
        .get_registered_user_count()
        .await
        .expect("the users are counted");
    let missing = store.get_user_by_id(&new_id()).await;
    assert!(missing.is_err(), "no such user");
    // A transaction that writes.
    let user_id = common::user(&store, "observed").await;
    store
        .get_user_by_id(&user_id)
        .await
        .expect("the user is there");
    let (calls, waits) = recorder.take();
    assert_eq!(
        shapes(&calls),
        [
            (Access::Read, Outcome::Done),
            (Access::Read, Outcome::Refused),
            (Access::Write, Outcome::Done),
            (Access::Read, Outcome::Done),
        ],
        "{calls:?}"
    );
    assert!(calls.iter().all(|call| call.db == db), "{calls:?}");
    assert_eq!(waits, calls.len(), "every call asked for a connection once");

    // Another handle to the same database tells the same observer.
    store
        .system()
        .get_registered_user_count()
        .await
        .expect("the users are counted");
    assert_eq!(recorder.take().0.len(), 1, "the system handle's call");

    // A call that gets no connection fails, and its wait is told.
    store.close().await;
    assert!(store.get_registered_user_count().await.is_err(), "closed");
    let (calls, waits) = recorder.take();
    assert_eq!(
        shapes(&calls),
        [(Access::Read, Outcome::Failed)],
        "{calls:?}"
    );
    assert_eq!(waits, 1, "the wait that got nothing");
    store.teardown().await;
}
