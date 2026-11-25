//! Store tests for migration 0066 (`course_schedules`), shared schedule CRUD and
//! auto-assignment, trash/restore/purge lifecycle, personal planned-date overrides,
//! personal redistribution, workspace today/overdue aggregation, and tenant isolation.

use fvoci_model::{
    AmountUnit, Block, CompletionChange, Container, ContainerKind, ContainerMember, ContainerRole,
    ContainerVisibility, CourseLimits, DiaryDate, LastPositionChange, NewUnit,
    RedistributeScheduleRequest, STUDY_DAYS_ALL, STUDY_DAYS_WEEKDAYS, StudyDays, UnitKind,
    UnitProgressChange, ValidatedCourseSchedule, new_id,
};
use fvoci_store::{DbType, Store};

use crate::common::{self, db_type};

fn day(text: &str) -> DiaryDate {
    text.parse().expect("valid date")
}

async fn create_course(
    store: &Store,
    workspace_id: &str,
    owner_id: &str,
    course_type: ContainerVisibility,
    title: &str,
) -> Container {
    let mut course = Container {
        id: new_id(),
        workspace_id: workspace_id.to_owned(),
        visibility: course_type,
        kind: ContainerKind::Course,
        title: title.to_owned(),
        ..Container::default()
    };
    store
        .insert_container_with_admin(&mut course, owner_id)
        .await
        .expect("course created");
    course
}

async fn create_unit(
    store: &Store,
    course_id: &str,
    parent_id: &str,
    title: &str,
    amount: Option<i64>,
    scheduled_date: Option<&str>,
    user_id: &str,
) -> Block {
    store
        .create_unit(
            course_id,
            &NewUnit {
                kind: UnitKind::Unit,
                parent_id: parent_id.to_owned(),
                title: title.to_owned(),
                amount,
                amount_unit: amount.map(|_| AmountUnit::Minutes),
                scheduled_date: scheduled_date.map(str::to_owned),
                ..NewUnit::default()
            },
            CourseLimits::DEFAULT,
            user_id,
        )
        .await
        .expect("unit created")
        .unit
}

async fn save_role(store: &Store, course_id: &str, user_id: &str, role: Option<ContainerRole>) {
    match role {
        Some(role) => {
            let member = ContainerMember {
                container_id: course_id.to_owned(),
                user_id: user_id.to_owned(),
                role,
                synthetic: false,
                inherited_role: None,
            };
            store.save_member(&member).await.expect("member saved");
        }
        None => {
            store
                .delete_member(course_id, user_id)
                .await
                .expect("member removed");
        }
    }
}

