//! Store tests for course exams, strictly personal exam scores, aggregate averages,
//! bounds checking, and cleanup on purge, workspace removal, org leave, and erasure.

use fvoci_model::{
    Container, ContainerKind, ContainerMember, ContainerType, CourseExamChange, ExamDateChange,
    NewCourseExam, ValidatedMyExamScore, new_id,
};
use fvoci_store::{ExamScoreOutcome, Store};

use crate::account_erasure::{T0, erase, founder};
use crate::common;

async fn create_course(store: &Store, workspace_id: &str, admin: &str) -> String {
    let mut course = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        container_type: ContainerType::Private,
        kind: ContainerKind::Course,
        title: "Algorithms".to_owned(),
        ..Container::default()
    };
    store
        .insert_container_with_admin(&mut course, admin)
        .await
        .expect("course created");
    course.id
}

async fn enroll(store: &Store, course_id: &str, user_id: &str) {
    store
        .save_member(&ContainerMember {
            container_id: course_id.to_owned(),
            user_id: user_id.to_owned(),
            scheme_viewer: true,
            ..ContainerMember::default()
        })
        .await
        .expect("enrolled");
}

fn new_exam(title: &str, exam_date: Option<&str>, max_score: f64) -> fvoci_model::ValidatedNewExam {
    NewCourseExam {
        title: title.to_owned(),
        exam_date: exam_date.map(str::to_owned),
        max_score,
        description: "Covers weeks 1-4".to_owned(),
    }
    .check_request()
    .expect("valid exam")
}

/// Exam CRUD, ordering (dated first, undated last), score bounds, per-caller isolation,
/// active-member average/count, and score/exam deletion.
#[tokio::test]
async fn exams_crud_score_isolation_and_active_member_aggregates() {
    let test = common::setup().await;
    let alice = common::user(&test, "alice").await;
    let bob = common::user(&test, "bob").await;
    let carol = common::user(&test, "carol").await;
    common::works_in(&test, &test.workspace_id, &alice).await;
    common::works_in(&test, &test.workspace_id, &bob).await;
    common::works_in(&test, &test.workspace_id, &carol).await;
    let course_id = create_course(&test, &test.workspace_id, &alice).await;
    enroll(&test, &course_id, &bob).await;
    enroll(&test, &course_id, &carol).await;

    let undated = test
        .create_course_exam(&course_id, &new_exam("Undated", None, 50.0))
        .await
        .expect("undated");
    let second = test
        .create_course_exam(&course_id, &new_exam("Final", Some("2026-12-20"), 100.0))
        .await
        .expect("final");
    let first = test
        .create_course_exam(&course_id, &new_exam("Midterm", Some("2026-12-01"), 100.0))
        .await
        .expect("midterm");

    // Score exceeding max_score is rejected cleanly without writing.
    let over = test
        .upsert_my_exam_score(
            &course_id,
            &first.id,
            &alice,
            &ValidatedMyExamScore {
                score: 100.5,
                note: "too high".to_owned(),
            },
        )
        .await
        .expect("outcome");
    assert_eq!(over, ExamScoreOutcome::ExceedsMaxScore { max_score: 100.0 });

    // 1st -> 2nd -> 3rd scorer k-anonymity progression and unjoined reader isolation.
    verify_k_anonymity_progression(&test, &course_id, &first.id, [&alice, &bob, &carol]).await;

    let for_alice = test
        .list_course_exams(&course_id, Some(&alice))
        .await
        .expect("alice list");
    assert_eq!(
        for_alice.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        [first.id.as_str(), second.id.as_str(), undated.id.as_str()],
        "dated ascending, undated last"
    );
    assert_eq!(for_alice[0].score_count, 3);
    assert!((for_alice[0].average_score.expect("avg") - 80.0).abs() < 1e-9);
    let alice_mine = for_alice[0].my_score.as_ref().expect("alice score");
    assert_eq!(alice_mine.note, "alice private note");
    assert!((alice_mine.score - 90.0).abs() < 1e-9);

    // Patching exam preserves scores and clears exam_date via Clear, and deletions clean up.
    verify_exam_patch_and_deletions(&test, &course_id, &first.id, &alice, &bob).await;
    test.teardown().await;
}

