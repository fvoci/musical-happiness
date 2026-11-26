//! The versions what an agent writes leaves: the body before it, as of who wrote it, so
//! that the body without the agent's words can be gone back to, and the body after, as
//! the agent's, named by its token; and the comments it writes, named by it too.

use serde_json::{Value, json};

use crate::api_tokens::agent_writing;
use crate::body_versions::{lines, versions};
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS};
use crate::rooms::paragraphs;

/// The cause of each version, and the token named by it, the newest first.
fn causes(versions: &[Value]) -> Vec<(String, Option<String>)> {
    versions
        .iter()
        .map(|version| {
            (
                version["cause"].as_str().expect("a cause").to_owned(),
                version["agentTokenName"].as_str().map(str::to_owned),
            )
        })
        .collect()
}

fn agents(name: &str) -> (String, Option<String>) {
    ("agent".to_owned(), Some(name.to_owned()))
}

fn servers() -> (String, Option<String>) {
    ("auto".to_owned(), None)
}

#[tokio::test]
async fn what_an_agent_adds_or_writes_whole_is_kept_as_its_version_after_the_body_before() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th
        .create_board(TEST_WORKSPACE_ID, fvoci_model::ContainerVisibility::Open)
        .await;
    let card: Value = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "notes", "body": paragraphs(&["before"])}),
        )
        .await
        .created_json();
    let card_id = card["id"].as_str().expect("an id");
    let me = th.client.me().await.id;
    let agent = agent_writing(&th, &th.client).await;

    let added: Value = agent
        .post(
            &format!("{TEST_WS}/cards/{card_id}/body/markdown"),
            &json!({"markdown": "added"}),
        )
        .await
        .ok_json();
    let kept = versions(&th.client, card_id).await;
    assert_eq!(causes(&kept), [agents("agent"), servers()]);
    assert_eq!(kept[0]["editors"], json!([me]), "{kept:?}");
    assert_eq!(kept[0]["isCurrent"], true, "{kept:?}");
    assert_eq!(
        lines(&th.client, card_id, &kept[0]).await,
        ["before", "added"]
    );
    assert_eq!(lines(&th.client, card_id, &kept[1]).await, ["before"]);
    let read: Value = th
        .client
        .get(&format!(
            "{TEST_WS}/cards/{card_id}/body-versions/{}",
            kept[0]["version"]
        ))
        .await
        .ok_json();
    assert_eq!(read["agentTokenName"], "agent", "{read}");

    // Written whole, the body before is the agent's version already.
    let whole: Value = agent
        .patch(
            &format!("{TEST_WS}/cards/{card_id}"),
            &json!({"bodyMarkdown": "whole", "readAt": added["updatedAt"]}),
        )
        .await
        .ok_json();
    let kept = versions(&th.client, card_id).await;
    assert_eq!(causes(&kept), [agents("agent"), agents("agent"), servers()]);
    assert_eq!(lines(&th.client, card_id, &kept[0]).await, ["whole"]);

    // What the person writes after is theirs, and names no token.
    th.client
        .patch(
            &format!("{TEST_WS}/cards/{card_id}"),
            &json!({"body": paragraphs(&["mine"]), "readAt": whole["updatedAt"]}),
        )
        .await
        .check_ok();
    let kept = versions(&th.client, card_id).await;
    assert_eq!(causes(&kept)[0], servers());
    assert!(kept[0].get("agentTokenName").is_none(), "{kept:?}");

    th.teardown().await;
}

async fn page_versions(client: &Client, page: &str) -> Vec<Value> {
    client.get(&format!("{page}/body-versions")).await.ok_json()
}

