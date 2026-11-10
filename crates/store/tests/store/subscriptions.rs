//! Port of `server/services/store/storetests/subscriptions.go`, followed by tests of what
//! the original suite does not cover.

use std::time::Duration;

use fvoci_model::{
    Block, BlockType, IdKind, SubscriberType, Subscription, SubscriptionError, User, new_id,
};
use fvoci_store::{Store, StoreError};

use crate::common;

/// `createTestUsers` of the original suite's `util.go`.
async fn create_test_users(store: &Store, num: usize) -> Vec<User> {
    let mut users = Vec::with_capacity(num);
    for index in 0..num {
        let mut user = User {
            id: new_id(IdKind::User),
            username: format!("mooncake.{index}"),
            email: format!("mooncake.{index}@example.com"),
            ..User::default()
        };
        store
            .create_user(&mut user)
            .await
            .expect("the user is created");
        users.push(user);
    }
    users
}

/// `createTestBlocks` of the original suite's `util.go`.
async fn create_test_blocks(store: &Store, user_id: &str, num: usize) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(num);
    for _ in 0..num {
        let mut block = Block {
            id: new_id(IdKind::Block),
            board_id: new_id(IdKind::Board),
            block_type: BlockType::Card,
            created_by: user_id.to_owned(),
            ..Block::default()
        };
        store
            .insert_block(&mut block, user_id)
            .await
            .expect("the block is inserted");
        blocks.push(block);
    }
    blocks
}

/// The subscription of `user` to `block` as the original suite builds it.
fn subscription(block: &Block, user: &User) -> Subscription {
    Subscription {
        block_type: block.block_type.clone(),
        block_id: block.id.clone(),
        subscriber_type: SubscriberType::User,
        subscriber_id: user.id.clone(),
        notified_at: 0,
        create_at: 0,
        delete_at: 0,
    }
}

async fn first_user(store: &Store) -> User {
    create_test_users(store, 1)
        .await
        .pop()
        .expect("one user was created")
}

async fn first_block(store: &Store, user_id: &str) -> Block {
    create_test_blocks(store, user_id, 1)
        .await
        .pop()
        .expect("one block was created")
}

/// `testCreateSubscription`.
#[tokio::test]
async fn create_subscription() {
    let test = common::setup().await;
    let store = &test.store;

    // "create subscriptions"
    {
        let users = create_test_users(store, 10).await;
        let blocks = create_test_blocks(store, &users[0].id, 50).await;

        for (i, user) in users.iter().enumerate() {
            for block in &blocks[..i] {
                let sub = subscription(block, user);
                let sub_new = store
                    .create_subscription(sub)
                    .await
                    .expect("create subscription should not error");

                assert_ne!(sub_new.notified_at, 0);
                assert_ne!(sub_new.create_at, 0);
                assert_eq!(sub_new.delete_at, 0);
            }
        }

        // ensure each user has the right number of subscriptions
        for (i, user) in users.iter().enumerate() {
            let subs = store
                .get_subscriptions(&user.id)
                .await
                .expect("get subscriptions should not error");
            assert_eq!(subs.len(), i);
        }
    }

    // "duplicate subscription"
    {
        let admin = first_user(store).await;
        let user = first_user(store).await;
        let block = first_block(store, &admin.id).await;

        let sub = subscription(&block, &user);
        let sub_new = store
            .create_subscription(sub)
            .await
            .expect("create subscription should not error");

        let sub = subscription(&block, &user);

        let sub_dup = store
            .create_subscription(sub)
            .await
            .expect("create duplicate subscription should not error");

        assert_eq!(sub_new.block_id, sub_dup.block_id);
        assert_eq!(sub_new.subscriber_id, sub_dup.subscriber_id);
    }

    // "invalid subscription"
    {
        let admin = first_user(store).await;
        let user = first_user(store).await;
        let block = first_block(store, &admin.id).await;

        // The original starts from a subscription whose subscriber type is the empty
        // string and sets it to "user" in its fourth step. `SubscriberType` has no such
        // state, so the type is "user" from the start and the fourth step repeats the
        // third.
        let mut sub = Subscription {
            block_type: BlockType::default(),
            block_id: String::new(),
            subscriber_type: SubscriberType::User,
            subscriber_id: String::new(),
            notified_at: 0,
            create_at: 0,
            delete_at: 0,
        };
        let assert_invalid = |result: Result<Subscription, StoreError>| {
            assert!(
                matches!(result, Err(StoreError::InvalidSubscription(_))),
                "invalid subscription should error: got {result:?}"
            );
        };

        assert_invalid(store.create_subscription(sub.clone()).await);

        sub.block_type = block.block_type.clone();
        assert_invalid(store.create_subscription(sub.clone()).await);

        sub.block_id.clone_from(&block.id);
        assert_invalid(store.create_subscription(sub.clone()).await);

        sub.subscriber_type = SubscriberType::User;
        assert_invalid(store.create_subscription(sub.clone()).await);

        sub.subscriber_id.clone_from(&user.id);
        let sub_new = store
            .create_subscription(sub)
            .await
            .expect("valid subscription should not error");

        let valid: Result<(), SubscriptionError> = sub_new.validate();
        assert_eq!(valid, Ok(()), "created subscription should be valid");
    }

    test.teardown().await;
}

