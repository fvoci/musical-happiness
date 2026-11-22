//! Diaries through the routes: a space has its first diary once and so many at most, a
//! day has one entry however it is asked for, a day is a day of the calendar that is
//! not far ahead, an entry is changed, deleted and brought back to a day that is free,
//! and a diary goes into an archive and comes out of one as a new diary, in a space
//! alone.

use fvoci_model::{DIARIES_PER_SPACE, DiaryDate};
use reqwest::StatusCode;
use serde_json::{Value, json};

use crate::common::{ApiResponse, Client, ROOT_WORKSPACE_ID, Setup, TestServer, UNKNOWN_ID};

/// A server, and the path of the workspace of its first user's own space.
async fn in_a_space() -> (TestServer, String) {
    let th = Setup::local_mode().start().await.init_basic().await;
    let own = crate::own_space::own_workspace(&th.client).await.id;
    (th, format!("/w/{own}"))
}

fn id_of(answer: &ApiResponse) -> String {
    text_of(&answer.value(), "id")
}

fn text_of(value: &Value, member: &str) -> String {
    value[member]
        .as_str()
        .unwrap_or_else(|| panic!("{member} in {value}"))
        .to_owned()
}

/// Makes a diary in the space and returns its path.
async fn a_diary(client: &Client, ws: &str) -> String {
    let made = client.post(&format!("{ws}/diaries"), &json!({})).await;
    made.check_created();
    format!("{ws}/diaries/{}", id_of(&made))
}

/// The identifier a path ends with.
fn last_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Makes the entry of the day, which had none, and returns it.
async fn written(client: &Client, diary: &str, entry: &Value) -> Value {
    let made = client.post(&format!("{diary}/entries"), entry).await;
    made.check_created();
    made.value()
}

fn body(text: &str) -> Value {
    json!({"type": "doc", "content": [
        {"type": "paragraph", "content": [{"type": "text", "text": text}]},
    ]})
}

/// Writes the body of the entry `made`, as who read it as it was made.
async fn write_body(client: &Client, entry: &str, made: &Value, text: &str) -> Value {
    let patch = json!({"readAt": made["updateAt"], "body": body(text)});
    let patched = client.patch(entry, &patch).await;
    patched.check_ok();
    patched.value()
}

/// The days of the diary that are written, as `query` asks, or those of 2024 (the year
/// the tests write in) where it asks nothing: each day and its entry.
async fn days(client: &Client, diary: &str, query: &str) -> Vec<(String, String)> {
    let query = match query {
        "" => "?from=2024-01-01&to=2024-12-31",
        query => query,
    };
    let listed = client.get(&format!("{diary}/days{query}")).await;
    listed.check_ok();
    let listed = listed.value();
    let listed = listed.as_array().expect("a list");
    listed
        .iter()
        .map(|day| (text_of(day, "date"), text_of(day, "entryId")))
        .collect()
}

/// The statuses of two answers, the lower first.
fn both(one: &ApiResponse, other: &ApiResponse) -> [StatusCode; 2] {
    let mut statuses = [one.status, other.status];
    statuses.sort();
    statuses
}

