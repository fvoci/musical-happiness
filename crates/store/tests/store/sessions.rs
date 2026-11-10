//! Sessions: found by the digest of their token, ended by disuse, by their end time, and
//! with the password they were opened under.

use fvoci_model::{Session, new_id};
use fvoci_store::{Store, StoreError};

use crate::common;

/// A time in milliseconds that the tests count from.
const T0: i64 = 1_800_000_000_000;

/// A session of the user that was made at [`T0`] and ends, whatever its use, at `T0 +
/// 1000`.
fn session_of(user_id: &str) -> Session {
    Session {
        id: new_id(),
        user_id: user_id.to_owned(),
        created_at: T0,
        last_used_at: T0,
        expires_at: T0 + 1000,
        user_agent: "a browser".to_owned(),
    }
}

/// Whether the store has a valid session under the digest at `now`, counting as idle
/// what was not used after `idle_since`.
async fn is_valid(store: &Store, digest: &str, now: i64, idle_since: i64) -> bool {
    match store.get_session(digest, now, idle_since).await {
        Ok(_) => true,
        Err(StoreError::NoRows) => false,
        Err(other) => panic!("looking a session up failed: {other:?}"),
    }
}

/// `testCreateAndGetAndDeleteSession`.
#[tokio::test]
async fn create_and_get_and_delete_session() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;

    // "CreateAndGetSession"
    let session = session_of(&user_id);
    store
        .create_session(&session, "digest")
        .await
        .expect("the session is stored");
    let got = store
        .get_session("digest", T0 + 1, T0 - 1)
        .await
        .expect("the session is found by its digest");
    assert_eq!(got, session, "as it was stored");

    // "Get nonexistent session"
    let error = store
        .get_session("another-digest", T0 + 1, T0 - 1)
        .await
        .expect_err("no session has that digest");
    assert!(matches!(error, StoreError::NoRows), "got {error:?}");
    assert!(error.is_not_found(), "which counts as not found");

    // "DeleteAndGetSession"
    store
        .delete_session(&session.id)
        .await
        .expect("the session is deleted");
    assert!(
        !is_valid(store, "digest", T0 + 1, T0 - 1).await,
        "a deleted session is gone"
    );
    store
        .delete_session(&session.id)
        .await
        .expect("deleting it again succeeds");
    store
        .delete_session(&new_id())
        .await
        .expect("as does deleting one that never was");

    test.teardown().await;
}

/// A session is stored under what the caller gives as the digest, and a digest names
/// one session.
#[tokio::test]
async fn a_session_is_found_by_its_digest_and_two_cannot_share_one() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;

    store
        .create_session(&session_of(&user_id), "digest-one")
        .await
        .expect("the first session is stored");
    let error = store
        .create_session(&session_of(&user_id), "digest-one")
        .await
        .expect_err("a second session under the same digest is refused");
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");
    store
        .create_session(&session_of(&user_id), "digest-two")
        .await
        .expect("a user has as many sessions as they sign in");

    let same_id = session_of(&user_id);
    store
        .create_session(&same_id, "digest-three")
        .await
        .expect("a third session");
    let error = store
        .create_session(&same_id, "digest-four")
        .await
        .expect_err("a session identifier is used once");
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");

    test.teardown().await;
}

/// A session that has no user, or text for one, is not stored.
#[tokio::test]
async fn a_session_needs_identifiers() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;

    for (id, user) in [
        (new_id(), String::new()),
        (new_id(), "nobody".to_owned()),
        ("no-identifier".to_owned(), user_id.clone()),
    ] {
        let session = Session {
            id,
            ..session_of(&user)
        };
        let error = store
            .create_session(&session, &new_id())
            .await
            .expect_err("text that is no identifier is refused");
        assert!(matches!(error, StoreError::InvalidId(_)), "got {error:?}");
    }

    test.teardown().await;
}

