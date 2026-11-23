//! Who comes to watch what, and who is told of what they may join: whoever may join an
//! open board is told of it and watches what they are given there until it is closed;
//! nobody comes to watch, or to know the identifier of, what they do not read; a card
//! written quietly (`disableNotify`) is watched by its writer alone; and who watched a
//! card or a page watches it again once it is back from the trash.

use fvoci_model::{BlockType, Card, ContainerType, SubscriberType, Subscription};
use fvoci_server::app::tend_notifications;
use fvoci_store::DbType;
use fvoci_store::testing::db_type_of_the_tests;
use serde_json::{Value, json};

use crate::blocks::{block_path, delete, undelete_path};
use crate::boards::patch_board;
use crate::common::{Client, HIDDEN_WORKSPACE_ID, Setup, TEST_WS, TestServer};
use crate::content_search::set_role;
use crate::notifications::{
    OWNER, board_with_owner, card_with, comment_on, kinds, mentioning, plain, told,
};
use crate::rooms::give;
use crate::search_pages::{new_page, new_wiki};
use crate::workspaces::another_organization;

/// What `client`'s user is listed as watching in the test workspace.
async fn listed(client: &Client) -> Vec<Subscription> {
    let user_id = client.me().await.id;
    client
        .get(&format!("{TEST_WS}/subscriptions/{user_id}"))
        .await
        .ok_json()
}

/// The blocks `client`'s user is listed as watching in the test workspace, sorted.
async fn watched(client: &Client) -> Vec<String> {
    let mut ids: Vec<String> = listed(client)
        .await
        .into_iter()
        .map(|one| one.block_id)
        .collect();
    ids.sort();
    ids
}

/// The watchers of a block as the database holds them, sorted.
async fn watchers(th: &TestServer, block_id: &str) -> Vec<String> {
    let mut ids: Vec<String> = th
        .app
        .store()
        .get_subscribers_for_block(block_id)
        .await
        .expect("the watchers are read")
        .into_iter()
        .map(|subscriber| subscriber.subscriber_id)
        .collect();
    ids.sort();
    ids
}

/// Gives the card to `user_id`, as `client`'s user.
async fn give_card(client: &Client, card: &Card, user_id: &str) {
    client
        .patch(
            &block_path(&card.board_id, &card.id),
            &json!({"updatedFields": {"properties": {OWNER: user_id}}}),
        )
        .await
        .check_no_content();
}

/// Makes the board open (`O`) or invited-only (`P`), as the first client's user.
async fn make(th: &TestServer, board_id: &str, board_type: &str) {
    patch_board(&th.client, board_id, &json!({"type": board_type}))
        .await
        .check_ok();
}

/// Whoever works in the workspace of an open board reads it once they join it: named
/// there before they do, they are told of it (which opens the board with its way to
/// join), and they watch the card they are given. A guest of the workspace, who may not
/// join it, is told nothing and watches nothing. Once the board is for the invited
/// only, what they were told says nothing, nothing more is told, and the card is not
/// among what they watch; open again, it is.
#[tokio::test]
async fn whoever_may_join_an_open_board_is_told_of_it_until_it_is_closed() {
    let th = Setup::native().start().await.init_basic().await;
    let board = board_with_owner(&th, ContainerType::Open).await;
    let worker = th.add_user("worker").await;
    let worker_id = worker.me().await.id;
    let guest = th.add_user("guest").await;
    let guest_id = guest.me().await.id;
    set_role(&th, &guest_id, "guest").await;

    let card = card_with(&th.client, &board.id, &mentioning(&[&worker_id, &guest_id])).await;
    give_card(&th.client, &card, &worker_id).await;
    give_card(&th.client, &card, &guest_id).await;
    let theirs = told(&th, &worker).await;
    assert_eq!(
        kinds(&theirs),
        (vec!["assignment", "mention"], 0),
        "{theirs:?}"
    );
    for one in &theirs {
        let subject = one.subject.as_ref().expect("the worker may read it");
        assert_eq!(
            (subject.container_id.as_str(), subject.block_id.as_str()),
            (board.id.as_str(), card.id.as_str())
        );
    }
    assert_eq!(
        watched(&worker).await,
        std::slice::from_ref(&card.id),
        "given, and watched"
    );
    assert_eq!(told(&th, &guest).await, [], "a guest may not join it");
    assert!(!watchers(&th, &card.id).await.contains(&guest_id));

    make(&th, &board.id, "P").await;
    comment_on(&th.client, &card, &mentioning(&[&worker_id])).await;
    let closed = told(&th, &worker).await;
    assert_eq!(
        kinds(&closed),
        (vec![], 2),
        "nothing more, and nothing of it: {closed:?}"
    );
    assert_eq!(watched(&worker).await, Vec::<String>::new(), "not listed");
    assert!(
        watchers(&th, &card.id).await.contains(&worker_id),
        "though kept, for the board to open again"
    );

    make(&th, &board.id, "O").await;
    assert_eq!(watched(&worker).await, std::slice::from_ref(&card.id));
    assert_eq!(
        kinds(&told(&th, &worker).await),
        (vec!["assignment", "mention"], 0)
    );
    th.teardown().await;
}

