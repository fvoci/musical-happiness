//! Which operations reach a board, which a wiki, and which both.
//!
//! A wiki is a board of the kind `wiki`. An operation that is about what a board holds
//! (cards, views) answers for a wiki as for a board that is not there,
//! an operation on a wiki's pages answers so for a board, and what is the same for
//! both (the container, who is on it, its sharing, its files, its place in the sidebar
//! and the trash, a copy of it whole and its archive) reaches either. [`REACHED`] says which is which, in one place, and
//! the test asks every operation of the contract that names a board or something of
//! one: an operation that is added without a row fails it.

use std::collections::BTreeSet;

use reqwest::Method;
use reqwest::multipart::{Form, Part};
use serde_json::{Value, json};

use crate::common::{
    ApiResponse, Client, ROOT_WORKSPACE_ID, ROOT_WS, Setup, TestServer, UNKNOWN_ID,
};

/// The kinds of board an operation reaches by what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reaches {
    /// Boards; a wiki named there is not there.
    Boards,
    /// Wikis; a board named there is not there.
    Wikis,
    /// Boards and wikis alike.
    Both,
}

/// Every operation whose path names a board, a wiki, or something that is of one (a
/// block, a file, an upload), and the kinds it reaches: the extractor of its handler
/// (`InWorkspace` for boards, `InWiki` for wikis, `InContainer` for both; the token of a shared
/// board or wiki opens its own kind's routes). What names one in its body or its query is asked in
/// [`what_names_a_wiki_in_a_body_or_a_query_is_refused_as_what_names_nothing`].
const REACHED: &[(&str, Reaches)] = &[
    // The container itself, in the trash too.
    ("getContainer", Reaches::Both),
    ("patchContainer", Reaches::Both),
    ("deleteContainer", Reaches::Both),
    ("undeleteContainer", Reaches::Both),
    // Who is on it.
    ("getMembers", Reaches::Both),
    ("addMember", Reaches::Both),
    ("updateMember", Reaches::Both),
    ("removeMember", Reaches::Both),
    ("joinContainer", Reaches::Both),
    ("leaveContainer", Reaches::Both),
    ("getContainerTeams", Reaches::Both),
    ("setContainerTeam", Reaches::Both),
    ("removeContainerTeam", Reaches::Both),
    // Its sharing: a board's link reads its cards, a wiki's its pages.
    ("getSharing", Reaches::Both),
    ("setSharing", Reaches::Both),
    // Its files.
    ("uploadFile", Reaches::Both),
    ("getFile", Reaches::Both),
    ("getFileInfo", Reaches::Both),
    ("getFileVersions", Reaches::Both),
    ("getFileVersion", Reaches::Both),
    ("addFileVersion", Reaches::Both),
    ("restoreFileVersion", Reaches::Both),
    ("createUpload", Reaches::Both),
    ("getUpload", Reaches::Both),
    ("putUploadPart", Reaches::Both),
    ("completeUpload", Reaches::Both),
    ("deleteUpload", Reaches::Both),
    // Its place in the sidebar.
    ("setContainerCategory", Reaches::Both),
    ("hideContainer", Reaches::Both),
    ("unhideContainer", Reaches::Both),
    // A copy of it whole, and its archive.
    ("duplicateContainer", Reaches::Both),
    ("exportContainerArchive", Reaches::Both),
    // What a board holds.
    ("reorderCardProperties", Reaches::Boards),
    ("createBlocks", Reaches::Boards),
    ("getBlocks", Reaches::Boards),
    ("patchBlocks", Reaches::Boards),
    ("patchBlock", Reaches::Boards),
    ("deleteBlock", Reaches::Boards),
    ("undeleteBlock", Reaches::Boards),
    ("duplicateBlock", Reaches::Boards),
    ("getCommentVersions", Reaches::Boards),
    ("createCard", Reaches::Boards),
    ("getCards", Reaches::Boards),
    ("getCard", Reaches::Boards),
    ("patchCard", Reaches::Boards),
    ("getCardBodyVersions", Reaches::Boards),
    ("getCardBodyVersion", Reaches::Boards),
    ("restoreCardBodyVersion", Reaches::Boards),
    ("getDeletedCards", Reaches::Boards),
    ("deleteSubscription", Reaches::Boards),
    // What a wiki holds.
    ("getPageTree", Reaches::Wikis),
    ("createPage", Reaches::Wikis),
    ("getPage", Reaches::Wikis),
    ("patchPage", Reaches::Wikis),
    ("deletePage", Reaches::Wikis),
    ("getPageComments", Reaches::Wikis),
    ("createPageComment", Reaches::Wikis),
    ("patchPageComment", Reaches::Wikis),
    ("deletePageComment", Reaches::Wikis),
    ("getPageCommentVersions", Reaches::Wikis),
    ("undeletePageComment", Reaches::Wikis),
    ("getPageBodyVersions", Reaches::Wikis),
    ("getPageBodyVersion", Reaches::Wikis),
    ("restorePageBodyVersion", Reaches::Wikis),
    ("movePage", Reaches::Wikis),
    ("duplicatePage", Reaches::Wikis),
    ("getDeletedPages", Reaches::Wikis),
    ("undeletePage", Reaches::Wikis),
    // What names a card or a page from a body: a card and a page name each other.
    ("getReferencesTo", Reaches::Both),
    // What the holder of a read token reads.
    ("getSharedContainer", Reaches::Both),
    ("getSharedBlocks", Reaches::Boards),
    ("getSharedCards", Reaches::Boards),
    ("getSharedFile", Reaches::Both),
    ("getSharedFileInfo", Reaches::Both),
    ("getSharedPageTree", Reaches::Wikis),
    ("getSharedPage", Reaches::Wikis),
];

