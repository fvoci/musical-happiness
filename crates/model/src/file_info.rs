//! The record kept about an uploaded file.

use crate::serde_util::null_as_default;
use serde::{Deserialize, Serialize};

/// What the server records about an uploaded file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct FileInfo {
    /// The record's identifier.
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    /// The board the file belongs to. Never sent to clients: they ask for a file
    /// through its board.
    #[serde(skip)]
    pub board_id: String,
    /// The name the file is stored and asked for under: random, with the extension.
    /// Never sent to clients in this record; they have it from the upload's answer.
    #[serde(skip)]
    pub stored_name: String,
    /// Creation time in milliseconds since the Unix epoch.
    #[serde(deserialize_with = "null_as_default")]
    pub create_at: i64,
    /// Where the file lives in the file storage. Never sent to clients.
    #[serde(skip)]
    pub path: String,
    /// The name the file was uploaded under.
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    /// The lower-cased extension of the name, with its leading dot.
    #[serde(deserialize_with = "null_as_default")]
    pub extension: String,
    /// The size of the file in bytes.
    #[serde(deserialize_with = "null_as_default")]
    pub size: i64,
}

/// Whether `path` is a place of the file storage written the one way the storage takes:
/// relative, its parts parted by single slashes, none of them empty, `.` or `..`, and
/// no backslash or NUL in it. The storage refuses any other spelling, and the store
/// takes no other into a record, a mark of writing or an upload session: a place has
/// one spelling, which the checks of whether an object is held compare.
#[must_use]
pub fn is_storage_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_place_has_one_spelling() {
        for path in [
            "boards/20261112/b/x.png",
            "b/x.png",
            "x",
            "a/.hidden/b.c..d",
        ] {
            assert!(is_storage_path(path), "{path}");
        }
        for path in [
            "",
            "/boards/x.png",
            "boards//x.png",
            "boards/x.png/",
            "./boards/x.png",
            "boards/./x.png",
            "boards/../x.png",
            "boards\\x.png",
            "boards/x\0.png",
        ] {
            assert!(!is_storage_path(path), "{path:?}");
        }
    }

    #[test]
    fn json_form_has_no_place_in_the_storage() {
        let file_info = FileInfo {
            id: "abc".to_owned(),
            board_id: "board".to_owned(),
            stored_name: "random.png".to_owned(),
            create_at: 1,
            path: "boards/20240101/board/random.png".to_owned(),
            name: "picture.png".to_owned(),
            extension: ".png".to_owned(),
            size: 4,
        };
        let encoded = serde_json::to_value(&file_info).expect("a file info serializes");
        assert_eq!(
            encoded,
            json!({
                "id": "abc",
                "createAt": 1,
                "name": "picture.png",
                "extension": ".png",
                "size": 4,
            }),
            "neither the path, nor the board, nor the stored name"
        );
    }
}
