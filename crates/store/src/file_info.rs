//! Records about uploaded files.
//!
//! A file is one board's. It is stored and asked for under a name the server made up,
//! and the record ties that name to the board, to the place in the file storage and to
//! the name the file was uploaded with.

use fvoci_model::FileInfo;

use crate::Store;
use crate::db::{Dialect, Id, Row, Scope};
use crate::error::StoreError;

pub(crate) const FILE_INFO_COLUMNS: &str =
    "id, container_id, name, original_name, extension, size, path, create_at";

/// How many columns [`FILE_INFO_COLUMNS`] names: the index of a column read after them.
pub(crate) const FILE_INFO_COLUMN_COUNT: usize = 8;

impl Store {
    /// Returns the record of the file a board has under a stored name.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::NotFound`] if the board has no file of that name. A file of
    /// another board is not found.
    pub async fn get_file_info(
        &self,
        container_id: &str,
        stored_name: &str,
    ) -> Result<FileInfo, StoreError> {
        self.with_conn(async |scope| {
            let mut sql = scope.sql(&format!(
                "SELECT {FILE_INFO_COLUMNS} FROM file_info WHERE container_id = "
            ));
            sql.bind(Id(container_id))
                .push_bind(" AND name = ", stored_name);
            let row = scope
                .fetch_optional(sql)
                .await?
                .ok_or_else(|| StoreError::not_found("file"))?;
            file_info_from_row(&row)
        })
        .await
    }

    /// Stores a record about a file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidId`] if the record's or the board's identifier is not
    /// one, [`StoreError::InvalidPath`] for a place spelled otherwise than the one way,
    /// [`StoreError::NotFound`] if the board does not exist, and
    /// [`StoreError::Database`] if the board has a file of that stored name already.
    pub async fn save_file_info(&self, file_info: &FileInfo) -> Result<(), StoreError> {
        check_new_path(&file_info.path)?;
        self.with_conn(async |scope| save_file_info(scope, file_info).await)
            .await
    }
}

impl Store {
    /// Returns up to `limit` places that file records name, live or kept with a board in
    /// the trash, after `after` in their order: a page of every record the handle
    /// reaches, for the comparison of the records with the file storage.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Database`] if the query fails.
    pub async fn recorded_paths_after(
        &self,
        after: &str,
        limit: i64,
    ) -> Result<Vec<String>, StoreError> {
        self.with_conn(async |scope| {
            let (trashed_path, trashed_files) = match scope.dialect() {
                Dialect::Sqlite => ("json_extract(f.value, '$.path')", "json_each(d.files) AS f"),
                Dialect::Postgres => (
                    "f.value ->> 'path'",
                    "json_array_elements(d.files) AS f(value)",
                ),
            };
            // Each side takes its own page first, so that the live records are read a
            // page at a time by their index; the trash has none to read by.
            let mut sql = scope
                .sql("SELECT path FROM (SELECT * FROM (SELECT path FROM file_info WHERE path > ");
            sql.bind(after)
                .push_bind(" ORDER BY path LIMIT ", limit)
                .push(&format!(
                    ") AS live UNION SELECT * FROM (SELECT {trashed_path} AS path FROM \
                     deleted_containers AS d, {trashed_files} WHERE {trashed_path} > "
                ))
                .bind(after)
                .push_bind(" ORDER BY 1 LIMIT ", limit)
                .push_bind(") AS trashed) AS recorded ORDER BY path LIMIT ", limit);
            scope
                .fetch_all(sql)
                .await?
                .iter()
                .map(|row| row.text(0))
                .collect()
        })
        .await
    }
}

/// Refuses a place of the file storage that a new record, mark or session would name
/// otherwise than the one way ([`fvoci_model::is_storage_path`]); the empty place of a
/// record or session that names none passes. A record the store held already, as one
/// that comes back from the trash, is taken as it was.
pub(crate) fn check_new_path(path: &str) -> Result<(), StoreError> {
    if path.is_empty() || fvoci_model::is_storage_path(path) {
        Ok(())
    } else {
        Err(StoreError::InvalidPath(path.to_owned()))
    }
}

