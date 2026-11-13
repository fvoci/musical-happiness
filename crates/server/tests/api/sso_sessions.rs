//! The sessions made through a provider end with what made them: the identity of their
//! issuer unlinked from the account, or the organization's provider removed. Their
//! connections close with them, and leave the rooms of the cards' bodies they were in.

use fvoci_model::LinkedIdentity;
use serde_json::json;

use crate::common::{Client, PASSWORD, ROOT_WORKSPACE_ID, TEST_WORKSPACE_ID, TestServer};
use crate::fake_oidc::FakeProvider;
use crate::oidc::{org_of, server_for, set_provider};
use crate::rooms::{Incoming, Peer, paragraphs, table};
use crate::sso::admin_through_provider;
use crate::websocket::{Event, PATIENCE, QUIET, Socket, subscribe_to_workspace};

/// What one session holds open: a connection that follows the root workspace, and one
/// in the room of a card's body.
struct Open {
    socket: Socket,
    peer: Peer,
}

impl Open {
    async fn of(ts: &TestServer, client: &Client, card_id: &str, editor: u64) -> Self {
        let user_id = client.me().await.id;
        let mut socket = Socket::connect_as(client).await;
        subscribe_to_workspace(ts, &mut socket, ROOT_WORKSPACE_ID, &user_id).await;
        let mut peer = Peer::connect(client, editor).await;
        peer.joined(TEST_WORKSPACE_ID, card_id).await;
        Self { socket, peer }
    }

    /// Whether the server closed both connections, after what was on its way to them.
    async fn closed(&mut self) -> bool {
        let socket = loop {
            match self.socket.next(PATIENCE).await {
                Event::Closed => break true,
                Event::Nothing => break false,
                Event::Json(_) | Event::Ping => {}
            }
        };
        let peer = loop {
            match self.peer.incoming(PATIENCE).await {
                Incoming::Closed => break true,
                Incoming::Nothing => break false,
                Incoming::Text(_) | Incoming::Frame(_) => {}
            }
        };
        socket && peer
    }

    /// Whether both connections are still open: nothing closes them for a while.
    async fn open(&mut self) -> bool {
        let socket = !matches!(self.socket.next(QUIET).await, Event::Closed);
        let peer = loop {
            match self.peer.incoming(QUIET).await {
                Incoming::Closed => break false,
                Incoming::Nothing => break true,
                Incoming::Text(_) | Incoming::Frame(_) => {}
            }
        };
        socket && peer
    }
}

async fn identities(client: &Client) -> Vec<LinkedIdentity> {
    client.get("/users/me/identities").await.ok_json()
}

async fn unlink(client: &Client, identity: &LinkedIdentity) {
    client
        .delete_json(
            "/users/me/identities",
            &json!({
                "issuer": identity.issuer, "subject": identity.subject, "password": PASSWORD,
            }),
        )
        .await
        .check_no_content();
}

/// Unlinked from a password session, the identity takes the account's sessions made
/// through its issuer along, with their connections and their seats in the rooms; the
/// password session, which said it was not one of them, stays where it was.
#[tokio::test]
async fn unlinking_ends_the_sessions_of_that_issuer_and_what_they_had_open() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let through = admin_through_provider(&ts, &fake, &org_id).await;
    let tied = identities(&ts.client).await;
    assert_eq!(tied.len(), 1, "{tied:?}");
    assert!(!tied[0].made_this_session, "the password's session");
    assert!(
        identities(&through).await[0].made_this_session,
        "the provider's"
    );

    let card = table(&ts, &paragraphs(&["first"])).await.card_id;
    let mut of_password = Open::of(&ts, &ts.client, &card, 101).await;
    let mut of_provider = Open::of(&ts, &through, &card, 102).await;
    unlink(&ts.client, &tied[0]).await;

    assert!(of_provider.closed().await, "its connections closed");
    through.get("/users/me").await.check_unauthorized();
    assert!(of_password.open().await, "the password's stay");
    ts.client.get("/users/me").await.check_ok();
    of_password.peer.write_acked("still in the room").await;
    ts.teardown().await;
    fake.stop();
}

/// Unlinked from the session made through its issuer, the identity ends that session
/// too: the request is answered, and the session is over after it.
#[tokio::test]
async fn unlinking_from_the_session_of_that_issuer_ends_it() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let through = admin_through_provider(&ts, &fake, &org_id).await;
    let mine = identities(&through).await.remove(0);
    assert!(mine.made_this_session);

    unlink(&through, &mine).await;

    through.get("/users/me").await.check_unauthorized();
    ts.client.get("/users/me").await.check_ok();
    assert_eq!(identities(&ts.client).await, []);
    ts.teardown().await;
    fake.stop();
}

/// Removing the organization's provider ends the sessions made through it, of every
/// person, with their connections; a password's go on.
#[tokio::test]
async fn removing_the_provider_ends_its_sessions_and_what_they_had_open() {
    let fake = FakeProvider::start().await;
    let ts = server_for(&fake).await;
    let org_id = org_of(&ts).await;
    set_provider(&ts, &org_id, &fake).await;
    let through = admin_through_provider(&ts, &fake, &org_id).await;
    let card = table(&ts, &paragraphs(&["first"])).await.card_id;
    let mut of_password = Open::of(&ts, &ts.client, &card, 101).await;
    let mut of_provider = Open::of(&ts, &through, &card, 102).await;

    ts.client
        .delete(&format!("/org/{org_id}/oidc"))
        .await
        .check_no_content();

    assert!(of_provider.closed().await, "its connections closed");
    through.get("/users/me").await.check_unauthorized();
    assert!(of_password.open().await, "the password's stay");
    ts.teardown().await;
    fake.stop();
}