/// What opens "the diary" of a space makes it once, whoever asks first, and a space
/// has so many diaries: one more is refused with the limit, until one goes to the
/// trash.
#[tokio::test]
async fn a_space_has_its_first_diary_once_and_so_many_at_most() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diaries = format!("{ws}/diaries");
    let first = json!({"ifNone": true});
    let (one, other) = tokio::join!(client.post(&diaries, &first), client.post(&diaries, &first));
    assert_eq!(both(&one, &other), [StatusCode::OK, StatusCode::CREATED]);
    assert_eq!(id_of(&one), id_of(&other), "one diary for both");
    let made = one.value();
    assert_eq!(
        (&made["kind"], &made["type"], &made["title"]),
        (&json!("diary"), &json!("P"), &json!("")),
        "{made}"
    );
    let again = client.post(&diaries, &first).await;
    again.check_ok();
    assert_eq!(id_of(&again), id_of(&one));
    // It is its owner's, as a board they made: among their roles.
    let memberships = client.get("/users/me/memberships").await.text();
    assert!(memberships.contains(&id_of(&one)), "{memberships}");

    let unknown = client.post(&diaries, &json!({"kind": "wiki"})).await;
    unknown.check_bad_request();
    for number in 1..DIARIES_PER_SPACE {
        let more = json!({"title": format!("Volume {number}")});
        client.post(&diaries, &more).await.check_created();
    }
    let listed = client.get(&diaries).await.value();
    let most = usize::try_from(DIARIES_PER_SPACE).expect("a count");
    assert_eq!(listed.as_array().map(Vec::len), Some(most));
    assert_eq!(listed[0]["id"], made["id"], "the oldest first");
    let over = client.post(&diaries, &json!({})).await;
    over.check_bad_request();
    assert_eq!(over.error_reason(), "too_many_diaries", "{}", over.text());
    assert_eq!(over.value()["limit"], json!(DIARIES_PER_SPACE));
    // One in the trash is not counted, and does not come back to a full space.
    let trashed = format!("{ws}/containers/{}", id_of(&one));
    let deleted = client.delete(&trashed).await;
    deleted.check_status(StatusCode::NO_CONTENT);
    client.post(&diaries, &json!({})).await.check_created();
    let back = client
        .post(&format!("{trashed}/undelete"), &json!({}))
        .await;
    back.check_bad_request();
    assert_eq!(back.error_reason(), "too_many_diaries", "{}", back.text());
    th.teardown().await;
}

/// A day has one entry: who asks for the entry of a day that has one is answered with
/// it, and nothing of the request is stored.
#[tokio::test]
async fn a_day_has_one_entry_whoever_asks_for_it() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let entries = format!("{diary}/entries");
    let leap = json!({"date": "2024-02-29", "mood": "good", "weather": "맑음", "place": "집"});
    let (one, other) = tokio::join!(client.post(&entries, &leap), client.post(&entries, &leap));
    assert_eq!(both(&one, &other), [StatusCode::OK, StatusCode::CREATED]);
    assert_eq!(id_of(&one), id_of(&other), "one entry for both");
    let again = json!({"date": "2024-02-29", "mood": "awful", "weather": "비"});
    let there = client.post(&entries, &again).await;
    there.check_ok();
    let entry = there.value();
    assert_eq!(
        (&entry["mood"], &entry["weather"], &entry["place"]),
        (&json!("good"), &json!("맑음"), &json!("집")),
        "the entry as it was: {entry}"
    );
    assert_eq!(
        (&entry["diaryId"], &entry["body"], &entry["date"]),
        (&json!(last_of(&diary)), &Value::Null, &json!("2024-02-29")),
        "{entry}"
    );
    let listed = format!("{diary}/days?from=2024-02-01&to=2024-02-29");
    let listed = client.get(&listed).await;
    assert_eq!(
        listed.value(),
        json!([{"date": "2024-02-29", "entryId": id_of(&one), "mood": "good"}]),
        "a day says its entry and its mood, and nothing else"
    );
    th.teardown().await;
}

/// The day of an entry is a day of the calendar, written as one, and no more than two
/// days after the server's own; the mood, the weather and the place are what they may
/// be, and an entry is made with nothing else.
#[tokio::test]
async fn the_day_of_an_entry_is_a_day_of_the_calendar_not_far_ahead() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let entries = format!("{diary}/entries");
    for date in [
        "2026-02-30",
        "2025-02-29",
        "2026-13-01",
        "2026-00-10",
        "0000-01-01",
        "20260101",
        "2026-1-1",
        "2026-11-21T00:00:00Z",
        " 2026-11-21",
        "어제",
        "",
    ] {
        let refused = client.post(&entries, &json!({ "date": date })).await;
        refused.check_bad_request();
        assert_eq!(refused.refused_at(), "/date", "{date}: {}", refused.text());
    }
    for (member, value) in [
        ("date", json!(20_261_121)),
        ("mood", json!("ecstatic")),
        ("mood", json!(3)),
        ("weather", json!("가".repeat(101))),
        ("place", json!("p".repeat(101))),
        ("title", json!("a title of its own")),
        ("body", body("not written here")),
    ] {
        let mut entry = json!({"date": "2024-03-01"});
        entry[member] = value;
        client.post(&entries, &entry).await.check_bad_request();
    }
    let longest = json!({"date": "2024-03-01", "weather": "가".repeat(100), "mood": null});
    written(client, &diary, &longest).await;

    // The server does not know where the writer is: every time zone's today is within
    // two days of its own day.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("a time after 1970");
    let now = i64::try_from(now.as_millis()).expect("milliseconds");
    let today = DiaryDate::of_utc_millis(now).expect("today");
    let ahead = |days: i32| today.plus_days(days).expect("a day").to_string();
    for days in [3, 400] {
        let refused = client.post(&entries, &json!({"date": ahead(days)})).await;
        refused.check_bad_request();
        assert_eq!(refused.refused_at(), "/date", "{}", refused.text());
    }
    for days in [-1, 0, 2] {
        written(client, &diary, &json!({"date": ahead(days)})).await;
    }
    written(client, &diary, &json!({"date": "1987-06-10"})).await;
    for (from, to, written) in [
        (ahead(-1), ahead(2), 3),
        ("1987-06-10".to_owned(), "1987-06-10".to_owned(), 1),
        ("2024-03-01".to_owned(), "2024-03-01".to_owned(), 1),
    ] {
        let listed = days(client, &diary, &format!("?from={from}&to={to}")).await;
        assert_eq!(listed.len(), written, "from {from} to {to}");
    }
    th.teardown().await;
}

