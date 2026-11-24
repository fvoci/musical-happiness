//! Isolation, authorization, notification, and non-leakage matrix for course meetings,
//! attendance records, exams, and strictly personal exam scores.

use std::io::Read as _;
use std::sync::{Arc, Mutex, OnceLock};

use axum::Router;
use axum::routing::post;
use fvoci_model::{
    AttendanceStatus, ContainerKind, CourseExam, CourseMeeting, MeetingAttendance, MyExamScore,
    Notification, NotificationKind, WorkspaceRole,
};
use fvoci_store::DbType;
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::admin::socket::{socket_path, socket_request};
use crate::board_broadcasts::WorkspaceSocket;
use crate::common::{
    Client, PASSWORD, ROOT_WS, Setup, TEST_WORKSPACE_ID, TEST_WS, TestServer, USER2_USERNAME,
};
use crate::notifications::unread;
use crate::oidc::org_of;
use crate::own_space::own_workspace;

const SECRET_SCORE_NOTE: &str = "어려웠다-secret-exam-note-9f4e";
static WEBHOOK_URL: OnceLock<String> = OnceLock::new();

async fn create_course(client: &Client, ws: &str, course_type: &str, title: &str) -> String {
    let made = client
        .post(
            &format!("{ws}/courses"),
            &json!({"visibility": course_type, "title": title}),
        )
        .await;
    made.check_created();
    made.value()["id"].as_str().expect("course id").to_owned()
}

async fn add_member(client: &Client, ws: &str, course_id: &str, user_id: &str, role: &str) {
    let body = match role {
        "editor" => json!({"containerId": course_id, "userId": user_id, "schemeEditor": true}),
        "viewer" => json!({"containerId": course_id, "userId": user_id, "schemeViewer": true}),
        other => panic!("unexpected role {other}"),
    };
    client
        .post(&format!("{ws}/containers/{course_id}/members"), &body)
        .await
        .check_ok();
}

async fn create_exam(client: &Client, ws: &str, course_id: &str, title: &str) -> CourseExam {
    let made = client
        .post(
            &format!("{ws}/courses/{course_id}/exams"),
            &json!({
                "title": title,
                "examDate": "2026-12-10",
                "maxScore": 100.0,
                "description": "Shared exam details"
            }),
        )
        .await;
    made.check_created();
    made.created_json()
}

async fn create_meeting(
    client: &Client,
    ws: &str,
    course_id: &str,
    title: &str,
    starts_at: i64,
) -> CourseMeeting {
    let made = client
        .post(
            &format!("{ws}/courses/{course_id}/meetings"),
            &json!({
                "title": title,
                "startsAt": starts_at,
                "location": "https://meet.example/course",
                "notes": "Shared agenda"
            }),
        )
        .await;
    made.check_created();
    made.created_json()
}

