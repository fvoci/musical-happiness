//! Reactions to blocks: there once and gone when taken off, at most twenty emoji on a
//! block, nothing of the block written, into the trash with it and back, and gone with
//! whoever leaves the organization.

use std::collections::HashMap;

use fvoci_model::{Block, BlockType, Emoji, REACTION_KINDS_PER_BLOCK, Reaction, new_id};
use fvoci_store::{Departure, Reacted, Store, StoreError};

use crate::common::{self, TestStore};

fn emoji(text: &str) -> Emoji {
    text.parse().expect("an emoji")
}

/// The reactions `emoji` of the users, in that order.
fn reaction(text: &str, user_ids: &[&str]) -> Reaction {
    Reaction {
        emoji: emoji(text),
        user_ids: user_ids.iter().map(|&id| id.to_owned()).collect(),
    }
}

/// A card on a board of a workspace that the users work in, and a comment on it.
async fn card_and_comment(store: &Store, users: &[&str]) -> (Block, Block) {
    let workspace_id = common::workspace_of(store, users).await;
    let boards = common::create_test_boards(store, &workspace_id, users[0], 1).await;
    let card = Block {
        id: new_id(),
        container_id: boards[0].id.clone(),
        block_type: BlockType::Card,
        ..Block::default()
    };
    let comment = Block {
        id: new_id(),
        container_id: boards[0].id.clone(),
        parent_id: card.id.clone(),
        block_type: BlockType::Comment,
        ..Block::default()
    };
    let mut blocks = [card, comment];
    common::insert_blocks(store, &mut blocks, users[0]).await;
    let [card, comment] = blocks;
    (card, comment)
}

async fn react(store: &Store, block_id: &str, user_id: &str, text: &str) -> Reacted {
    store
        .add_reaction(block_id, user_id, &emoji(text))
        .await
        .expect("the reaction is added")
}

async fn reactions(store: &Store, block_id: &str) -> Vec<Reaction> {
    store
        .reactions_of(&[block_id])
        .await
        .expect("the reactions are read")
        .remove(block_id)
        .unwrap_or_default()
}

async fn rows(store: &Store) -> i64 {
    store
        .system()
        .count_rows("reactions")
        .await
        .expect("counted")
}

