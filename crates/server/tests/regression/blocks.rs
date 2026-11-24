//! Scenarios: blocks, and cards with their content blocks.
#![expect(
    clippy::too_many_lines,
    reason = "a scenario is one request sequence; cutting it up would hide the order it pins"
)]

use serde_json::{Value, json};

use crate::fixtures::{add_member, block, new_blocks, new_board};
use crate::harness::ROOT_API;
use crate::harness::{API, Body, Session, id_by_title};

pub(crate) async fn blocks(s: &mut Session) {
    let [alice, bob, carol] = s.users("blocks", ["alice", "bob", "carol"]).await;
    let board = new_board(s, &alice, "Blocks", "private", json!({})).await;
    add_member(s, &alice, &board, &bob, "commenter").await;
    let path = format!("{ROOT_API}/boards/{board}/blocks");

    s.get("list: empty", &path).user(&alice).await;
    let created = new_blocks(
        s,
        &alice,
        &board,
        json!([
            block(
                "card1",
                &board,
                "card",
                "First card",
                "",
                json!({"icon": "x", "contentOrder": ["text1"]})
            ),
            block(
                "text1",
                &board,
                "attachment",
                "Some text",
                "card1",
                json!({})
            ),
            block(
                "view1",
                &board,
                "view",
                "Board view",
                "",
                json!({"viewType": "board"})
            ),
        ]),
        "insert blocks",
    )
    .await;
    let card = id_by_title(&created, "First card");
    let text = id_by_title(&created, "Some text");
    let view = id_by_title(&created, "Board view");

    s.get("list", &path).user(&alice).await;
    s.get("list: by type", &format!("{path}?type=card"))
        .user(&alice)
        .await;
    s.get("list: by parent", &format!("{path}?parentId={card}"))
        .user(&alice)
        .await;
    s.get(
        "list: by parent and type",
        &format!("{path}?parentId={card}&type=attachment"),
    )
    .user(&alice)
    .await;
    s.get(
        "list: the old name of the parameter is no filter",
        &format!("{path}?parent_id={card}&type=attachment"),
    )
    .user(&alice)
    .await;
    s.get("list: unknown type", &format!("{path}?type=h1"))
        .user(&alice)
        .await;
    s.get("list: all", &format!("{path}?all=true"))
        .user(&alice)
        .await;
    s.get("list: one block", &format!("{path}?blockId={card}"))
        .user(&alice)
        .await;
    s.get(
        "list: unknown block id",
        &format!("{path}?blockId=unknown-block"),
    )
    .user(&alice)
    .await;
    s.get("list: paged", &format!("{path}?page=0&perPage=2"))
        .user(&alice)
        .unrecorded()
        .await;
    s.get("list: commenter", &path).user(&bob).await;
    s.get("list: non-member", &path).user(&carol).await;
    s.get("list: anonymous", &path).await;
    s.get(
        "list: unknown board",
        &format!("{ROOT_API}/boards/unknown-board/blocks"),
    )
    .user(&alice)
    .await;

    s.post("insert: empty list", &path)
        .json(json!([]))
        .user(&alice)
        .await;
    s.post("insert: malformed body", &path)
        .raw(b"{")
        .user(&alice)
        .await;
    s.post("insert: no type", &path)
        .json(json!([{"id": "x1", "containerId": board, "createAt": 1, "updateAt": 1}]))
        .user(&alice)
        .await;
    s.post("insert: no createAt", &path)
        .json(json!([{"id": "x1", "containerId": board, "type": "card", "updateAt": 1}]))
        .user(&alice)
        .await;
    s.post("insert: no updateAt", &path)
        .json(json!([{"id": "x1", "containerId": board, "type": "card", "createAt": 1}]))
        .user(&alice)
        .await;
    s.post("insert: other board id", &path)
        .json(json!([block(
            "x1",
            "another-board",
            "card",
            "x",
            "",
            json!({})
        )]))
        .user(&alice)
        .await;
    s.post("insert: long title", &path)
        .json(json!([block(
            "x1",
            &board,
            "card",
            &"t".repeat(65_536),
            "",
            json!({})
        )]))
        .user(&alice)
        .await;
    s.post("insert: non-member", &path)
        .json(json!([block("x1", &board, "card", "x", "", json!({}))]))
        .user(&carol)
        .await;
    s.post("insert: commenter inserts a card", &path)
        .json(json!([block("x1", &board, "card", "x", "", json!({}))]))
        .user(&bob)
        .await;
    s.post("insert: commenter inserts a comment", &path)
        .json(json!([block(
            "cm1",
            &board,
            "comment",
            "A comment",
            &card,
            json!({})
        )]))
        .user(&bob)
        .await;
    s.post("insert: keeping ids", &format!("{path}?disableNotify=true"))
        .json(json!([block(
            "x2",
            &board,
            "card",
            "Quiet card",
            "",
            json!({})
        )]))
        .user(&alice)
        .await;

    s.patch("patch", &format!("{path}/{card}"))
        .json(json!({
            "title": "Renamed card",
            "updatedFields": {"icon": "y", "extra": [1, 2]},
            "deletedFields": ["contentOrder"],
        }))
        .user(&alice)
        .await;
    s.patch("patch: commenter", &format!("{path}/{card}"))
        .json(json!({"title": "hijack"}))
        .user(&bob)
        .await;
    s.patch("patch: non-member", &format!("{path}/{card}"))
        .json(json!({"title": "hijack"}))
        .user(&carol)
        .await;
    s.patch("patch: unknown block", &format!("{path}/unknown-block"))
        .json(json!({"title": "x"}))
        .user(&alice)
        .await;
    s.patch("patch: malformed body", &format!("{path}/{card}"))
        .raw(b"{")
        .user(&alice)
        .await;
    s.patch("patch: long title", &format!("{path}/{card}"))
        .json(json!({"title": "t".repeat(65_536)}))
        .user(&alice)
        .await;
    s.patch("patch several", &path)
        .json(json!({
            "blockIds": [card, text],
            "blockPatches": [{"title": "Card again"}, {"title": "Text again"}],
        }))
        .user(&alice)
        .await;
    s.patch("patch several: unknown block", &path)
        .json(json!({
            "blockIds": [card, "unknown-block"],
            "blockPatches": [{"title": "a"}, {"title": "b"}],
        }))
        .user(&alice)
        .await;
    s.patch("patch several: commenter", &path)
        .json(json!({"blockIds": [card], "blockPatches": [{"title": "hijack"}]}))
        .user(&bob)
        .await;
    s.get("list after patches", &path).user(&alice).await;

    s.post("duplicate", &format!("{path}/{card}/duplicate"))
        .user(&alice)
        .await;
    s.post(
        "duplicate as template",
        &format!("{path}/{card}/duplicate?asTemplate=true"),
    )
    .user(&alice)
    .await;
    s.post("duplicate: commenter", &format!("{path}/{card}/duplicate"))
        .user(&bob)
        .await;
    s.post(
        "duplicate: unknown block",
        &format!("{path}/unknown-block/duplicate"),
    )
    .user(&alice)
    .await;

    s.delete("delete: commenter", &format!("{path}/{view}"))
        .user(&bob)
        .await;
    s.delete("delete: non-member", &format!("{path}/{view}"))
        .user(&carol)
        .await;
    s.delete("delete", &format!("{path}/{view}"))
        .user(&alice)
        .await;
    s.delete("delete: again", &format!("{path}/{view}"))
        .user(&alice)
        .await;
    s.delete("delete: unknown block", &format!("{path}/unknown-block"))
        .user(&alice)
        .await;
    s.get("list: views after delete", &format!("{path}?type=view"))
        .user(&alice)
        .await;
    s.post("undelete: commenter", &format!("{path}/{view}/undelete"))
        .user(&bob)
        .await;
    s.post("undelete", &format!("{path}/{view}/undelete"))
        .user(&alice)
        .await;
    s.post("undelete: again", &format!("{path}/{view}/undelete"))
        .user(&alice)
        .await;
    s.post(
        "undelete: unknown block",
        &format!("{path}/unknown-block/undelete"),
    )
    .user(&alice)
    .await;
    s.get("list: views after undelete", &format!("{path}?type=view"))
        .user(&alice)
        .await;

    s.delete("delete a card with content", &format!("{path}/{card}"))
        .user(&alice)
        .await;
    s.get(
        "list: children of the deleted card",
        &format!("{path}?parentId={card}"),
    )
    .user(&alice)
    .await;

    // Blocks without identifiers, and a request that has the children before their
    // parents: each block gets an identifier of its own, and the list has the blocks
    // of a request in the order the request had them in.
    let second = new_board(s, &alice, "Order of a request", "private", json!({})).await;
    let second_path = format!("{ROOT_API}/boards/{second}/blocks");
    s.post("insert without identifiers", &second_path)
        .json(json!([
            {"containerId": second, "type": "card", "title": "Nameless one", "createAt": 1, "updateAt": 1},
            {"containerId": second, "type": "card", "title": "Nameless two", "createAt": 1, "updateAt": 1},
        ]))
        .user(&alice)
        .await;
    new_blocks(
        s,
        &alice,
        &second,
        json!([
            block("t1", &second, "attachment", "Text of A", "a", json!({})),
            block(
                "a",
                &second,
                "card",
                "Card A",
                "",
                json!({"contentOrder": ["t1"]})
            ),
            block("t2", &second, "attachment", "Text of B", "b", json!({})),
            block(
                "b",
                &second,
                "card",
                "Card B",
                "",
                json!({"contentOrder": ["t2"]})
            ),
        ]),
        "insert children before their parents",
    )
    .await;
    s.get("list: in the order of the requests", &second_path)
        .user(&alice)
        .await;
}