/// Upgrading a database seeded before migration 0066 applies 0066 cleanly without
/// altering existing course rows, creates `course_schedules` behind PostgreSQL forced
/// RLS (`org_wall`), and supports saving/reading schedules immediately.
#[tokio::test]
async fn migration_before_0066_to_0066_creates_course_schedules_behind_rls() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_course_schedules(&database).await;
    common::execute_raw(&common::raw_connection(&database), "DELETE FROM data_steps").await;
    let pre = Store::open(database.db_type(), database.database_connection())
        .await
        .expect("open before 0066");
    let (org_id, workspace_id) = common::organization(&pre, "academy").await;
    let scoped = pre.for_org(&org_id).expect("org handle");
    let owner = common::pre0072_user(&scoped, &org_id, &workspace_id, "teacher").await;
    let course_id = new_id();
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO containers \
             (id, workspace_id, created_by, modified_by, type, kind, title, description, icon, \
              show_description, is_template, properties, card_properties, create_at, update_at) \
             VALUES ('{course_id}', '{workspace_id}', '{owner}', '{owner}', 'P', 'course', \
              'Math', '', '', false, false, '{{}}', '[]', 1, 1)"
        ))
        .await
        .expect("pre-0066 course stored");
    scoped
        .execute_unchecked(&format!(
            "INSERT INTO container_members \
             (container_id, user_id, {}, {}, {}, {}) \
             VALUES ('{course_id}', '{owner}', true, true, false, false)",
            concat!("scheme", "_admin"),
            concat!("scheme", "_editor"),
            concat!("scheme", "_commenter"),
            concat!("scheme", "_viewer"),
        ))
        .await
        .expect("pre-0066 course admin stored");
    let course = Container {
        id: course_id,
        ..Container::default()
    };
    let unit = create_unit(
        &scoped.telling_nobody(),
        &course.id,
        "",
        "Lesson 1",
        Some(30),
        None,
        &owner,
    )
    .await;
    pre.close().await;

    let migrated = Store::connect(database.db_type(), database.database_connection())
        .await
        .expect("migration 0066 applies cleanly");
    let scoped = migrated.for_org(&org_id).expect("org handle");
    assert_eq!(
        scoped.get_container(&course.id).await.expect("course").kind,
        ContainerKind::Course
    );
    let schedule = ValidatedCourseSchedule {
        start_date: day("2026-11-02"),
        target_date: day("2026-11-27"),
        study_days: StudyDays::try_new(STUDY_DAYS_WEEKDAYS).expect("weekdays"),
        auto_assign: true,
    };
    let (saved, updated) = scoped
        .upsert_course_schedule(&course.id, &schedule, &owner)
        .await
        .expect("schedule stored");
    assert_eq!(saved.start_date, "2026-11-02");
    assert_eq!(saved.target_date, "2026-11-27");
    assert_eq!(saved.study_days, STUDY_DAYS_WEEKDAYS);
    assert_eq!(updated, vec![unit.id.clone()]);
    assert_eq!(
        scoped
            .get_course_schedule(&course.id)
            .await
            .expect("schedule read"),
        Some(saved)
    );

    if db_type() == DbType::Postgres {
        let mut conn = common::connect_past_the_store(database.database_connection()).await;
        let (enabled, forced): (bool, bool) = sqlx::query_as(
            "SELECT relrowsecurity, relforcerowsecurity FROM pg_class \
             WHERE oid = to_regclass('course_schedules')",
        )
        .fetch_one(&mut conn)
        .await
        .expect("pg_class entry");
        assert!(enabled && forced, "course_schedules has forced RLS");
        let policy_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pg_policies \
             WHERE schemaname = current_schema() \
               AND tablename = 'course_schedules' AND policyname = 'org_wall'",
        )
        .fetch_one(&mut conn)
        .await
        .expect("pg_policies entry");
        assert_eq!(policy_count, 1);
    }

    migrated.close().await;
    database.remove().await;
}

/// Schedule rows survive soft deletion to the trash, come back on restore, and are
/// cleaned up when the course is permanently purged.
#[tokio::test]
async fn schedule_survives_trash_restore_and_is_cleaned_up_on_purge() {
    let test = common::setup().await;
    let owner = common::user(&test, "owner").await;
    common::works_in(&test, &test.workspace_id, &owner).await;
    let course = create_course(
        &test,
        &test.workspace_id,
        &owner,
        ContainerVisibility::Private,
        "History",
    )
    .await;
    let schedule = ValidatedCourseSchedule {
        start_date: day("2026-11-01"),
        target_date: day("2026-11-30"),
        study_days: StudyDays::try_new(STUDY_DAYS_ALL).expect("all"),
        auto_assign: false,
    };
    test.upsert_course_schedule(&course.id, &schedule, &owner)
        .await
        .expect("saved");

    test.delete_container(&course.id, &owner)
        .await
        .expect("trashed");
    assert_eq!(
        test.get_course_schedule(&course.id).await.expect("trashed"),
        None
    );

    test.undelete_container(&course.id, &owner)
        .await
        .expect("restored");
    assert!(
        test.get_course_schedule(&course.id)
            .await
            .expect("restored")
            .is_some(),
        "schedule survives trash and restore"
    );

    test.delete_container(&course.id, &owner)
        .await
        .expect("trashed again");
    test.system().purge_trash(i64::MAX).await.expect("purged");
    assert_eq!(
        test.system()
            .count_rows("course_schedules")
            .await
            .expect("counted"),
        0,
        "course_schedules row is removed when course is permanently purged"
    );
    test.teardown().await;
}