#[tokio::test]
async fn a_reaction_is_there_once_and_goes_when_taken_off() {
    let test = common::setup().await;
    let (ann, bob) = (
        common::user(&test, "ann").await,
        common::user(&test, "bob").await,
    );
    let (card, comment) = card_and_comment(&test, &[&ann, &bob]).await;

    assert_eq!(react(&test, &comment.id, &ann, "🎉").await, Reacted::Added);
    assert_eq!(
        react(&test, &comment.id, &ann, "🎉").await,
        Reacted::AlreadyThere,
        "a second time adds nothing"
    );
    assert_eq!(react(&test, &comment.id, &bob, "🎉").await, Reacted::Added);
    assert_eq!(react(&test, &comment.id, &bob, "👍").await, Reacted::Added);
    assert_eq!(react(&test, &card.id, &bob, "👀").await, Reacted::Added);
    let both = test
        .reactions_of(&[&comment.id, &card.id, &new_id()])
        .await
        .expect("read");
    assert_eq!(
        both,
        HashMap::from([
            (
                comment.id.clone(),
                vec![reaction("🎉", &[&ann, &bob]), reaction("👍", &[&bob])]
            ),
            (card.id.clone(), vec![reaction("👀", &[&bob])]),
        ]),
        "by block, the blocks without any left out"
    );

    let taken_off = test.remove_reaction(&comment.id, &ann, &emoji("🎉")).await;
    assert!(taken_off.expect("taken off"), "there was one");
    let again = test.remove_reaction(&comment.id, &ann, &emoji("🎉")).await;
    assert!(!again.expect("asked"), "there is none any more");
    assert_eq!(
        reactions(&test, &comment.id).await,
        [reaction("🎉", &[&bob]), reaction("👍", &[&bob])]
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_block_is_answered_with_twenty_emoji_at_most() {
    let test = common::setup().await;
    let (ann, bob) = (
        common::user(&test, "ann").await,
        common::user(&test, "bob").await,
    );
    let (_, comment) = card_and_comment(&test, &[&ann, &bob]).await;
    let animals: Vec<String> = ('\u{1F400}'..='\u{1F43F}').map(String::from).collect();
    for animal in &animals[..REACTION_KINDS_PER_BLOCK] {
        assert_eq!(
            react(&test, &comment.id, &ann, animal).await,
            Reacted::Added
        );
    }
    let one_more = &animals[REACTION_KINDS_PER_BLOCK];
    assert_eq!(
        react(&test, &comment.id, &bob, one_more).await,
        Reacted::TooManyKinds,
        "a twenty-first emoji is refused"
    );
    assert_eq!(
        react(&test, &comment.id, &bob, &animals[0]).await,
        Reacted::Added,
        "one of the twenty is not"
    );
    let kinds = reactions(&test, &comment.id).await;
    assert_eq!(kinds.len(), REACTION_KINDS_PER_BLOCK);
    assert_eq!(kinds[0], reaction(&animals[0], &[&ann, &bob]));
    test.teardown().await;
}

#[tokio::test]
async fn a_reaction_writes_nothing_of_the_block() {
    let test = common::setup().await;
    let (ann, bob) = (
        common::user(&test, "ann").await,
        common::user(&test, "bob").await,
    );
    let (card, comment) = card_and_comment(&test, &[&ann, &bob]).await;
    let before = [
        test.get_block(&card.id).await.expect("the card"),
        test.get_block(&comment.id).await.expect("the comment"),
    ];
    react(&test, &card.id, &bob, "🚀").await;
    react(&test, &comment.id, &bob, "❤️").await;
    test.remove_reaction(&card.id, &bob, &emoji("🚀"))
        .await
        .expect("taken off");
    let after = [
        test.get_block(&card.id).await.expect("the card"),
        test.get_block(&comment.id).await.expect("the comment"),
    ];
    assert_eq!(after, before, "the blocks are as they were");
    test.teardown().await;
}

#[tokio::test]
async fn whoever_is_not_in_the_workspace_reacts_to_nothing() {
    let test = common::setup().await;
    let ann = common::user(&test, "ann").await;
    let stranger = common::user(&test, "stranger").await;
    let gone = common::user(&test, "gone").await;
    let (card, _) = card_and_comment(&test, &[&ann, &gone]).await;
    let refused = |result: Result<Reacted, StoreError>| match result {
        Err(StoreError::NotFound(_)) => "not found",
        Err(StoreError::UserNotFound(_)) => "no account in use",
        other => panic!("refused: {other:?}"),
    };
    assert_eq!(
        refused(test.add_reaction(&card.id, &stranger, &emoji("👍")).await),
        "not found"
    );
    assert_eq!(
        refused(test.add_reaction(&new_id(), &ann, &emoji("👍")).await),
        "not found"
    );
    test.execute_unchecked(&format!(
        "UPDATE users SET deleted_at = 1 WHERE id = '{gone}'"
    ))
    .await
    .expect("the account is put out of use");
    assert_eq!(
        refused(test.add_reaction(&card.id, &gone, &emoji("👍")).await),
        "no account in use"
    );
    assert_eq!(rows(&test).await, 0);
    test.teardown().await;
}

/// A card that goes into the trash takes the reactions to it and to its comment along,
/// and brings them back as they were: those of an account erased meanwhile come back
/// to nothing.
#[tokio::test]
async fn reactions_go_into_the_trash_with_the_block_and_come_back() {
    let test = common::setup().await;
    let ann = common::user(&test, "ann").await;
    let bob = common::user(&test, "bob").await;
    let erased = common::user(&test, "erased").await;
    let (card, comment) = card_and_comment(&test, &[&ann, &bob, &erased]).await;
    for (block, user, text) in [
        (&card.id, &bob, "👍"),
        (&card.id, &erased, "👍"),
        (&card.id, &ann, "🎉"),
        (&comment.id, &erased, "😄"),
        (&comment.id, &ann, "😄"),
    ] {
        react(&test, block, user, text).await;
    }
    let before = (
        reactions(&test, &card.id).await,
        reactions(&test, &comment.id).await,
    );

    test.delete_block(&card.id, &ann)
        .await
        .expect("to the trash");
    assert_eq!(rows(&test).await, 0, "they are in the trash with the card");
    test.undelete_block(&card.id, &ann)
        .await
        .expect("out of the trash");
    let back = (
        reactions(&test, &card.id).await,
        reactions(&test, &comment.id).await,
    );
    assert_eq!(back, before, "they come back as they were");

    test.delete_block(&card.id, &ann)
        .await
        .expect("to the trash again");
    test.execute_unchecked(&format!(
        "UPDATE users SET deleted_at = 1, erase_started_at = 1, erased_at = 1 \
         WHERE id = '{erased}'"
    ))
    .await
    .expect("the account is erased");
    test.undelete_block(&card.id, &ann)
        .await
        .expect("out of the trash");
    assert_eq!(
        (
            reactions(&test, &card.id).await,
            reactions(&test, &comment.id).await,
        ),
        (
            vec![reaction("👍", &[&bob]), reaction("🎉", &[&ann])],
            vec![reaction("😄", &[&ann])],
        ),
        "those of the erased account do not"
    );
    test.teardown().await;
}

/// Gives the user a stored password hash, so that leaving is not refused to an account
/// that could not sign in again.
async fn give_password(test: &TestStore, user_id: &str) {
    test.execute_unchecked(&format!(
        "UPDATE users SET password_hash = '$argon2id$stub' WHERE id = '{user_id}'"
    ))
    .await
    .expect("password set");
}

/// Whoever leaves the organization takes their reactions out of it, also those its
/// trash keeps; those of others stay.
#[tokio::test]
async fn whoever_leaves_the_organization_takes_their_reactions_along() {
    let test = common::setup().await;
    let founder = common::user(&test, "founder").await;
    let leaver = common::user(&test, "leaver").await;
    give_password(&test, &founder).await;
    give_password(&test, &leaver).await;
    let (card, comment) = card_and_comment(&test, &[&founder, &leaver]).await;
    for (block, user, text) in [
        (&card.id, &leaver, "👍"),
        (&card.id, &founder, "👍"),
        (&comment.id, &leaver, "👀"),
        (&comment.id, &founder, "🚀"),
    ] {
        react(&test, block, user, text).await;
    }
    test.delete_block(&comment.id, &founder)
        .await
        .expect("the comment to the trash");

    let left = test.leave_organization(&leaver).await.expect("asked");
    assert!(matches!(left, Departure::Left(_)), "they leave: {left:?}");

    assert_eq!(
        reactions(&test, &card.id).await,
        [reaction("👍", &[&founder])]
    );
    let kept = test
        .system()
        .texts_unchecked(&format!(
            "SELECT CAST(block AS TEXT) FROM deleted_blocks WHERE id = '{}'",
            comment.id
        ))
        .await
        .expect("read");
    assert!(
        kept.len() == 1 && !kept[0].contains(&leaver),
        "the trash keeps none of theirs: {kept:?}"
    );
    test.undelete_block(&comment.id, &founder)
        .await
        .expect("out of the trash");
    assert_eq!(
        reactions(&test, &comment.id).await,
        [reaction("🚀", &[&founder])]
    );
    test.teardown().await;
}
