//! Card relations: a card property of the `type` `relation` whose value names other
//! cards of the same board, by their identifiers.
//!
//! A value is `null` or a list of card identifiers, each once, never the card itself,
//! at most [`RELATION_LIMIT`] of them. What this module checks needs nothing but the
//! value; whether each identifier is a card of the board is the store's to ask
//! ([`RelationProblem::NotACard`], [`RelationProblem::CardNotFound`]).

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;

/// The `type` of a property definition whose values name cards of the board.
pub const RELATION_PROPERTY_TYPE: &str = "relation";

/// The most cards one relation value names.
pub const RELATION_LIMIT: usize = 50;

/// What is wrong with a relation value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationProblem {
    /// The value is neither `null` nor a list of strings.
    NotAList,
    /// The value names the card it is the value of.
    ToItself,
    /// The value names a card twice.
    Repeated,
    /// The value names more than [`RELATION_LIMIT`] cards.
    TooMany,
    /// The value names a block of the board that is no card.
    NotACard,
    /// The value names no card of the board, in the trash or out of it: there is none
    /// of that identifier, or it is another board's. The two are not told apart.
    CardNotFound,
}

/// A relation value that is refused, with the property it is the value of.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{}", message(.property, *.problem))]
pub struct RelationError {
    /// The identifier of the property.
    pub property: String,
    /// What is wrong with the value.
    pub problem: RelationProblem,
}

fn message(property: &str, problem: RelationProblem) -> String {
    match problem {
        RelationProblem::NotAList => format!(
            "the value of card relation {property:?} must be a list of card identifiers or null"
        ),
        RelationProblem::ToItself => format!("card relation {property:?} names the card itself"),
        RelationProblem::Repeated => format!("card relation {property:?} names a card twice"),
        RelationProblem::TooMany => {
            format!("card relation {property:?} names more than {RELATION_LIMIT} cards")
        }
        RelationProblem::NotACard => {
            format!("card relation {property:?} names a block of the board that is no card")
        }
        RelationProblem::CardNotFound => {
            format!("card relation {property:?} names a card that is not on the board")
        }
    }
}

impl RelationError {
    /// The refusal of the value of `property`.
    #[must_use]
    pub fn new(property: &str, problem: RelationProblem) -> Self {
        Self {
            property: property.to_owned(),
            problem,
        }
    }
}

/// The identifiers of the relation properties among a board's definitions.
#[must_use]
pub fn relation_property_ids(definitions: &[JsonMap]) -> HashSet<&str> {
    definitions
        .iter()
        .filter(|definition| {
            definition.get("type").and_then(Value::as_str) == Some(RELATION_PROPERTY_TYPE)
        })
        .filter_map(|definition| definition.get("id").and_then(Value::as_str))
        .collect()
}

/// Checks a relation value of the card `card_id` that a write gives it, and returns the
/// identifiers it names (none for `null`).
///
/// # Errors
///
/// Returns the [`RelationError`] of the first rule the value breaks: no list of
/// strings, more than [`RELATION_LIMIT`] identifiers, the card itself, one twice.
pub fn check_relation_value<'v>(
    card_id: &str,
    property: &str,
    value: &'v Value,
) -> Result<Vec<&'v str>, RelationError> {
    let refuse = |problem| Err(RelationError::new(property, problem));
    let items = match value {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) => items,
        Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => {
            return refuse(RelationProblem::NotAList);
        }
    };
    let Some(ids) = items.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
        return refuse(RelationProblem::NotAList);
    };
    if ids.len() > RELATION_LIMIT {
        return refuse(RelationProblem::TooMany);
    }
    if ids.contains(&card_id) {
        return refuse(RelationProblem::ToItself);
    }
    let mut seen = HashSet::with_capacity(ids.len());
    if !ids.iter().all(|id| seen.insert(*id)) {
        return refuse(RelationProblem::Repeated);
    }
    Ok(ids)
}

/// Makes the relation values of a card that is copied ones a write takes: a value that
/// is no list becomes `null`, and of a list what is no identifier, the card itself and
/// what is named a second time go. For copies of stored cards, whose values may be older
/// than the rules or than a change of the definitions. Which of the cards named are
/// there, and so what is left of [`RELATION_LIMIT`], is for the copy to settle.
pub fn tidy_copied_relations(values: &mut JsonMap, card_id: &str, definitions: &[JsonMap]) {
    let relations = relation_property_ids(definitions);
    for (id, value) in values.iter_mut() {
        if !relations.contains(id.as_str()) {
            continue;
        }
        let Value::Array(items) = value else {
            if !value.is_null() {
                *value = Value::Null;
            }
            continue;
        };
        let mut seen = HashSet::with_capacity(items.len());
        items.retain(|item| {
            item.as_str().is_some_and(|named| {
                !named.is_empty() && named != card_id && seen.insert(named.to_owned())
            })
        });
    }
}

