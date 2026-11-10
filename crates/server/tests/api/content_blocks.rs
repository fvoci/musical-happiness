//! Moving a content block within its card: the route is gone.
//!
//! A card's content is its body (`docs/s6-design.md` 5.7), so there are no content
//! blocks to order and the route `moveContentBlock` was taken out with the card's
//! `contentOrder` (`docs/contract-pending.md` 1). The ports of
//! `server/integrationtests/content_blocks_test.go` and
//! `server/app/content_blocks_test.go` went with it; `crates/server/PORTING.md` lists
//! them.

use fvoci_model::{Block, BlockType, BoardType, new_id};

use crate::common::{Setup, TEST_TEAM_ID};

/// The route of `moveContentBlock` answers 404 for two blocks of one card, which it
/// moved (204) or refused (400) before.
#[tokio::test]
async fn moving_a_content_block_is_no_route() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let block = |id: &str, parent: &str, block_type: BlockType| Block {
        id: id.to_owned(),
        board_id: board.id.clone(),
        parent_id: parent.to_owned(),
        block_type,
        create_at: 1,
        update_at: 1,
        ..Block::default()
    };
    let (card, first, second) = (new_id(), new_id(), new_id());
    let created: Vec<Block> = ts
        .client
        .insert_blocks(
            &board.id,
            &[
                block(&card, &board.id, BlockType::Card),
                block(&first, &card, BlockType::Attachment),
                block(&second, &card, BlockType::Attachment),
            ],
        )
        .await
        .created_json();
    let [_, first, second] = &created[..] else {
        panic!("three blocks are made: {created:?}");
    };
    for position in ["before", "after"] {
        let response = ts
            .client
            .post_empty(&format!(
                "/content-blocks/{}/moveto/{position}/{}",
                first.id, second.id
            ))
            .await;
        response.check_status(reqwest::StatusCode::NOT_FOUND);
    }
    ts.teardown().await;
}
