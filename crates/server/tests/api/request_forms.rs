//! The forms requests are read in: a member that is `null` only where it may be, the
//! paging of lists from 0, and a block on its board that has no parent.

use fvoci_model::{Block, BlockType, ContainerType};
use reqwest::StatusCode;
use serde_json::json;

use crate::blocks::{
    blocks_of, blocks_path, create_board_in_store, insert, insert_one, new_block, titles,
};
use crate::common::{Setup, TEST_WORKSPACE_ID, TEST_WS};

/// A request member that is no `Option` refuses `null`, as it refuses a value of another
/// type; one that is (a block's `body`, a patch's `title`) takes it as none.
#[tokio::test]
async fn null_is_refused_where_the_contract_does_not_allow_it() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, ContainerType::Open).await;
    let path = blocks_path(&board.id);
    let block = |member: &str, value: serde_json::Value| {
        let mut block = json!({
            "id": "x1", "containerId": board.id, "type": "card", "title": "t",
            "createAt": 1, "updateAt": 1,
        });
        block[member] = value;
        json!([block])
    };

    for member in ["title", "fields", "parentId", "createAt", "type"] {
        let response = th.client.post(&path, &block(member, json!(null))).await;
        response.check_bad_request();
        assert_eq!(
            response.problem_type(),
            "/problems/bad-request",
            "a null {member}"
        );
    }
    assert!(
        blocks_of(&th.client, &board.id).await.is_empty(),
        "nothing was stored"
    );

    let made: Vec<Block> = th
        .client
        .post(&path, &block("body", json!(null)))
        .await
        .created_json();
    assert_eq!(made[0].body, None, "a null body is no body");

    let patch_path = format!("{path}/{}", made[0].id);
    for patch in [
        json!({"updatedFields": null}),
        json!({"deletedFields": null}),
    ] {
        th.client
            .patch(&patch_path, &patch)
            .await
            .check_bad_request();
    }
    let response = th.client.patch(&patch_path, &json!({"title": null})).await;
    assert!(
        response.status.is_success(),
        "a null title of a patch changes nothing: {}",
        response.text()
    );
    assert_eq!(blocks_of(&th.client, &board.id).await[0].title, "t");

    th.teardown().await;
}

/// A new card and a patch of a card refuse `null` for their members that are no
/// `Option`, and a patch takes it as none for those that are.
#[tokio::test]
async fn null_is_refused_by_cards_where_the_contract_does_not_allow_it() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, ContainerType::Open).await;
    let cards = format!("{TEST_WS}/boards/{}/cards", board.id);

    for member in ["title", "properties", "icon"] {
        let response = th.client.post(&cards, &json!({ member: null })).await;
        response.check_bad_request();
    }
    let card: serde_json::Value = th.client.post(&cards, &json!({})).await.created_json();
    let path = format!("{TEST_WS}/cards/{}", card["id"].as_str().expect("an id"));
    for member in ["updatedProperties", "deletedProperties"] {
        let response = th.client.patch(&path, &json!({ member: null })).await;
        response.check_bad_request();
    }
    let response = th
        .client
        .patch(&path, &json!({"title": null, "icon": null}))
        .await;
    assert!(response.status.is_success(), "{}", response.text());

    th.teardown().await;
}

/// `page` and `perPage` are whole numbers from 0, and `perPage` 0 is every item: a
/// negative one is refused at the parameter, on every list that takes them.
#[tokio::test]
async fn the_pages_of_a_list_are_counted_from_zero() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, ContainerType::Open).await;
    let cards: Vec<Block> = ["a", "b", "c"]
        .iter()
        .map(|title| new_block(&board.id, BlockType::Card, title))
        .collect();
    insert(&th.client, &board.id, &cards).await;
    let cards_path = format!("{TEST_WS}/boards/{}/cards", board.id);
    let count = |query: &str| {
        let path = format!("{cards_path}?{query}");
        let client = &th.client;
        async move {
            client
                .get(&path)
                .await
                .ok_json::<Vec<serde_json::Value>>()
                .len()
        }
    };

    assert_eq!(count("perPage=0").await, 3, "0 per page is every card");
    assert_eq!(count("page=5&perPage=0").await, 3, "whatever the page");
    assert_eq!(count("page=1&perPage=2").await, 1, "the second page of two");
    assert_eq!(
        count("page=9&perPage=2").await,
        0,
        "a page past the last is empty"
    );
    let largest = "page=2147483647&perPage=2147483647";
    assert_eq!(
        count(largest).await,
        0,
        "the last page there can be is empty"
    );

    let trash_path = format!("{TEST_WS}/boards/{}/trash/cards", board.id);
    for path in [&cards_path, &trash_path] {
        for (query, refused) in [("page=-1", "page"), ("perPage=-1", "perPage")] {
            let response = th.client.get(&format!("{path}?{query}")).await;
            response.check_bad_request();
            assert_eq!(
                response.refused_at(),
                format!("parameter {refused}"),
                "{path}?{query}"
            );
        }
        let response = th.client.get(&format!("{path}?page=-1&perPage=-2")).await;
        response.check_bad_request();
        assert_eq!(
            response.refused_places(),
            ["parameter page", "parameter perPage"],
            "both are told of"
        );
        let response = th.client.get(&format!("{path}?page=2147483648")).await;
        assert_eq!(
            response.refused_at(),
            "parameter page",
            "beyond what a page can be"
        );
    }

    th.teardown().await;
}

