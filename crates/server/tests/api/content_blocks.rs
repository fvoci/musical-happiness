//! Moving a content block within its card: the route is gone.
//!
//! A card's content is its body (`docs/s6-design.md` 5.7), so there are no content
//! blocks to order and the route `moveContentBlock` was taken out with the card's
//! `contentOrder` (`docs/contract-pending.md` 1). The ports of
//! `server/integrationtests/content_blocks_test.go` and
//! `server/app/content_blocks_test.go` went with it; `crates/server/PORTING.md` lists
//! them.

use fvoci_model::BoardType;

use crate::common::{Setup, TEST_TEAM_ID};

/// The route of `moveContentBlock` answers 404 whoever asks and whatever it names.
#[tokio::test]
async fn moving_a_content_block_is_no_route() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    for position in ["before", "after"] {
        let response = ts
            .client
            .post_empty(&format!(
                "/content-blocks/{}/moveto/{position}/{}",
                board.id, board.id
            ))
            .await;
        response.check_status(reqwest::StatusCode::NOT_FOUND);
    }
    ts.teardown().await;
}
