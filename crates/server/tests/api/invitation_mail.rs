//! Existing invitation links sent as individual messages under their original permissions.

use crate::common::{ROOT_WORKSPACE_ID, Setup, TEST_WORKSPACE_ID};
use crate::fake_smtp::FakeSmtp;
use fvoci_model::Invitation;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn invitation_mail_delivers_the_same_link_privately_in_recipient_or_sender_language() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    th.client2
        .patch("/users/me/settings", &json!({"language":"ko"}))
        .await
        .check_status(StatusCode::OK);
    let path = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link: Invitation = th
        .client
        .post(&path, &json!({"role":"member"}))
        .await
        .created_json();
    let before: Vec<Invitation> = th.client.get(&path).await.ok_json();
    let token = link.token.as_deref().expect("token");
    th.client.post(&format!("{path}/mail"), &json!({"token":token,"recipients":["user2@sample.com","outside@example.test"," outside@example.test "],"language":"en"})).await.check_no_content();
    let messages = smtp.received(2).await;
    assert!(th.app.finish_background(Duration::from_secs(5)).await);
    assert_eq!(
        smtp.messages().await.len(),
        2,
        "duplicates receive one message"
    );
    for message in messages {
        assert!(message.decoded.contains(&format!("/invite/{token}")));
        assert!(!message.decoded.contains(&link.id) && !message.decoded.contains(&link.created_by));
        assert!(!message.decoded.contains(ROOT_WORKSPACE_ID));
        if message.recipient == "user2@sample.com" {
            assert!(message.decoded.contains("lang=\"ko\""));
            assert!(!message.message.contains("outside@example.test"));
        } else {
            assert!(message.decoded.contains("lang=\"en\""));
            assert!(!message.message.contains("user2@sample.com"));
        }
    }
    let links: Vec<Invitation> = th.client.get(&path).await.ok_json();
    assert_eq!(
        links, before,
        "mail leaves every invitation and its use count unchanged"
    );
    th.anonymous()
        .post("/invitations/preview", &json!({"token":token}))
        .await
        .check_status(StatusCode::OK);
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn invitation_mail_rejects_unauthorized_foreign_revoked_and_injected_requests() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let path = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link: Invitation = th
        .client
        .post(&path, &json!({"role":"member"}))
        .await
        .created_json();
    let request = json!({"token":link.token,"recipients":["outside@example.test"]});
    th.anonymous()
        .post(&format!("{path}/mail"), &request)
        .await
        .check_status(StatusCode::UNAUTHORIZED);
    th.client2
        .post(&format!("{path}/mail"), &request)
        .await
        .check_status(StatusCode::FORBIDDEN);
    th.client
        .post(
            &format!("/w/{TEST_WORKSPACE_ID}/invitations/mail"),
            &request,
        )
        .await
        .check_status(StatusCode::NOT_FOUND);
    for recipients in [
        json!([]),
        json!(["valid@example.test", "bad..dots@example.test"]),
        json!([
            "valid@example.test",
            "victim@example.test\r\nBcc: other@example.test"
        ]),
    ] {
        th.client
            .post(
                &format!("{path}/mail"),
                &json!({"token":link.token,"recipients":recipients}),
            )
            .await
            .check_status(StatusCode::BAD_REQUEST);
    }
    th.client
        .delete(&format!("{path}/{}", link.id))
        .await
        .check_no_content();
    th.client
        .post(&format!("{path}/mail"), &request)
        .await
        .check_status(StatusCode::NOT_FOUND);
    assert!(th.app.finish_background(Duration::from_secs(1)).await);
    assert!(smtp.messages().await.is_empty());
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn invitation_mail_sends_guest_links_and_stops_when_guests_are_disabled() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let board = th
        .create_board(ROOT_WORKSPACE_ID, fvoci_model::BoardType::Open)
        .await;
    let path = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link: Invitation = th
        .client
        .post(&path, &json!({"role":"guest","boardId":board.id}))
        .await
        .created_json();
    let request = json!({"token":link.token,"recipients":["guest@example.test"],"language":"en"});
    th.client
        .post(&format!("{path}/mail"), &request)
        .await
        .check_no_content();
    let mail = smtp.received(1).await;
    assert!(mail[0].decoded.contains("one person"));
    assert!(!mail[0].decoded.contains(&board.id));
    assert_eq!(link.max_uses, Some(1));
    th.client
        .put(
            &format!("/w/{ROOT_WORKSPACE_ID}/guest-invitations"),
            &json!({"enabled":false}),
        )
        .await
        .check_status(StatusCode::OK);
    th.client
        .post(&format!("{path}/mail"), &request)
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn invitation_mail_requires_smtp_but_plain_links_still_work() {
    let th = Setup::native().start().await.init_basic().await;
    let path = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link: Invitation = th
        .client
        .post(&path, &json!({"role":"member"}))
        .await
        .created_json();
    th.client
        .post(
            &format!("{path}/mail"),
            &json!({"token":link.token,"recipients":["outside@example.test"]}),
        )
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.anonymous()
        .post("/invitations/preview", &json!({"token":link.token}))
        .await
        .check_status(StatusCode::OK);
    th.teardown().await;
}