async fn verify_k_anonymity_progression(
    test: &Store,
    course_id: &str,
    exam_id: &str,
    [alice, bob, carol]: [&String; 3],
) {
    // 1st scorer (Alice = 90): neither Alice nor unsubmitted Bob may see averageScore yet.
    let saved_a = test
        .upsert_my_exam_score(
            course_id,
            exam_id,
            alice,
            &ValidatedMyExamScore {
                score: 90.0,
                note: "alice private note".to_owned(),
            },
        )
        .await
        .expect("alice saved");
    assert!(
        matches!(saved_a, ExamScoreOutcome::Saved(ref s) if (s.score - 90.0).abs() < 1e-9),
        "alice score saved as 90.0"
    );
    for viewer in [alice, bob] {
        let one = test
            .list_course_exams(course_id, Some(viewer))
            .await
            .expect("1 scorer list");
        assert_eq!(one[0].score_count, 1, "single scorer counted");
        assert_eq!(one[0].average_score, None, "1 scorer must not leak score");
    }

    // 2nd scorer (Bob = 70): Alice and Bob each have only 1 other scorer (`averageScore: None`),
    // whereas unsubmitted Carol has 2 other scorers and sees `averageScore: Some(80.0)`.
    test.upsert_my_exam_score(
        course_id,
        exam_id,
        bob,
        &ValidatedMyExamScore {
            score: 70.0,
            note: "bob private note".to_owned(),
        },
    )
    .await
    .expect("bob saved");
    for scorer in [alice, bob] {
        let two = test
            .list_course_exams(course_id, Some(scorer))
            .await
            .expect("2 scorers list");
        assert_eq!(two[0].score_count, 2, "two scorers counted");
        assert_eq!(two[0].average_score, None, "2 scorers block subtraction");
    }
    let for_unsubmitted_carol = test
        .list_course_exams(course_id, Some(carol))
        .await
        .expect("carol unsubmitted list");
    assert_eq!(
        for_unsubmitted_carol[0].score_count, 2,
        "carol sees count 2"
    );
    assert!(
        (for_unsubmitted_carol[0].average_score.expect("carol avg") - 80.0).abs() < 1e-9,
        "unsubmitted carol has 2 other scorers so average 80.0 is shown"
    );

    // Unjoined viewer (`None`) receives neither `my_score` nor aggregate stats (`0` / `None`).
    let for_reader = test
        .list_course_exams(course_id, None)
        .await
        .expect("reader list");
    assert_eq!(for_reader[0].my_score, None, "reader has no personal score");
    assert_eq!(
        for_reader[0].average_score, None,
        "reader does not see exam average"
    );
    assert_eq!(
        for_reader[0].score_count, 0,
        "reader does not see exam score count"
    );

    // 3rd scorer (Carol = 80): all three scorers now have 2 other scorers and see average 80.0.
    test.upsert_my_exam_score(
        course_id,
        exam_id,
        carol,
        &ValidatedMyExamScore {
            score: 80.0,
            note: "carol private note".to_owned(),
        },
    )
    .await
    .expect("carol saved");
}

async fn verify_exam_patch_and_deletions(
    test: &Store,
    course_id: &str,
    exam_id: &str,
    alice: &str,
    bob: &str,
) {
    let patched = test
        .patch_course_exam(
            course_id,
            exam_id,
            Some(bob),
            &CourseExamChange {
                title: Some("Midterm Revised".to_owned()),
                exam_date: ExamDateChange::Clear,
                max_score: Some(80.0),
                description: None,
            },
        )
        .await
        .expect("patched");
    assert_eq!(patched.title, "Midterm Revised", "title updated");
    assert_eq!(patched.exam_date, None, "exam_date cleared");
    assert!(
        (patched.max_score - 80.0).abs() < 1e-9,
        "max_score updated to 80.0"
    );
    assert_eq!(
        patched.my_score.as_ref().map(|s| s.note.as_str()),
        Some("bob private note"),
        "bob still sees his own note"
    );

    test.delete_my_exam_score(course_id, exam_id, alice)
        .await
        .expect("alice score deleted");
    let after_del = test
        .list_course_exams(course_id, Some(alice))
        .await
        .expect("listed");
    let first_view = after_del
        .iter()
        .find(|e| e.id == exam_id)
        .expect("first exam");
    assert_eq!(first_view.my_score, None, "alice score removed");
    assert_eq!(first_view.score_count, 2, "bob and carol remain");
    assert!(
        (first_view.average_score.expect("avg") - 75.0).abs() < 1e-9,
        "unsubmitted alice sees average of bob (70) and carol (80)"
    );
    let bob_after_del = test
        .list_course_exams(course_id, Some(bob))
        .await
        .expect("bob listed");
    let bob_first = bob_after_del
        .iter()
        .find(|e| e.id == exam_id)
        .expect("bob first exam");
    assert_eq!(
        bob_first.average_score, None,
        "submitted bob now has only 1 other scorer so average is hidden again"
    );

    test.delete_course_exam(course_id, exam_id)
        .await
        .expect("exam deleted");
    assert_eq!(
        test.count_rows("exam_scores").await.expect("count"),
        0,
        "deleting exam removes scores"
    );
}

