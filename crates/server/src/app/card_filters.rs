//! The cards of a board that a list asks for by more than its page: by their title or
//! number, their place, the people they are given to, a value, their last change, and
//! in another order than the oldest first. They are read whole and chosen here.

use fvoci_model::{BlockType, Card, CardPropertyDefinition, assigned_users};
use fvoci_store::{BlockFilter, ListedBlock};
use serde_json::Value;

use super::App;
use super::cards::CardPreviews;
use super::list_page::ListPage;
use crate::auth::now_millis;
use crate::error::AppError;

/// Where the cards are on the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardsUnder {
    /// The cards of the top level.
    Top,
    /// The cards right below this one.
    Card(String),
}

/// The order of the cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardsOrder {
    /// The oldest first.
    #[default]
    CreatedFirst,
    /// The last changed first.
    UpdatedLast,
    /// By their numbers; templates, which show none, last.
    Numbered,
}

/// Which cards of a board a list gives, and in what order.
#[derive(Debug, Clone, Default)]
pub struct CardsFilter {
    /// A text the title holds, whatever the case of its letters, or `#12`: the card of
    /// that number.
    pub text: Option<String>,
    /// Where the cards are.
    pub under: Option<CardsUnder>,
    /// A user a property that gives the card to people (`person`, `multiPerson`) holds.
    pub given_to: Option<String>,
    /// A property and a value it is, or holds among its values.
    pub value: Option<(String, String)>,
    /// Changed at or after this time, in milliseconds since the Unix epoch.
    pub updated_since: Option<i64>,
    /// Their order.
    pub order: CardsOrder,
}

impl CardsFilter {
    /// Whether the filter chooses every card in the order of the store's pages.
    #[must_use]
    pub fn is_everything(&self) -> bool {
        self.text.is_none()
            && self.under.is_none()
            && self.given_to.is_none()
            && self.value.is_none()
            && self.updated_since.is_none()
            && self.order == CardsOrder::CreatedFirst
    }
}

impl CardsFilter {
    /// Whether the card is one the filter chooses, `definitions` being the properties
    /// of its board.
    fn chooses(&self, card: &Card, definitions: &[CardPropertyDefinition]) -> bool {
        self.text
            .as_deref()
            .is_none_or(|text| title_holds(card, text))
            && self.under.as_ref().is_none_or(|under| match under {
                CardsUnder::Top => card.parent_id.is_empty(),
                CardsUnder::Card(parent) => &card.parent_id == parent,
            })
            && self
                .given_to
                .as_deref()
                .is_none_or(|user| assigned_users(&card.properties, definitions).contains(&user))
            && self
                .value
                .as_ref()
                .is_none_or(|(property, value)| holds(card.properties.get(property), value))
            && self
                .updated_since
                .is_none_or(|since| card.updated_at >= since)
    }
}

impl CardsOrder {
    fn sort(self, cards: &mut [Card]) {
        match self {
            Self::CreatedFirst => {
                cards.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
            }
            Self::UpdatedLast => {
                cards.sort_by(|a, b| (b.updated_at, &b.id).cmp(&(a.updated_at, &a.id)));
            }
            Self::Numbered => {
                cards.sort_by_key(|card| (card.number.is_none(), card.number, card.created_at));
            }
        }
    }
}

fn holds(value: Option<&Value>, wanted: &str) -> bool {
    match value {
        Some(Value::String(text)) => text == wanted,
        Some(Value::Array(items)) => items.iter().any(|item| item.as_str() == Some(wanted)),
        _ => false,
    }
}

fn title_holds(card: &Card, text: &str) -> bool {
    if let Some(number) = text.strip_prefix('#').and_then(|n| n.parse::<u32>().ok()) {
        return card.number == Some(number);
    }
    card.title.to_lowercase().contains(&text.to_lowercase())
}

impl App {
    /// A card of a list, from its block as the list read it.
    pub(super) fn card_of_listed(listed: &ListedBlock, now: i64) -> Result<Card, AppError> {
        let card = super::cards::card_from_stored_block(&listed.block, now)
            .map_err(|error| AppError::from(error).context("Block2Card fail"))?;
        Ok(Card {
            last_comment_at: listed.last_comment_at,
            preview: listed.preview.as_ref().map(|preview| {
                fvoci_model::CardPreview::new(
                    preview.first_image.clone().unwrap_or_default(),
                    &preview.text,
                )
            }),
            ..card.listed(listed.summary)
        })
    }

    /// One page of the cards of the board `board_id` that `filter` chooses, in its
    /// order.
    ///
    /// # Errors
    ///
    /// Returns the store's error if the cards or the board cannot be read.
    pub async fn filtered_cards(
        &self,
        board_id: &str,
        filter: &CardsFilter,
        page: ListPage,
        previews: CardPreviews,
    ) -> Result<Vec<Card>, AppError> {
        let filter_of_store = BlockFilter::container(board_id).with_type(BlockType::Card);
        let listed = self
            .store()
            .get_listed_blocks(&filter_of_store, previews.into())
            .await?;
        let definitions = match filter.given_to {
            Some(_) => self.store().get_container(board_id).await?.card_properties,
            None => Vec::new(),
        };
        let now = now_millis();
        let mut cards = Vec::new();
        for listed in &listed {
            let card = Self::card_of_listed(listed, now)?;
            if filter.chooses(&card, &definitions) {
                cards.push(card);
            }
        }
        filter.order.sort(&mut cards);
        let paging = page.paging();
        if paging.per_page == 0 {
            return Ok(cards);
        }
        let per_page = usize::try_from(paging.per_page).unwrap_or(usize::MAX);
        let skipped = usize::try_from(paging.page)
            .unwrap_or(usize::MAX)
            .saturating_mul(per_page);
        Ok(cards.into_iter().skip(skipped).take(per_page).collect())
    }
}
