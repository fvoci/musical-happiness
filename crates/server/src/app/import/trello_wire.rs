//! Deserialization wire types for Trello board JSON exports.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloBoard {
    pub(super) name: String,
    #[serde(default)]
    pub(super) desc: String,
    pub(super) lists: Option<Vec<TrelloList>>,
    pub(super) cards: Option<Vec<TrelloCard>>,
    #[serde(default)]
    pub(super) labels: Vec<TrelloLabel>,
    #[serde(default)]
    pub(super) checklists: Vec<TrelloChecklist>,
    #[serde(default)]
    pub(super) actions: Vec<TrelloAction>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloList {
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) closed: bool,
    #[serde(default)]
    pub(super) pos: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloLabel {
    #[serde(default)]
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) color: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloCard {
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) desc: String,
    #[serde(default)]
    pub(super) closed: bool,
    #[serde(default)]
    pub(super) id_list: String,
    #[serde(default)]
    pub(super) pos: f64,
    #[serde(default)]
    pub(super) due: Option<String>,
    #[serde(default)]
    pub(super) start: Option<String>,
    #[serde(default)]
    pub(super) id_labels: Vec<String>,
    #[serde(default)]
    pub(super) labels: Vec<TrelloLabel>,
    #[serde(default)]
    pub(super) id_checklists: Vec<String>,
    #[serde(default)]
    pub(super) attachments: Vec<TrelloAttachment>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloChecklist {
    pub(super) id: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) id_card: String,
    #[serde(default)]
    pub(super) pos: f64,
    #[serde(default)]
    pub(super) check_items: Vec<TrelloCheckItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloCheckItem {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) state: String,
    #[serde(default)]
    pub(super) pos: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloAttachment {
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloAction {
    #[serde(default, rename = "type")]
    pub(super) action_type: String,
    #[serde(default)]
    pub(super) date: Option<String>,
    #[serde(default)]
    pub(super) data: TrelloActionData,
    #[serde(default)]
    pub(super) member_creator: Option<TrelloMember>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloActionData {
    #[serde(default)]
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) card: Option<TrelloActionCard>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloActionCard {
    #[serde(default)]
    pub(super) id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TrelloMember {
    #[serde(default)]
    pub(super) full_name: Option<String>,
    #[serde(default)]
    pub(super) username: Option<String>,
}
