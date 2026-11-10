//! Records about uploaded files.

use fvoci_model::FileInfo;

use crate::Store;
use crate::db::Scope;
use crate::error::StoreError;

impl Store {
    /// Returns the record with the given identifier, whether marked as deleted or not.
    ///
    /// The creator, the modification time and the media type are not stored and read as
    /// empty.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if there is no such record.
    pub async fn get_file_info(&self, id: &str) -> Result<FileInfo, StoreError> {
        self.with_conn(async |scope| get_file_info(scope, id).await)
            .await
    }

    /// Stores a record about a file.
    ///
    /// The identifier, the creation and deletion times, the name, the extension, the
    /// size and the path are stored. The record is stored as not archived whatever
    /// `file_info` says, and nothing prevents a second record with the same identifier.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the statement fails.
    pub async fn save_file_info(&self, file_info: &FileInfo) -> Result<(), StoreError> {
        self.with_conn(async |scope| save_file_info(scope, file_info).await)
            .await
    }
}

pub(crate) async fn get_file_info(scope: &mut Scope<'_>, id: &str) -> Result<FileInfo, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT id, create_at, delete_at, name, extension, size, archived, path FROM {} \
         WHERE id = ",
        scope.table("file_info")
    ));
    sql.bind(id);
    let row = scope
        .fetch_optional(sql)
        .await?
        .ok_or_else(|| StoreError::not_found(format_args!("file info ID={id}")))?;
    Ok(FileInfo {
        id: row.text(0)?,
        create_at: row.int(1)?,
        delete_at: row.int(2)?,
        name: row.text(3)?,
        extension: row.text(4)?,
        size: row.int(5)?,
        archived: row.bool(6)?,
        path: row.text(7)?,
        ..FileInfo::default()
    })
}

pub(crate) async fn save_file_info(
    scope: &mut Scope<'_>,
    file_info: &FileInfo,
) -> Result<(), StoreError> {
    // Preserved quirk: `archived` is always written as false, and the table has no key,
    // so saving one identifier twice leaves two rows
    // (`server/services/store/sqlstore/file.go:36`).
    let mut sql = scope.sql(&format!(
        "INSERT INTO {} (id, create_at, name, extension, size, delete_at, path, archived) \
         VALUES (",
        scope.table("file_info")
    ));
    sql.bind(&file_info.id)
        .push_bind(", ", file_info.create_at)
        .push_bind(", ", &file_info.name)
        .push_bind(", ", &file_info.extension)
        .push_bind(", ", file_info.size)
        .push_bind(", ", file_info.delete_at)
        .push_bind(", ", &file_info.path)
        .push_bind(", ", false)
        .push(")");
    scope.execute(sql).await?;
    Ok(())
}
