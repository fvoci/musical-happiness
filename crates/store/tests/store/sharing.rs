//! Sharing a board: the state that is set is the state that is read, a shared board
//! keeps its token until a new one is asked for, and turning sharing off forgets it.
//!
//! The first test began as a port of `server/services/store/storetests/sharing.go`.

use fvoci_model::{Sharing, new_id};
use fvoci_store::{SharingChange, StoreError};

use crate::common;

/// `testUpsertSharingAndGetSharing`.
#[tokio::test]
async fn set_sharing_and_get_sharing() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "user-id").await;
    let user_id2 = common::user(store, "user-id2").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    // "Insert first sharing and get it"
    let first = store
        .set_sharing(board_id, SharingChange::On, "token", &user_id)
        .await
        .expect("the board is shared");
    assert_eq!(
        first,
        Sharing {
            board_id: board_id.clone(),
            enabled: true,
            token: "token".to_owned(),
            modified_by: user_id.clone(),
            update_at: first.update_at,
        },
        "the state that was stored is returned"
    );
    assert!(first.update_at > 0, "the store sets the time");
    let stored = store.get_sharing(board_id).await.expect("it is read");
    assert_eq!(stored, first, "and read");

    // "Upsert the inserted sharing and get it": under a new token, by another user.
    let second = store
        .set_sharing(board_id, SharingChange::OnWithNewToken, "token2", &user_id2)
        .await
        .expect("the board is shared anew");
    assert_eq!(second.token, "token2", "the new token");
    assert_eq!(second.modified_by, user_id2, "the later user");
    let stored = store.get_sharing(board_id).await.expect("it is read");
    assert_eq!(stored, second, "the later state replaced the first");

    // "Get not existing sharing"
    let error = store.get_sharing(&new_id()).await.unwrap_err();
    assert!(error.is_not_found(), "got {error:?}");

    test.teardown().await;
}

/// A board that is shared keeps its token when it is shared again; the token the caller
/// offers is used only when one is needed. Turning sharing off forgets the token, and
/// sharing again takes a new one.
#[tokio::test]
async fn a_token_is_kept_while_shared_and_forgotten_when_not() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "sharer").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 2).await;
    let (board_id, other_board_id) = (&boards[0].id, &boards[1].id);
    let token_after = async |change: SharingChange, offered: &str| {
        store
            .set_sharing(board_id, change, offered, &user_id)
            .await
            .expect("the sharing is set")
            .token
    };

    assert_eq!(token_after(SharingChange::On, "first").await, "first");
    assert_eq!(
        token_after(SharingChange::On, "second").await,
        "first",
        "shared already: the token stays"
    );
    assert_eq!(
        token_after(SharingChange::OnWithNewToken, "third").await,
        "third",
        "a new token on request"
    );

    // Switched off by nobody in particular: the modifier reads as empty.
    let off = store
        .set_sharing(board_id, SharingChange::Off, "unused", "")
        .await
        .expect("sharing is turned off");
    assert!(!off.enabled, "got {off:?}");
    assert_eq!(off.token, "", "the token is forgotten");
    let stored = store.get_sharing(board_id).await.expect("it is read");
    assert_eq!(stored, off, "as stored");
    assert_eq!(stored.modified_by, "", "no user is stored as none");
    assert_eq!(
        token_after(SharingChange::Off, "unused").await,
        "",
        "off twice is off"
    );
    assert_eq!(
        token_after(SharingChange::On, "fourth").await,
        "fourth",
        "shared again: the token that is offered now, not the old one"
    );

    // Two boards that are not shared both have no token; two that are cannot have the
    // same one.
    let missing = store.get_sharing(other_board_id).await.unwrap_err();
    assert_eq!(missing.to_string(), "sql: no rows in result set");
    for board in [board_id, other_board_id] {
        store
            .set_sharing(board, SharingChange::Off, "unused", &user_id)
            .await
            .expect("two boards without a token are no conflict");
    }
    store
        .set_sharing(board_id, SharingChange::On, "same", &user_id)
        .await
        .expect("the first board takes the token");
    let error = store
        .set_sharing(other_board_id, SharingChange::On, "same", &user_id)
        .await
        .expect_err("a token opens one board");
    assert!(matches!(error, StoreError::Database(_)), "got {error:?}");
    let other = store.get_sharing(other_board_id).await.expect("it is read");
    assert!(!other.enabled, "the refused change changed nothing");

    test.teardown().await;
}

/// What does not exist cannot be shared: a board that is not there, by a well-formed
/// identifier or by other text, and on behalf of text that names no user.
#[tokio::test]
async fn sharing_needs_a_board_and_a_user_that_exist() {
    let test = common::setup().await;
    let store = &test.store;
    let user_id = common::user(store, "sharer").await;
    let team_id = common::team(store).await;
    let boards = common::create_test_boards(store, &team_id, &user_id, 1).await;
    let board_id = &boards[0].id;

    for nothing in [new_id(), "not-a-board".to_owned()] {
        let missing = store.get_sharing(&nothing).await.unwrap_err();
        assert!(matches!(missing, StoreError::NoRows), "got {missing:?}");
        let error = store
            .set_sharing(&nothing, SharingChange::On, "token", &user_id)
            .await
            .unwrap_err();
        assert!(error.is_not_found(), "{nothing}: got {error:?}");
    }
    let error = store
        .set_sharing(board_id, SharingChange::On, "token", "not-a-user")
        .await
        .unwrap_err();
    assert!(
        matches!(&error, StoreError::InvalidId(refused) if refused == "not-a-user"),
        "got {error:?}"
    );
    let missing = store.get_sharing(board_id).await.unwrap_err();
    assert!(matches!(missing, StoreError::NoRows), "got {missing:?}");

    test.teardown().await;
}

/// Reading the state and writing the new one are one step: requests that share a board
/// at once all get the token that was stored, whichever of them it came from.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_that_share_a_board_at_once_agree_on_its_token() {
    const ROUNDS: usize = 25;
    const RACERS: usize = 6;
    let test = common::setup().await;
    let user_id = common::user(&test, "sharer").await;
    let team_id = common::team(&test).await;
    let boards = common::create_test_boards(&test, &team_id, &user_id, ROUNDS).await;

    for (round, board) in boards.iter().enumerate() {
        let mut racers = tokio::task::JoinSet::new();
        for racer in 0..RACERS {
            let store = test.store.clone();
            let (board_id, user_id) = (board.id.clone(), user_id.clone());
            racers.spawn(async move {
                let offered = format!("token-{round}-{racer}");
                store
                    .set_sharing(&board_id, SharingChange::On, &offered, &user_id)
                    .await
                    .expect("the board is shared")
                    .token
            });
        }
        let answered = racers.join_all().await;

        let stored = test.get_sharing(&board.id).await.expect("it is read").token;
        assert!(
            answered.iter().all(|token| *token == stored),
            "round {round}: all are answered the stored token {stored}: {answered:?}"
        );
    }

    test.teardown().await;
}