/// The parameters of a path that name a board or a wiki, or something that is of one.
const NAMING: [&str; 9] = [
    "{containerId}",
    "{boardId}",
    "{wikiId}",
    "{cardId}",
    "{blockId}",
    "{pageId}",
    "{commentId}",
    "{uploadId}",
    "{fileId}",
];

/// A board or a wiki of the first user with one of everything a path names; or nothing
/// at all, under identifiers of nothing.
#[derive(Debug, Clone)]
struct Things {
    container: String,
    /// A card of the board; a page of the wiki.
    block: String,
    /// A comment on the card; on the page.
    comment: String,
    file: String,
    upload: String,
    /// The read token of the container, which is shared.
    token: String,
}

impl Things {
    fn nothing() -> Self {
        Self {
            container: UNKNOWN_ID.to_owned(),
            block: UNKNOWN_ID.to_owned(),
            comment: UNKNOWN_ID.to_owned(),
            file: "missing-file".to_owned(),
            upload: "missing-upload".to_owned(),
            token: "no-token".to_owned(),
        }
    }
}

/// The server, and who and what the requests name besides a board or a wiki.
struct World {
    ts: TestServer,
    me: String,
    other: String,
    team: String,
    category: String,
}

impl World {
    async fn start() -> Self {
        let ts = Setup::local_mode().start().await.init_basic().await;
        let (me, other) = (ts.client.me().await.id, ts.client2.me().await.id);
        let team = ts
            .client
            .post(&format!("{ROOT_WS}/teams"), &json!({"name": "Readers"}))
            .await
            .value();
        let categories = ts
            .client
            .get(&format!("{ROOT_WS}/categories"))
            .await
            .value();
        Self {
            me,
            other,
            team: text(&team, "id"),
            category: text(&categories[0], "id"),
            ts,
        }
    }

