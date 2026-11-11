//! What a deletion takes along and brings back: [`Store::get_blocks_below`] before it,
//! [`Store::get_deletion`] after it.

use fvoci_model::{Block, BlockType, new_id};
use fvoci_store::Store;

use crate::common::{self, sorted_ids};

/// A block of the board below `parent_id` (none if empty).
fn block(id: &str, board_id: &str, parent_id: &str, block_type: BlockType) -> Block {
    Block {
        id: id.to_owned(),
        board_id: board_id.to_owned(),
        parent_id: parent_id.to_owned(),
        block_type,
        ..Block::default()
    }
}

/// The identifiers of the blocks, in their order.
fn ids(blocks: &[Block]) -> Vec<&str> {
    blocks.iter().map(|block| block.id.as_str()).collect()
}

#[tokio::test]
async fn a_deletion_is_the_block_with_all_below_it_the_block_first() {
    let test = common::setup().await;
    let store: &Store = &test.store;
    let user_id = common::user(store, "author").await;
    let team_id = common::team(store).await;
    let board = common::create_test_boards(store, &team_id, &user_id, 1)
        .await
        .remove(0);
    // The comment's identifier sorts after its reply's, so that the order of the
    // deletion is not that of the identifiers.
    let mut pair = [new_id(), new_id()];
    pair.sort();
    let [reply_id, comment_id] = pair;
    let card_id = new_id();
    let mut blocks = [
        block(&card_id, &board.id, "", BlockType::Card),
        block(&comment_id, &board.id, &card_id, BlockType::Comment),
        block(&reply_id, &board.id, &comment_id, BlockType::Comment),
    ];
    common::insert_blocks(store, &mut blocks, &user_id).await;

    let below = store.get_blocks_below(&card_id).await.expect("read");
    assert_eq!(ids(&below), sorted_ids(&blocks[1..]), "at any depth, by id");
    for nothing in [reply_id.as_str(), "no-such-block"] {
        let below = store.get_blocks_below(nothing).await.expect("read");
        assert!(below.is_empty(), "{nothing}: {below:?}");
    }

    store
        .delete_block(&comment_id, &user_id)
        .await
        .expect("deleted");
    for named in [&comment_id, &reply_id] {
        let deletion = store.get_deletion(named).await.expect("in the trash");
        assert_eq!(ids(&deletion), [comment_id.as_str(), reply_id.as_str()]);
    }
    let error = store.get_deletion(&card_id).await.expect_err("not deleted");
    assert!(error.is_not_found(), "{error:?}");

    // A later deletion is one of its own.
    store
        .delete_block(&card_id, &user_id)
        .await
        .expect("deleted");
    let deletion = store.get_deletion(&card_id).await.expect("in the trash");
    assert_eq!(ids(&deletion), [card_id.as_str()]);
    let deletion = store.get_deletion(&reply_id).await.expect("in the trash");
    assert_eq!(ids(&deletion), [comment_id.as_str(), reply_id.as_str()]);
}
