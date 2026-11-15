//! A wiki open to the workspace is read by whoever works there without joining it: its
//! pages over HTTP, its changes over the websocket, the rooms of its pages (to read),
//! the search and the references. Joined, it is written in. A guest is not among who
//! works there, and an open board is still read only once joined.

use serde_json::{Value, json};

use crate::board_broadcasts::WorkspaceSocket;
use crate::common::{Client, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer};
use crate::presence::{people, visit};
use crate::rooms::Peer;
use crate::websocket::Socket;

/// A wiki of the test workspace made by `maker`, of the type `wiki_type`, with a page:
/// the identifiers of both.
async fn wiki_of(maker: &Client, wiki_type: &str) -> (String, String) {
    let wiki = maker
        .post(&format!("{TEST_WS}/wikis"), &json!({"type": wiki_type}))
        .await;
    wiki.check_created();
    let wiki = wiki.value()["id"].as_str().expect("an id").to_owned();
    let page = maker
        .post(
            &format!("{TEST_WS}/wikis/{wiki}/pages"),
            &json!({"title": "A page"}),
        )
        .await;
    page.check_created();
    let page = page.value()["id"].as_str().expect("an id").to_owned();
    (wiki, page)
}

/// The statuses of reading the tree, the page and the members, and of making a page,
/// changing the page and making the wiki open (which it is or is to be), as `client`.
async fn answers(client: &Client, wiki: &str, page: &str) -> [u16; 6] {
    let pages = format!("{TEST_WS}/wikis/{wiki}/pages");
    [
        client.get(&pages).await,
        client.get(&format!("{pages}/{page}")).await,
        client
            .get(&format!("{TEST_WS}/boards/{wiki}/members"))
            .await,
        client.post(&pages, &json!({"title": "made"})).await,
        client
            .patch(&format!("{pages}/{page}"), &json!({"title": "changed"}))
            .await,
        client
            .patch(&format!("{TEST_WS}/boards/{wiki}"), &json!({"type": "O"}))
            .await,
    ]
    .map(|response| response.status.as_u16())
}

/// Changes the title of the page as `client`.
async fn retitle(client: &Client, wiki: &str, page: &str, title: &str) {
    client
        .patch(
            &format!("{TEST_WS}/wikis/{wiki}/pages/{page}"),
            &json!({"title": title}),
        )
        .await
        .check_ok();
}

/// Makes the wiki `wiki_type` (`O` or `P`) as `admin`.
async fn make(admin: &Client, wiki: &str, wiki_type: &str) {
    admin
        .patch(
            &format!("{TEST_WS}/boards/{wiki}"),
            &json!({"type": wiki_type}),
        )
        .await
        .check_ok();
}

/// What `socket` is told of pages and wikis until it is told of the page `until`: the
/// messages `UPDATE_BLOCK` and `UPDATE_BOARD`, with the last one about `until`.
async fn told_until(socket: &mut WorkspaceSocket, until: &str) -> Vec<Value> {
    let mut heard = Vec::new();
    for _ in 0..20 {
        heard.extend(socket.drain().await);
        if heard.iter().any(|told| told["block"]["id"] == json!(until)) {
            break;
        }
    }
    heard.retain(|told| {
        ["UPDATE_BLOCK", "UPDATE_BOARD"].contains(&told["action"].as_str().unwrap_or(""))
    });
    heard
}

/// The identifiers of the pages and wikis among what was told.
fn about(heard: &[Value]) -> Vec<String> {
    heard
        .iter()
        .map(|told| {
            let of = if told["action"] == json!("UPDATE_BLOCK") {
                &told["block"]["id"]
            } else {
                &told["board"]["id"]
            };
            of.as_str().expect("an identifier").to_owned()
        })
        .collect()
}

/// A member of the workspace reads the open wiki and its members without joining it,
/// and changes nothing of it; joined, they write; left, they read again.
#[tokio::test]
async fn an_open_wiki_is_read_without_joining_and_written_in_once_joined() {
    let th = Setup::native().start().await.init_basic().await;
    let (wiki, page) = wiki_of(&th.client, "O").await;
    let member = &th.client2;
    assert_eq!(
        answers(member, &wiki, &page).await,
        [200, 200, 200, 403, 403, 403]
    );
    let read: Value = member
        .get(&format!("{TEST_WS}/wikis/{wiki}/pages/{page}"))
        .await
        .ok_json();
    assert_eq!(read["title"], json!("A page"));

    member
        .post(&format!("{TEST_WS}/boards/{wiki}/join"), &json!({}))
        .await
        .check_ok();
    let joined = answers(member, &wiki, &page).await;
    assert_eq!(&joined[..5], [200, 200, 200, 201, 200], "an editor writes");
    assert_eq!(joined[5], 403, "and does not manage the wiki");

    member
        .post(&format!("{TEST_WS}/boards/{wiki}/leave"), &json!({}))
        .await
        .check_no_content();
    assert_eq!(
        answers(member, &wiki, &page).await,
        [200, 200, 200, 403, 403, 403]
    );

    // Made private, it is not there for them; open again, it is read again.
    make(&th.client, &wiki, "P").await;
    assert_eq!(answers(member, &wiki, &page).await, [404; 6]);
    make(&th.client, &wiki, "O").await;
    assert_eq!(answers(member, &wiki, &page).await[..3], [200, 200, 200]);
    th.teardown().await;
}

