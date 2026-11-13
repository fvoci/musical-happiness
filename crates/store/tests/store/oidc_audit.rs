//! Durable provider changes, operator attribution, and immutable records on both databases.
use crate::common;
use fvoci_model::{AuditAction, AuditActorKind, new_id};
use fvoci_store::{AuditPlace, Paging, StoredProvider};

fn settings() -> StoredProvider {
    StoredProvider {
        id: String::new(),
        org_id: String::new(),
        issuer: "https://issuer.example.test".to_owned(),
        client_id: "client".to_owned(),
        sealed_client_secret: String::new(),
        auto_join: true,
        enabled: true,
        sso_required: true,
        revision: 0,
        create_at: 0,
        update_at: 0,
    }
}

#[tokio::test]
async fn operator_records_are_distinct_immutable_and_made_with_the_release() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    test.put_oidc_provider_as_read(&settings(), None, &actor)
        .await
        .expect("stored");
    assert!(
        test.release_sso_by_operator()
            .await
            .expect("released")
            .is_some()
    );
    assert!(
        test.release_sso_by_operator()
            .await
            .expect("already released")
            .is_none()
    );
    let records = test
        .audit_records(
            AuditPlace::Organization,
            Paging {
                page: 0,
                per_page: 100,
            },
        )
        .await
        .expect("records");
    assert_eq!(records.len(), 3);
    let operators: Vec<_> = records
        .iter()
        .filter(|record| record.actor_kind == AuditActorKind::ServerOperator)
        .collect();
    assert_eq!(operators.len(), 1);
    assert_eq!(operators[0].action, AuditAction::SsoReleased);
    assert!(operators[0].actor.is_none());
    for record in records
        .iter()
        .filter(|record| record.actor_kind == AuditActorKind::Person)
    {
        assert_eq!(record.actor.as_ref().expect("a person").id, actor);
    }
    for statement in [
        "UPDATE audit_log SET actor_kind = 'person' WHERE actor_kind = 'server_operator'",
        "UPDATE audit_log SET create_at = 0",
        "DELETE FROM audit_log WHERE actor_kind = 'server_operator'",
    ] {
        assert!(
            test.execute_unchecked(statement).await.is_err(),
            "{statement}"
        );
    }
    for (actor_id, kind) in [
        ("NULL".to_owned(), "person"),
        (format!("'{}'", new_id()), "server_operator"),
    ] {
        let statement = format!(
            "INSERT INTO audit_log (id, org_id, actor_id, actor_kind, action, target_id, create_at) \
             SELECT '{}', org_id, {actor_id}, '{kind}', 'sso_released', target_id, 0 FROM audit_log LIMIT 1",
            new_id()
        );
        assert!(
            test.execute_unchecked(&statement).await.is_err(),
            "inconsistent actor rejected"
        );
    }
    assert_eq!(
        test.audit_records(
            AuditPlace::Organization,
            Paging {
                page: 0,
                per_page: 100
            }
        )
        .await
        .expect("unchanged"),
        records
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_missing_actor_rolls_back_the_provider_change() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let before = test
        .put_oidc_provider_as_read(&settings(), None, &actor)
        .await
        .expect("stored")
        .expect("new");
    let mut changed = before.clone();
    changed.client_id = "replacement".to_owned();
    assert!(
        test.put_oidc_provider_as_read(&changed, Some(&before), "")
            .await
            .is_err()
    );
    assert_eq!(test.oidc_provider().await.expect("read"), Some(before));
    test.teardown().await;
}

#[tokio::test]
async fn migration_keeps_existing_people_subjects_and_times() {
    let database = common::empty_database().await;
    fvoci_store::testing::migrate_to_before_audit_actors(&database).await;
    let (org, record, actor, subject) = (new_id(), new_id(), new_id(), new_id());
    common::execute_raw(
        &common::raw_dbconfig(&database),
        &format!(
            "INSERT INTO organizations (id, name, settings, create_at, update_at) \
         VALUES ('{org}', 'Org', '{{}}', 1, 1); \
         INSERT INTO audit_log (id, org_id, actor_id, action, target_id, subject_id, create_at) \
         VALUES ('{record}', '{org}', '{actor}', 'org_admin_granted', '{org}', '{subject}', 42);"
        ),
    )
    .await;
    let store = fvoci_store::Store::connect(database.db_type(), database.dbconfig())
        .await
        .expect("migrated");
    let records = store
        .for_org(&org)
        .expect("organization")
        .audit_records(
            AuditPlace::Organization,
            Paging {
                page: 0,
                per_page: 100,
            },
        )
        .await
        .expect("records");
    assert_eq!(records.len(), 1);
    let after = &records[0];
    assert_eq!(after.id, record);
    assert_eq!(after.actor_kind, AuditActorKind::Person);
    assert_eq!(after.actor.as_ref().expect("actor").id, actor);
    assert_eq!(after.subject.as_ref().expect("subject").id, subject);
    assert_eq!(after.action, AuditAction::OrgAdminGranted);
    assert_eq!(after.target_id, org);
    assert_eq!(after.create_at, 42);
    store.close().await;
    database.remove().await;
}