async fn start_webhook_sink() -> (String, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let app = Router::new().route(
        "/hook",
        post(move |body: String| {
            let captured = Arc::clone(&captured);
            async move {
                captured.lock().expect("hook lock").push(body);
                StatusCode::OK
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind webhook sink");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/hook"), seen)
}

fn assert_no_score_leak(raw: &str, context: &str) {
    assert!(
        !raw.contains(SECRET_SCORE_NOTE),
        "{context} leaked private exam note: {raw}"
    );
}

struct IsolationActors {
    ws2_id: String,
    ws2: String,
    alice: Client,
    alice_id: String,
    bob: Client,
    ws_admin: Client,
    guest: Client,
    unjoined: Client,
    removed: Client,
    removed_id: String,
    course_id: String,
    exam: CourseExam,
}

async fn setup_isolation_actors(ts: &TestServer, org_id: &str) -> IsolationActors {
    let ws2_resp = ts
        .client
        .post(
            &format!("/org/{org_id}/workspaces"),
            &json!({"name": "WS2"}),
        )
        .await;
    ws2_resp.check_created();
    let ws2_id = ws2_resp.value()["id"].as_str().expect("ws2 id").to_owned();
    let ws2 = format!("/w/{ws2_id}");

    let alice = ts.add_user("exam-alice").await;
    let bob = ts.add_user("exam-bob").await;
    let ws_admin = ts.add_user("exam-ws-admin").await;
    let guest = ts.add_user("exam-guest").await;
    let unjoined = ts.add_user("exam-unjoined").await;
    let removed = ts.add_user("exam-removed").await;
    let (alice_id, bob_id) = (alice.me().await.id, bob.me().await.id);
    let (ws_admin_id, guest_id) = (ws_admin.me().await.id, guest.me().await.id);
    let (unjoined_id, removed_id) = (unjoined.me().await.id, removed.me().await.id);

    for (uid, role) in [
        (&alice_id, WorkspaceRole::Member),
        (&bob_id, WorkspaceRole::Member),
        (&ws_admin_id, WorkspaceRole::Admin),
        (&unjoined_id, WorkspaceRole::Member),
        (&removed_id, WorkspaceRole::Member),
        (&guest_id, WorkspaceRole::Guest),
    ] {
        ts.app
            .store()
            .add_workspace_member(&ws2_id, uid, role)
            .await
            .expect("ws2 member");
    }

    let course_id = create_course(&bob, &ws2, "open", "Isolated Exams Course").await;
    add_member(&bob, &ws2, &course_id, &alice_id, "viewer").await;
    add_member(&bob, &ws2, &course_id, &guest_id, "viewer").await;
    add_member(&bob, &ws2, &course_id, &removed_id, "viewer").await;
    let exam = create_exam(&bob, &ws2, &course_id, "Midterm").await;

    IsolationActors {
        ws2_id,
        ws2,
        alice,
        alice_id,
        bob,
        ws_admin,
        guest,
        unjoined,
        removed,
        removed_id,
        course_id,
        exam,
    }
}

/// Core score-isolation matrix across course admins, workspace admins, org admins
/// (`joinOrgWorkspace`), other-org admins, guests, unjoined readers, removed members,
/// own spaces, and public links (`/shared/{id}/...`).
#[tokio::test]
async fn exam_scores_never_leak_across_roles_admins_orgs_guests_or_public_links() {
    let ts = Setup::native()
        .configure(|cfg| {
            cfg.allow_public_links = true;
            if fvoci_store::testing::db_type_of_the_tests() == DbType::Postgres {
                "open".clone_into(&mut cfg.org_creation);
            }
        })
        .start()
        .await
        .init_basic()
        .await;
    let org_id = org_of(&ts).await;
    let a = setup_isolation_actors(&ts, &org_id).await;

    a.alice
        .put(
            &format!(
                "{}/courses/{}/exams/{}/my-score",
                a.ws2, a.course_id, a.exam.id
            ),
            &json!({"score": 100.1, "note": "too high"}),
        )
        .await
        .check_bad_request();
    a.unjoined
        .put(
            &format!(
                "{}/courses/{}/exams/{}/my-score",
                a.ws2, a.course_id, a.exam.id
            ),
            &json!({"score": 50.0}),
        )
        .await
        .check_forbidden();

    let saved: MyExamScore = a
        .alice
        .put(
            &format!(
                "{}/courses/{}/exams/{}/my-score",
                a.ws2, a.course_id, a.exam.id
            ),
            &json!({"score": 95.0, "note": SECRET_SCORE_NOTE}),
        )
        .await
        .ok_json();
    assert!((saved.score - 95.0).abs() < 1e-9);
    assert_eq!(saved.note, SECRET_SCORE_NOTE);

    // With only 1 scorer (Alice = 95), other enrolled viewers see `scoreCount = 1` and
    // `averageScore = None` (single-scorer k-anonymity), while unjoined readers see 0 / None.
    for (label, client, expected_count) in [
        ("course admin Bob", &a.bob, 1_u32),
        ("invited guest", &a.guest, 1_u32),
        ("unjoined workspace admin", &a.ws_admin, 0_u32),
        ("unjoined member", &a.unjoined, 0_u32),
    ] {
        let resp = client
            .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
            .await;
        resp.check_ok();
        assert_no_score_leak(&resp.text(), label);
        let exams: Vec<CourseExam> = resp.ok_json();
        assert_eq!(exams[0].my_score, None, "{label}");
        assert_eq!(exams[0].score_count, expected_count, "{label}");
        assert_eq!(exams[0].average_score, None, "{label}");
    }

    assert_k_anonymity_api_progression(&a).await;

    // Org admin joins WS2 via `joinOrgWorkspace`: STILL sees no private score or note.
    ts.client
        .post(
            &format!("/org/{org_id}/workspaces/{}/join", a.ws2_id),
            &json!({}),
        )
        .await
        .check_ok();
    let oa_resp = ts
        .client
        .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
        .await;
    oa_resp.check_ok();
    assert_no_score_leak(&oa_resp.text(), "org admin after joinOrgWorkspace");
    let oa_exams: Vec<CourseExam> = oa_resp.ok_json();
    assert_eq!(oa_exams[0].my_score, None);
    ts.client
        .post(
            &format!("/org/{org_id}/workspaces/{}/leave", a.ws2_id),
            &json!({}),
        )
        .await
        .check_ok();

    assert_removed_public_cross_org_and_own_space(&ts, &a).await;
    ts.teardown().await;
}

async fn assert_k_anonymity_api_progression(a: &IsolationActors) {
    // 2nd scorer (invited guest = 85): Alice and guest each have only 1 other scorer (`None`),
    // unsubmitted Bob has 2 other scorers (`Some(90.0)`), and unjoined reader still sees 0 / None.
    a.guest
        .put(
            &format!(
                "{}/courses/{}/exams/{}/my-score",
                a.ws2, a.course_id, a.exam.id
            ),
            &json!({"score": 85.0, "note": "guest private"}),
        )
        .await
        .check_ok();
    for (label, scorer) in [
        ("Alice (2 scorers)", &a.alice),
        ("Guest (2 scorers)", &a.guest),
    ] {
        let exams: Vec<CourseExam> = scorer
            .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
            .await
            .ok_json();
        assert_eq!(exams[0].score_count, 2, "{label}");
        assert_eq!(exams[0].average_score, None, "{label}");
    }
    let bob_two: Vec<CourseExam> = a
        .bob
        .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
        .await
        .ok_json();
    assert_eq!(bob_two[0].score_count, 2, "unsubmitted bob sees count 2");
    assert!(
        (bob_two[0].average_score.expect("bob 2 others") - 90.0).abs() < 1e-9,
        "unsubmitted bob sees average 90.0"
    );
    let unjoined_two: Vec<CourseExam> = a
        .unjoined
        .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
        .await
        .ok_json();
    assert_eq!(
        unjoined_two[0].score_count, 0,
        "unjoined reader sees count 0"
    );
    assert_eq!(
        unjoined_two[0].average_score, None,
        "unjoined reader sees no average"
    );

    // 3rd scorer (Bob = 90): all three enrolled scorers now have 2 other scorers and see 90.0.
    a.bob
        .put(
            &format!(
                "{}/courses/{}/exams/{}/my-score",
                a.ws2, a.course_id, a.exam.id
            ),
            &json!({"score": 90.0, "note": "bob private"}),
        )
        .await
        .check_ok();
    for (label, scorer) in [("Alice", &a.alice), ("Bob", &a.bob), ("Guest", &a.guest)] {
        let exams: Vec<CourseExam> = scorer
            .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
            .await
            .ok_json();
        assert_eq!(exams[0].score_count, 3, "{label}");
        assert!(
            (exams[0].average_score.expect("3 scorers avg") - 90.0).abs() < 1e-9,
            "{label} sees average 90.0"
        );
    }
}

async fn assert_removed_public_cross_org_and_own_space(ts: &TestServer, a: &IsolationActors) {
    let priv_course = create_course(&a.bob, &a.ws2, "private", "Private Exams").await;
    add_member(&a.bob, &a.ws2, &priv_course, &a.alice_id, "viewer").await;
    add_member(&a.bob, &a.ws2, &priv_course, &a.removed_id, "viewer").await;
    let priv_exam = create_exam(&a.bob, &a.ws2, &priv_course, "Secret Quiz").await;
    a.alice
        .put(
            &format!(
                "{}/courses/{priv_course}/exams/{}/my-score",
                a.ws2, priv_exam.id
            ),
            &json!({"score": 95.0, "note": SECRET_SCORE_NOTE}),
        )
        .await
        .check_ok();
    a.bob
        .delete(&format!(
            "{}/containers/{priv_course}/members/{}",
            a.ws2, a.removed_id
        ))
        .await
        .check_no_content();
    a.removed
        .get(&format!("{}/courses/{priv_course}/exams", a.ws2))
        .await
        .check_not_found();

    let share = a
        .bob
        .post(
            &format!("{}/containers/{}/public-link", a.ws2, a.course_id),
            &json!({"enabled": true}),
        )
        .await;
    share.check_ok();
    let token = share.value()["token"].as_str().expect("token").to_owned();
    let anon = ts.anonymous();
    let shared_container = anon
        .get(&format!("/shared/{}?readToken={token}", a.course_id))
        .await;
    shared_container.check_ok();
    assert_no_score_leak(&shared_container.text(), "shared container");
    for suffix in ["meetings", "exams"] {
        anon.get(&format!(
            "/shared/{}/{suffix}?readToken={token}",
            a.course_id
        ))
        .await
        .check_not_found();
    }

    if ts.app.store().db_type() == DbType::Postgres {
        ts.anonymous()
            .post(
                "/register",
                &json!({
                    "username": "otherorgadmin",
                    "email": "otherorgadmin@example.com",
                    "password": PASSWORD,
                    "organizationName": "Other Org"
                }),
            )
            .await
            .check_created();
        let other = ts.login("otherorgadmin", PASSWORD).await;
        other
            .get(&format!("{}/courses/{}/exams", a.ws2, a.course_id))
            .await
            .check_not_found();
        other
            .get(&format!("{}/courses/{}/meetings", a.ws2, a.course_id))
            .await
            .check_not_found();
    }

    let own = own_workspace(&a.alice).await;
    let own_ws = format!("/w/{}", own.id);
    let own_course = create_course(&a.alice, &own_ws, "private", "Self Study").await;
    let own_exam = create_exam(&a.alice, &own_ws, &own_course, "Practice").await;
    a.alice
        .put(
            &format!(
                "{own_ws}/courses/{own_course}/exams/{}/my-score",
                own_exam.id
            ),
            &json!({"score": 88.0, "note": "self note"}),
        )
        .await
        .check_ok();
    a.bob
        .get(&format!("{own_ws}/courses/{own_course}/exams"))
        .await
        .check_not_found();
}

/// Personal exam score writes emit ZERO websocket broadcasts, search hits, archive export
/// entries, webhook deliveries, or audit log records.
#[tokio::test]
async fn personal_exam_scores_leave_no_trace_in_ws_search_export_webhooks_or_audit() {
    let (hook_url, hook_bodies) = start_webhook_sink().await;
    let _ = WEBHOOK_URL.set(hook_url);
    let ts = Setup::native()
        .configure(|cfg| {
            if let Some(url) = WEBHOOK_URL.get() {
                cfg.block_change_webhooks = vec![url.clone()];
            }
        })
        .start()
        .await
        .init_basic()
        .await;
    let admin_id = ts.client.me().await.id;
    let bob_id = ts.client2.me().await.id;
    let course_id = create_course(&ts.client, TEST_WS, "private", "Silent Scores").await;
    add_member(&ts.client, TEST_WS, &course_id, &bob_id, "viewer").await;
    let exam = create_exam(&ts.client, TEST_WS, &course_id, "Final").await;

    let mut ws = WorkspaceSocket::subscribe(&ts, &ts.client, TEST_WORKSPACE_ID, &admin_id).await;
    ts.app
        .finish_background(std::time::Duration::from_secs(2))
        .await;
    let _ = ws.drain().await;
    hook_bodies.lock().expect("hook lock").clear();

    let audit_before = ts
        .client
        .get(&format!("{TEST_WS}/audit"))
        .await
        .value()
        .as_array()
        .expect("audit array")
        .len();

    let score_path = format!("{TEST_WS}/courses/{course_id}/exams/{}/my-score", exam.id);
    ts.client2
        .put(
            &score_path,
            &json!({"score": 95.0, "note": SECRET_SCORE_NOTE}),
        )
        .await
        .check_ok();
    ts.client2.delete(&score_path).await.check_no_content();
    ts.client2
        .put(
            &score_path,
            &json!({"score": 95.0, "note": SECRET_SCORE_NOTE}),
        )
        .await
        .check_ok();
    ts.app
        .finish_background(std::time::Duration::from_secs(2))
        .await;

    assert_eq!(
        ws.drain().await,
        Vec::<Value>::new(),
        "exam score writes must not broadcast over WebSocket"
    );

    let search = ts
        .client
        .get(&format!("{TEST_WS}/search?q={SECRET_SCORE_NOTE}"))
        .await;
    search.check_ok();
    assert_eq!(
        search.value(),
        json!([]),
        "search must not index exam scores"
    );

    let export = ts
        .client
        .get(&format!("{TEST_WS}/containers/{course_id}/archive/export"))
        .await;
    export.check_ok();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&export.body)).expect("zip archive");
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).expect("zip entry");
        let mut contents = String::new();
        let _ = file.read_to_string(&mut contents);
        assert_no_score_leak(&contents, "archive export");
    }

    for body in hook_bodies.lock().expect("hook lock").iter() {
        assert_no_score_leak(body, "webhook payload");
    }
    let audit_after = ts.client.get(&format!("{TEST_WS}/audit")).await;
    audit_after.check_ok();
    assert_no_score_leak(&audit_after.text(), "audit log");
    assert_eq!(
        audit_after.value().as_array().expect("audit array").len(),
        audit_before
    );
    ts.teardown().await;
}

