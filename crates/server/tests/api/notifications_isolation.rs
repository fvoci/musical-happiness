//! Who is told, and who reads what they were told: one table of everybody a body may
//! name who does not read it (another organization, another workspace, a guest who
//! was not invited, somebody who left, a reader by link, an account that is gone,
//! one's own space), none of whom is told anything; and one of everything that stops
//! being read after somebody was told of it (the trash, what is gone for good, a
//! comment that went, a board or a workspace that was left), each of which then says
//! nothing.

use std::collections::HashMap;

use fvoci_model::{Block, BlockType, Container, ContainerType, Notification, new_id};
use fvoci_server::app::tend_notifications;
use fvoci_store::testing::db_type_of_the_tests;
use fvoci_store::{DbType, Paging, Store};
use serde_json::{Value, json};

use crate::blocks::{delete, undelete_path};
use crate::boards::{board_with_admin, delete_member, member, new_board};
use crate::common::{
    HIDDEN_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer, UNKNOWN_ID,
};
use crate::content_search::set_role;
use crate::notifications::{
    OWNER, board_with_owner, card_with, comment_on, kinds, mentioning, told, unread,
};
use crate::own_space::own_workspace;
use crate::rooms::{give, give_in};
use crate::search_pages::{new_page, new_wiki};
use crate::workspaces::another_organization;

/// How many notifications the user has, read past the API: of somebody who never
/// signs in, too.
async fn held_by(th: &TestServer, user_id: &str) -> usize {
    let all = Paging {
        page: 0,
        per_page: 1000,
    };
    th.app
        .store()
        .notifications_of_user(user_id, all)
        .await
        .expect("the notifications are read")
        .len()
}

/// How many rows the list of whom to tell has, of every organization.
async fn waiting(th: &TestServer) -> i64 {
    th.app
        .housekeeping()
        .store()
        .waiting_notification_events()
        .await
        .expect("counted")
}

/// Erases the account at once, as the housekeeping does one whose wait ran out.
async fn erase(th: &TestServer, user_id: &str) {
    th.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE users SET delete_at = 1, delete_after = 1 WHERE id = '{user_id}'"
        ))
        .await
        .expect("the account waits to be deleted");
    th.app.erase_due_accounts().await;
}

/// A card stored past the API on the board, written by `author`, whose body mentions
/// `users`: through whichever handle reaches the board.
async fn stored_card_mentioning(store: &Store, board_id: &str, author: &str, users: &[&str]) {
    let mut card = Block {
        id: new_id(),
        container_id: board_id.to_owned(),
        block_type: BlockType::Card,
        title: "Elsewhere".to_owned(),
        body: fvoci_model::Body::parse(mentioning(users)).expect("a body"),
        ..Block::default()
    };
    store
        .insert_block(&mut card, author)
        .await
        .expect("the card is stored");
}