async fn seed_chapter_with_three_units(
    store: &Store,
    course_id: &str,
    owner_id: &str,
) -> (Block, Block, Block, Block) {
    let chapter = store
        .create_unit(
            course_id,
            &NewUnit {
                kind: UnitKind::Chapter,
                title: "Part 1".to_owned(),
                ..NewUnit::default()
            },
            CourseLimits::DEFAULT,
            owner_id,
        )
        .await
        .expect("chapter")
        .unit;
    let u1 = create_unit(
        store,
        course_id,
        &chapter.id,
        "U1",
        Some(10),
        Some("2026-11-10"),
        owner_id,
    )
    .await;
    let u2 = create_unit(
        store,
        course_id,
        &chapter.id,
        "U2",
        Some(20),
        Some("2026-11-11"),
        owner_id,
    )
    .await;
    let u3 = create_unit(
        store,
        course_id,
        &chapter.id,
        "U3",
        Some(20),
        Some("2026-11-12"),
        owner_id,
    )
    .await;
    (chapter, u1, u2, u3)
}

/// Personal `planned_date` overrides preserve `completed_at` and `last_position`, can be
/// cleared back to `None`, and refuse chapters.
#[tokio::test]
async fn personal_planned_date_preserves_progress_and_refuses_chapters() {
    let test = common::setup().await;
    let alice = common::user(&test, "alice").await;
    common::works_in(&test, &test.workspace_id, &alice).await;
    let course = create_course(
        &test,
        &test.workspace_id,
        &alice,
        ContainerVisibility::Private,
        "Rust",
    )
    .await;
    let (chapter, u1, _, _) = seed_chapter_with_three_units(&test, &course.id, &alice).await;

    // Direct store-level chapter creation with schedule fields is refused by Block::validate.
    assert!(
        test.create_unit(
            &course.id,
            &NewUnit {
                kind: UnitKind::Chapter,
                title: "Bad Chapter".to_owned(),
                amount: Some(10),
                ..NewUnit::default()
            },
            CourseLimits::DEFAULT,
            &alice,
        )
        .await
        .is_err(),
        "chapter creation with amount must be rejected at store boundary"
    );

    assert!(
        test.set_unit_planned_date(&course.id, &chapter.id, &alice, Some(day("2026-11-25")))
            .await
            .expect_err("chapter refused")
            .is_not_found(),
        "chapters must not accept planned_date"
    );

    test.upsert_unit_progress(
        &course.id,
        &u1.id,
        &alice,
        &UnitProgressChange {
            completion: CompletionChange::Complete,
            last_position: LastPositionChange::Set("sec-3".to_owned()),
        },
    )
    .await
    .expect("progress saved");

    let set = test
        .set_unit_planned_date(&course.id, &u1.id, &alice, Some(day("2026-11-28")))
        .await
        .expect("planned date set");
    assert_eq!(set.planned_date.as_deref(), Some("2026-11-28"));
    assert!(
        set.completed_at.is_some(),
        "completed_at preserved when setting planned_date"
    );
    assert_eq!(set.last_position.as_deref(), Some("sec-3"));

    let cleared = test
        .set_unit_planned_date(&course.id, &u1.id, &alice, None)
        .await
        .expect("planned date cleared");
    assert_eq!(cleared.planned_date, None);
    assert!(
        cleared.completed_at.is_some(),
        "completed_at preserved when clearing planned_date"
    );
    assert_eq!(cleared.last_position.as_deref(), Some("sec-3"));

    test.teardown().await;
}