/// A guest of the workspace is not among who works there: an open wiki is not there
/// for them, and nothing of it is told them.
#[tokio::test]
async fn an_open_wiki_is_nothing_to_a_guest() {
    let th = Setup::native().start().await.init_basic().await;
    let guest = crate::guests::guest_of_the_workspace(&th).await;
    let (open, open_page) = wiki_of(&th.client, "O").await;
    let (invited, invited_page) = wiki_of(&th.client, "P").await;
    crate::references::let_read(&th.client, &invited, &guest).await;
    assert_eq!(answers(&th.client2, &open, &open_page).await, [404; 6]);
    let listed: Vec<Value> = th.client2.get(&format!("{TEST_WS}/wikis")).await.ok_json();
    let listed: Vec<&Value> = listed.iter().map(|wiki| &wiki["id"]).collect();
    assert_eq!(listed, [&json!(invited)], "the invited wiki alone");

    let mut socket = WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &guest).await;
    retitle(&th.client, &open, &open_page, "told nobody").await;
    retitle(&th.client, &invited, &invited_page, "told the guest").await;
    let heard = told_until(&mut socket, &invited_page).await;
    assert_eq!(about(&heard), [invited_page], "{heard:?}");

    let mut peer = Peer::connect(&th.client2, 401).await;
    let refused = peer.join(TEST_WORKSPACE_ID, &open_page).await;
    assert_eq!(refused["reason"], json!("not_found"), "{refused}");
    th.teardown().await;
}

/// Who works in the workspace is told what changes in an open wiki they did not join,
/// as its members are; made private it is gone for them, told as a deletion, and they
/// are told nothing of it any more; open again, they are told again, and of its
/// deletion.
#[tokio::test]
async fn who_works_there_is_told_the_changes_of_an_open_wiki_they_did_not_join() {
    let th = Setup::native().start().await.init_basic().await;
    let member = th.client2.me().await.id;
    let (open, page) = wiki_of(&th.client, "O").await;
    let (own, own_page) = wiki_of(&th.client2, "P").await;
    let mut socket = WorkspaceSocket::subscribe(&th, &th.client2, TEST_WORKSPACE_ID, &member).await;

    retitle(&th.client, &open, &page, "told who reads it").await;
    th.client
        .patch(
            &format!("{TEST_WS}/boards/{open}"),
            &json!({"title": "renamed"}),
        )
        .await
        .check_ok();
    retitle(&th.client2, &own, &own_page, "the end").await;
    let heard = told_until(&mut socket, &own_page).await;
    assert_eq!(
        about(&heard),
        [page.clone(), open.clone(), own_page.clone()],
        "{heard:?}"
    );
    assert_eq!(heard[0]["block"]["title"], json!("told who reads it"));

    make(&th.client, &open, "P").await;
    retitle(&th.client, &open, &page, "told its members alone").await;
    retitle(&th.client2, &own, &own_page, "the end again").await;
    let heard = told_until(&mut socket, &own_page).await;
    assert_eq!(about(&heard), [open.clone(), own_page.clone()], "{heard:?}");
    assert_ne!(
        heard[0]["board"]["deleteAt"],
        json!(0),
        "gone for them: {heard:?}"
    );

    make(&th.client, &open, "O").await;
    th.client
        .delete(&format!("{TEST_WS}/boards/{open}"))
        .await
        .check_no_content();
    retitle(&th.client2, &own, &own_page, "the very end").await;
    let heard = told_until(&mut socket, &own_page).await;
    let of_open: Vec<&Value> = heard
        .iter()
        .filter(|told| told["board"]["id"] == json!(open))
        .collect();
    assert_eq!(of_open.len(), 2, "made open and deleted: {heard:?}");
    assert_eq!(of_open[0]["board"]["type"], json!("O"));
    assert_ne!(of_open[1]["board"]["deleteAt"], json!(0), "{heard:?}");
    th.teardown().await;
}