/// Nobody comes to watch, or to know the identifier of, what they do not read: a card
/// given to somebody of the workspace with no role on its invited-only board, to
/// somebody of another workspace, or of another organization, makes none of them a
/// watcher. A subscription to it made past the API is not listed to its subscriber,
/// who may still end it.
#[tokio::test]
async fn nobody_watches_or_is_listed_what_they_do_not_read() {
    let th = Setup::native().start().await.init_basic().await;
    let a = th.client.me().await;
    let board = board_with_owner(&th, ContainerType::Private).await;
    let outsider = th.add_user("outsider").await;
    let outsider_id = outsider.me().await.id;
    let elsewhere = th.stranger_in(HIDDEN_WORKSPACE_ID).await;
    let mut given = vec![outsider_id.clone(), elsewhere];
    if db_type_of_the_tests() == DbType::Postgres {
        let (theirs, _, their_board) = another_organization(&th).await;
        let owner = theirs
            .get_container(&their_board)
            .await
            .expect("their board")
            .created_by;
        given.push(owner);
    }

    let card = card_with(&th.client, &board.id, &Value::Null).await;
    for user in &given {
        give_card(&th.client, &card, user).await;
    }
    tend_notifications(&th.app).await;
    assert_eq!(
        watchers(&th, &card.id).await,
        std::slice::from_ref(&a.id),
        "its writer alone"
    );
    assert_eq!(told(&th, &outsider).await, []);

    th.app
        .store()
        .create_subscription(Subscription {
            block_type: BlockType::Card,
            block_id: card.id.clone(),
            subscriber_type: SubscriberType::User,
            subscriber_id: outsider_id.clone(),
            notified_at: 0,
            create_at: 0,
            delete_at: 0,
        })
        .await
        .expect("a subscription past the API");
    assert_eq!(watched(&outsider).await, Vec::<String>::new(), "not listed");
    outsider
        .delete(&format!(
            "{TEST_WS}/subscriptions/{}/{outsider_id}",
            card.id
        ))
        .await
        .check_no_content();
    assert_eq!(
        watchers(&th, &card.id).await,
        std::slice::from_ref(&a.id),
        "ended"
    );
    th.teardown().await;
}

/// A card written quietly (`disableNotify`), as the web writes a copy or a card made
/// again, tells nobody it names or is given to, and nobody but its writer comes to
/// watch it; a change made quietly gives it to somebody without telling them or making
/// them a watcher. Its writer is told of a comment on it, as of any card they wrote.
#[tokio::test]
async fn a_quiet_card_tells_nobody_and_its_writer_alone_watches_it() {
    let th = Setup::native().start().await.init_basic().await;
    let a = th.client.me().await;
    let b = th.client2.me().await;
    let c = th.add_user("third").await;
    let c_id = c.me().await.id;
    let board = board_with_owner(&th, ContainerType::Private).await;
    give(&th, &board.id, &b.id, "editor").await;
    give(&th, &board.id, &c_id, "viewer").await;

    let card: Card = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards?disableNotify=true", board.id),
            &json!({"title": "A copy", "body": mentioning(&[&b.id]), "properties": {OWNER: c_id}}),
        )
        .await
        .created_json();
    assert_eq!(told(&th, &th.client2).await, []);
    assert_eq!(told(&th, &c).await, []);
    assert_eq!(watchers(&th, &card.id).await, std::slice::from_ref(&a.id));

    let _: Card = th
        .client
        .patch(
            &format!("{TEST_WS}/cards/{}?disableNotify=true", card.id),
            &json!({"updatedProperties": {OWNER: b.id}}),
        )
        .await
        .ok_json();
    assert_eq!(told(&th, &th.client2).await, [], "given back quietly");
    assert_eq!(watchers(&th, &card.id).await, std::slice::from_ref(&a.id));

    comment_on(&th.client2, &card, &plain("Mine now?")).await;
    assert_eq!(kinds(&told(&th, &th.client).await), (vec!["comment"], 0));
    th.teardown().await;
}

/// Who watched a card or a page watches it again once it is back from the trash, from
/// when they first did: the subscriptions it took along come back, and a comment on
/// the card is told to them as before.
#[tokio::test]
async fn who_watched_a_card_or_a_page_watches_it_again_once_it_is_back() {
    let th = Setup::native().start().await.init_basic().await;
    let b = th.client2.me().await;
    let board = board_with_owner(&th, ContainerType::Private).await;
    give(&th, &board.id, &b.id, "editor").await;
    let card = card_with(&th.client, &board.id, &Value::Null).await;
    comment_on(&th.client2, &card, &plain("Watching")).await;
    let wiki = new_wiki(&th.client).await;
    let page = new_page(&th.client, &wiki, "", "A page", "text").await;
    let mut both = vec![card.id.clone(), page.id.clone()];
    both.sort();
    assert_eq!(watched(&th.client).await, both, "the writer of both");
    let before = listed(&th.client2).await;
    assert_eq!(before.len(), 1, "who commented");

    let as_block = fvoci_model::Block {
        id: card.id.clone(),
        container_id: board.id.clone(),
        ..fvoci_model::Block::default()
    };
    delete(&th.client, &as_block).await;
    th.client
        .delete(&format!("{TEST_WS}/wikis/{wiki}/pages/{}", page.id))
        .await
        .check_no_content();
    assert_eq!(watched(&th.client).await, Vec::<String>::new());
    assert_eq!(watched(&th.client2).await, Vec::<String>::new());

    th.client
        .post_empty(&undelete_path(&board.id, &card.id))
        .await
        .check_ok();
    th.client
        .post(
            &format!("{TEST_WS}/wikis/{wiki}/pages/{}/undelete", page.id),
            &json!({}),
        )
        .await
        .check_ok();
    assert_eq!(watched(&th.client).await, both, "back, both");
    assert_eq!(listed(&th.client2).await, before, "as it was made");

    comment_on(&th.client, &card, &plain("Back")).await;
    assert_eq!(kinds(&told(&th, &th.client2).await), (vec!["comment"], 0));
    th.teardown().await;
}