/// Attendance permission matrix: editors+ mark any active member, enrolled viewers/guests
/// mark themselves only, unjoined readers get 403, non-member targets get 404.
#[tokio::test]
async fn meeting_attendance_permission_matrix() {
    let ts = Setup::native()
        .configure(|cfg| {
            cfg.allow_public_links = true;
            if fvoci_store::testing::db_type_of_the_tests() == DbType::Postgres {
                "open".clone_into(&mut cfg.org_creation);
            }
        })
        .start()
        .await
        .init_basic()
        .await;
    let viewer = ts.client2.clone();
    let viewer_id = viewer.me().await.id;
    let editor = ts.add_user("att-editor").await;
    let editor_id = editor.me().await.id;
    let unjoined = ts.add_user("att-unjoined").await;
    let unjoined_id = unjoined.me().await.id;
    let guest = ts.add_user("att-guest").await;
    let guest_id = guest.me().await.id;

    // `add_user` places users in `TEST_WORKSPACE_ID` as Member; demote `guest` to Guest.
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' \
             WHERE workspace_id = '{TEST_WORKSPACE_ID}' AND user_id = '{guest_id}'",
            WorkspaceRole::Guest.as_str()
        ))
        .await
        .expect("ws guest role");

    let course_id = create_course(&ts.client, TEST_WS, "open", "Attendance Course").await;
    add_member(&ts.client, TEST_WS, &course_id, &editor_id, "editor").await;
    add_member(&ts.client, TEST_WS, &course_id, &viewer_id, "viewer").await;
    add_member(&ts.client, TEST_WS, &course_id, &guest_id, "viewer").await;

    let meeting =
        create_meeting(&editor, TEST_WS, &course_id, "Session 1", 1_800_000_000_000).await;
    let att_path = |uid: &str| {
        format!(
            "{TEST_WS}/courses/{course_id}/meetings/{}/attendance/{uid}",
            meeting.id
        )
    };

    viewer
        .post(
            &format!("{TEST_WS}/courses/{course_id}/meetings"),
            &json!({"title": "Nope", "startsAt": 1_800_000_000_000_i64}),
        )
        .await
        .check_forbidden();

    let own_att: MeetingAttendance = viewer
        .put(&att_path(&viewer_id), &json!({"status": "present"}))
        .await
        .ok_json();
    assert_eq!(own_att.status, AttendanceStatus::Present);
    viewer
        .put(&att_path(&editor_id), &json!({"status": "absent"}))
        .await
        .check_forbidden();

    guest
        .put(&att_path(&guest_id), &json!({"status": "late"}))
        .await
        .check_ok();
    guest
        .put(&att_path(&viewer_id), &json!({"status": "absent"}))
        .await
        .check_forbidden();

    unjoined
        .get(&format!("{TEST_WS}/courses/{course_id}/meetings"))
        .await
        .check_ok();
    unjoined
        .put(&att_path(&unjoined_id), &json!({"status": "present"}))
        .await
        .check_forbidden();

    editor
        .put(&att_path(&viewer_id), &json!({"status": "excused"}))
        .await
        .check_ok();
    editor
        .put(&att_path(&unjoined_id), &json!({"status": "present"}))
        .await
        .check_not_found();

    ts.teardown().await;
}