pub(crate) async fn cards(s: &mut Session) {
    let [alice, bob] = s.users("cards", ["alice", "bob"]).await;
    let board = new_board(s, &alice, "Cards", "private", json!({})).await;
    add_member(s, &alice, &board, &bob, "viewer").await;
    let path = format!("{ROOT_API}/boards/{board}/cards");

    let first = s
        .post("create", &path)
        .json(json!({
            "title": "Card one", "icon": "🃏", "contentOrder": ["a", "b"], "properties": {"p1": "v1"},
        }))
        .user(&alice)
        .await;
    s.post("create: quiet", &format!("{path}?disableNotify=true"))
        .json(json!({"title": "Card two"}))
        .user(&alice)
        .await;
    s.post("create: viewer", &path)
        .json(json!({"title": "x"}))
        .user(&bob)
        .await;
    s.post("create: malformed body", &path)
        .raw(b"{")
        .user(&alice)
        .await;
    s.post(
        "create: unknown board",
        &format!("{ROOT_API}/boards/unknown-board/cards"),
    )
    .json(json!({"title": "x"}))
    .user(&alice)
    .await;
    s.get("list", &path).user(&alice).await;
    s.get("list: viewer", &path).user(&bob).await;
    s.get("list: paged", &format!("{path}?page=0&perPage=1"))
        .user(&alice)
        .unrecorded()
        .await;
    // Pins that an invalid pagination parameter returns the error alone with no trailing body.
    let bad_page = s
        .get("list: bad page", &format!("{path}?page=x"))
        .user(&alice)
        .unrecorded()
        .await;
    let text = match &bad_page.body {
        Body::Json(value) => value.to_string(),
        Body::Text(text) => text.clone(),
        Body::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    };
    // The first JSON value of the body, and whether anything comes after it. As JSON,
    // so that what differs from request to request is normalized like in any other
    // answer.
    let mut values = serde_json::Deserializer::from_str(&text).into_iter::<Value>();
    let first_object = match values.next() {
        Some(Ok(value)) => value,
        _ => Value::String(text.clone()),
    };
    let what_follows = if text[values.byte_offset()..].trim().is_empty() {
        "alone"
    } else {
        "followed by more"
    };
    s.note(
        "list: bad page",
        &json!([bad_page.status, first_object, what_follows]),
    );
    s.get(
        "list: unknown board",
        &format!("{ROOT_API}/boards/unknown-board/cards"),
    )
    .user(&alice)
    .await;
    let card = first.string("id");
    let card_path = format!("{ROOT_API}/cards/{card}");
    s.get("get", &card_path).user(&alice).await;
    s.get("get: viewer", &card_path).user(&bob).await;
    s.get(
        "get: unknown card",
        &format!("{ROOT_API}/cards/unknown-card"),
    )
    .user(&alice)
    .await;
    s.patch("patch", &card_path)
        .json(json!({
            "title": "Card one, renamed", "icon": "🂡", "contentOrder": ["b", "a"],
            "updatedProperties": {"p2": "v2"},
        }))
        .user(&alice)
        .await;
    s.patch("patch: viewer", &card_path)
        .json(json!({"title": "hijack"}))
        .user(&bob)
        .await;
    s.patch(
        "patch: unknown card",
        &format!("{ROOT_API}/cards/unknown-card"),
    )
    .json(json!({"title": "x"}))
    .user(&alice)
    .await;
    s.patch("patch: malformed body", &card_path)
        .raw(b"{")
        .user(&alice)
        .await;
    s.get("get after patch", &card_path).user(&alice).await;

    // Blocks below the card. A content order is no member of a card any more: setting
    // one changes nothing, and the route that moved content blocks is gone.
    let content = new_blocks(
        s,
        &alice,
        &board,
        json!([
            block("t1", &board, "attachment", "one", &card, json!({})),
            block("t2", &board, "attachment", "two", &card, json!({})),
            block("t3", &board, "attachment", "three", &card, json!({})),
        ]),
        "insert content",
    )
    .await;
    let one = id_by_title(&content, "one");
    let two = id_by_title(&content, "two");
    let three = id_by_title(&content, "three");
    s.patch("set content order", &card_path)
        .json(json!({"contentOrder": [one, two, three]}))
        .user(&alice)
        .await;
    // The route that moved content blocks is gone: a card's content is its body.
    s.post(
        "move: the route is gone",
        &format!("{API}/content-blocks/{one}/moveto/after/{three}"),
    )
    .user(&alice)
    .await;
}
