//! The record kept about an uploaded file.

use serde::{Deserialize, Serialize};

/// What the server records about an uploaded file.
///
/// The members are the ones the original server stores or sets for a new upload. Its
/// JSON form of this record has further members, which a standalone server never fills
/// in; they are not part of this type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileInfo {
    /// The record's identifier: the stored file name without its leading kind character
    /// and without its extension.
    pub id: String,
    /// Who uploaded the file; the original sets the fixed text `boards`. Not stored.
    #[serde(rename = "user_id")]
    pub creator_id: String,
    /// Creation time in milliseconds since the Unix epoch.
    pub create_at: i64,
    /// Last modification time in milliseconds since the Unix epoch. Not stored.
    pub update_at: i64,
    /// Deletion time in milliseconds since the Unix epoch; zero if not deleted.
    pub delete_at: i64,
    /// Where the file lives in the file storage. Never sent to clients.
    #[serde(skip)]
    pub path: String,
    /// The name the file was uploaded under.
    pub name: String,
    /// The lower-cased extension of the name, with its leading dot.
    pub extension: String,
    /// The size of the file in bytes.
    pub size: i64,
    /// The media type derived from the extension. Not stored.
    pub mime_type: String,
    /// Whether the file has been archived.
    pub archived: bool,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_form_has_the_original_member_names_and_no_path() {
        let file_info = FileInfo {
            id: "abc".to_owned(),
            creator_id: "boards".to_owned(),
            create_at: 1,
            update_at: 2,
            delete_at: 3,
            path: "boards/20240101/7abc.png".to_owned(),
            name: "picture.png".to_owned(),
            extension: ".png".to_owned(),
            size: 4,
            mime_type: "image/png".to_owned(),
            archived: true,
        };
        let encoded = serde_json::to_string(&file_info).expect("a file info serializes");
        assert_eq!(
            encoded,
            r#"{"id":"abc","user_id":"boards","create_at":1,"update_at":2,"delete_at":3,"name":"picture.png","extension":".png","size":4,"mime_type":"image/png","archived":true}"#,
            "members in the original's order"
        );

        let decoded: FileInfo = serde_json::from_value(json!({
            "id": "abc",
            "user_id": "boards",
            "path": "ignored",
            "channel_id": "",
            "mini_preview": null,
            "remote_id": null,
        }))
        .expect("the original's extra members are ignored");
        assert_eq!(
            decoded,
            FileInfo {
                id: "abc".to_owned(),
                creator_id: "boards".to_owned(),
                ..FileInfo::default()
            },
            "missing members default and the path is never read"
        );
    }
}
