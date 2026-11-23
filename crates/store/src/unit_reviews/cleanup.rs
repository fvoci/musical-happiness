//! Lifecycle cleanup of `unit_reviews` and `unit_notes` on permanent purge,
//! workspace removal, org-admin unjoining, and organization departure.

use crate::containers::inherited_members_source;
use crate::db::{Dialect, Id, Scope};
use crate::error::StoreError;

/// Both personal learning tables cleaned up on purge and membership removal.
const TABLES: [&str; 2] = ["unit_reviews", "unit_notes"];

/// Forgets personal review and note rows of units that are gone for good: neither live in
/// `blocks` nor awaiting restore in `deleted_blocks` of the row's organization.
pub(crate) async fn forget_gone_blocks(scope: &mut Scope<'_>) -> Result<(), StoreError> {
    for table in TABLES {
        let statement = [
            "DELETE FROM ",
            table,
            " WHERE NOT EXISTS (SELECT 1 FROM blocks AS b WHERE b.id = ",
            table,
            ".unit_id",
            scope.same_organization(&[("b", table)]).as_str(),
            ") AND NOT EXISTS (SELECT 1 FROM deleted_blocks AS d WHERE d.id = ",
            table,
            ".unit_id",
            scope.same_organization(&[("d", table)]).as_str(),
            ")",
        ]
        .concat();
        let mut sql = scope.sql(&statement);
        sql.changing_no_access();
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Deletes every `unit_reviews` and `unit_notes` row of `user_id` for courses belonging
/// to `workspace_id` (both live and in the trash).
pub(crate) async fn forget_user_in_workspace(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    for table in TABLES {
        let mut sql = scope.sql(&format!("DELETE FROM {table} WHERE user_id = "));
        sql.changing_no_access()
            .bind(Id(user_id))
            .push_bind(
                " AND container_id IN (\
                 SELECT id FROM containers WHERE workspace_id = ",
                Id(workspace_id),
            )
            .push_bind(
                " UNION SELECT id FROM deleted_containers WHERE workspace_id = ",
                Id(workspace_id),
            )
            .push(")");
        if scope.dialect() == Dialect::Postgres {
            sql.push_bind(" AND org_id = ", scope.org()?);
        }
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Deletes `unit_reviews` and `unit_notes` rows of `user_id` in `workspace_id` for
/// courses where `user_id` no longer holds any direct or inherited membership.
pub(crate) async fn forget_user_in_workspace_unjoined(
    scope: &mut Scope<'_>,
    workspace_id: &str,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    let inherited = inherited_members_source(scope);
    for table in TABLES {
        let mut sql = scope.sql(&format!("DELETE FROM {table} WHERE user_id = "));
        sql.changing_no_access()
            .bind(Id(user_id))
            .push_bind(
                " AND container_id IN (\
                 SELECT id FROM containers WHERE workspace_id = ",
                Id(workspace_id),
            )
            .push_bind(
                " UNION SELECT id FROM deleted_containers WHERE workspace_id = ",
                Id(workspace_id),
            )
            .push(&format!(
                ") AND NOT EXISTS (\
                 SELECT 1 FROM container_members AS cm \
                 WHERE cm.container_id = {table}.container_id \
                   AND cm.user_id = {table}.user_id \
                   AND (cm.scheme_admin = "
            ))
            .bind(true)
            .push_bind(" OR cm.scheme_editor = ", true)
            .push_bind(" OR cm.scheme_commenter = ", true)
            .push_bind(" OR cm.scheme_viewer = ", true)
            .push(")")
            .push(&scope.same_organization(&[("cm", table)]))
            .push(") AND NOT EXISTS (SELECT 1 FROM ")
            .push(&inherited)
            .push(&format!(
                " AS im WHERE im.container_id = {table}.container_id \
                   AND im.user_id = {table}.user_id"
            ))
            .push_bind(" AND im.org_id = ", org.clone())
            .push(&scope.same_organization(&[("im", table)]))
            .push(")");
        if scope.dialect() == Dialect::Postgres {
            sql.push_bind(" AND org_id = ", org.clone());
        }
        scope.execute(sql).await?;
    }
    Ok(())
}

/// Deletes every `unit_reviews` and `unit_notes` row of `user_id` belonging to the
/// scope's organization, without touching other organizations or personal spaces.
pub(crate) async fn forget_user_in_organization(
    scope: &mut Scope<'_>,
    user_id: &str,
) -> Result<(), StoreError> {
    let org = scope.org()?;
    for table in TABLES {
        let mut sql = scope.sql(&format!("DELETE FROM {table} WHERE user_id = "));
        sql.changing_no_access().bind(Id(user_id));
        match scope.dialect() {
            Dialect::Postgres => {
                sql.push_bind(" AND org_id = ", org.clone());
            }
            Dialect::Sqlite => {
                sql.push_bind(" AND (org_id = ", org.clone())
                    .push_bind(
                        " OR (org_id = '' AND container_id IN (\
                         SELECT c.id FROM containers AS c \
                         JOIN workspaces AS w ON w.id = c.workspace_id WHERE w.org_id = ",
                        org.clone(),
                    )
                    .push_bind(
                        " UNION SELECT d.id FROM deleted_containers AS d \
                         JOIN workspaces AS w ON w.id = d.workspace_id WHERE w.org_id = ",
                        org.clone(),
                    )
                    .push(")))");
            }
        }
        scope.execute(sql).await?;
    }
    Ok(())
}