/// A session ends when it was not used after the idle limit, and at its end time
/// whatever its use. Using it moves the first and never the second.
#[tokio::test]
async fn a_session_ends_by_disuse_and_at_its_end_time() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;
    let session = session_of(&user_id);
    store
        .create_session(&session, "digest")
        .await
        .expect("the session is stored");

    // Used at T0. With "idle since" before that, it is in use; at or after, it is idle.
    assert!(is_valid(store, "digest", T0 + 1, T0 - 1).await, "in use");
    assert!(
        !is_valid(store, "digest", T0 + 1, T0).await,
        "not used after the limit: idle"
    );
    assert!(!is_valid(store, "digest", T0 + 1, T0 + 5).await, "idle");

    // Using it moves the time it was last used.
    store
        .touch_session(&session.id, T0 + 10)
        .await
        .expect("the use is recorded");
    assert!(
        is_valid(store, "digest", T0 + 11, T0 + 5).await,
        "in use again"
    );
    let touched = store
        .get_session("digest", T0 + 11, T0 + 5)
        .await
        .expect("the session is found");
    assert_eq!(touched.last_used_at, T0 + 10, "the recorded use");
    assert_eq!(touched.created_at, T0, "the creation time stays");
    assert_eq!(touched.expires_at, T0 + 1000, "and so does the end time");

    // The end time is the end, however recently the session was used.
    store
        .touch_session(&session.id, T0 + 999)
        .await
        .expect("the use is recorded");
    assert!(
        is_valid(store, "digest", T0 + 999, T0 + 900).await,
        "the moment before the end"
    );
    assert!(
        !is_valid(store, "digest", T0 + 1000, T0 + 900).await,
        "at its end time the session has ended"
    );
    assert!(
        !is_valid(store, "digest", T0 + 5000, T0 + 900).await,
        "and after"
    );

    // Recording the use of a session that is not there changes nothing.
    store
        .touch_session(&new_id(), T0)
        .await
        .expect("touching no session succeeds");

    test.teardown().await;
}

/// The clean-up deletes exactly the sessions that have ended: the idle ones and the
/// ones past their end time.
#[tokio::test]
async fn clean_up_deletes_the_sessions_that_have_ended() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user").await;

    let live = session_of(&user_id);
    let idle = Session {
        last_used_at: T0 - 500,
        created_at: T0 - 500,
        ..session_of(&user_id)
    };
    let past_its_end = Session {
        expires_at: T0 + 50,
        ..session_of(&user_id)
    };
    for (session, digest) in [(&live, "live"), (&idle, "idle"), (&past_its_end, "past")] {
        store
            .create_session(session, digest)
            .await
            .expect("the session is stored");
    }

    let now = T0 + 100;
    let idle_since = T0 - 100;
    let deleted = store
        .clean_up_sessions(now, idle_since)
        .await
        .expect("the clean-up runs");
    assert_eq!(deleted, 2, "the idle one and the one past its end");
    assert!(
        is_valid(store, "live", now, idle_since).await,
        "the live one stays"
    );
    // Gone, not only refused: with limits that would accept them they are not found.
    assert!(
        !is_valid(store, "idle", now, T0 - 1000).await,
        "the idle one"
    );
    assert!(
        !is_valid(store, "past", T0, idle_since).await,
        "the ended one"
    );
    assert_eq!(
        store
            .clean_up_sessions(now, idle_since)
            .await
            .expect("the clean-up runs again"),
        0,
        "nothing is left to delete"
    );

    test.teardown().await;
}

