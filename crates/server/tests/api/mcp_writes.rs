//! What an agent writes through the MCP server: as the routes write it, kept as the
//! agent's version after the body before, added to a body people type in without
//! taking what they typed, and refused where the routes refuse it.

use serde_json::{Value, json};

use crate::api_tokens::agent_writing;
use crate::body_versions::versions;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS};
use crate::mcp::{agent_of, call, called, refused};
use crate::rooms::{QUIET, paragraphs, two_in_a_room};

/// Whether `text` says each of `words`.
#[track_caller]
fn check_says(text: &Value, words: &[&str]) {
    let text = text
        .as_str()
        .map_or_else(|| text.to_string(), str::to_owned);
    for word in words {
        assert!(text.contains(word), "{word} in {text}");
    }
}

/// Whether one of `kept` is the agent's, named by its token.
#[track_caller]
fn check_an_agents_version(kept: &[Value]) {
    assert!(
        kept.iter()
            .any(|version| version["cause"] == "agent" && version["agentTokenName"] == "agent"),
        "{kept:?}"
    );
}

#[tokio::test]
async fn markdown_added_to_a_card_people_type_in_keeps_what_they_typed_and_is_the_agents() {
    let th = Setup::native().start().await.init_basic().await;
    let (table, mut one, mut two) = two_in_a_room(&th, &paragraphs(&["before"])).await;
    let epochs = (one.epoch, two.epoch);
    let unsent = one.editor().append_paragraph("typed by one");
    let agent = agent_writing(&th, &th.client).await;
    let card = |more: Value| {
        let mut arguments = json!({"workspaceId": TEST_WORKSPACE_ID, "cardId": table.card_id});
        arguments
            .as_object_mut()
            .expect("an object")
            .extend(more.as_object().expect("an object").clone());
        arguments
    };

    // Written whole, the body would lose what they typed.
    let read: Value = th
        .client
        .get(&format!("{TEST_WS}/cards/{}", table.card_id))
        .await
        .ok_json();
    let problem = refused(
        &agent,
        "save_card",
        &card(json!({"bodyMarkdown": "whole", "readAt": read["updatedAt"]})),
    )
    .await;
    assert_eq!(problem["reason"], "room_has_active_editors", "{problem}");

    let added = called(
        &agent,
        "save_card",
        &card(json!({"appendMarkdown": "From the **agent**"})),
    )
    .await;
    check_says(&added["bodyMarkdown"], &["before", "agent"]);
    while one.step(QUIET).await {}
    while two.step(QUIET).await {}
    assert_eq!(
        (one.epoch, two.epoch),
        epochs,
        "nobody loads the document anew"
    );
    assert!(one.editor.is_some() && two.editor.is_some(), "no docReset");
    one.send_update(&unsent).await;
    one.text("docAck").await;
    while two.step(QUIET).await {}
    assert_eq!(one.body(), two.body(), "the editors hold one document");
    th.app
        .collab()
        .close(&th.app, &table.card_id)
        .await
        .expect("closed");
    let stored = called(
        &agent,
        "get_item",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "card", "id": table.card_id}),
    )
    .await;
    check_says(
        &stored["item"]["bodyMarkdown"],
        &["before", "typed by one", "agent"],
    );
    check_an_agents_version(&versions(&th.client, &table.card_id).await);

    // The fields first, then what is added; and never both bodies at once.
    let renamed = called(
        &agent,
        "save_card",
        &card(json!({"title": "renamed", "appendMarkdown": "and more"})),
    )
    .await;
    assert_eq!(renamed["title"], "renamed", "{renamed}");
    check_says(&renamed["bodyMarkdown"], &["agent", "and more"]);
    let params = json!({"name": "save_card", "arguments":
        card(json!({"bodyMarkdown": "a", "appendMarkdown": "b", "readAt": 1}))});
    let answer: Value = agent
        .post(
            "/mcp",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": params}),
        )
        .await
        .ok_json();
    assert_eq!(answer["error"]["code"], -32_602, "{answer}");

    th.teardown().await;
}

/// Makes a wiki or course of `kind` in the test workspace as `client`.
async fn container(client: &Client, kind: &str, visibility: &str) -> String {
    let made: Value = client
        .post(
            &format!("{TEST_WS}/{kind}"),
            &json!({"visibility": visibility, "title": kind}),
        )
        .await
        .created_json();
    made["id"].as_str().expect("an id").to_owned()
}