/// Personal redistribution updates only the caller's incomplete units without touching
/// shared `scheduledDate` or other learners, and `get_workspace_learning_today` reflects
/// each learner's own effective dates independently.
#[tokio::test]
async fn personal_redistribution_isolates_learners_and_drives_workspace_today() {
    let test = common::setup().await;
    let alice = common::user(&test, "alice").await;
    let bob = common::user(&test, "bob").await;
    common::works_in(&test, &test.workspace_id, &alice).await;
    common::works_in(&test, &test.workspace_id, &bob).await;

    let course = create_course(
        &test,
        &test.workspace_id,
        &alice,
        ContainerVisibility::Private,
        "Rust",
    )
    .await;
    save_role(&test, &course.id, &bob, Some(ContainerRole::Viewer)).await;
    let (_, u1, u2, u3) = seed_chapter_with_three_units(&test, &course.id, &alice).await;

    test.upsert_unit_progress(
        &course.id,
        &u1.id,
        &alice,
        &UnitProgressChange {
            completion: CompletionChange::Complete,
            last_position: LastPositionChange::Keep,
        },
    )
    .await
    .expect("u1 completed");

    let request = RedistributeScheduleRequest {
        from_date: Some("2026-11-24".to_owned()),
        target_date: Some("2026-11-25".to_owned()),
        study_days: Some(i32::from(STUDY_DAYS_ALL)),
    };
    test.redistribute_personal_schedule(&course.id, &alice, &request, day("2026-11-24"))
        .await
        .expect("personal redistribute");

    // Redistributing a nonexistent/trashed course returns NotFound (not 400 Refused) even
    // when targetDate and studyDays are omitted.
    assert!(
        test.redistribute_course_schedule(
            &new_id(),
            &RedistributeScheduleRequest::default(),
            day("2026-11-24"),
            &alice,
        )
        .await
        .expect_err("missing course")
        .is_not_found(),
        "missing course must return NotFound before resolving omitted schedule defaults"
    );

    let alice_rows = test
        .get_my_course_progress(&course.id, &alice)
        .await
        .expect("alice rows");
    let by_id = |id: &str| {
        alice_rows
            .iter()
            .find(|r| r.unit_id == id)
            .and_then(|r| r.planned_date.as_deref())
    };
    assert_eq!(by_id(&u1.id), None, "completed unit is untouched");
    assert_eq!(by_id(&u2.id), Some("2026-11-24"));
    assert_eq!(by_id(&u3.id), Some("2026-11-25"));
    assert!(
        test.get_my_course_progress(&course.id, &bob)
            .await
            .expect("bob rows")
            .is_empty(),
        "bob sees none of alice's personal planned dates"
    );
    let tree = test.get_unit_tree(&course.id).await.expect("tree");
    let u2_node = tree.iter().find(|n| n.id == u2.id).expect("u2");
    assert_eq!(
        u2_node.scheduled_date.as_deref(),
        Some("2026-11-11"),
        "shared scheduledDate is untouched"
    );

    let alice_feed = test
        .get_workspace_learning_today(&test.workspace_id, &alice, day("2026-11-24"))
        .await
        .expect("alice feed");
    assert_eq!(alice_feed.today_units.len(), 1);
    assert_eq!(alice_feed.today_units[0].unit_id, u2.id);
    assert!(
        alice_feed.overdue_units.is_empty(),
        "alice has no overdue units after personal redistribute"
    );

    let bob_feed = test
        .get_workspace_learning_today(&test.workspace_id, &bob, day("2026-11-24"))
        .await
        .expect("bob feed");
    assert!(
        bob_feed.today_units.is_empty(),
        "bob has no units scheduled on 2026-11-24"
    );
    assert_eq!(
        bob_feed
            .overdue_units
            .iter()
            .map(|i| i.unit_id.as_str())
            .collect::<Vec<_>>(),
        [u1.id.as_str(), u2.id.as_str(), u3.id.as_str()]
    );

    test.teardown().await;
}