    /// Makes a board, or a wiki, with the second user and the team on it, a file, an
    /// open upload and a link; a board with a card the first user is subscribed to and
    /// a comment on it besides, a wiki with a page.
    async fn make(&self, kind: Reaches) -> Things {
        let client = &self.ts.client;
        let made_at = match kind {
            Reaches::Wikis => format!("{ROOT_WS}/wikis"),
            Reaches::Boards | Reaches::Both => format!("{ROOT_WS}/boards"),
        };
        let made = client.post(&made_at, &json!({"type": "P"})).await;
        made.check_done();
        let container = text(&made.value(), "id");
        let at = format!("{ROOT_WS}/containers/{container}");
        let member = json!({"containerId": container, "userId": self.other, "schemeEditor": true});
        client
            .post(&format!("{at}/members"), &member)
            .await
            .check_done();
        let on_team = format!("{at}/teams/{}", self.team);
        let assigned = client.put(&on_team, &json!({"role": "viewer"})).await;
        assert!(assigned.status.is_success(), "{}", assigned.text());
        let file = Part::bytes(b"kept".to_vec()).file_name("kept.txt");
        let uploaded = Client::send(
            client
                .request(Method::POST, &format!("{at}/files"))
                .multipart(Form::new().part("file", file)),
        )
        .await;
        uploaded.check_done();
        let opened = client
            .post(
                &format!("{at}/uploads"),
                &json!({"name": "parts.bin", "size": 4}),
            )
            .await;
        opened.check_done();
        client
            .post(&format!("{at}/sharing"), &json!({"enabled": true}))
            .await
            .check_done();
        let sharing = client.get(&format!("{at}/sharing")).await.value();
        let (block, comment) = match kind {
            Reaches::Wikis => {
                let pages = format!("{ROOT_WS}/wikis/{container}/pages");
                let page = client.post(&pages, &json!({"title": "a page"})).await;
                page.check_done();
                let page = page.value();
                // A body written to the page is a version of it, as the card's is: an
                // operation on a card's versions that reached it would find one.
                let body = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
                    {"type": "text", "text": "written"}
                ]}]});
                let page_id = text(&page, "id");
                client
                    .patch(
                        &format!("{pages}/{page_id}"),
                        &json!({"readAt": page["updateAt"], "body": body}),
                    )
                    .await
                    .check_ok();
                let comment = client
                    .post(
                        &format!("{pages}/{page_id}/comments"),
                        &json!({ "body": body }),
                    )
                    .await;
                comment.check_done();
                (page_id, text(&comment.value(), "id"))
            }
            Reaches::Boards | Reaches::Both => self.card_with_a_comment(&container).await,
        };
        Things {
            container,
            block,
            comment,
            file: text(&uploaded.value(), "fileId"),
            upload: text(&opened.value(), "uploadId"),
            token: text(&sharing, "token"),
        }
    }

    async fn card_with_a_comment(&self, board: &str) -> (String, String) {
        let client = &self.ts.client;
        let at = format!("{ROOT_WS}/boards/{board}");
        let card = client
            .post(&format!("{at}/cards"), &json!({"title": "a card"}))
            .await;
        card.check_done();
        let made = card.value();
        let card = text(&made, "id");
        let text_of = json!({"type": "doc", "content": [{"type": "paragraph", "content": [
            {"type": "text", "text": "said"}
        ]}]});
        // A body written to the card is the first version of it.
        client
            .patch(
                &format!("{ROOT_WS}/cards/{card}"),
                &json!({"readAt": made["updateAt"], "body": text_of}),
            )
            .await
            .check_ok();
        let comments = client
            .post(
                &format!("{at}/blocks"),
                &json!([{
                    "containerId": board, "parentId": card, "type": "comment", "body": text_of,
                    "createAt": 1, "updateAt": 1,
                }]),
            )
            .await;
        comments.check_done();
        let subscribed = client
            .post(
                &format!("{ROOT_WS}/subscriptions"),
                &json!({
                    "blockId": card, "blockType": "card", "subscriberId": self.me,
                    "subscriberType": "user",
                }),
            )
            .await;
        subscribed.check_done();
        (card, text(&comments.value()[0], "id"))
    }

    /// Brings `things` to where the operation does something: what it brings back is
    /// in the trash first.
    async fn prepare(&self, operation: &str, things: &Things) {
        let at = format!("{ROOT_WS}/containers/{}", things.container);
        let deleted = match operation {
            "undeleteContainer" => self.ts.client.delete(&at).await,
            "undeleteBlock" => {
                let block = format!(
                    "{ROOT_WS}/boards/{}/blocks/{}",
                    things.container, things.block
                );
                self.ts.client.delete(&block).await
            }
            "undeletePage" => {
                let page = format!(
                    "{ROOT_WS}/wikis/{}/pages/{}",
                    things.container, things.block
                );
                self.ts.client.delete(&page).await
            }
            "undeletePageComment" => {
                let comment = format!(
                    "{ROOT_WS}/wikis/{}/pages/{}/comments/{}",
                    things.container, things.block, things.comment
                );
                self.ts.client.delete(&comment).await
            }
            _ => return,
        };
        // Whether it went is what the operation itself shows: a wiki's card cannot go.
        drop(deleted);
    }

    /// Sends the operation about `things`.
    async fn ask(
        &self,
        method: &Method,
        template: &str,
        operation: &str,
        things: &Things,
    ) -> ApiResponse {
        let block = if operation == "getCommentVersions" {
            &things.comment
        } else {
            &things.block
        };
        let path = template
            .trim_start_matches("/api/v1")
            .replace("{workspaceId}", ROOT_WORKSPACE_ID)
            .replace("{containerId}", &things.container)
            .replace("{boardId}", &things.container)
            .replace("{wikiId}", &things.container)
            .replace("{cardId}", block)
            .replace("{blockId}", block)
            .replace("{pageId}", block)
            .replace("{commentId}", &things.comment)
            .replace("{fileId}", &things.file)
            .replace("{uploadId}", &things.upload)
            .replace("{userId}", &self.other)
            .replace("{teamId}", &self.team)
            .replace("{categoryId}", &self.category)
            .replace("{subscriberId}", &self.me)
            .replace("{partNumber}", "1")
            .replace("{version}", "1");
        assert!(
            !path.contains('{'),
            "{operation}: a parameter of {path} is not filled in"
        );
        if template.starts_with("/api/v1/shared/") {
            let path = format!("{path}?readToken={}", things.token);
            return self.ts.anonymous().get(&path).await;
        }
        let request = self.ts.client.request(method.clone(), &path);
        Client::send(match operation {
            "uploadFile" => {
                let file = Part::bytes(b"more".to_vec()).file_name("more.txt");
                request.multipart(Form::new().part("file", file))
            }
            "putUploadPart" => request.body(b"part".to_vec()),
            _ => request.json(&self.body(operation, things)),
        })
        .await
    }

    /// The body an operation is sent with: one it takes.
    fn body(&self, operation: &str, things: &Things) -> Value {
        match operation {
            "patchContainer" | "patchBlock" | "patchCard" | "patchPage" => {
                json!({"title": "changed"})
            }
            "addMember" | "updateMember" => {
                json!({"containerId": things.container, "userId": self.other, "schemeViewer": true})
            }
            "setContainerTeam" => json!({"role": "editor"}),
            "setSharing" => json!({"enabled": false}),
            "createUpload" => json!({"name": "more.bin", "size": 4}),
            // The file itself is no version of it: refused as what it is, where the file is.
            "addFileVersion" => json!({"baseVersion": 1, "fileId": things.file}),
            "restoreFileVersion" => json!({"baseVersion": 1}),
            "patchBlocks" => {
                json!({"blockIds": [things.block], "blockPatches": [{"title": "changed"}]})
            }
            "createBlocks" | "reorderCardProperties" => json!([]),
            "createCard" => json!({"title": "another card"}),
            "createPageComment" => json!({"body": said()}),
            "patchPageComment" => json!({"body": said(), "readAt": 1}),
            _ => json!({}),
        }
    }
}