/// The card relation properties of boards, by board identifier: whose values name
/// cards that get new identifiers along with them ([`crate::renew_block_ids`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationProperties {
    by_board: HashMap<String, HashSet<String>>,
}

impl RelationProperties {
    /// No board has relation properties.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The relation properties among one board's definitions.
    #[must_use]
    pub fn of_board(board_id: &str, definitions: &[JsonMap]) -> Self {
        let mut properties = Self::new();
        properties.add_board(board_id, definitions);
        properties
    }

    /// Adds the relation properties among a board's definitions.
    pub fn add_board(&mut self, board_id: &str, definitions: &[JsonMap]) {
        let ids: HashSet<String> = relation_property_ids(definitions)
            .into_iter()
            .map(str::to_owned)
            .collect();
        if !ids.is_empty() {
            self.by_board.insert(board_id.to_owned(), ids);
        }
    }

    /// The relation properties of a board, if it has any.
    pub(crate) fn of(&self, board_id: &str) -> Option<&HashSet<String>> {
        self.by_board.get(board_id)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> JsonMap {
        match value {
            Value::Object(object) => object,
            other => panic!("not an object: {other}"),
        }
    }

    fn problem_of(value: &Value) -> Option<RelationProblem> {
        check_relation_value("me", "rel", value)
            .err()
            .map(|error| error.problem)
    }

    #[test]
    fn a_value_is_null_or_a_list_of_other_cards_each_once() {
        assert_eq!(
            check_relation_value("me", "rel", &json!(null)),
            Ok(Vec::new())
        );
        assert_eq!(
            check_relation_value("me", "rel", &json!(["a", "b"])),
            Ok(vec!["a", "b"])
        );
        assert_eq!(problem_of(&json!("a")), Some(RelationProblem::NotAList));
        assert_eq!(
            problem_of(&json!({"a": 1})),
            Some(RelationProblem::NotAList)
        );
        assert_eq!(
            problem_of(&json!(["a", 1])),
            Some(RelationProblem::NotAList)
        );
        assert_eq!(
            problem_of(&json!(["a", "me"])),
            Some(RelationProblem::ToItself)
        );
        assert_eq!(
            problem_of(&json!(["a", "b", "a"])),
            Some(RelationProblem::Repeated)
        );
    }

    #[test]
    fn a_value_names_at_most_the_limit() {
        let ids = |count: usize| -> Value {
            Value::Array((0..count).map(|index| json!(format!("c{index}"))).collect())
        };
        assert!(check_relation_value("me", "rel", &ids(RELATION_LIMIT)).is_ok());
        assert_eq!(
            problem_of(&ids(RELATION_LIMIT + 1)),
            Some(RelationProblem::TooMany)
        );
    }

    #[test]
    fn the_refusal_names_the_property_and_not_the_cards() {
        let error = check_relation_value("me", "rel", &json!(["me"]))
            .expect_err("the card itself is refused");
        assert_eq!(error.property, "rel");
        assert!(error.to_string().contains("\"rel\""), "{error}");
        assert!(!error.to_string().contains("me\""), "{error}");
    }

    #[test]
    fn relation_properties_are_those_of_the_type() {
        let definitions = [
            object(json!({"id": "rel", "type": "relation"})),
            object(json!({"id": "text", "type": "text"})),
            object(json!({"id": "untyped"})),
        ];
        assert_eq!(relation_property_ids(&definitions), HashSet::from(["rel"]));
        let properties = RelationProperties::of_board("b", &definitions);
        assert!(properties.of("b").is_some_and(|ids| ids.contains("rel")));
        assert!(properties.of("other").is_none());
        assert!(
            RelationProperties::of_board("b", &definitions[1..])
                .of("b")
                .is_none(),
            "a board without relation properties is not listed"
        );
    }

    #[test]
    fn copied_relations_are_made_ones_a_write_takes() {
        let definitions = [
            object(json!({"id": "rel", "type": "relation"})),
            object(json!({"id": "gone", "type": "relation"})),
            object(json!({"id": "text", "type": "text"})),
        ];
        let many: Vec<String> = (0..60).map(|index| format!("c{index}")).collect();
        let mut values = object(json!({
            "rel": ["a", "me", "a", 7, "", "b"],
            "gone": "left by a text",
            "text": ["me", "me"],
            "unknown": ["me"],
        }));
        tidy_copied_relations(&mut values, "me", &definitions);
        assert_eq!(
            Value::Object(values),
            json!({
                "rel": ["a", "b"],
                "gone": null,
                "text": ["me", "me"],
                "unknown": ["me"],
            })
        );
        let mut values = object(json!({ "rel": many }));
        tidy_copied_relations(&mut values, "me", &definitions);
        assert_eq!(
            values["rel"].as_array().map(Vec::len),
            Some(60),
            "what is past the limit is for the copy to cut, once it dropped what it cannot name"
        );
    }
}