/// Upcoming-meeting notifications (`App::notify_upcoming_meetings`) deliver once to active
/// course members, render visible subjects in `GET /notifications`, hide subjects if the
/// member is later removed, and never notify unjoined readers or non-member guests.
#[tokio::test]
async fn upcoming_meeting_notifications_deliver_once_and_hide_when_access_ends() {
    let ts = Setup::native()
        .configure(|cfg| {
            cfg.allow_public_links = true;
            if fvoci_store::testing::db_type_of_the_tests() == DbType::Postgres {
                "open".clone_into(&mut cfg.org_creation);
            }
        })
        .start()
        .await
        .init_basic()
        .await;
    let member = ts.client2.clone();
    let member_id = member.me().await.id;
    let unjoined = ts.add_user("notif-unjoined").await;
    let guest = ts.add_user("notif-guest").await;
    let guest_id = guest.me().await.id;
    ts.app
        .store()
        .execute_unchecked(&format!(
            "UPDATE workspace_members SET role = '{}' \
             WHERE workspace_id = '{TEST_WORKSPACE_ID}' AND user_id = '{guest_id}'",
            WorkspaceRole::Guest.as_str()
        ))
        .await
        .expect("guest role in ws");

    let course_id = create_course(&ts.client, TEST_WS, "open", "Reminder Course").await;
    add_member(&ts.client, TEST_WS, &course_id, &member_id, "viewer").await;

    let now = 1_800_000_000_000_i64;
    let meeting = create_meeting(
        &ts.client,
        TEST_WS,
        &course_id,
        "Weekly Sync",
        now + 3_600_000,
    )
    .await;

    let count = ts
        .app
        .notify_upcoming_meetings(now)
        .await
        .expect("notified");
    assert_eq!(count, 2, "only admin and enrolled member notified");
    assert_eq!(
        ts.app
            .notify_upcoming_meetings(now)
            .await
            .expect("idempotent"),
        0,
        "zero duplicate notifications"
    );

    let list: Vec<Notification> = member.get("/notifications").await.ok_json();
    assert_eq!(list.len(), 1);
    let subject = list[0].subject.as_ref().expect("visible subject");
    assert_eq!(subject.kind, NotificationKind::MeetingUpcoming);
    assert_eq!(subject.container_kind, ContainerKind::Course);
    assert_eq!(subject.container_id, course_id);
    assert_eq!(subject.block_id, meeting.id);
    assert_eq!(subject.title, "Weekly Sync");
    assert_eq!(unread(&unjoined).await, 0);
    assert_eq!(unread(&guest).await, 0);

    ts.client
        .delete(&format!(
            "{TEST_WS}/containers/{course_id}/members/{member_id}"
        ))
        .await
        .check_no_content();
    let after_removal: Vec<Notification> = member.get("/notifications").await.ok_json();
    assert_eq!(after_removal.len(), 1);
    assert_eq!(after_removal[0].subject, None);

    ts.teardown().await;
}

