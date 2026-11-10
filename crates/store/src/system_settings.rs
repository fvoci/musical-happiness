//! System settings: named text values the server keeps about itself.

use std::collections::BTreeMap;

use crate::Store;
use crate::db::Scope;
use crate::error::StoreError;

impl Store {
    /// Returns the value of the setting named `key`, or `None` if it was never set.
    ///
    /// The original returns the empty string for a setting that was never set.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_system_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.with_conn(async |scope| get_system_setting(scope, key).await)
            .await
    }

    /// Returns every setting by name.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn get_system_settings(&self) -> Result<BTreeMap<String, String>, StoreError> {
        self.with_conn(async |scope| {
            let sql = scope.sql(&format!(
                "SELECT id, value FROM {}",
                scope.table("system_settings")
            ));
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| Ok((row.text(0)?, row.text(1)?)))
                .collect()
        })
        .await
    }

    /// Sets the setting named `key` to `value`, replacing any earlier value.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn set_system_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.with_conn(async |scope| set_system_setting(scope, key, value).await)
            .await
    }
}

pub(crate) async fn get_system_setting(
    scope: &mut Scope<'_>,
    key: &str,
) -> Result<Option<String>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT value FROM {} WHERE id = ",
        scope.table("system_settings")
    ));
    sql.bind(key);
    scope
        .fetch_optional(sql)
        .await?
        .map(|row| row.text(0))
        .transpose()
}

pub(crate) async fn set_system_setting(
    scope: &mut Scope<'_>,
    key: &str,
    value: &str,
) -> Result<(), StoreError> {
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} (id, value) VALUES (",
        scope.table("system_settings")
    ));
    sql.bind(key)
        .push_bind(", ", value)
        .push(") ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value");
    scope.execute(sql).await?;
    Ok(())
}