#[tokio::test]
async fn a_release_or_removal_without_its_record_rolls_back() {
    for required in [false, true] {
        let test = common::setup().await;
        let actor = common::user(&test, "administrator").await;
        let mut provider = settings();
        provider.sso_required = required;
        let before = test
            .put_oidc_provider_as_read(&provider, None, &actor)
            .await
            .expect("stored")
            .expect("new");
        test.execute_unchecked("DROP TABLE audit_log")
            .await
            .expect("recording fails");
        if required {
            assert!(test.release_sso_by_operator().await.is_err());
        } else {
            assert!(test.delete_oidc_provider(&actor).await.is_err());
        }
        assert_eq!(test.oidc_provider().await.expect("read"), Some(before));
        test.teardown().await;
    }
}

#[tokio::test]
async fn an_operator_role_change_is_recorded_only_in_the_instances_organization() {
    let test = common::setup().await;
    common::user(&test, "first-admin").await;
    let user = common::user(&test, "next-admin").await;
    let other = if common::db_type() == fvoci_store::DbType::Postgres {
        let (org, _) = common::organization(&test.system(), "Another organization").await;
        let other = test.for_org(&org).expect("organization");
        other
            .execute_unchecked(&format!(
                "INSERT INTO organization_members (org_id, user_id, role, create_at) \
             VALUES ('{org}', '{user}', 'member', 1)"
            ))
            .await
            .expect("also a member elsewhere");
        Some(other)
    } else {
        None
    };
    test.make_admin_by_operator(&user).await.expect("promoted");
    test.make_admin_by_operator(&user)
        .await
        .expect("already an administrator");
    let paging = Paging {
        page: 0,
        per_page: 100,
    };
    let records = test
        .audit_records(AuditPlace::Organization, paging)
        .await
        .expect("records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].actor_kind, AuditActorKind::ServerOperator);
    assert!(records[0].actor.is_none());
    assert_eq!(records[0].action, AuditAction::OrgAdminGranted);
    assert_eq!(records[0].subject.as_ref().expect("subject").id, user);
    if let Some(other) = other {
        assert_eq!(
            other
                .audit_records(AuditPlace::Organization, paging)
                .await
                .expect("records"),
            []
        );
    }
    test.reset_user_password("next-admin", "replacement hash")
        .await
        .expect("reset");
    assert_eq!(
        test.audit_records(AuditPlace::Organization, paging)
            .await
            .expect("records"),
        records
    );
    test.teardown().await;
}

#[tokio::test]
async fn a_save_records_only_what_it_changed() {
    let test = common::setup().await;
    let actor = common::user(&test, "administrator").await;
    let mut provider = settings();
    provider.sso_required = false;
    let first = test
        .put_oidc_provider_as_read(&provider, None, &actor)
        .await
        .expect("stored")
        .expect("new");
    let unchanged = test
        .put_oidc_provider_as_read(&provider, Some(&first), &actor)
        .await
        .expect("stored")
        .expect("as read");
    provider.sso_required = true;
    let enforced = test
        .put_oidc_provider_as_read(&provider, Some(&unchanged), &actor)
        .await
        .expect("stored")
        .expect("as read");
    provider.auto_join = false;
    test.put_oidc_provider_as_read(&provider, Some(&enforced), &actor)
        .await
        .expect("stored")
        .expect("as read");
    let mut actions: Vec<AuditAction> = test
        .audit_records(
            AuditPlace::Organization,
            Paging {
                page: 0,
                per_page: 100,
            },
        )
        .await
        .expect("records")
        .into_iter()
        .map(|record| record.action)
        .collect();
    test.teardown().await;
    // The newest first, as the list reads.
    actions.reverse();
    assert_eq!(
        actions,
        [
            AuditAction::OidcProviderConfigured,
            AuditAction::SsoRequired,
            AuditAction::OidcProviderConfigured,
        ],
        "the first save, the switch alone, and the change of joining; nothing for the save that changed nothing"
    );
}
