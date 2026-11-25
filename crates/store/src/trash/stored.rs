//! The JSON forms of a board's sidebar places and file records in `deleted_containers`.

use fvoci_model::FileRecord;

use crate::category_containers::ContainerPlace;

/// A [`ContainerPlace`] as the trash keeps it, in `deleted_containers.places`. Its own type, so
/// that what is stored does not change with the store's interface.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StoredPlace {
    user_id: String,
    category_id: String,
    sort_order: i64,
    hidden: bool,
}

impl From<ContainerPlace> for StoredPlace {
    fn from(place: ContainerPlace) -> Self {
        Self {
            user_id: place.user_id,
            category_id: place.category_id,
            sort_order: place.sort_order,
            hidden: place.hidden,
        }
    }
}

impl From<StoredPlace> for ContainerPlace {
    fn from(place: StoredPlace) -> Self {
        Self {
            user_id: place.user_id,
            category_id: place.category_id,
            sort_order: place.sort_order,
            hidden: place.hidden,
        }
    }
}

/// The file records of a board as they are kept with the board in the trash. A file
/// record's own JSON form is the one for clients, which leaves out where the file is.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(super) struct StoredFiles(Vec<StoredFile>);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredFile {
    id: String,
    container_id: String,
    stored_name: String,
    name: String,
    extension: String,
    size: i64,
    path: String,
    created_at: i64,
    /// The file's newest version; 1 in what was stored before files had versions.
    #[serde(default = "first_version")]
    version: i64,
    #[serde(default)]
    created_by: String,
}

const fn first_version() -> i64 {
    1
}

impl From<Vec<FileRecord>> for StoredFiles {
    fn from(files: Vec<FileRecord>) -> Self {
        Self(
            files
                .into_iter()
                .map(|file| StoredFile {
                    id: file.id,
                    container_id: file.container_id,
                    stored_name: file.stored_name,
                    name: file.name,
                    extension: file.extension,
                    size: file.size,
                    path: file.path,
                    created_at: file.created_at,
                    version: file.version,
                    created_by: file.created_by,
                })
                .collect(),
        )
    }
}

impl From<StoredFiles> for Vec<FileRecord> {
    fn from(files: StoredFiles) -> Self {
        files
            .0
            .into_iter()
            .map(|file| FileRecord {
                id: file.id,
                container_id: file.container_id,
                stored_name: file.stored_name,
                name: file.name,
                extension: file.extension,
                size: file.size,
                path: file.path,
                created_at: file.created_at,
                version: file.version,
                created_by: file.created_by,
            })
            .collect()
    }
}
