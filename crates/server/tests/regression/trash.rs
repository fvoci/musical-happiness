//! Scenario: the trash. Deleting a block or a board, bringing it back, and what does
//! not come back.

use serde_json::json;

use crate::common::{ROOT_TEAM_ID, UNKNOWN_ID};
use crate::fixtures::{add_member, block, id_of, new_blocks, new_board};
use crate::harness::{API, Session};

#[expect(
    clippy::too_many_lines,
    reason = "one sequence of requests, in the order it is recorded"
)]
pub(crate) async fn trash(s: &mut Session) {
    let [alice, bob, carol, dave] = s.users("trash", ["alice", "bob", "carol", "dave"]).await;
    let board = new_board(s, &alice, "Trash", "P", json!({})).await;
    add_member(s, &alice, &board, &bob, "editor").await;
    add_member(s, &alice, &board, &carol, "viewer").await;
    let alice_id = s.user_id(&alice).await;
    let board_path = format!("{API}/boards/{board}");
    let blocks = format!("{board_path}/blocks");

    // A card with a comment and a text on it, a view next to it. Blocks below the card
    // name it as their parent, so the card is created first.
    let created = new_blocks(
        s,
        &alice,
        &board,
        json!([
            block("c1", &board, "card", "Card", "", json!({})),
            block(
                "v1",
                &board,
                "view",
                "View",
                "",
                json!({"viewType": "board"})
            ),
        ]),
        "insert a card and a view",
    )
    .await;
    let (card, view) = (id_of(&created[0]), id_of(&created[1]));
    let created = new_blocks(
        s,
        &alice,
        &board,
        json!([
            block("m1", &board, "comment", "Comment", &card, json!({})),
            block("t1", &board, "attachment", "Text", &card, json!({})),
        ]),
        "insert a comment and a text on the card",
    )
    .await;
    let text = id_of(&created[1]);
    let created = new_blocks(
        s,
        &alice,
        &board,
        json!([block(
            "d1",
            &board,
            "attachment",
            "Below the text",
            &text,
            json!({})
        )]),
        "insert a block below the text",
    )
    .await;
    let deep = id_of(&created[0]);

    // -- a block and what is below it ---------------------------------------------------

    s.post("subscribe to the card", &format!("{API}/subscriptions"))
        .json(json!({
            "blockType": "card", "blockId": card,
            "subscriberType": "user", "subscriberId": alice_id,
        }))
        .user(&alice)
        .await;
    s.delete("delete the card", &format!("{blocks}/{card}"))
        .user(&alice)
        .await;
    s.get("blocks: the card took everything below it", &blocks)
        .user(&alice)
        .await;
    s.get(
        "subscriptions: ended with the card",
        &format!("{API}/subscriptions/{alice_id}"),
    )
    .user(&alice)
    .await;
    s.post("undelete: viewer", &format!("{blocks}/{card}/undelete"))
        .user(&carol)
        .await;
    s.post("undelete: non-member", &format!("{blocks}/{card}/undelete"))
        .user(&dave)
        .await;
    s.post(
        "undelete: unknown block",
        &format!("{blocks}/{UNKNOWN_ID}/undelete"),
    )
    .user(&alice)
    .await;
    s.post(
        "undelete: a block that is not deleted",
        &format!("{blocks}/{view}/undelete"),
    )
    .user(&alice)
    .await;
    s.post(
        "undelete: a child brings back the whole deletion",
        &format!("{blocks}/{deep}/undelete"),
    )
    .user(&bob)
    .await;
    s.get("blocks: all back, with their parents", &blocks)
        .user(&alice)
        .await;
    s.post("undelete: again", &format!("{blocks}/{card}/undelete"))
        .user(&alice)
        .await;
    s.get(
        "subscriptions: not back with the card",
        &format!("{API}/subscriptions/{alice_id}"),
    )
    .user(&alice)
    .await;

    // -- a block deleted on its own, then what it was on ---------------------------------

    s.delete("delete the text", &format!("{blocks}/{text}"))
        .user(&alice)
        .await;
    s.delete("delete the card again", &format!("{blocks}/{card}"))
        .user(&alice)
        .await;
    s.post(
        "undelete the text: its card is in the trash",
        &format!("{blocks}/{text}/undelete"),
    )
    .user(&alice)
    .await;
    s.post("undelete the card", &format!("{blocks}/{card}/undelete"))
        .user(&alice)
        .await;
    s.get(
        "blocks on the card: the text is still deleted",
        &format!("{blocks}?parentId={card}"),
    )
    .user(&alice)
    .await;
    s.post("undelete the text", &format!("{blocks}/{text}/undelete"))
        .user(&alice)
        .await;
    s.get(
        "blocks on the card: the text is back",
        &format!("{blocks}?parentId={card}"),
    )
    .user(&alice)
    .await;

    // -- a board ------------------------------------------------------------------------

    s.delete("delete the view on its own", &format!("{blocks}/{view}"))
        .user(&alice)
        .await;
    s.delete("delete the board", &board_path).user(&alice).await;
    s.get("board: gone", &board_path).user(&alice).await;
    s.get(
        "members: gone with the board",
        &format!("{board_path}/members"),
    )
    .user(&alice)
    .await;
    s.get(
        "memberships of a member: none of the board",
        &format!("{API}/users/me/memberships"),
    )
    .user(&bob)
    .await;
    s.get(
        "boards of the team: not listed",
        &format!("{API}/teams/{ROOT_TEAM_ID}/boards"),
    )
    .user(&alice)
    .await;
    s.post(
        "undelete a block of the deleted board",
        &format!("{blocks}/{card}/undelete"),
    )
    .user(&alice)
    .await;
    s.post(
        "undelete the board: viewer",
        &format!("{board_path}/undelete"),
    )
    .user(&carol)
    .await;
    s.post(
        "undelete the board: editor",
        &format!("{board_path}/undelete"),
    )
    .user(&bob)
    .await;
    s.post(
        "undelete the board: never a member",
        &format!("{board_path}/undelete"),
    )
    .user(&dave)
    .await;
    s.post(
        "undelete: unknown board",
        &format!("{API}/boards/{UNKNOWN_ID}/undelete"),
    )
    .user(&alice)
    .await;
    s.post("undelete the board", &format!("{board_path}/undelete"))
        .user(&alice)
        .await;
    s.get("board: back", &board_path).user(&alice).await;
    s.get(
        "members: back with their roles",
        &format!("{board_path}/members"),
    )
    .user(&alice)
    .await;
    s.get("blocks: back, but for the view deleted before", &blocks)
        .user(&bob)
        .await;
    s.post(
        "undelete the board: again",
        &format!("{board_path}/undelete"),
    )
    .user(&alice)
    .await;
}