/// Account erasure via the operator socket keeps `meeting_attendance` as a tombstone
/// while deleting `exam_scores` so the erased learner immediately disappears from exam
/// averages and counts.
#[tokio::test]
async fn account_erasure_keeps_attendance_tombstone_and_removes_exam_score_from_average() {
    let sock = socket_path();
    let ts: TestServer = Setup::native()
        .admin_socket(&sock)
        .start()
        .await
        .init_basic()
        .await;
    let a = ts.client2.clone();
    let a_id = a.me().await.id;
    let peer = ts.add_user("exam-peer").await;
    let peer_id = peer.me().await.id;
    let observer = ts.add_user("exam-observer").await;
    let observer_id = observer.me().await.id;
    let course_id = create_course(&ts.client, ROOT_WS, "private", "Erasure Course").await;
    for uid in [&a_id, &peer_id, &observer_id] {
        add_member(&ts.client, ROOT_WS, &course_id, uid, "viewer").await;
    }

    let meeting = create_meeting(&ts.client, ROOT_WS, &course_id, "Study", 1_800_000_000_000).await;
    a.put(
        &format!(
            "{ROOT_WS}/courses/{course_id}/meetings/{}/attendance/{a_id}",
            meeting.id
        ),
        &json!({"status": "present"}),
    )
    .await
    .check_ok();

    let exam = create_exam(&ts.client, ROOT_WS, &course_id, "Final").await;
    for (client, score, note) in [
        (&ts.client, 80.0, "admin score"),
        (&a, 100.0, SECRET_SCORE_NOTE),
        (&peer, 90.0, "peer score"),
    ] {
        client
            .put(
                &format!("{ROOT_WS}/courses/{course_id}/exams/{}/my-score", exam.id),
                &json!({"score": score, "note": note}),
            )
            .await
            .check_ok();
    }

    let before: Vec<CourseExam> = ts
        .client
        .get(&format!("{ROOT_WS}/courses/{course_id}/exams"))
        .await
        .ok_json();
    assert_eq!(before[0].score_count, 3);
    assert!((before[0].average_score.expect("avg") - 90.0).abs() < 1e-9);

    socket_request(
        &sock,
        "POST",
        &format!("/api/v1/admin/users/{USER2_USERNAME}/erase"),
        "",
    )
    .await
    .check_no_content();

    let meetings: Vec<CourseMeeting> = ts
        .client
        .get(&format!("{ROOT_WS}/courses/{course_id}/meetings"))
        .await
        .ok_json();
    assert_eq!(meetings[0].attendance.len(), 1);
    assert_eq!(meetings[0].attendance[0].user_id, a_id);
    assert_eq!(meetings[0].attendance[0].status, AttendanceStatus::Present);

    let after: Vec<CourseExam> = ts
        .client
        .get(&format!("{ROOT_WS}/courses/{course_id}/exams"))
        .await
        .ok_json();
    assert_eq!(after[0].score_count, 2);
    assert_eq!(
        after[0].average_score, None,
        "scorer with 1 remaining peer has average withheld"
    );
    let after_obs: Vec<CourseExam> = observer
        .get(&format!("{ROOT_WS}/courses/{course_id}/exams"))
        .await
        .ok_json();
    assert_eq!(after_obs[0].score_count, 2);
    assert!((after_obs[0].average_score.expect("obs avg") - 85.0).abs() < 1e-9);
    assert_eq!(
        ts.app
            .store()
            .system()
            .count_rows("exam_scores")
            .await
            .expect("count"),
        2,
        "erased user's score row removed while active peers remain"
    );

    ts.teardown().await;
}
