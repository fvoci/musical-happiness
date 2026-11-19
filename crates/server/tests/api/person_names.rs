//! A person's name: its parts, which of them they are shown by, their title and
//! credentials, changed by them (`patchMe`) and seen by whoever sees them.

use fvoci_model::{Credential, NameShown, Title, User};
use serde_json::{Value, json};

use crate::common::{ROOT_WS, Setup};

/// A person writes the parts of their name and chooses one to be shown by; whoever sees
/// them sees every part, and the name they chose.
#[tokio::test]
async fn a_person_names_themselves_and_others_see_the_name_they_chose() {
    let th = Setup::native().start().await.init_basic().await;
    let kim: User = th
        .client2
        .patch(
            "/users/me",
            &json!({
                "firstName": "영환", "lastName": "김", "latinFirstName": "Younghwan",
                "latinLastName": "Kim", "customName": "YH", "title": "dr",
                "credentials": ["phd", "kr_attorney"],
            }),
        )
        .await
        .ok_json();
    assert_eq!(
        kim.display_name, "김영환",
        "a new account is shown by its name"
    );
    assert_eq!(kim.name.title, Some(Title::Dr));
    assert_eq!(
        kim.name.credentials,
        [Credential::Phd, Credential::KrAttorney]
    );

    let profile: Value = th
        .client
        .get(&format!("{ROOT_WS}/users/{}", kim.id))
        .await
        .value();
    for (member, value) in [
        ("displayName", json!("김영환")),
        ("firstName", json!("영환")),
        ("latinLastName", json!("Kim")),
        ("customName", json!("YH")),
        ("nameShown", json!("name")),
        ("title", json!("dr")),
        ("credentials", json!(["phd", "kr_attorney"])),
    ] {
        assert_eq!(profile[member], value, "{member} of {profile}");
    }

    for (shown, want) in [
        ("latin_name", "Younghwan Kim"),
        ("custom_name", "YH"),
        ("username", ""),
    ] {
        let changed: User = th
            .client2
            .patch("/users/me", &json!({ "nameShown": shown }))
            .await
            .ok_json();
        assert_eq!(changed.display_name, want, "{shown}");
        let other: Vec<User> = th
            .client
            .post(&format!("{ROOT_WS}/users"), &[&kim.id])
            .await
            .ok_json();
        assert_eq!(other[0].display_name, want, "others see it: {shown}");
    }
    let cleared: User = th
        .client2
        .patch(
            "/users/me",
            &json!({"nameShown": "latin_name", "latinFirstName": null, "latinLastName": "  "}),
        )
        .await
        .ok_json();
    assert_eq!(cleared.display_name, "YH", "down the list");
    assert_eq!(cleared.name.name_shown, NameShown::LatinName);

    // Found by every part of the name.
    for query in ["영환", "김영환", "YH"] {
        let found: Vec<User> = th
            .client
            .get(&format!("{ROOT_WS}/users?search={query}"))
            .await
            .ok_json();
        assert_eq!(found.len(), 1, "{query}: {found:?}");
        assert_eq!(found[0].id, kim.id);
    }
    th.teardown().await;
}

/// A title or a credential that is no code of its list, the same credential twice, more
/// than three, or a choice that is none are refused, naming each, and change nothing.
#[tokio::test]
async fn codes_off_their_lists_are_refused() {
    let th = Setup::native().start().await.init_basic().await;
    let before = th.client2.me().await;
    for (body, pointers) in [
        (json!({"title": "sir"}), vec!["/title"]),
        (json!({"title": ["dr"]}), vec!["/title"]),
        (
            json!({"credentials": ["phd", "esq"]}),
            vec!["/credentials/1"],
        ),
        (
            json!({"credentials": ["phd", "phd"]}),
            vec!["/credentials/1"],
        ),
        (
            json!({"credentials": ["phd", "md", "mba", "ms"]}),
            vec!["/credentials"],
        ),
        (json!({"credentials": "phd"}), vec!["/credentials"]),
        (json!({"nameShown": "nickname"}), vec!["/nameShown"]),
        (
            json!({"firstName": "a\u{200B}", "displayName": "x", "title": "dr"}),
            vec!["/displayName", "/firstName"],
        ),
    ] {
        let response = th.client2.patch("/users/me", &body).await;
        response.check_bad_request();
        assert_eq!(
            response.refused_places(),
            pointers,
            "{body}: {}",
            response.text()
        );
    }
    assert_eq!(th.client2.me().await, before, "nothing changed");
    th.teardown().await;
}