#[tokio::test]
async fn a_page_an_agent_writes_whole_is_kept_as_its_version() {
    let th = Setup::native().start().await.init_basic().await;
    let wiki: Value = th
        .client
        .post(&format!("{TEST_WS}/wikis"), &json!({"visibility": "open"}))
        .await
        .created_json();
    let wiki = format!("{TEST_WS}/wikis/{}", wiki["id"].as_str().expect("an id"));
    let made: Value = th
        .client
        .post(&format!("{wiki}/pages"), &json!({"title": "page"}))
        .await
        .created_json();
    let page = format!("{wiki}/pages/{}", made["id"].as_str().expect("an id"));
    let made: Value = th
        .client
        .patch(
            &page,
            &json!({"readAt": made["updatedAt"], "body": paragraphs(&["before"])}),
        )
        .await
        .ok_json();
    let agent = agent_writing(&th, &th.client).await;

    agent
        .patch(
            &page,
            &json!({"readAt": made["updatedAt"], "body": paragraphs(&["by the agent"])}),
        )
        .await
        .check_ok();
    let kept = page_versions(&th.client, &page).await;
    assert_eq!(causes(&kept), [agents("agent"), servers()]);

    th.teardown().await;
}

#[tokio::test]
async fn a_comment_an_agent_writes_names_its_token_and_one_a_person_writes_does_not() {
    let th = Setup::native().start().await.init_basic().await;
    let board = th
        .create_board(TEST_WORKSPACE_ID, fvoci_model::ContainerVisibility::Open)
        .await;
    let card: Value = th
        .client
        .post(
            &format!("{TEST_WS}/boards/{}/cards", board.id),
            &json!({"title": "talked about"}),
        )
        .await
        .created_json();
    let comments = format!(
        "{TEST_WS}/boards/{}/cards/{}/comments",
        board.id,
        card["id"].as_str().expect("an id")
    );
    let agent = agent_writing(&th, &th.client).await;
    let text = paragraphs(&["a word"]);
    let by_agent: Value = agent
        .post(&comments, &json!({"body": text}))
        .await
        .created_json();
    let by_person: Value = th
        .client
        .post(&comments, &json!({"body": text}))
        .await
        .created_json();
    assert_eq!(by_agent["fields"]["agentTokenName"], "agent", "{by_agent}");
    assert!(
        by_person["fields"].get("agentTokenName").is_none(),
        "{by_person}"
    );
    let listed: Vec<Value> = th.client.get(&comments).await.ok_json();
    assert_eq!(listed[0]["fields"]["agentTokenName"], "agent");

    // The agent writes the person's text over: the comment is the agent's.
    let id_of = |comment: &Value| comment["id"].as_str().expect("an id").to_owned();
    let rewritten: Value = agent
        .patch(
            &format!("{comments}/{}", id_of(&by_person)),
            &json!({"body": paragraphs(&["a word more"]), "readAt": by_person["updatedAt"]}),
        )
        .await
        .ok_json();
    assert_eq!(
        rewritten["fields"]["agentTokenName"], "agent",
        "{rewritten}"
    );
    // What a person sends of the name is no one's word: it is the server's to say.
    let comment_block = |comment: &Value| fvoci_model::Block {
        id: id_of(comment),
        container_id: board.id.clone(),
        ..fvoci_model::Block::default()
    };
    th.client
        .patch(
            &crate::blocks::block_path(&board.id, &id_of(&by_agent)),
            &json!({"updatedFields": {"agentTokenName": "forged"}}),
        )
        .await
        .check_no_content();
    th.client
        .patch(
            &crate::blocks::block_path(&board.id, &id_of(&by_person)),
            &json!({"deletedFields": ["agentTokenName"]}),
        )
        .await
        .check_no_content();
    let mut forged =
        crate::blocks::child_of(&comment_block(&card), fvoci_model::BlockType::Comment, "");
    forged.body = fvoci_model::Body::parse(text.clone()).expect("a body");
    forged
        .fields
        .insert("agentTokenName".to_owned(), json!("forged"));
    let forged = crate::blocks::insert_one(&th.client, &forged).await;
    assert!(
        !forged.fields.contains_key("agentTokenName"),
        "{:?}",
        forged.fields
    );
    let listed: Vec<Value> = th.client.get(&comments).await.ok_json();
    let names: Vec<&Value> = listed
        .iter()
        .map(|comment| &comment["fields"]["agentTokenName"])
        .collect();
    assert_eq!(
        names,
        [&json!("agent"), &json!("agent"), &Value::Null],
        "{listed:?}"
    );

    th.teardown().await;
}
