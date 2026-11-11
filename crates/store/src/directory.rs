//! What is asked before the organization of a request is known, and what makes an
//! organization: which organization there is to work in, and (to come) whose a
//! workspace or a shared board is.
//!
//! Every function here runs one statement, or one short transaction, that reaches the
//! rows of every organization ([`Tenant::System`](crate::Tenant::System)) and is
//! narrowed by what the caller holds. Nothing else of the server asks across
//! organizations on behalf of a request.

use fvoci_model::new_id;

use crate::Store;
use crate::db::{Dialect, Id, Scope, now_millis};
use crate::error::StoreError;

impl Store {
    /// Makes an organization with the given name and returns its identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails, as it does on an SQLite
    /// database that holds an organization already.
    pub async fn create_organization(&self, name: &str) -> Result<String, StoreError> {
        self.system()
            .transaction(async |scope| create_organization(scope, name).await)
            .await
    }

    /// The organization that the routes which name no workspace work in: the oldest
    /// one, made now if there is none.
    ///
    /// TEMPORARY. It stands in for the workspace a request will name, and goes with
    /// the last route that names none.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if a statement fails.
    pub async fn only_organization(&self) -> Result<String, StoreError> {
        self.system()
            .transaction(async |scope| {
                // One at a time: of two servers that start on an empty database, the
                // second waits here and then finds the organization of the first. On
                // SQLite a transaction that writes holds the whole database.
                let mut sql = scope.sql("SELECT id FROM instance WHERE id = 1");
                if scope.dialect() == Dialect::Postgres {
                    sql.push(" FOR UPDATE");
                }
                scope.fetch_optional(sql).await?;

                let mut sql = scope.sql("SELECT id FROM organizations WHERE is_system = ");
                sql.bind(false).push(" ORDER BY create_at, id LIMIT 1");
                match scope.fetch_optional(sql).await? {
                    Some(row) => row.id(0),
                    None => create_organization(scope, "").await,
                }
            })
            .await
    }
}

async fn create_organization(scope: &mut Scope<'_>, name: &str) -> Result<String, StoreError> {
    let id = new_id();
    let now = now_millis();
    let mut sql =
        scope.sql("INSERT INTO organizations (id, name, settings, create_at, update_at) VALUES (");
    sql.bind(Id(&id))
        .push_bind(", ", name)
        .push(", '{}'")
        .push_bind(", ", now)
        .push_bind(", ", now)
        .push(")");
    scope.execute(sql).await?;
    Ok(id)
}