/// Who works in the workspace and did not join an open wiki comes into the room of a
/// page to read: what they write is refused. Joined, they write; when they leave,
/// they read again; when the wiki is made private, they are put out.
#[tokio::test]
async fn who_did_not_join_an_open_wiki_reads_in_the_rooms_of_its_pages() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_recheck_seconds = 1)
        .start()
        .await
        .init_basic()
        .await;
    let (wiki, page) = wiki_of(&th.client, "O").await;
    let mut writer = Peer::connect(&th.client, 501).await;
    writer.joined(TEST_WORKSPACE_ID, &page).await;
    let mut reader = Peer::connect(&th.client2, 502).await;
    let entered = reader.join(TEST_WORKSPACE_ID, &page).await;
    assert_eq!(entered["mode"], json!("read"), "{entered}");
    reader.write("not to be kept").await;
    let refused = reader.text("DOC_NACK").await;
    assert_eq!(refused["reason"], json!("document_read_only"), "{refused}");
    writer.write_acked("read by who did not join").await;
    reader.settle().await;
    assert!(
        reader
            .lines()
            .contains(&"read by who did not join".to_owned())
    );

    th.client2
        .post(&format!("{TEST_WS}/boards/{wiki}/join"), &json!({}))
        .await
        .check_ok();
    assert_eq!(reader.text("DOC_MODE").await["mode"], json!("write"));
    th.client2
        .post(&format!("{TEST_WS}/boards/{wiki}/leave"), &json!({}))
        .await
        .check_no_content();
    assert_eq!(
        reader.text("DOC_MODE").await["mode"],
        json!("read"),
        "left, it reads"
    );

    make(&th.client, &wiki, "P").await;
    let left = reader.text("DOC_LEFT").await;
    assert_eq!(left["reason"], json!("not_found"), "{left}");
    th.teardown().await;
}

/// Who reads an open wiki without joining it is on it as its members are: who else is
/// there sees them by name, and they see who else is there.
#[tokio::test]
async fn who_did_not_join_an_open_wiki_is_on_it_by_name() {
    let th = Setup::native().start().await.init_basic().await;
    let (wiki, page) = wiki_of(&th.client, "O").await;
    let member = th.client2.me().await.id;
    let owner = th.client.me().await.id;
    let mut there = Socket::connect_as(&th.client).await;
    let mut reader = Socket::connect_as(&th.client2).await;
    visit(&mut there, &wiki, &page).await;
    visit(&mut reader, &wiki, &page).await;
    let seen = people(&mut there, 2).await;
    let seen_by_reader = people(&mut reader, 2).await;
    for (roster, who) in [(&seen, &member), (&seen_by_reader, &owner)] {
        assert!(roster.to_string().contains(who.as_str()), "{roster}");
        assert_eq!(roster["anonymousCount"], json!(0), "{roster}");
    }
    th.teardown().await;
}

/// A server whose bodies are searched in a search server of the test.
async fn search_world() -> (crate::fake_search::FakeSearch, TestServer) {
    let fake = crate::fake_search::FakeSearch::start().await;
    let th = crate::content_search::started(&fake).await;
    (fake, th)
}

/// The page of an open wiki is found by who works in the workspace, joined or not; a
/// guest does not find it.
#[tokio::test]
async fn the_pages_of_an_open_wiki_are_searched_by_who_works_there() {
    let (fake, th) = search_world().await;
    let wiki = th
        .client
        .post(&format!("{TEST_WS}/wikis"), &json!({"type": "O"}))
        .await
        .value()["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let page = crate::search_pages::new_page(&th.client, &wiki, "", "환불 안내", "환불").await;
    let own = crate::search_pages::new_wiki(&th.client2).await;
    let own_page = crate::search_pages::new_page(&th.client2, &own, "", "환불 내 것", "환불").await;
    fvoci_server::search::tend(&th.app).await;
    let found = |client: &Client| {
        let client = client.clone();
        async move {
            let hits: Vec<Value> = client
                .get(&format!("{TEST_WS}/search?q=%ED%99%98%EB%B6%88"))
                .await
                .ok_json();
            let mut ids: Vec<String> = hits
                .iter()
                .map(|hit| hit["id"].as_str().expect("an id").to_owned())
                .collect();
            ids.sort();
            ids
        }
    };
    let mut both = vec![page.id.clone(), own_page.id.clone()];
    both.sort();
    assert_eq!(
        found(&th.client2).await,
        both,
        "unjoined, the open wiki's page"
    );

    crate::guests::guest_of_the_workspace(&th).await;
    assert_eq!(
        found(&th.client2).await,
        std::slice::from_ref(&own_page.id),
        "a guest, not"
    );
    drop(fake);
    th.teardown().await;
}