/// `testDeleteSubscription`.
#[tokio::test]
async fn delete_subscription() {
    let test = common::setup().await;
    let store = &test.store;

    // "delete subscription"
    {
        let user = first_user(store).await;
        let block = first_block(store, &user.id).await;

        let sub = subscription(&block, &user);
        let sub_new = store
            .create_subscription(sub)
            .await
            .expect("create subscription should not error");

        // check the subscription exists
        let subs = store
            .get_subscriptions(&user.id)
            .await
            .expect("get subscriptions should not error");
        assert_eq!(subs.len(), 1);
        assert_eq!(sub_new.block_id, subs[0].block_id);
        assert_eq!(sub_new.subscriber_id, subs[0].subscriber_id);

        store
            .delete_subscription(&block.id, &user.id)
            .await
            .expect("delete subscription should not error");

        // check the subscription was deleted
        let subs = store
            .get_subscriptions(&user.id)
            .await
            .expect("get subscriptions should not error");
        assert!(subs.is_empty(), "got {subs:?}");
    }

    // "delete non-existent subscription"
    {
        let error = store
            .delete_subscription("bogus", "bogus")
            .await
            .expect_err("delete non-existent subscription should error");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error: got {error:?}"
        );
    }

    test.teardown().await;
}

/// `testUndeleteSubscription`.
#[tokio::test]
async fn undelete_subscription() {
    let test = common::setup().await;
    let store = &test.store;

    // "undelete subscription"
    let user = first_user(store).await;
    let block = first_block(store, &user.id).await;

    let sub = subscription(&block, &user);
    let sub_new = store
        .create_subscription(sub.clone())
        .await
        .expect("create subscription should not error");

    // check the subscription exists
    let subs = store
        .get_subscriptions(&user.id)
        .await
        .expect("get subscriptions should not error");
    assert_eq!(subs.len(), 1);
    assert_eq!(sub_new.block_id, subs[0].block_id);
    assert_eq!(sub_new.subscriber_id, subs[0].subscriber_id);

    store
        .delete_subscription(&block.id, &user.id)
        .await
        .expect("delete subscription should not error");

    // check the subscription was deleted
    let subs = store
        .get_subscriptions(&user.id)
        .await
        .expect("get subscriptions should not error");
    assert!(subs.is_empty(), "got {subs:?}");

    // re-create the subscription
    let sub_undeleted = store
        .create_subscription(sub)
        .await
        .expect("create subscription should not error");

    // check the undeleted subscription exists
    let subs = store
        .get_subscriptions(&user.id)
        .await
        .expect("get subscriptions should not error");
    assert_eq!(subs.len(), 1);
    assert_eq!(sub_undeleted.block_id, subs[0].block_id);
    assert_eq!(sub_undeleted.subscriber_id, subs[0].subscriber_id);

    test.teardown().await;
}

/// `testGetSubscription`.
#[tokio::test]
async fn get_subscription() {
    let test = common::setup().await;
    let store = &test.store;

    // "get subscription"
    {
        let user = first_user(store).await;
        let block = first_block(store, &user.id).await;

        let sub = subscription(&block, &user);
        let sub_new = store
            .create_subscription(sub)
            .await
            .expect("create subscription should not error");

        // make sure subscription can be fetched
        let sub = store
            .get_subscription(&block.id, &user.id)
            .await
            .expect("get subscription should not error");
        assert_eq!(sub_new, sub);
    }

    // "get non-existent subscription"
    {
        let error = store
            .get_subscription("bogus", "bogus")
            .await
            .expect_err("get non-existent subscription should error");
        assert!(
            error.is_not_found(),
            "Should be ErrNotFound compatible error: got {error:?}"
        );
    }

    test.teardown().await;
}

/// `testGetSubscriptions`.
#[tokio::test]
async fn get_subscriptions() {
    let test = common::setup().await;
    let store = &test.store;

    // "get subscriptions"
    {
        let author = first_user(store).await;
        let user = first_user(store).await;
        let blocks = create_test_blocks(store, &author.id, 50).await;

        for block in &blocks {
            let sub = subscription(block, &user);
            store
                .create_subscription(sub)
                .await
                .expect("create subscription should not error");
        }

        // ensure user has the right number of subscriptions
        let subs = store
            .get_subscriptions(&user.id)
            .await
            .expect("get subscriptions should not error");
        assert_eq!(subs.len(), blocks.len());

        // ensure author has no subscriptions
        let subs = store
            .get_subscriptions(&author.id)
            .await
            .expect("get subscriptions should not error");
        assert!(subs.is_empty(), "got {subs:?}");
    }

    // "get subscriptions for invalid user"
    {
        let subs = store
            .get_subscriptions("bogus")
            .await
            .expect("get subscriptions should not error");
        assert!(subs.is_empty(), "got {subs:?}");
    }

    test.teardown().await;
}