/// A password set by name ends the sessions of that user, and only theirs, together
/// with the password change.
#[tokio::test]
async fn reset_user_password_deletes_the_sessions_of_that_user_only() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let bob = common::user(store, "bob").await;
    for (user, digest) in [(&alice, "alice-1"), (&alice, "alice-2"), (&bob, "bob-1")] {
        store
            .create_session(&session_of(user), digest)
            .await
            .expect("the session is stored");
    }

    let reset = store
        .reset_user_password("alice", "new-hash")
        .await
        .expect("the password is set");
    assert_eq!(reset, alice, "the user whose password was set");

    for digest in ["alice-1", "alice-2"] {
        assert!(
            !is_valid(store, digest, T0 + 1, T0 - 1).await,
            "{digest} ended with the old password"
        );
    }
    assert!(
        is_valid(store, "bob-1", T0 + 1, T0 - 1).await,
        "another user's session stays"
    );
    let stored = store
        .get_user_by_id(&alice)
        .await
        .expect("the user is read");
    assert_eq!(stored.password_hash, "new-hash", "the new hash is stored");

    // No such user: nothing changes.
    let error = store
        .reset_user_password("nobody", "hash")
        .await
        .expect_err("no user has that name");
    assert!(
        matches!(error, StoreError::UserNotFound(_)),
        "got {error:?}"
    );
    assert!(is_valid(store, "bob-1", T0 + 1, T0 - 1).await, "untouched");

    test.teardown().await;
}

/// A password a user changes themselves ends their other sessions and keeps the one it
/// was changed through.
#[tokio::test]
async fn change_user_password_keeps_one_session_and_ends_the_others() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    let bob = common::user(store, "bob").await;
    let kept = session_of(&alice);
    store
        .create_session(&kept, "alice-here")
        .await
        .expect("the session is stored");
    for (user, digest) in [
        (&alice, "alice-phone"),
        (&alice, "alice-old"),
        (&bob, "bob"),
    ] {
        store
            .create_session(&session_of(user), digest)
            .await
            .expect("the session is stored");
    }

    store
        .change_user_password(&alice, "new-hash", &kept.id)
        .await
        .expect("the password is changed");

    assert!(
        is_valid(store, "alice-here", T0 + 1, T0 - 1).await,
        "the session the change came through stays"
    );
    for digest in ["alice-phone", "alice-old"] {
        assert!(
            !is_valid(store, digest, T0 + 1, T0 - 1).await,
            "{digest} ended with the old password"
        );
    }
    assert!(
        is_valid(store, "bob", T0 + 1, T0 - 1).await,
        "another user's session stays"
    );
    let stored = store
        .get_user_by_id(&alice)
        .await
        .expect("the user is read");
    assert_eq!(stored.password_hash, "new-hash", "the new hash is stored");

    // A user who is not there: an error, and no session is touched.
    let error = store
        .change_user_password(&new_id(), "hash", &kept.id)
        .await
        .expect_err("no such user");
    assert!(
        matches!(error, StoreError::UserNotFound(_)),
        "got {error:?}"
    );
    assert!(
        is_valid(store, "alice-here", T0 + 1, T0 - 1).await,
        "untouched"
    );

    test.teardown().await;
}

/// A hash is replaced by a newer one of the same password only while it is still the
/// stored one, and sessions stay.
#[tokio::test]
async fn replace_password_hash_replaces_the_hash_it_was_made_from() {
    let test = common::setup().await;
    let store = &test.store;
    let alice = common::user(store, "alice").await;
    store
        .reset_user_password("alice", "old-hash")
        .await
        .expect("a first hash");
    store
        .create_session(&session_of(&alice), "alice")
        .await
        .expect("the session is stored");

    assert!(
        store
            .replace_password_hash(&alice, "old-hash", "current-hash")
            .await
            .expect("the statement runs"),
        "the stored hash is the one named: replaced"
    );
    assert!(
        !store
            .replace_password_hash(&alice, "old-hash", "another-hash")
            .await
            .expect("the statement runs"),
        "the password hash changed in the meantime: left alone"
    );
    assert!(
        !store
            .replace_password_hash(&new_id(), "old-hash", "another-hash")
            .await
            .expect("the statement runs"),
        "no such user"
    );
    let stored = store
        .get_user_by_id(&alice)
        .await
        .expect("the user is read");
    assert_eq!(stored.password_hash, "current-hash");
    assert!(
        is_valid(store, "alice", T0 + 1, T0 - 1).await,
        "the password did not change, the session stays"
    );

    test.teardown().await;
}