/// The page of a day lists its entries in every year, the latest first, and the 29th of
/// February those of leap years; the days that are written are listed the earliest
/// first, as far as they are asked.
#[tokio::test]
async fn the_days_of_a_diary_are_read_by_month_and_day_and_by_range() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    for date in ["2023-02-28", "2020-02-29", "2024-02-29", "2020-02-28"] {
        written(client, &diary, &json!({ "date": date })).await;
    }
    let other = a_diary(client, &ws).await;
    written(client, &other, &json!({"date": "2016-02-29"})).await;
    let on = async |month_day: &str| {
        let listed = client.get(&format!("{diary}/days/{month_day}")).await;
        listed.check_ok();
        let listed = listed.value();
        let listed = listed.as_array().expect("a list");
        let dates: Vec<String> = listed.iter().map(|entry| text_of(entry, "date")).collect();
        dates
    };
    assert_eq!(on("02-29").await, ["2024-02-29", "2020-02-29"]);
    assert_eq!(on("02-28").await, ["2023-02-28", "2020-02-28"]);
    assert_eq!(on("12-25").await, [""; 0]);
    for no_day in ["02-30", "2-29", "13-01", "0229", "2024-02-29", "today"] {
        let refused = client.get(&format!("{diary}/days/{no_day}")).await;
        refused.check_bad_request();
        assert_eq!(refused.refused_at(), "parameter monthDay", "{no_day}");
    }

    let dates = async |query: &str| -> Vec<String> {
        let listed = days(client, &diary, query).await;
        listed.into_iter().map(|(date, _)| date).collect()
    };
    assert_eq!(
        dates("?from=2020-01-01&to=2020-12-31").await,
        ["2020-02-28", "2020-02-29"]
    );
    assert_eq!(
        dates("?from=2024-02-01&to=2024-02-29").await,
        ["2024-02-29"]
    );
    // A year at most, both of its ends counted; a range that ends before it begins is
    // of no days.
    assert_eq!(
        dates("?from=2023-02-28&to=2024-02-28").await,
        ["2023-02-28"]
    );
    assert_eq!(dates("?from=2024-03-01&to=2024-03-31").await, [""; 0]);
    assert_eq!(dates("?from=2024-03-01&to=2020-01-01").await, [""; 0]);
    for (query, place) in [
        ("?from=2024-02-30&to=2024-03-01", "from"),
        ("?from=2024-02-01&to=march", "to"),
        ("", "from"),
        ("?to=2024-02-01", "from"),
        ("?from=2024-02-01", "to"),
        ("?from=2023-02-28&to=2024-02-29", "to"),
    ] {
        let refused = client.get(&format!("{diary}/days{query}")).await;
        refused.check_bad_request();
        assert_eq!(
            refused.refused_at(),
            format!("parameter {place}"),
            "{query}"
        );
    }
    let too_wide = format!("{diary}/days?from=2023-02-28&to=2024-02-29");
    let too_wide = client.get(&too_wide).await.value();
    assert_eq!(too_wide["limit"], json!(fvoci_model::DAYS_PER_LIST));
    th.teardown().await;
}