/// `testGetSubscribersForBlock`.
#[tokio::test]
async fn get_subscribers_for_block() {
    let test = common::setup().await;
    let store = &test.store;

    // "get subscribers for block"
    {
        let users = create_test_users(store, 50).await;
        let blocks = create_test_blocks(store, &users[0].id, 2).await;

        for user in &users {
            let sub = subscription(&blocks[1], user);
            store
                .create_subscription(sub)
                .await
                .expect("create subscription should not error");
        }

        // make sure block[1] has the right number of users subscribed
        let subs = store
            .get_subscribers_for_block(&blocks[1].id)
            .await
            .expect("get subscribers for block should not error");
        assert_eq!(subs.len(), 50);

        let count = store
            .get_subscribers_count_for_block(&blocks[1].id)
            .await
            .expect("get subscribers for block should not error");
        assert_eq!(count, 50);

        // make sure block[0] has zero users subscribed
        let subs = store
            .get_subscribers_for_block(&blocks[0].id)
            .await
            .expect("get subscribers for block should not error");
        assert!(subs.is_empty(), "got {subs:?}");

        let count = store
            .get_subscribers_count_for_block(&blocks[0].id)
            .await
            .expect("get subscribers for block should not error");
        assert_eq!(count, 0);
    }

    // "get subscribers for invalid block"
    {
        let subs = store
            .get_subscribers_for_block("bogus")
            .await
            .expect("get subscribers for block should not error");
        assert!(subs.is_empty(), "got {subs:?}");
    }

    test.teardown().await;
}

// The tests below are not in the original suite. They pin behavior of the original's
// standalone store that its suite leaves untested.

/// Subscribing again after unsubscribing revives the stored row: its creation time stays,
/// although the returned subscription carries the current time.
#[tokio::test]
async fn resubscribing_keeps_the_stored_creation_time() {
    let test = common::setup().await;
    let store = &test.store;

    let user = first_user(store).await;
    let block = first_block(store, &user.id).await;
    let sub = subscription(&block, &user);

    let first = store.create_subscription(sub.clone()).await.unwrap();
    assert_eq!(first.notified_at, first.create_at);
    assert_ne!(first.create_at, 0, "stamped with the current time");

    tokio::time::sleep(Duration::from_millis(5)).await;
    store
        .delete_subscription(&block.id, &user.id)
        .await
        .unwrap();
    let missing = store
        .get_subscription(&block.id, &user.id)
        .await
        .unwrap_err();
    assert_eq!(
        missing.to_string(),
        format!(
            "{{subscription BlockID={} SubscriberID={}}} not found",
            block.id, user.id
        )
    );
    // The deleted row still matches, so deleting it again succeeds.
    store
        .delete_subscription(&block.id, &user.id)
        .await
        .unwrap();

    let second = store.create_subscription(sub).await.unwrap();
    assert!(second.create_at > first.create_at, "returned: the new time");
    let stored = store.get_subscription(&block.id, &user.id).await.unwrap();
    assert_eq!(stored.create_at, first.create_at, "stored: the old time");
    assert_eq!(stored.notified_at, second.notified_at);
    assert_eq!(stored.delete_at, 0);
    assert_eq!(stored.block_type, BlockType::Card);
    assert_eq!(stored.subscriber_type, SubscriberType::User);

    test.teardown().await;
}

/// Subscribers come back ordered by the time they were last notified, and deleted
/// subscriptions are not counted.
#[tokio::test]
async fn subscribers_are_ordered_by_notification_time() {
    let test = common::setup().await;
    let store = &test.store;

    let users = create_test_users(store, 3).await;
    let block = first_block(store, &users[0].id).await;
    for user in users.iter().rev() {
        store
            .create_subscription(Subscription {
                subscriber_type: SubscriberType::Channel,
                ..subscription(&block, user)
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let subscribers = store.get_subscribers_for_block(&block.id).await.unwrap();
    let ids: Vec<&str> = subscribers
        .iter()
        .map(|subscriber| subscriber.subscriber_id.as_str())
        .collect();
    let expected: Vec<&str> = users.iter().rev().map(|user| user.id.as_str()).collect();
    assert_eq!(ids, expected, "earliest notified first");
    assert!(
        subscribers.iter().all(
            |subscriber| subscriber.subscriber_type == SubscriberType::Channel
                && subscriber.notified_at != 0
        ),
        "got {subscribers:?}"
    );

    store
        .delete_subscription(&block.id, &users[1].id)
        .await
        .unwrap();
    assert_eq!(
        store
            .get_subscribers_count_for_block(&block.id)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        store
            .get_subscribers_for_block(&block.id)
            .await
            .unwrap()
            .len(),
        2
    );

    test.teardown().await;
}