/// Workspace removal, account erasure, and permanent course purge clean up `exam_scores`
/// and immediately exclude departed/erased users from averages and counts.
#[tokio::test]
async fn exam_scores_removed_on_workspace_removal_erasure_and_course_purge() {
    let test = common::setup().await;
    let owner = founder(&test).await;
    let bob = common::user(&test, "bob").await;
    let carol = common::user(&test, "carol").await;
    let dave = common::user(&test, "dave").await;
    common::works_in(&test, &test.workspace_id, &bob).await;
    common::works_in(&test, &test.workspace_id, &carol).await;
    common::works_in(&test, &test.workspace_id, &dave).await;
    let course_id = create_course(&test, &test.workspace_id, &owner).await;
    enroll(&test, &course_id, &bob).await;
    enroll(&test, &course_id, &carol).await;
    enroll(&test, &course_id, &dave).await;

    let exam = test
        .create_course_exam(&course_id, &new_exam("Quiz", Some("2026-12-05"), 100.0))
        .await
        .expect("exam");
    for (uid, score) in [(&owner, 80.0), (&bob, 60.0), (&carol, 100.0)] {
        test.upsert_my_exam_score(
            &course_id,
            &exam.id,
            uid,
            &ValidatedMyExamScore {
                score,
                note: format!("note for {uid}"),
            },
        )
        .await
        .expect("saved");
    }

    // With 3 scorers (owner=80, bob=60, carol=100), owner has 2 other scorers -> avg 80.
    let before = test
        .list_course_exams(&course_id, Some(&owner))
        .await
        .expect("before removal");
    assert_eq!(before[0].score_count, 3);
    assert!((before[0].average_score.expect("avg") - 80.0).abs() < 1e-9);

    // Remove Bob from workspace -> Bob's score row deleted; unsubmitted Dave (2 other scorers:
    // owner=80 + carol=100) sees avg 90, while submitted owner (1 other scorer) gets None.
    test.remove_workspace_member(&test.workspace_id, &bob, &owner)
        .await
        .expect("bob removed");
    let exams_owner = test
        .list_course_exams(&course_id, Some(&owner))
        .await
        .expect("listed for owner");
    assert_eq!(exams_owner[0].score_count, 2);
    assert_eq!(exams_owner[0].average_score, None);
    let exams_dave = test
        .list_course_exams(&course_id, Some(&dave))
        .await
        .expect("listed for dave");
    assert_eq!(exams_dave[0].score_count, 2);
    assert!((exams_dave[0].average_score.expect("dave avg") - 90.0).abs() < 1e-9);

    // Erase Carol -> Carol's score row deleted; only 1 scorer remains so average is hidden for all.
    erase(&test, &carol, fvoci_store::Erasing::AtOnce, T0 + 1000).await;
    let exams = test
        .list_course_exams(&course_id, Some(&dave))
        .await
        .expect("listed");
    assert_eq!(exams[0].score_count, 1);
    assert_eq!(exams[0].average_score, None);

    // Trash & permanently purge course -> exam and remaining score deleted.
    test.delete_container(&course_id, &owner)
        .await
        .expect("trashed");
    test.purge_trash(i64::MAX).await.expect("purged");
    assert_eq!(test.count_rows("course_exams").await.expect("count"), 0);
    assert_eq!(test.count_rows("exam_scores").await.expect("count"), 0);
    test.teardown().await;
}