/// What the fixtures need of an answer: that the request did what it was sent to do.
trait Done {
    fn check_done(&self);
}

impl Done for ApiResponse {
    #[track_caller]
    fn check_done(&self) {
        assert!(self.status.is_success(), "{}: {}", self.status, self.text());
    }
}

/// A text, as a body or a comment says it.
fn said() -> Value {
    json!({"type": "doc", "content": [{"type": "paragraph", "content": [
        {"type": "text", "text": "said"}
    ]}]})
}

fn text(value: &Value, member: &str) -> String {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("{member} in {value}"))
        .to_owned()
}

/// The operations of the contract whose path names a board, a wiki or something of
/// one: method, path, operation.
fn operations_that_name_one() -> Vec<(Method, String, String)> {
    let contract: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../contracts/openapi.json"
    )))
    .expect("the contract is JSON");
    let mut found = Vec::new();
    for (path, methods) in contract["paths"].as_object().expect("paths") {
        if !NAMING.iter().any(|parameter| path.contains(parameter)) {
            continue;
        }
        for (method, operation) in methods.as_object().expect("operations") {
            if let Some(operation) = operation["operationId"].as_str() {
                let method = method.to_uppercase().parse().expect("an HTTP method");
                found.push((method, path.clone(), operation.to_owned()));
            }
        }
    }
    found
}

#[test]
fn the_table_has_a_row_for_every_operation_that_names_a_board_and_for_no_other() {
    let named: BTreeSet<String> = operations_that_name_one()
        .into_iter()
        .map(|(_, _, operation)| operation)
        .collect();
    let rows: BTreeSet<String> = REACHED.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(rows.len(), REACHED.len(), "an operation has one row");
    assert_eq!(
        rows, named,
        "an operation whose path names a board, a wiki or something of one has a row \
         that says which kinds it reaches"
    );
}