/// The runs of a diary are its written days one after the other, across the ends of
/// months and years and through 29 February, of that diary alone; an entry in the trash
/// breaks its run, and one brought back joins it again.
#[tokio::test]
async fn the_runs_of_a_diary_are_its_written_days_one_after_the_other() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let runs = async || -> Vec<(String, String, u64)> {
        let listed = client.get(&format!("{diary}/runs")).await;
        listed.check_ok();
        let listed = listed.value();
        let listed = listed.as_array().expect("a list");
        listed
            .iter()
            .map(|run| {
                let days = run["days"].as_u64().expect("a count");
                (text_of(run, "from"), text_of(run, "to"), days)
            })
            .collect()
    };
    let run = |from: &str, to: &str, days: u64| (from.to_owned(), to.to_owned(), days);
    assert_eq!(runs().await, []);
    let mut made = Vec::new();
    for date in [
        "2024-03-03",
        "2023-12-31",
        "2024-02-28",
        "2024-01-01",
        "2024-02-29",
        "2024-03-01",
        "2025-02-28",
        "2025-03-01",
    ] {
        made.push(written(client, &diary, &json!({ "date": date })).await);
    }
    let other = a_diary(client, &ws).await;
    written(client, &other, &json!({"date": "2024-03-02"})).await;
    assert_eq!(
        runs().await,
        [
            run("2023-12-31", "2024-01-01", 2),
            run("2024-02-28", "2024-03-01", 3),
            run("2024-03-03", "2024-03-03", 1),
            // 2025 has no 29 February.
            run("2025-02-28", "2025-03-01", 2),
        ]
    );
    let leap = format!("{diary}/entries/{}", text_of(&made[4], "id"));
    client
        .delete(&leap)
        .await
        .check_status(StatusCode::NO_CONTENT);
    assert_eq!(
        runs().await,
        [
            run("2023-12-31", "2024-01-01", 2),
            run("2024-02-28", "2024-02-28", 1),
            run("2024-03-01", "2024-03-01", 1),
            run("2024-03-03", "2024-03-03", 1),
            run("2025-02-28", "2025-03-01", 2),
        ]
    );
    client
        .post(&format!("{leap}/undelete"), &json!({}))
        .await
        .check_ok();
    assert_eq!(runs().await[1], run("2024-02-28", "2024-03-01", 3));
    th.teardown().await;
}

/// The body and the notes of a day go in one patch under one `readAt`, as a page sends
/// them when it goes: both are written, or, where the entry changed since it was read,
/// neither.
#[tokio::test]
async fn the_body_and_the_notes_are_written_together_or_not_at_all() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let made = written(client, &diary, &json!({"date": "2024-03-01"})).await;
    let entry = format!("{diary}/entries/{}", text_of(&made, "id"));
    let together = json!({
        "readAt": made["updateAt"], "body": body("함께"), "mood": "good", "weather": "비",
    });
    let both = client.patch(&entry, &together).await.value();
    assert_eq!(
        (&both["body"], &both["mood"], &both["weather"]),
        (&body("함께"), &json!("good"), &json!("비"))
    );
    let stale = json!({
        "readAt": made["updateAt"], "body": body("늦게"), "mood": "awful", "place": "집",
    });
    client
        .patch(&entry, &stale)
        .await
        .check_status(StatusCode::CONFLICT);
    let read = client.get(&entry).await.value();
    assert_eq!(
        (&read["body"], &read["mood"], &read["place"]),
        (&body("함께"), &json!("good"), &json!(""))
    );
    th.teardown().await;
}