#[tokio::test]
async fn invitation_mail_cannot_use_another_organizations_token() {
    // Several organizations are a PostgreSQL deployment; SQLite refuses that mode.
    if fvoci_store::testing::db_type_of_the_tests() != fvoci_store::DbType::Postgres {
        return;
    }
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .configure(|config| {
            "open".clone_into(&mut config.org_creation);
        })
        .start()
        .await
        .init_basic()
        .await;
    let mut other = th.anonymous();
    other.post("/register", &json!({"organizationName":"Another organization","username":"mail-other-org","email":"other-org@example.test","password":"Other-password"})).await.check_status(StatusCode::CREATED);
    other
        .login("mail-other-org", "Other-password")
        .await
        .check_no_content();
    let workspaces: Vec<fvoci_model::UserWorkspace> =
        other.get("/users/me/workspaces").await.ok_json();
    let own = &workspaces[0].id;
    let link: Invitation = other
        .post(&format!("/w/{own}/invitations"), &json!({"role":"member"}))
        .await
        .created_json();
    let request = json!({"token":link.token,"recipients":["outside@example.test"]});
    th.client
        .post(
            &format!("/w/{ROOT_WORKSPACE_ID}/invitations/mail"),
            &request,
        )
        .await
        .check_status(StatusCode::NOT_FOUND);
    th.client
        .post(&format!("/w/{own}/invitations/mail"), &request)
        .await
        .check_status(StatusCode::NOT_FOUND);
    assert!(smtp.messages().await.is_empty());
    th.teardown().await;
    smtp.stop().await;
}

#[tokio::test]
async fn invitation_mail_preserves_distinct_case_sensitive_mailboxes() {
    let smtp = FakeSmtp::start_on(0).await;
    let th = Setup::native()
        .smtp(smtp.settings())
        .start()
        .await
        .init_basic()
        .await;
    let path = format!("/w/{ROOT_WORKSPACE_ID}/invitations");
    let link: Invitation = th
        .client
        .post(&path, &json!({"role":"member"}))
        .await
        .created_json();
    th.client
        .post(
            &format!("{path}/mail"),
            &json!({
                "token":link.token,
                "recipients":["Smith@example.test", "smith@example.test", " Smith@example.test "]
            }),
        )
        .await
        .check_no_content();
    smtp.received(1).await;
    assert!(th.app.finish_background(Duration::from_secs(5)).await);
    let mut recipients: Vec<String> = smtp
        .messages()
        .await
        .into_iter()
        .map(|message| message.recipient)
        .collect();
    recipients.sort();
    assert_eq!(
        recipients,
        ["Smith@example.test", "smith@example.test"],
        "local-part case identifies a mailbox; only exact duplicates collapse"
    );
    th.teardown().await;
    smtp.stop().await;
}