/// Every operation that names a board or something of one is asked about a wiki, about
/// a board and about nothing, each time about ones made for it. One that reaches boards
/// answers for the wiki, and for what is the wiki's, exactly as for nothing; one that
/// reaches wikis answers so for the board; one that reaches both does to the wiki what
/// it does to the board.
#[tokio::test]
async fn every_operation_reaches_the_kinds_of_its_row_and_answers_for_the_other_as_for_nothing() {
    let world = World::start().await;
    let mut wrong = Vec::new();
    for (method, template, operation) in operations_that_name_one() {
        let Some((_, reaches)) = REACHED.iter().find(|(name, _)| *name == operation) else {
            continue;
        };
        let (board, wiki) = (
            world.make(Reaches::Boards).await,
            world.make(Reaches::Wikis).await,
        );
        world.prepare(&operation, &board).await;
        world.prepare(&operation, &wiki).await;
        let ask = async |things: &Things| world.ask(&method, &template, &operation, things).await;
        let nothing = ask(&Things::nothing()).await;
        let (of_wiki, of_board) = (ask(&wiki).await, ask(&board).await);
        let right = match reaches {
            Reaches::Boards => {
                of_wiki.problem() == nothing.problem()
                    && !of_wiki.status.is_success()
                    && of_board.problem() != nothing.problem()
            }
            Reaches::Wikis => {
                of_board.problem() == nothing.problem()
                    && !of_board.status.is_success()
                    && of_wiki.problem() != nothing.problem()
            }
            Reaches::Both => {
                of_wiki.status == of_board.status && of_wiki.problem() != nothing.problem()
            }
        };
        // A user has only so many uploads open at once.
        for things in [&board, &wiki] {
            let upload = format!("{ROOT_WS}/uploads/{}", things.upload);
            drop(world.ts.client.delete(&upload).await);
        }
        if !right {
            wrong.push(format!(
                "{operation} ({method} {template}) reaches {reaches:?}: wiki={:?}, board={:?}, \
                 nothing={:?}",
                of_wiki.problem(),
                of_board.problem(),
                nothing.problem()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    world.ts.teardown().await;
}

/// The identifiers of what a list answers with, in its order.
fn ids(listed: &ApiResponse) -> Vec<String> {
    let listed = listed.value();
    let listed = listed.as_array().expect("a list");
    listed.iter().map(|entry| text(entry, "id")).collect()
}

/// The lists of boards and the search are of one kind each; the trash of boards and the
/// archive of the workspace hold both, and the trash says of each which it is.
#[tokio::test]
async fn every_list_is_of_its_kinds() {
    let world = World::start().await;
    let client = &world.ts.client;
    let (board, wiki) = (
        world.make(Reaches::Boards).await,
        world.make(Reaches::Wikis).await,
    );
    let (the_board, the_wiki) = (
        std::slice::from_ref(&board.container),
        std::slice::from_ref(&wiki.container),
    );

    let boards = client.get(&format!("{ROOT_WS}/boards")).await;
    assert_eq!(ids(&boards), the_board);
    let wikis = client.get(&format!("{ROOT_WS}/wikis")).await;
    assert_eq!(ids(&wikis), the_wiki);
    assert_eq!(wikis.value()[0]["kind"], "wiki");
    assert_eq!(boards.value()[0]["kind"], "board");
    let titled = json!({"title": "findable"});
    for things in [&board, &wiki] {
        let at = format!("{ROOT_WS}/containers/{}", things.container);
        client.patch(&at, &titled).await.check_done();
    }
    let found = client
        .get(&format!("{ROOT_WS}/boards/search?q=findable"))
        .await;
    assert_eq!(ids(&found), the_board, "the search is for boards");
    let exported = client.get(&format!("{ROOT_WS}/archive/export")).await;
    exported.check_done();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(exported.body))
        .expect("the export is an archive");
    let boards_in: BTreeSet<String> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter_map(|name| name.strip_suffix("/board.jsonl").map(str::to_owned))
        .collect();
    let both = BTreeSet::from([board.container.clone(), wiki.container.clone()]);
    assert_eq!(boards_in, both, "the export is of boards and wikis");

    for things in [&board, &wiki] {
        let at = format!("{ROOT_WS}/containers/{}", things.container);
        client.delete(&at).await.check_done();
    }
    let trash = client.get(&format!("{ROOT_WS}/trash/containers")).await;
    trash.check_done();
    let mut listed: Vec<(String, String)> = trash
        .value()
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| (text(entry, "kind"), text(entry, "id")))
        .collect();
    listed.sort();
    assert_eq!(
        listed,
        [
            ("board".to_owned(), board.container.clone()),
            ("wiki".to_owned(), wiki.container.clone()),
        ]
    );
    // A card in the trash says no kind.
    let card = format!(
        "{ROOT_WS}/boards/{}/blocks/{}",
        board.container, board.block
    );
    let again = client
        .post(
            &format!("{ROOT_WS}/containers/{}/undelete", board.container),
            &json!({}),
        )
        .await;
    again.check_done();
    client.delete(&card).await.check_done();
    let cards = client
        .get(&format!("{ROOT_WS}/boards/{}/trash/cards", board.container))
        .await;
    assert_eq!(ids(&cards), std::slice::from_ref(&board.block));
    assert!(cards.value()[0].get("kind").is_none(), "{}", cards.text());
    world.ts.teardown().await;
}

/// What names a board in a body or a query takes no wiki: the answer is the one for an
/// identifier of nothing, and nothing is made or changed. The board a guest is
/// invited to may be a wiki.
#[tokio::test]
async fn what_names_a_wiki_in_a_body_or_a_query_is_refused_as_what_names_nothing() {
    let world = World::start().await;
    let client = &world.ts.client;
    let (board, wiki) = (
        world.make(Reaches::Boards).await,
        world.make(Reaches::Wikis).await,
    );

    // A board is not made a wiki of, one at a time or with blocks.
    let as_wiki = json!({"id": "made", "type": "P", "kind": "wiki", "title": "no board"});
    let card =
        json!({"id": "card", "containerId": "made", "type": "card", "createAt": 1, "updateAt": 1});
    let one = client
        .post(
            &format!("{ROOT_WS}/boards"),
            &json!({"type": "P", "kind": "wiki"}),
        )
        .await;
    let many = client
        .post(
            &format!("{ROOT_WS}/boards-and-blocks"),
            &json!({"containers": [as_wiki], "blocks": [card]}),
        )
        .await;
    for (refused, at) in [(one, "/kind"), (many, "/containers/0/kind")] {
        refused.check_bad_request();
        assert_eq!(refused.refused_at(), at, "{}", refused.text());
    }
    assert_eq!(ids(&client.get(&format!("{ROOT_WS}/wikis")).await).len(), 1);

    // What changes or deletes boards together, and the source of copied blocks.
    let same = async |method: Method, path: &str, body: &dyn Fn(&str) -> Value| {
        let send = async |id: &str| {
            Client::send(client.request(method.clone(), path).json(&body(id))).await
        };
        let (of_wiki, nothing) = (send(&wiki.container).await, send(UNKNOWN_ID).await);
        assert_eq!(of_wiki.problem(), nothing.problem(), "{method} {path}");
        assert!(
            !of_wiki.status.is_success(),
            "{method} {path}: {}",
            of_wiki.text()
        );
    };
    let together = format!("{ROOT_WS}/boards-and-blocks");
    same(Method::PATCH, &together, &|id| {
        json!({
            "containerIds": [id],
            "containerPatches": [{"title": "changed"}],
            "blockIds": [board.block],
            "blockPatches": [{"title": "changed"}],
        })
    })
    .await;
    same(
        Method::DELETE,
        &together,
        &|id| json!({"containers": [id], "blocks": [board.block]}),
    )
    .await;
    let copied =
        json!([{"containerId": board.container, "type": "card", "createAt": 1, "updateAt": 1}]);
    let from = async |source: &str| {
        let path = format!(
            "{ROOT_WS}/boards/{}/blocks?sourceBoardId={source}",
            board.container
        );
        client.post(&path, &copied).await
    };
    let other = client
        .post(&format!("{ROOT_WS}/boards"), &json!({"type": "P"}))
        .await;
    other.check_done();
    from(&text(&other.value(), "id")).await.check_done();
    let (of_wiki, nothing) = (from(&wiki.container).await, from(UNKNOWN_ID).await);
    assert_eq!(of_wiki.problem(), nothing.problem(), "sourceBoardId");
    assert!(!of_wiki.status.is_success(), "{}", of_wiki.text());
    client
        .put(
            &format!("{ROOT_WS}/guest-invitations"),
            &json!({"enabled": true}),
        )
        .await
        .check_done();
    // A guest is invited to a wiki as to a board.
    client
        .post(
            &format!("{ROOT_WS}/invitations"),
            &json!({"role": "guest", "containerId": wiki.container}),
        )
        .await
        .check_done();

    // The wiki is as it was, and so is the board's card.
    let read = client
        .get(&format!("{ROOT_WS}/containers/{}", wiki.container))
        .await;
    read.check_done();
    assert_ne!(read.value()["title"], "changed");
    let card = client
        .get(&format!("{ROOT_WS}/cards/{}", board.block))
        .await;
    card.check_done();
    assert_eq!(card.value()["title"], "a card");
    world.ts.teardown().await;
}

/// The audit of the workspace names the wiki an administrator of the workspace put
/// themselves on, as it names a board, to who sees it.
#[tokio::test]
async fn the_audit_names_a_wiki_as_it_names_a_board() {
    let world = World::start().await;
    let (administrator, maker) = (&world.ts.client, &world.ts.client2);
    let mut made = Vec::new();
    for (path, title) in [("boards", "Roadmap"), ("wikis", "Plans")] {
        let container = maker
            .post(
                &format!("{ROOT_WS}/{path}"),
                &json!({"type": "P", "title": title}),
            )
            .await;
        container.check_done();
        let id = text(&container.value(), "id");
        administrator
            .post(
                &format!("{ROOT_WS}/containers/{id}/members"),
                &json!({"containerId": id, "userId": world.me, "schemeAdmin": true}),
            )
            .await
            .check_done();
        made.push((id, title));
    }
    let audit = administrator.get(&format!("{ROOT_WS}/audit")).await;
    audit.check_done();
    let audit = audit.value();
    let records = audit.as_array().expect("a list");
    for (id, title) in made {
        let named: Vec<&Value> = records
            .iter()
            .filter(|record| record["targetId"] == json!(id))
            .map(|record| &record["targetName"])
            .collect();
        assert_eq!(named, [&json!(title)], "the records about {title}");
    }
    world.ts.teardown().await;
}

/// Nothing changes the kind of a board or a wiki: a patch that names a kind, its own
/// too, is refused at `/kind` and changes nothing; a member a patch does not know is
/// left out, as it was.
#[tokio::test]
async fn a_patch_that_names_a_kind_is_refused() {
    let world = World::start().await;
    let client = &world.ts.client;
    let (board, wiki) = (
        world.make(Reaches::Boards).await,
        world.make(Reaches::Wikis).await,
    );
    for (things, own) in [(&board, "board"), (&wiki, "wiki")] {
        let at = format!("{ROOT_WS}/containers/{}", things.container);
        for kind in ["board", "wiki", "page", ""] {
            let refused = client
                .patch(&at, &json!({"kind": kind, "title": "changed"}))
                .await;
            refused.check_bad_request();
            assert_eq!(refused.refused_at(), "/kind", "{own} as {kind:?}");
        }
        let read = client.get(&at).await.value();
        assert_eq!(
            (read["kind"].as_str(), read["title"].as_str()),
            (Some(own), Some(""))
        );
        client
            .patch(&at, &json!({"unknown": true, "title": "changed"}))
            .await
            .check_done();
    }
    // Who does not see the board is told it is not there, whatever the patch names.
    let unseen = client
        .post(&format!("{ROOT_WS}/boards"), &json!({"type": "P"}))
        .await;
    unseen.check_done();
    let unseen = format!("{ROOT_WS}/containers/{}", text(&unseen.value(), "id"));
    let (with_kind, without) = (
        world
            .ts
            .client2
            .patch(&unseen, &json!({"kind": "wiki"}))
            .await,
        world
            .ts
            .client2
            .patch(&unseen, &json!({"title": "x"}))
            .await,
    );
    with_kind.check_not_found();
    assert_eq!(with_kind.problem(), without.problem());
    // Nor is a kind changed with blocks.
    let together = client
        .patch(
            &format!("{ROOT_WS}/boards-and-blocks"),
            &json!({
                "containerIds": [board.container],
                "containerPatches": [{"kind": "wiki"}],
                "blockIds": [],
                "blockPatches": [],
            }),
        )
        .await;
    together.check_bad_request();
    assert_eq!(together.refused_at(), "/containerPatches/0/kind");
    world.ts.teardown().await;
}