/// An entry is changed by its patch, which has no member for its day, and its body is
/// kept in versions as a card's is. The entry of another diary, and of nothing, is no
/// entry of this one.
#[tokio::test]
async fn an_entry_is_changed_by_its_patch_and_keeps_its_day() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let made = written(
        client,
        &diary,
        &json!({"date": "2024-02-29", "mood": "bad"}),
    )
    .await;
    let id = text_of(&made, "id");
    let entry = format!("{diary}/entries/{id}");

    let noted = json!({"mood": "great", "weather": "눈", "place": "속초"});
    let patched = client.patch(&entry, &noted).await;
    patched.check_ok();
    let patched = patched.value();
    assert_eq!(
        (&patched["mood"], &patched["weather"], &patched["place"]),
        (&json!("great"), &json!("눈"), &json!("속초"))
    );
    let cleared = json!({"mood": null, "place": ""});
    let cleared = client.patch(&entry, &cleared).await.value();
    assert_eq!(
        (&cleared["mood"], &cleared["weather"], &cleared["place"]),
        (&Value::Null, &json!("눈"), &json!(""))
    );
    for refused in [
        json!({"date": "2024-03-01"}),
        json!({"title": "2024-03-01"}),
        json!({"fields": {"date": "2024-03-01"}}),
        json!({"mood": "ecstatic"}),
        json!({"weather": "w".repeat(101)}),
        json!({"body": body("without having read it")}),
    ] {
        client.patch(&entry, &refused).await.check_bad_request();
    }
    let read = client.get(&entry).await.value();
    assert_eq!(read["date"], "2024-02-29", "the day is what it was");

    // The body, written whole by who read the entry as it is.
    let with_body = write_body(client, &entry, &read, "첫 줄").await;
    assert_eq!(with_body["body"], body("첫 줄"));
    let stale = json!({"readAt": read["updateAt"], "body": body("늦은 줄")});
    let late = client.patch(&entry, &stale).await;
    late.check_status(StatusCode::CONFLICT);
    write_body(client, &entry, &with_body, "둘째 줄").await;
    let versions = client.get(&format!("{entry}/body-versions")).await;
    versions.check_ok();
    let versions = versions.value();
    let numbers = versions.as_array().expect("a list").iter();
    let oldest = numbers
        .filter_map(|version| version["version"].as_i64())
        .min()
        .unwrap_or_else(|| panic!("the body has versions: {versions}"));
    let version = format!("{entry}/body-versions/{oldest}");
    let kept = client.get(&version).await;
    kept.check_ok();
    let restored = client.post(&format!("{version}/restore"), &json!({})).await;
    restored.check_ok();
    assert_eq!(restored.value()["body"], kept.value()["body"]);
    assert_eq!(restored.value()["date"], "2024-02-29");
    let no_version = client.get(&format!("{entry}/body-versions/999")).await;
    no_version.check_not_found();

    let other = a_diary(client, &ws).await;
    for elsewhere in [
        format!("{other}/entries/{id}"),
        format!("{diary}/entries/{UNKNOWN_ID}"),
        format!("{ws}/diaries/{UNKNOWN_ID}/entries/{id}"),
    ] {
        client.get(&elsewhere).await.check_not_found();
        client.patch(&elsewhere, &noted).await.check_not_found();
        client.delete(&elsewhere).await.check_not_found();
    }
    assert_eq!(days(client, &other, "").await, []);
    th.teardown().await;
}

/// An entry in the trash holds no day: the day takes another entry, and then the first
/// does not come back until the day is free again.
#[tokio::test]
async fn an_entry_in_the_trash_holds_no_day_and_comes_back_to_a_free_one() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let made = written(
        client,
        &diary,
        &json!({"date": "2024-02-29", "mood": "bad"}),
    )
    .await;
    let id = text_of(&made, "id");
    let entry = format!("{diary}/entries/{id}");
    let with_body = write_body(client, &entry, &made, "남겨 둔 글").await;

    let deleted = client.delete(&entry).await;
    deleted.check_status(StatusCode::NO_CONTENT);
    client.get(&entry).await.check_not_found();
    assert_eq!(days(client, &diary, "").await, []);
    let trash = client.get(&format!("{diary}/trash/entries")).await;
    trash.check_ok();
    let trash = trash.value();
    assert_eq!(trash.as_array().map(Vec::len), Some(1), "{trash}");
    assert_eq!(
        (&trash[0]["id"], &trash[0]["date"], &trash[0]["mood"]),
        (&json!(id), &json!("2024-02-29"), &json!("bad")),
        "{trash}"
    );
    let anew = written(client, &diary, &json!({"date": "2024-02-29"})).await;
    assert_ne!(anew["id"], json!(id), "a new entry of the day");
    let undelete = format!("{entry}/undelete");
    let taken = client.post(&undelete, &json!({})).await;
    taken.check_status(StatusCode::CONFLICT);
    assert_eq!(taken.error_reason(), "day_taken", "{}", taken.text());
    assert_eq!(taken.problem_type(), "/problems/day-taken");
    let the_new = format!("{diary}/entries/{}", text_of(&anew, "id"));
    let deleted = client.delete(&the_new).await;
    deleted.check_status(StatusCode::NO_CONTENT);
    let back = client.post(&undelete, &json!({})).await;
    back.check_ok();
    assert_eq!(back.value()["body"], with_body["body"], "with its body");
    assert_eq!(
        days(client, &diary, "").await,
        [("2024-02-29".to_owned(), id.clone())]
    );
    client.post(&undelete, &json!({})).await.check_not_found();
    let nothing = format!("{diary}/entries/{UNKNOWN_ID}/undelete");
    client.post(&nothing, &json!({})).await.check_not_found();
    th.teardown().await;
}