/// A block directly on its board has no parent: the board's identifier is no parent, as
/// the identifier of no block is none, when a block is made, when it is moved, and when
/// the blocks of a board are narrowed to those below one.
#[tokio::test]
async fn the_board_is_no_parent_of_its_blocks() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, ContainerType::Open).await;
    let path = blocks_path(&board.id);

    let on_board = Block {
        parent_id: board.id.clone(),
        ..new_block(&board.id, BlockType::Card, "under the board")
    };
    let response = th.client.insert_blocks(&board.id, &[on_board]).await;
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "{}",
        response.text()
    );
    assert_eq!(response.refused_at(), "/0/parentId");

    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;
    assert_eq!(card.parent_id, "", "a block on its board has no parent");
    let response = th
        .client
        .patch(
            &format!("{path}/{}", card.id),
            &json!({"parentId": board.id}),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/parentId");

    let below_board: Vec<Block> = th
        .client
        .get(&format!("{path}?parentId={}", board.id))
        .await
        .ok_json();
    assert!(
        titles(&below_board).is_empty(),
        "no block is below the board"
    );
    assert_eq!(titles(&blocks_of(&th.client, &board.id).await), ["card"]);

    th.teardown().await;
}

/// The lists of an organization, of a workspace's audit and of a wiki's trash read
/// `page` and `perPage` as every list does.
#[tokio::test]
async fn every_list_refuses_a_page_below_0_or_beyond_the_largest() {
    let th = Setup::native().start().await.init_basic().await;
    let org = crate::org_admin::org_of(&th).await;
    let made = th
        .client
        .post(&format!("{TEST_WS}/wikis"), &json!({"type": "P"}))
        .await;
    made.check_created();
    let wiki = made.value()["id"].as_str().expect("an id").to_owned();
    for path in [
        format!("/org/{org}/members"),
        format!("/org/{org}/audit"),
        format!("{TEST_WS}/audit"),
        format!("{TEST_WS}/wikis/{wiki}/trash/pages"),
        format!("{TEST_WS}/trash/containers"),
    ] {
        th.client
            .get(&format!("{path}?page=0&perPage=0"))
            .await
            .check_ok();
        for (query, refused) in [
            ("page=-1", "page"),
            ("perPage=-1", "perPage"),
            ("perPage=2147483648", "perPage"),
        ] {
            let response = th.client.get(&format!("{path}?{query}")).await;
            response.check_bad_request();
            assert_eq!(
                response.refused_at(),
                format!("parameter {refused}"),
                "{path}?{query}"
            );
        }
    }

    th.teardown().await;
}

/// The board is no parent in a batch of patches, nor where boards and blocks are made
/// together, where the blocks name their board by the request's own name for it.
#[tokio::test]
async fn the_board_is_no_parent_in_a_batch_or_with_new_boards() {
    let th = Setup::one_user().start().await;
    let board = create_board_in_store(&th, ContainerType::Open).await;
    let card = insert_one(&th.client, &new_block(&board.id, BlockType::Card, "card")).await;

    let response = th
        .client
        .patch(
            &blocks_path(&board.id),
            &json!({"blockIds": [card.id], "blockPatches": [{"parentId": board.id}]}),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/blockPatches/0/parentId");
    assert_eq!(blocks_of(&th.client, &board.id).await[0].parent_id, "");

    let response = th
        .client
        .post(
            &format!("{TEST_WS}/boards-and-blocks"),
            &json!({
                "containers": [{"id": "b", "workspaceId": crate::common::TEST_WORKSPACE_ID, "type": "P"}],
                "blocks": [{"id": "x", "containerId": "b", "parentId": "b", "type": "card",
                            "createAt": 1, "updateAt": 1}],
            }),
        )
        .await;
    response.check_bad_request();
    assert_eq!(response.refused_at(), "/blocks/0/parentId");

    th.teardown().await;
}

/// A category's `type` and `name` are strings: `null` is neither, and refused where a
/// blank `type` is the default.
#[tokio::test]
async fn null_is_refused_by_categories() {
    let th = Setup::one_user().start().await;
    let user = th.client.me().await;
    let path = format!("{TEST_WS}/categories");
    for (member, blank) in [("type", json!(null)), ("name", json!(null))] {
        let mut body =
            json!({"name": "Named", "userId": user.id, "workspaceId": TEST_WORKSPACE_ID});
        body[member] = blank;
        let response = th.client.post(&path, &body).await;
        response.check_bad_request();
        assert!(
            response.text().contains("invalid type: null"),
            "{member}: {}",
            response.text()
        );
    }
    let body =
        json!({"name": "Blank", "userId": user.id, "workspaceId": TEST_WORKSPACE_ID, "type": ""});
    th.client.post(&path, &body).await.check_created();

    th.teardown().await;
}