/// Everybody a body may name. Those who read the card are told; nobody else is told
/// anything, whoever they are and whatever they were before.
#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "every way of not reading a card, side by side"
)]
async fn only_who_reads_a_card_is_told_of_being_named_on_it() {
    let th = Setup::native().start().await.init_basic().await;
    let a = th.client.me().await;
    let board = board_with_owner(&th, ContainerType::Private).await;
    let other_board = board_with_owner(&th, ContainerType::Private).await;

    // Who reads the card: a viewer, and a guest invited to this board.
    let viewer = th.client2.me().await.id;
    give(&th, &board.id, &viewer, "viewer").await;
    let invited_guest = th.add_user("invited").await.me().await.id;
    set_role(&th, &invited_guest, "guest").await;
    give(&th, &board.id, &invited_guest, "commenter").await;

    // Who does not.
    let other_guest = th.add_user("uninvited").await.me().await.id;
    set_role(&th, &other_guest, "guest").await;
    give(&th, &other_board.id, &other_guest, "editor").await;
    let former = th.add_user("former").await.me().await.id;
    give(&th, &board.id, &former, "editor").await;
    delete_member(&th.client, &member(&board.id, &former))
        .await
        .check_no_content();
    // Somebody of the workspace with no role on the board, which is shared by its
    // link: whoever has the link reads it, and is nobody it could name.
    let by_link = th.add_user("bylink").await.me().await.id;
    th.share(&board.id).await;
    let elsewhere = th.stranger_in(HIDDEN_WORKSPACE_ID).await;
    // Another organization, where the database holds more than one: its board, and
    // the user of its own who made it.
    let another = if db_type_of_the_tests() == DbType::Postgres {
        let (theirs, _, their_board) = another_organization(&th).await;
        let owner = theirs
            .get_container(&their_board)
            .await
            .expect("their board")
            .created_by;
        Some((theirs, their_board, owner))
    } else {
        None
    };
    let of_another_org = another
        .as_ref()
        .map_or_else(|| UNKNOWN_ID.to_owned(), |(_, _, owner)| owner.clone());
    let ghost = th.add_user("ghost").await.me().await.id;
    give(&th, &board.id, &ghost, "editor").await;
    erase(&th, &ghost).await;

    let read = [("viewer", &viewer), ("invited guest", &invited_guest)];
    let unread_by = [
        ("the writer", &a.id),
        ("a guest invited elsewhere", &other_guest),
        ("who left the board", &former),
        ("who has the link alone", &by_link),
        ("of another workspace", &elsewhere),
        ("of another organization", &of_another_org),
        ("an account that is gone", &ghost),
    ];
    let mut named: Vec<&str> = read.iter().map(|(_, user)| user.as_str()).collect();
    named.extend(unread_by.iter().map(|(_, user)| user.as_str()));
    named.push(UNKNOWN_ID);

    // Named in the body of a card and in a comment on it, and given the card: the
    // last one at a time, a person property holding one.
    let card = card_with(&th.client, &board.id, &mentioning(&named)).await;
    comment_on(&th.client, &card, &mentioning(&named)).await;
    for user in &named {
        th.client
            .patch(
                &crate::blocks::block_path(&board.id, &card.id),
                &json!({"updatedFields": {"properties": {OWNER: user}}}),
            )
            .await
            .check_no_content();
    }
    tend_notifications(&th.app).await;
    assert_eq!(waiting(&th).await, 0, "every row was worked off");
    for (who, user) in read {
        assert_eq!(held_by(&th, user).await, 3, "{who}");
    }
    for (who, user) in unread_by {
        assert_eq!(held_by(&th, user).await, 0, "{who}");
    }
    assert_eq!(held_by(&th, UNKNOWN_ID).await, 0, "nobody");

    // An open board is theirs to join, and not read until they do; an open wiki is
    // read by whoever works in the workspace.
    let open = board_with_owner(&th, ContainerType::Open).await;
    card_with(&th.client, &open.id, &mentioning(&[&by_link, &other_guest])).await;
    let wiki = th
        .client
        .post(&format!("{TEST_WS}/wikis"), &json!({"type": "O"}))
        .await
        .value()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let page = new_page(&th.client, &wiki, "", "A page", "text").await;
    th.client
        .patch(
            &format!("{TEST_WS}/wikis/{wiki}/pages/{}", page.id),
            &json!({"body": mentioning(&[&by_link, &other_guest, &elsewhere, &of_another_org, &ghost]),
                    "readAt": page.update_at}),
        )
        .await
        .check_ok();
    tend_notifications(&th.app).await;
    assert_eq!(
        held_by(&th, &by_link).await,
        1,
        "of the page of the open wiki, not of the card of the open board"
    );
    assert_eq!(held_by(&th, &other_guest).await, 0, "a guest reads neither");
    for (who, user) in [
        ("of another workspace", &elsewhere),
        ("of another organization", &of_another_org),
        ("an account that is gone", &ghost),
    ] {
        assert_eq!(held_by(&th, user).await, 0, "{who}, named in an open wiki");
    }

    // Written where the one named is not: in another workspace of the organization,
    // and in another organization.
    let hidden = board_with_admin(
        &th,
        new_board(HIDDEN_WORKSPACE_ID, ContainerType::Open, "Hidden"),
        &elsewhere,
    )
    .await;
    stored_card_mentioning(th.app.store(), &hidden.id, &elsewhere, &[&viewer, &a.id]).await;
    if let Some((theirs, their_board, owner)) = &another {
        stored_card_mentioning(theirs, their_board, owner, &[&viewer, &a.id]).await;
    }
    assert!(waiting(&th).await > 0, "they were noted, to be asked about");
    tend_notifications(&th.app).await;
    assert_eq!(waiting(&th).await, 0);
    assert_eq!(
        held_by(&th, &viewer).await,
        3,
        "nothing from where they are not"
    );
    assert_eq!(held_by(&th, &a.id).await, 0);

    // One's own space is one's own: whoever is named there is told nothing, and
    // nothing is so much as noted.
    let space = own_workspace(&th.client).await;
    let own: Container = th
        .client
        .create_board(&new_board(&space.id, ContainerType::Private, "Diary"))
        .await
        .created_json();
    let entry: Value = th
        .client
        .post(
            &format!("/w/{}/boards/{}/cards", space.id, own.id),
            &json!({"title": "Today", "body": mentioning(&[&viewer])}),
        )
        .await
        .created_json();
    assert!(entry["id"].is_string(), "{entry}");
    assert_eq!(waiting(&th).await, 0, "nothing is noted in one's own space");
    tend_notifications(&th.app).await;
    assert_eq!(held_by(&th, &viewer).await, 3);

    // The account of somebody who was told goes: so does what they were told.
    erase(&th, &invited_guest).await;
    assert_eq!(
        held_by(&th, &invited_guest).await,
        0,
        "gone with the account"
    );
    th.teardown().await;
}