/// `upsert_my_exam_score` enforces active course enrollment atomically and rejects NaN,
/// `delete_course_exam` returns `NotFound` for malformed IDs, and `end_joining` removes
/// scores only on courses where the org admin has no remaining membership.
#[tokio::test]
async fn exam_score_enrollment_guard_nan_rejection_and_org_admin_end_joining() {
    let test = common::setup().await;
    let org_admin = founder(&test).await;
    let teacher = common::user(&test, "teacher").await;
    let unjoined = common::user(&test, "unjoined").await;
    let ws2 = common::workspace(&test).await;
    common::works_in(&test, &ws2, &teacher).await;
    common::works_in(&test, &ws2, &unjoined).await;

    let course_id = create_course(&test, &ws2, &teacher).await;
    let exam = test
        .create_course_exam(&course_id, &new_exam("Guard Exam", None, 100.0))
        .await
        .expect("exam");

    // Unjoined workspace member cannot store a score directly through the store.
    assert!(matches!(
        test.upsert_my_exam_score(
            &course_id,
            &exam.id,
            &unjoined,
            &ValidatedMyExamScore {
                score: 50.0,
                note: String::new(),
            },
        )
        .await,
        Err(fvoci_store::StoreError::NotFound(_))
    ));
    // Non-finite score (NaN) is rejected even if validated struct is constructed directly.
    assert_eq!(
        test.upsert_my_exam_score(
            &course_id,
            &exam.id,
            &teacher,
            &ValidatedMyExamScore {
                score: f64::NAN,
                note: String::new(),
            },
        )
        .await
        .expect("outcome"),
        ExamScoreOutcome::ExceedsMaxScore { max_score: 100.0 }
    );
    assert!(matches!(
        test.delete_course_exam(&course_id, "not-a-uuid").await,
        Err(fvoci_store::StoreError::NotFound(_))
    ));

    // Org admin joins ws2 temporarily (prior = None), enrolls on course, records a score,
    // then ends joining -> score must be removed.
    verify_org_admin_joining_score_cleanup(&test, &ws2, &org_admin, &teacher, &course_id, &exam.id)
        .await;
    test.teardown().await;
}

async fn verify_org_admin_joining_score_cleanup(
    test: &Store,
    ws2: &str,
    org_admin: &str,
    teacher: &str,
    course_id: &str,
    exam_id: &str,
) {
    assert_eq!(
        test.join_workspace_as_org_admin(ws2, org_admin)
            .await
            .expect("joined ws2"),
        fvoci_store::Joining::Joined,
        "org admin joined ws2"
    );
    enroll(test, course_id, org_admin).await;
    test.upsert_my_exam_score(
        course_id,
        exam_id,
        org_admin,
        &ValidatedMyExamScore {
            score: 77.0,
            note: "temp".to_owned(),
        },
    )
    .await
    .expect("org admin score");
    assert!(
        test.leave_workspace_as_org_admin(ws2, org_admin)
            .await
            .expect("ended joining"),
        "ended temporary joining"
    );
    let exams = test
        .list_course_exams(course_id, Some(teacher))
        .await
        .expect("listed");
    assert_eq!(exams[0].score_count, 0, "temp joining score cleaned up");

    // Org admin with prior workspace membership (prior = Some) keeps score on enrolled
    // course when joining ends.
    test.add_workspace_member(ws2, org_admin, fvoci_model::WorkspaceRole::Member)
        .await
        .expect("prior ws2 member");
    let enrolled_course = create_course(test, ws2, teacher).await;
    enroll(test, &enrolled_course, org_admin).await;
    let enrolled_exam = test
        .create_course_exam(&enrolled_course, &new_exam("Kept", None, 100.0))
        .await
        .expect("kept exam");
    assert_eq!(
        test.join_workspace_as_org_admin(ws2, org_admin)
            .await
            .expect("re-joined ws2"),
        fvoci_store::Joining::Joined,
        "org admin re-joined ws2"
    );
    test.upsert_my_exam_score(
        &enrolled_course,
        &enrolled_exam.id,
        org_admin,
        &ValidatedMyExamScore {
            score: 88.0,
            note: "kept".to_owned(),
        },
    )
    .await
    .expect("kept score");
    assert!(
        test.leave_workspace_as_org_admin(ws2, org_admin)
            .await
            .expect("ended second joining"),
        "ended second joining"
    );
    let kept = test
        .list_course_exams(&enrolled_course, Some(org_admin))
        .await
        .expect("listed kept");
    assert_eq!(kept[0].score_count, 1, "enrolled prior-member score kept");
    assert_eq!(
        kept[0].my_score.as_ref().map(|s| s.note.as_str()),
        Some("kept"),
        "enrolled prior-member personal note kept"
    );
}