/// A diary of the space with the entries of two days, one with a body, and an entry in
/// the trash: its path, the entry with the body, and its archive.
async fn an_archived_diary(client: &Client, ws: &str) -> (String, Value, Vec<u8>) {
    let diary = a_diary(client, ws).await;
    let first = written(
        client,
        &diary,
        &json!({"date": "2024-02-29", "mood": "good"}),
    )
    .await;
    let entry = format!("{diary}/entries/{}", text_of(&first, "id"));
    write_body(client, &entry, &first, "보름달을 봤다").await;
    written(
        client,
        &diary,
        &json!({"date": "2024-03-01", "place": "강릉"}),
    )
    .await;
    let gone = written(client, &diary, &json!({"date": "2024-03-02"})).await;
    let gone = format!("{diary}/entries/{}", text_of(&gone, "id"));
    let deleted = client.delete(&gone).await;
    deleted.check_status(StatusCode::NO_CONTENT);
    let exported = format!("{ws}/containers/{}/archive/export", last_of(&diary));
    let exported = client.get(&exported).await;
    exported.check_ok();
    (diary, first, exported.body)
}

/// A diary goes into an archive with its entries and comes out of one as a new diary of
/// the space, beside the one it was taken of.
#[tokio::test]
async fn a_diary_is_archived_and_comes_back_as_a_new_one() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let own = last_of(&ws).to_owned();
    let (diary, first, archive) = an_archived_diary(client, &ws).await;
    crate::export::import(client, &own, &archive)
        .await
        .check_ok();
    let listed = client.get(&format!("{ws}/diaries")).await.value();
    let listed = listed.as_array().expect("a list");
    assert_eq!(listed.len(), 2, "a new diary beside the first");
    let copy = listed
        .iter()
        .find(|listed| listed["id"] != json!(last_of(&diary)))
        .expect("the new one");
    assert_eq!(copy["kind"], "diary", "{copy}");
    let copy = format!("{ws}/diaries/{}", text_of(copy, "id"));
    let (of_first, of_copy) = (
        days(client, &diary, "").await,
        days(client, &copy, "").await,
    );
    let dates = |days: &[(String, String)]| -> Vec<String> {
        days.iter().map(|(date, _)| date.clone()).collect()
    };
    assert_eq!(dates(&of_copy), ["2024-02-29", "2024-03-01"]);
    assert_eq!(dates(&of_copy), dates(&of_first));
    assert!(
        of_copy.iter().all(|day| !of_first.contains(day)),
        "the entries of the copy are its own: {of_copy:?}"
    );
    let read = client.get(&format!("{copy}/days/02-29")).await.value();
    assert_eq!(
        (&read[0]["mood"], &read[0]["body"]),
        (&json!("good"), &body("보름달을 봤다")),
        "{read}"
    );
    // What is asked of one diary is of that diary alone.
    let on_the_day = client.get(&format!("{diary}/days/02-29")).await.value();
    assert_eq!(on_the_day.as_array().map(Vec::len), Some(1));
    assert_eq!(on_the_day[0]["id"], first["id"]);
    th.teardown().await;
}

/// The files of an archive, each with its lines.
fn archive_lines(bytes: &[u8]) -> Vec<(String, Vec<Value>)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("an archive");
    let mut files = Vec::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("a file");
        let name = file.name().expect("its name").into_owned();
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text).expect("its text");
        let lines = text
            .lines()
            .map(|line| serde_json::from_str(line).expect("a line of JSON"))
            .collect();
        files.push((name, lines));
    }
    files
}

fn archive_of_lines(files: &[(String, Vec<Value>)]) -> Vec<u8> {
    use std::io::Write as _;

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, lines) in files {
        zip.start_file(name.as_str(), options).expect("a file");
        for line in lines {
            writeln!(zip, "{line}").expect("a line");
        }
    }
    zip.finish().expect("the archive").into_inner()
}