/// What stops being read after somebody was told of it says nothing from then on: a
/// card in the trash (until it is back), a card gone for good, a comment that went, a
/// page of a wiki in the trash, and everything of a workspace that was left. What its
/// writer's account going leaves is the notification, by nobody.
#[tokio::test]
async fn what_its_reader_reads_no_more_says_nothing_of_itself() {
    let th = Setup::native().start().await.init_basic().await;
    let b = th.client2.me().await;
    let c = th.add_user("third").await;
    let c_id = c.me().await.id;
    let board = board_with_owner(&th, ContainerType::Private).await;
    for user in [&b.id, &c_id] {
        give(&th, &board.id, user, "editor").await;
    }
    let body = mentioning(&[&b.id]);
    let kept = card_with(&th.client, &board.id, &body).await;
    let trashed = card_with(&th.client, &board.id, &body).await;
    let purged = card_with(&th.client, &board.id, &body).await;
    let commented = card_with(&th.client, &board.id, &Value::Null).await;
    let comment = comment_on(&th.client, &commented, &body).await;
    let by_ghost = card_with(&c, &board.id, &body).await;
    let wiki = new_wiki(&th.client).await;
    give_in(&th, TEST_WORKSPACE_ID, &wiki, &b.id, "viewer").await;
    let page = new_page(&th.client, &wiki, "", "A page", "text").await;
    th.client
        .patch(
            &format!("{TEST_WS}/wikis/{wiki}/pages/{}", page.id),
            &json!({"body": body, "readAt": page.update_at}),
        )
        .await
        .check_ok();

    let theirs = told(&th, &th.client2).await;
    assert_eq!(kinds(&theirs), (vec!["mention"; 6], 0), "{theirs:?}");
    // Which notification is of which card or page: asked while B reads them all.
    let of: HashMap<String, String> = theirs
        .iter()
        .map(|told| {
            let subject = told.subject.as_ref().expect("read");
            (subject.block_id.clone(), told.id.clone())
        })
        .collect();
    let seen = |told: &[Notification], block_id: &str| {
        let id = &of[block_id];
        told.iter()
            .find(|told| told.id == *id)
            .expect("still listed")
            .subject
            .clone()
    };
    let block = |id: &str| Block {
        id: id.to_owned(),
        container_id: board.id.clone(),
        ..Block::default()
    };

    delete(&th.client, &block(&trashed.id)).await;
    delete(&th.client, &block(&purged.id)).await;
    th.app
        .store()
        .execute_unchecked(&format!(
            "DELETE FROM deleted_blocks WHERE id = '{}'",
            purged.id
        ))
        .await
        .expect("the card is gone for good");
    delete(&th.client, &comment).await;
    th.client
        .delete(&format!("{TEST_WS}/containers/{wiki}"))
        .await
        .check_no_content();
    erase(&th, &c_id).await;

    let now = told(&th, &th.client2).await;
    assert_eq!(now.len(), 6, "all are still listed");
    for (what, block_id) in [
        ("in the trash", &trashed.id),
        ("gone for good", &purged.id),
        ("whose comment went", &commented.id),
        ("of a wiki in the trash", &page.id),
    ] {
        assert_eq!(seen(&now, block_id), None, "a card or a page {what}");
    }
    assert!(seen(&now, &kept.id).is_some(), "the one that stays");
    let orphaned = seen(&now, &by_ghost.id).expect("the card is read as before");
    assert_eq!(
        (orphaned.actor_id.as_str(), orphaned.actor_name.as_str()),
        (c_id.as_str(), ""),
        "by somebody who is gone, and has no name"
    );
    assert_eq!(unread(&th.client2).await, 6);

    // Back from the trash: read again.
    th.client
        .post_empty(&undelete_path(&board.id, &trashed.id))
        .await
        .check_ok();
    let back = told(&th, &th.client2).await;
    assert!(
        seen(&back, &trashed.id).is_some(),
        "the card that came back"
    );

    // B leaves the workspace: nothing of it says anything any more.
    th.app
        .store()
        .remove_workspace_member(TEST_WORKSPACE_ID, &b.id, "")
        .await
        .expect("B is taken out of the workspace");
    let left = told(&th, &th.client2).await;
    assert_eq!(kinds(&left), (vec![], 6), "{left:?}");
    th.teardown().await;
}