#[tokio::test]
async fn a_page_is_made_written_and_added_to_as_the_agents_and_commented_on() {
    let th = Setup::native().start().await.init_basic().await;
    let wiki = container(&th.client, "wikis", "private").await;
    let agent = agent_writing(&th, &th.client).await;
    let page = |more: Value| {
        let mut arguments =
            json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "page", "containerId": wiki});
        arguments
            .as_object_mut()
            .expect("an object")
            .extend(more.as_object().expect("an object").clone());
        arguments
    };

    let made = called(
        &agent,
        "save_page_or_unit",
        &page(json!({"title": "Notes", "bodyMarkdown": "# Notes\n\nfirst"})),
    )
    .await;
    assert_eq!(made["title"], "Notes", "{made}");
    check_says(&made["bodyMarkdown"], &["Notes", "first"]);
    let page_id = made["id"].as_str().expect("an id").to_owned();
    let added = called(
        &agent,
        "save_page_or_unit",
        &page(json!({"id": page_id, "appendMarkdown": "second"})),
    )
    .await;
    check_says(&added["bodyMarkdown"], &["first", "second"]);
    let kept: Vec<Value> = th
        .client
        .get(&format!(
            "{TEST_WS}/wikis/{wiki}/pages/{page_id}/body-versions"
        ))
        .await
        .ok_json();
    check_an_agents_version(&kept);
    // Written whole as it was read: refused once it changed since.
    let problem = refused(
        &agent,
        "save_page_or_unit",
        &page(json!({"id": page_id, "bodyMarkdown": "over", "readAt": made["updatedAt"]})),
    )
    .await;
    assert_eq!(problem["status"], 409, "{problem}");

    let comment = json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "page",
                         "containerId": wiki, "itemId": page_id});
    let mut posted = comment.clone();
    posted["markdown"] = json!("Read it, **twice**.");
    let written = called(&agent, "post_comment", &posted).await;
    assert_eq!(written["agentTokenName"], "agent", "{written}");
    check_says(&written["markdown"], &["twice"]);
    let listed = called(&agent, "list_comments", &comment).await;
    assert_eq!(listed["comments"][0]["id"], written["id"], "{listed}");
    let by_route: Vec<Value> = th
        .client
        .get(&format!("{TEST_WS}/wikis/{wiki}/pages/{page_id}/comments"))
        .await
        .ok_json();
    assert_eq!(by_route[0]["fields"]["agentTokenName"], "agent");
    let mut empty = comment.clone();
    empty["markdown"] = json!("   ");
    let problem = refused(&agent, "post_comment", &empty).await;
    assert_eq!(problem["status"], 400, "{problem}");

    th.teardown().await;
}

#[tokio::test]
async fn progress_is_recorded_in_a_course_the_caller_joined_and_refused_in_one_they_did_not() {
    let th = Setup::native().start().await.init_basic().await;
    let mine = container(&th.client, "courses", "private").await;
    let theirs = container(&th.client2, "courses", "open").await;
    let their_unit: Value = th
        .client2
        .post(
            &format!("{TEST_WS}/courses/{theirs}/units"),
            &json!({"kind": "unit", "title": "Theirs"}),
        )
        .await
        .created_json();
    let their_unit = their_unit["id"].as_str().expect("an id");
    let agent = agent_writing(&th, &th.client).await;
    let reader = agent_of(&th, &th.client, "read", &[TEST_WORKSPACE_ID]).await;

    let made = called(
        &agent,
        "save_page_or_unit",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "unit", "containerId": mine,
                "title": "Week 1", "bodyMarkdown": "Read chapter one.",
                "progress": {"completed": true}}),
    )
    .await;
    check_says(&made["bodyMarkdown"], &["chapter one"]);
    assert!(made["myProgress"]["completedAt"].is_i64(), "{made}");
    let outline = called(
        &reader,
        "get_outline",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "containerId": mine}),
    )
    .await;
    assert_eq!(outline["kind"], "course", "{outline}");
    assert!(outline["units"][0]["updatedAt"].is_i64(), "{outline}");
    assert_eq!(outline["myProgress"].as_array().map(Vec::len), Some(1));

    // Open to them, and not joined: read, with no progress of theirs, and none recorded.
    let open = called(
        &reader,
        "get_outline",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "containerId": theirs}),
    )
    .await;
    assert_eq!(open["myProgress"], json!([]), "{open}");
    let problem = refused(
        &agent,
        "save_page_or_unit",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "unit", "containerId": theirs,
                "id": their_unit, "progress": {"completed": true}}),
    )
    .await;
    assert_eq!(problem["status"], 403, "{problem}");
    let result = call(
        &reader,
        "get_item",
        &json!({"workspaceId": TEST_WORKSPACE_ID, "kind": "unit", "containerId": theirs,
                "id": their_unit}),
    )
    .await;
    assert_eq!(result["isError"], false, "{result}");

    th.teardown().await;
}