/// The places that rows name otherwise than the one way: those that a rough condition
/// finds, checked with [`fvoci_model::is_storage_path`].
pub(crate) async fn other_spellings(scope: &mut Scope<'_>) -> Result<Vec<String>, StoreError> {
    let (trashed_path, trashed_files, backslash) = match scope.dialect() {
        Dialect::Sqlite => (
            "json_extract(f.value, '$.path')",
            "json_each(d.files) AS f",
            "instr({p}, char(92)) > 0",
        ),
        Dialect::Postgres => (
            "f.value ->> 'path'",
            "json_array_elements(d.files) AS f(value)",
            "strpos({p}, chr(92)) > 0",
        ),
    };
    let rough = |p: &str| {
        format!(
            "{p} <> '' AND ({p} LIKE '%//%' OR {p} LIKE '/%' OR {p} LIKE '%/' \
             OR {p} LIKE '.%' OR {p} LIKE '%/.%' OR {})",
            backslash.replace("{p}", p)
        )
    };
    let statement = format!(
        "SELECT path FROM file_info WHERE {} \
         UNION SELECT path FROM uploads WHERE {} \
         UNION SELECT path FROM loose_objects WHERE {} \
         UNION SELECT {trashed_path} FROM deleted_containers AS d, {trashed_files} WHERE {}",
        rough("path"),
        rough("path"),
        rough("path"),
        rough(trashed_path)
    );
    let sql = scope.sql(&statement);
    let mut found = Vec::new();
    for row in scope.fetch_all(sql).await? {
        let path = row.text(0)?;
        if !fvoci_model::is_storage_path(&path) {
            found.push(path);
        }
    }
    Ok(found)
}

pub(crate) async fn save_file_info(
    scope: &mut Scope<'_>,
    file_info: &FileInfo,
) -> Result<(), StoreError> {
    // A new file is shown by no block until one is written that shows it.
    let mut sql = scope.sql(&format!(
        "INSERT INTO file_info ({FILE_INFO_COLUMNS}, unreferenced_at) VALUES ("
    ));
    sql.bind(Id(&file_info.id))
        .push_bind(", ", Id(&file_info.container_id))
        .push_bind(", ", &file_info.stored_name)
        .push_bind(", ", &file_info.name)
        .push_bind(", ", &file_info.extension)
        .push_bind(", ", file_info.size)
        .push_bind(", ", &file_info.path)
        .push_bind(", ", file_info.create_at)
        .push_bind(", ", file_info.create_at)
        .push(")");
    scope
        .execute(sql)
        .await
        .map_err(|error| error.missing_reference("board"))?;
    Ok(())
}

/// The records of every file of a board, oldest first, held on PostgreSQL until the
/// transaction ends: the board's deletion takes them to the trash, and a collection of
/// the files no block shows skips them meanwhile (PostgreSQL); or it took one before,
/// and that one is not taken to the trash.
pub(crate) async fn file_infos_of_container(
    scope: &mut Scope<'_>,
    container_id: &str,
) -> Result<Vec<FileInfo>, StoreError> {
    let mut sql = scope.sql(&format!(
        "SELECT {FILE_INFO_COLUMNS} FROM file_info WHERE container_id = "
    ));
    sql.bind(Id(container_id)).push(" ORDER BY create_at, id");
    if scope.dialect() == Dialect::Postgres {
        sql.push(" FOR UPDATE");
    }
    scope
        .fetch_all(sql)
        .await?
        .iter()
        .map(file_info_from_row)
        .collect()
}

pub(crate) fn file_info_from_row(row: &Row) -> Result<FileInfo, StoreError> {
    Ok(FileInfo {
        id: row.id(0)?,
        container_id: row.id(1)?,
        stored_name: row.text(2)?,
        name: row.text(3)?,
        extension: row.text(4)?,
        size: row.int(5)?,
        path: row.text(6)?,
        create_at: row.int(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_of_the_columns_is_the_columns() {
        assert_eq!(
            FILE_INFO_COLUMNS.split(',').count(),
            FILE_INFO_COLUMN_COUNT,
            "{FILE_INFO_COLUMNS}"
        );
    }
}