/// The archive with one more entry in each of its diaries: a copy of an entry of it
/// under a new identifier, changed by `change`, which is given the entry it copies.
fn spoiled(archive: &[u8], change: &dyn Fn(&mut Value, &Value)) -> Vec<u8> {
    let mut files = archive_lines(archive);
    for (_, lines) in &mut files {
        let Some(entry) = lines.iter().find(|line| line["data"]["type"] == "entry") else {
            continue;
        };
        let mut more = entry.clone();
        more["data"]["id"] = json!(fvoci_model::new_id());
        change(&mut more, entry);
        lines.push(more);
    }
    archive_of_lines(&files)
}

/// No workspace of an organization takes a diary from an archive, and an archive in
/// which a day has two entries, an entry is below another, a diary holds a card or a
/// day is none is taken nowhere: nothing of any of them is stored.
#[tokio::test]
async fn an_archive_puts_a_diary_in_a_space_alone_and_as_a_diary_is() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let own = last_of(&ws).to_owned();
    let (_, _, archive) = an_archived_diary(client, &ws).await;
    let stored = async || {
        let count = "SELECT CAST(count(*) AS TEXT) FROM containers WHERE kind = 'diary'";
        // Of every organization: the space is none of the server's own.
        let counted = th.app.store().system().texts_unchecked(count).await;
        counted.expect("counted")
    };
    assert_eq!(stored().await, ["1"]);
    let refused = crate::export::import(client, ROOT_WORKSPACE_ID, &archive).await;
    refused.check_bad_request();
    let whole = client.get(&format!("{ws}/archive/export")).await;
    whole.check_ok();
    let refused = crate::export::import(client, ROOT_WORKSPACE_ID, &whole.body).await;
    refused.check_bad_request();
    assert_eq!(stored().await, ["1"], "an organization takes no diary");

    let another_day = |more: &mut Value, date: &str| {
        more["data"]["title"] = json!(date);
        more["data"]["fields"]["date"] = json!(date);
    };
    let twice = spoiled(&archive, &|_, _| {});
    let below = spoiled(&archive, &|more, entry| {
        another_day(more, "2019-01-01");
        more["data"]["parentId"] = entry["data"]["id"].clone();
    });
    let a_card = spoiled(&archive, &|more, _| more["data"]["type"] = json!("card"));
    let no_day = spoiled(&archive, &|more, _| another_day(more, "2019-02-30"));
    let to_come = spoiled(&archive, &|more, _| another_day(more, "9999-12-31"));
    for (what, spoiled) in [
        ("a day twice", twice),
        ("an entry below an entry", below),
        ("a card", a_card),
        ("a day that is none", no_day),
        ("a day yet to come", to_come),
    ] {
        let refused = crate::export::import(client, &own, &spoiled).await;
        let (status, text) = (refused.status, refused.text());
        assert_eq!(status, StatusCode::BAD_REQUEST, "{what}: {text}");
        assert_eq!(stored().await, ["1"], "{what}: nothing was stored");
    }
    // One more entry of a day of its own is taken, as the archive itself is.
    let one_more = spoiled(&archive, &|more, _| another_day(more, "2019-01-01"));
    for archive in [&one_more, &archive] {
        crate::export::import(client, &own, archive)
            .await
            .check_ok();
    }
    assert_eq!(stored().await, ["3"]);
    th.teardown().await;
}

/// An archive whose diaries the workspace does not take stores nothing, not the board
/// that comes before a diary in it either: no workspace of an organization of people
/// takes a diary, and a space takes no more diaries than it may have.
#[tokio::test]
async fn an_archive_with_diaries_a_workspace_does_not_take_stores_nothing() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let own = last_of(&ws).to_owned();
    let board = json!({"type": "P", "title": "Beside the diaries"});
    let board = client.post(&format!("{ws}/boards"), &board).await;
    assert!(board.status.is_success(), "{}", board.text());
    let (_, _, one_diary) = an_archived_diary(client, &ws).await;
    a_diary(client, &ws).await;
    let whole = client.get(&format!("{ws}/archive/export")).await;
    whole.check_ok();
    // The board first, as an archive may have it: it would be stored before a diary
    // is refused.
    let of_a_diary = |lines: &Vec<Value>| lines.iter().any(|line| line["data"]["kind"] == "diary");
    let mut files = archive_lines(&whole.body);
    files.sort_by_key(|(_, lines)| of_a_diary(lines));
    let diaries = files.iter().filter(|(_, lines)| of_a_diary(lines)).count();
    let boards = files
        .iter()
        .filter(|(name, _)| name.ends_with("board.jsonl"));
    assert_eq!((boards.count(), diaries), (3, 2), "a board and two diaries");
    let whole = archive_of_lines(&files);
    let stored = async || {
        let count = "SELECT CAST(count(*) AS TEXT) FROM containers";
        let counted = th.app.store().system().texts_unchecked(count).await;
        counted.expect("counted")
    };

    let before = stored().await;
    let refused = crate::export::import(client, ROOT_WORKSPACE_ID, &whole).await;
    refused.check_bad_request();
    assert_eq!(stored().await, before, "nor the board beside the diaries");
    // One diary short of full, the space has no room for the two of the archive.
    for _ in 2..DIARIES_PER_SPACE - 1 {
        a_diary(client, &ws).await;
    }
    let before = stored().await;
    let refused = crate::export::import(client, &own, &whole).await;
    refused.check_bad_request();
    assert_eq!(
        refused.error_reason(),
        "too_many_diaries",
        "{}",
        refused.text()
    );
    assert_eq!(stored().await, before, "nor the board beside the diaries");
    crate::export::import(client, &own, &one_diary)
        .await
        .check_ok();
    let listed = client.get(&format!("{ws}/diaries")).await.value();
    let most = usize::try_from(DIARIES_PER_SPACE).expect("a count");
    assert_eq!(listed.as_array().map(Vec::len), Some(most));
    th.teardown().await;
}

/// An entry is found by what its body says, in the search of its space, as an entry of
/// its diary; and the account's deletion counts the diaries of its space.
#[tokio::test]
async fn an_entry_is_searched_and_a_diary_is_counted_in_its_space() {
    let (th, ws) = in_a_space().await;
    let client = &th.client;
    let diary = a_diary(client, &ws).await;
    let made = written(client, &diary, &json!({"date": "2024-02-29"})).await;
    let entry = format!("{diary}/entries/{}", text_of(&made, "id"));
    write_body(client, &entry, &made, "보름달을 봤다").await;
    let told = th.app.finish_background(std::time::Duration::from_secs(5));
    assert!(told.await, "the background work ends");

    let found = client.get(&format!("{ws}/search?q=보름달")).await;
    found.check_ok();
    let found = found.value();
    assert_eq!(found.as_array().map(Vec::len), Some(1), "{found}");
    let hit = &found[0];
    assert_eq!(
        (&hit["id"], &hit["kind"], &hit["containerKind"]),
        (&made["id"], &json!("entry"), &json!("diary")),
        "{found}"
    );
    assert_eq!(
        (&hit["containerId"], &hit["title"]),
        (&json!(last_of(&diary)), &json!("2024-02-29")),
        "an entry is titled by its day: {found}"
    );
    for (query, hits) in [("kind=entry", 1), ("kind=card", 0), ("kind=page", 0)] {
        let found = client.get(&format!("{ws}/search?q=보름달&{query}")).await;
        found.check_ok();
        assert_eq!(
            found.value().as_array().map(Vec::len),
            Some(hits),
            "{query}"
        );
    }
    // The workspace of the organization the user works in finds nothing of it.
    let elsewhere = format!("/w/{ROOT_WORKSPACE_ID}/search?q=보름달");
    let elsewhere = client.get(&elsewhere).await;
    elsewhere.check_ok();
    assert_eq!(elsewhere.value(), json!([]));
    // No reference names an entry.
    let resolve = format!("{ws}/references/resolve");
    let named = client.post(&resolve, &json!([made["id"]])).await;
    named.check_ok();
    assert!(!named.text().contains("2024-02-29"), "{}", named.text());

    let deletion = client.get("/users/me/deletion").await;
    deletion.check_ok();
    let deletion = deletion.value();
    let organizations = deletion["organizations"].as_array().expect("a list");
    let space = organizations
        .iter()
        .find(|organization| organization["kind"] == "personal")
        .expect("the space");
    assert_eq!(
        (&space["diaries"], &space["boards"]),
        (&json!(1), &json!(0)),
        "{deletion}"
    );
    th.teardown().await;
}
