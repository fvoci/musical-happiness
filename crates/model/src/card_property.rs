//! The shapes that card property definitions and card property values must have.
//!
//! A board defines the properties of its cards (`cardProperties`), and every card holds a
//! value per property (`fields.properties` of its block). Both are free-form JSON, and
//! clients render them as they are: a value is text or a list of texts, and a definition
//! names itself and its options with strings, and the value of a date property is a
//! date value (`docs/s6-design.md` 5.7). Anything else is refused before it is stored,
//! so that one member cannot break the board for the others.

use std::collections::BTreeSet;

use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;
use crate::date_value::{DateValueError, check_date_value};

/// The `type` of a property definition whose values are date values.
pub const DATE_PROPERTY_TYPE: &str = "date";

/// The members of a property definition that must be strings when present.
const DEFINITION_STRING_MEMBERS: [&str; 2] = ["name", "type"];

/// The members of a property option that must be strings when present.
const OPTION_STRING_MEMBERS: [&str; 3] = ["id", "value", "color"];

/// Reasons card property data is rejected before it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CardPropertyError {
    /// The properties of a block are not a JSON object.
    #[error("card properties must be an object of property values")]
    ValuesNotObject,
    /// A property value is neither a string, nor a list of strings, nor a date value,
    /// nor `null`.
    #[error(
        "the value of card property {id:?} must be a string, a list of strings or a date value"
    )]
    InvalidValue {
        /// The identifier of the property.
        id: String,
    },
    /// A property value is an object that is no date value.
    #[error("the date value of card property {id:?} is refused: {reason}")]
    InvalidDateValue {
        /// The identifier of the property.
        id: String,
        /// What is wrong with it.
        reason: DateValueError,
    },
    /// The value of a date property is not a date value.
    #[error("card property {id:?} is a date: its value is a date value or null")]
    NotADateValue {
        /// The identifier of the property.
        id: String,
    },
    /// A date value is the value of a property that is no date.
    #[error("card property {id:?} is not a date property, so its value cannot be a date value")]
    DateValueOfOtherProperty {
        /// The identifier of the property.
        id: String,
    },
    /// A property definition has no `id`, or one that is not a non-empty string.
    #[error("card property definition {position} needs a non-empty string as id")]
    MissingId {
        /// Which definition of the list it is, counted from 1.
        position: usize,
    },
    /// Two property definitions have the same `id`.
    #[error("two card property definitions have the id {id:?}")]
    DuplicateId {
        /// The identifier that is there twice.
        id: String,
    },
    /// A member of a property definition that must be a string is something else.
    #[error("{member} of card property {id:?} must be a string")]
    MemberNotString {
        /// The identifier of the property.
        id: String,
        /// The name of the member.
        member: &'static str,
    },
    /// The options of a property definition are not a list of objects whose `id`,
    /// `value` and `color` are strings.
    #[error(
        "the options of card property {id:?} must be a list of objects with strings as id, value and color"
    )]
    InvalidOptions {
        /// The identifier of the property.
        id: String,
    },
}

/// Checks the property values of a card: each is a string, a list of strings, a date
/// value, or `null`. Whether a date value belongs to a date property is for
/// [`validate_card_property_kinds`], which needs the board's definitions.
///
/// # Errors
///
/// Returns [`CardPropertyError::InvalidValue`] for the first value of another shape,
/// and [`CardPropertyError::InvalidDateValue`] for an object that is no date value.
pub fn validate_card_property_values(values: &JsonMap) -> Result<(), CardPropertyError> {
    for (id, value) in values {
        let valid = match value {
            Value::Null | Value::String(_) => true,
            Value::Array(items) => items.iter().all(Value::is_string),
            Value::Object(date) => {
                check_date_value(date).map_err(|reason| CardPropertyError::InvalidDateValue {
                    id: id.clone(),
                    reason,
                })?;
                true
            }
            Value::Bool(_) | Value::Number(_) => false,
        };
        if !valid {
            return Err(CardPropertyError::InvalidValue { id: id.clone() });
        }
    }
    Ok(())
}

/// Checks that the date values of a card are the values of its board's date properties,
/// and that a date property has a date value or `null`. A value of a property the board
/// does not define is no date value.
///
/// # Errors
///
/// Returns [`CardPropertyError::NotADateValue`] or
/// [`CardPropertyError::DateValueOfOtherProperty`] for the first value in the wrong
/// place.
pub fn validate_card_property_kinds(
    values: &JsonMap,
    definitions: &[JsonMap],
) -> Result<(), CardPropertyError> {
    for (id, value) in values {
        if fits_kind(id, value, definitions) {
            continue;
        }
        return Err(if value.is_object() {
            CardPropertyError::DateValueOfOtherProperty { id: id.clone() }
        } else {
            CardPropertyError::NotADateValue { id: id.clone() }
        });
    }
    Ok(())
}

/// Whether a property is one of the board's date properties.
fn is_date_property(id: &str, definitions: &[JsonMap]) -> bool {
    definitions.iter().any(|definition| {
        definition.get("id").and_then(Value::as_str) == Some(id)
            && definition.get("type").and_then(Value::as_str) == Some(DATE_PROPERTY_TYPE)
    })
}

/// Whether a value fits the kind of its property: a date property holds a date value
/// or `null`, any other property no date value.
fn fits_kind(id: &str, value: &Value, definitions: &[JsonMap]) -> bool {
    matches!(
        (is_date_property(id, definitions), value),
        (true, Value::Null | Value::Object(_))
            | (false, Value::Null | Value::String(_) | Value::Array(_))
    )
}

/// Sets to `null` the values that no longer fit the kind of their property: those of a
/// property that became or stopped being a date, or that the board no longer defines
/// while holding a date value. For what is copied from stored cards, which may be older
/// than a change of the definitions. Returns how many were cleared.
pub fn clear_values_of_another_kind(values: &mut JsonMap, definitions: &[JsonMap]) -> usize {
    let mut cleared = 0;
    for (id, value) in values.iter_mut() {
        // A value of the other kind; a malformed one is left to be refused.
        let other_kind = match value {
            Value::Object(_) => !is_date_property(id, definitions),
            Value::String(_) | Value::Array(_) => is_date_property(id, definitions),
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        };
        if other_kind {
            *value = Value::Null;
            cleared += 1;
        }
    }
    cleared
}

/// Checks the `properties` among a block's fields, if there are any: a JSON object of
/// valid property values.
///
/// # Errors
///
/// Returns [`CardPropertyError::ValuesNotObject`] if `properties` is there and is not an
/// object, or the error of [`validate_card_property_values`].
pub fn validate_block_properties(fields: &JsonMap) -> Result<(), CardPropertyError> {
    match fields.get("properties") {
        None => Ok(()),
        Some(Value::Object(values)) => validate_card_property_values(values),
        Some(_) => Err(CardPropertyError::ValuesNotObject),
    }
}

/// Checks the card property definitions of a board: each has a non-empty string `id`,
/// strings as `name` and `type` if it has them, and as `options`, if it has them, a list
/// of objects whose `id`, `value` and `color` are strings.
///
/// # Errors
///
/// Returns the [`CardPropertyError`] of the first definition that is malformed or has
/// the identifier of an earlier one: a card's value could not say which of the two it
/// belongs to.
pub fn validate_card_property_definitions(
    definitions: &[JsonMap],
) -> Result<(), CardPropertyError> {
    let mut seen = std::collections::HashSet::new();
    for (index, definition) in definitions.iter().enumerate() {
        let id = validate_definition(definition, index + 1)?;
        if !seen.insert(id) {
            return Err(CardPropertyError::DuplicateId { id: id.to_owned() });
        }
    }
    Ok(())
}

/// Checks one definition, the `position`th of its list, and returns its identifier.
fn validate_definition(definition: &JsonMap, position: usize) -> Result<&str, CardPropertyError> {
    let id = match definition.get("id") {
        Some(Value::String(id)) if !id.is_empty() => id,
        _ => return Err(CardPropertyError::MissingId { position }),
    };
    for member in DEFINITION_STRING_MEMBERS {
        if definition
            .get(member)
            .is_some_and(|value| !value.is_string())
        {
            return Err(CardPropertyError::MemberNotString {
                id: id.clone(),
                member,
            });
        }
    }
    let options_are_valid = match definition.get("options") {
        None | Some(Value::Null) => true,
        Some(Value::Array(options)) => options.iter().all(is_valid_option),
        Some(_) => false,
    };
    if options_are_valid {
        Ok(id)
    } else {
        Err(CardPropertyError::InvalidOptions { id: id.clone() })
    }
}

fn is_valid_option(option: &Value) -> bool {
    option.as_object().is_some_and(|option| {
        OPTION_STRING_MEMBERS
            .iter()
            .all(|member| option.get(*member).is_none_or(Value::is_string))
    })
}

/// The `type`s of the property definitions whose values name users: a person, several
/// people, and who made or last changed a card. The last two hold no value of their own,
/// but a view may filter by them.
const PERSON_PROPERTY_TYPES: [&str; 4] = ["person", "multiPerson", "createdBy", "updatedBy"];

/// The properties of a board whose values name users, as the board defines them.
///
/// Whoever reads a shared board by its read token alone is not told who the server's
/// users are (`docs/s6-design.md`, decision 7): the values of these properties go to
/// them emptied, one empty string for each user, so that how many people a card has
/// still shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersonProperties(BTreeSet<String>);

impl PersonProperties {
    /// The person properties among a board's definitions.
    #[must_use]
    pub fn of(definitions: &[JsonMap]) -> Self {
        Self(
            definitions
                .iter()
                .filter(|definition| {
                    definition
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| PERSON_PROPERTY_TYPES.contains(&kind))
                })
                .filter_map(|definition| definition.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Whether the property is one of them.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    /// Empties the users that a card's property values name: a text becomes the empty
    /// text, and each text of a list the empty text.
    pub fn hide_values(&self, values: &mut JsonMap) {
        for (id, value) in values.iter_mut() {
            if self.contains(id) {
                hide_users(value);
            }
        }
    }

    /// Empties the users that the clauses of a view's filter (`fields.filter`) name by
    /// these properties. A filter is a group `{operation, filters}` whose filters are
    /// groups or clauses `{propertyId, condition, values}`.
    pub fn hide_filter_values(&self, filter: &mut Value) {
        if let Some(filters) = filter.get_mut("filters").and_then(Value::as_array_mut) {
            for filter in filters {
                self.hide_filter_values(filter);
            }
            return;
        }
        let names_a_person = filter
            .get("propertyId")
            .and_then(Value::as_str)
            .is_some_and(|id| self.contains(id));
        if names_a_person && let Some(values) = filter.get_mut("values") {
            hide_users(values);
        }
    }
}

/// Empties a text, or each text of a list; leaves anything else as it is.
fn hide_users(value: &mut Value) {
    match value {
        Value::String(user) => user.clear(),
        Value::Array(users) => {
            for user in users.iter_mut() {
                if let Value::String(user) = user {
                    user.clear();
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::date_value::DateValueError;

    fn object(value: Value) -> JsonMap {
        match value {
            Value::Object(object) => object,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn the_users_of_person_properties_are_emptied_one_by_one() {
        let people = PersonProperties::of(&[
            object(json!({"id": "owner", "type": "person"})),
            object(json!({"id": "team", "type": "multiPerson"})),
            object(json!({"id": "by", "type": "createdBy"})),
            object(json!({"id": "note", "type": "text"})),
            object(json!({"type": "person"})),
        ]);
        let mut values = object(json!({
            "owner": "u1", "team": ["u1", "u2"], "note": "u1", "other": "u1", "by": null,
        }));
        people.hide_values(&mut values);
        assert_eq!(
            Value::Object(values),
            json!({"owner": "", "team": ["", ""], "note": "u1", "other": "u1", "by": null}),
            "the count of people stays, who they are goes"
        );

        let mut filter = json!({"operation": "and", "filters": [
            {"propertyId": "owner", "condition": "includes", "values": ["u1"]},
            {"operation": "or", "filters": [
                {"propertyId": "by", "condition": "includes", "values": ["u1", "u2"]},
                {"propertyId": "note", "condition": "includes", "values": ["u1"]},
            ]},
        ]});
        people.hide_filter_values(&mut filter);
        assert_eq!(
            filter,
            json!({"operation": "and", "filters": [
                {"propertyId": "owner", "condition": "includes", "values": [""]},
                {"operation": "or", "filters": [
                    {"propertyId": "by", "condition": "includes", "values": ["", ""]},
                    {"propertyId": "note", "condition": "includes", "values": ["u1"]},
                ]},
            ]}),
            "in nested groups too"
        );
    }

    #[test]
    fn strings_lists_of_strings_and_null_are_property_values() {
        let values = object(json!({
            "text": "hello", "empty": "", "many": ["a", "b"], "none": [], "unset": null,
        }));
        assert_eq!(validate_card_property_values(&values), Ok(()), "{values:?}");
        assert_eq!(
            validate_card_property_values(&JsonMap::new()),
            Ok(()),
            "no values at all"
        );
    }

    #[test]
    fn an_object_is_a_date_value_or_refused_as_one() {
        let values = object(json!({"d": {"from": "2026-03-05"}, "t": {"from": 1}}));
        assert_eq!(validate_card_property_values(&values), Ok(()));
        let values = object(json!({"p1": {"nested": "x"}}));
        assert_eq!(
            validate_card_property_values(&values),
            Err(CardPropertyError::InvalidDateValue {
                id: "p1".to_owned(),
                reason: DateValueError::UnknownMember,
            })
        );
    }

    #[test]
    fn date_values_are_the_values_of_date_properties_only() {
        let definitions = [
            object(json!({"id": "due", "name": "Due", "type": "date"})),
            object(json!({"id": "note", "name": "Note", "type": "text"})),
        ];
        let accepted = object(json!({
            "due": {"from": "2026-03-05"}, "note": "x", "other": ["a"],
        }));
        assert_eq!(
            validate_card_property_kinds(&accepted, &definitions),
            Ok(())
        );
        let unset = object(json!({"due": null}));
        assert_eq!(validate_card_property_kinds(&unset, &definitions), Ok(()));
        for (values, error) in [
            (
                json!({"due": "{\"from\":1}"}),
                CardPropertyError::NotADateValue {
                    id: "due".to_owned(),
                },
            ),
            (
                json!({"due": ["2026-03-05"]}),
                CardPropertyError::NotADateValue {
                    id: "due".to_owned(),
                },
            ),
            (
                json!({"note": {"from": "2026-03-05"}}),
                CardPropertyError::DateValueOfOtherProperty {
                    id: "note".to_owned(),
                },
            ),
            (
                json!({"undefined": {"from": "2026-03-05"}}),
                CardPropertyError::DateValueOfOtherProperty {
                    id: "undefined".to_owned(),
                },
            ),
        ] {
            assert_eq!(
                validate_card_property_kinds(&object(values.clone()), &definitions),
                Err(error),
                "{values}"
            );
        }
    }

    #[test]
    fn other_shapes_are_no_property_values() {
        for value in [
            json!(3),
            json!(1.5),
            json!(true),
            json!(["a", 1]),
            json!([["a"]]),
            json!([null]),
        ] {
            let values = object(json!({"p1": value}));
            assert_eq!(
                validate_card_property_values(&values),
                Err(CardPropertyError::InvalidValue {
                    id: "p1".to_owned()
                }),
                "value {value}"
            );
        }
    }

    #[test]
    fn the_properties_of_a_block_are_an_object_of_values_or_absent() {
        assert_eq!(
            validate_block_properties(&object(json!({"icon": "x"}))),
            Ok(()),
            "no properties"
        );
        assert_eq!(
            validate_block_properties(&object(json!({"properties": {"p1": "v"}}))),
            Ok(()),
            "an object of values"
        );
        for properties in [json!("text"), json!(["a"]), json!(7), json!(null)] {
            assert_eq!(
                validate_block_properties(&object(json!({"properties": properties}))),
                Err(CardPropertyError::ValuesNotObject),
                "properties {properties}"
            );
        }
        assert_eq!(
            validate_block_properties(&object(json!({"properties": {"p1": 1}}))),
            Err(CardPropertyError::InvalidValue {
                id: "p1".to_owned()
            }),
            "an object with a malformed value"
        );
    }

    #[test]
    fn well_formed_definitions_pass() {
        let definitions = [
            object(json!({"id": "p1"})),
            object(json!({"id": "p2", "name": "Status", "type": "select", "options": []})),
            object(json!({"id": "p3", "options": null})),
            object(json!({
                "id": "p4", "name": "", "type": "multiSelect",
                "options": [{"id": "o1", "value": "Open", "color": "propColorRed"}, {}],
                "anythingElse": {"is": ["kept", 1]},
            })),
        ];
        assert_eq!(
            validate_card_property_definitions(&definitions),
            Ok(()),
            "{definitions:?}"
        );
        assert_eq!(validate_card_property_definitions(&[]), Ok(()), "none");
    }

    #[test]
    fn malformed_definitions_are_named_by_what_is_wrong() {
        let p1 = || "p1".to_owned();
        let cases = [
            (
                json!({"name": "x"}),
                CardPropertyError::MissingId { position: 1 },
            ),
            (
                json!({"id": ""}),
                CardPropertyError::MissingId { position: 1 },
            ),
            (
                json!({"id": 7}),
                CardPropertyError::MissingId { position: 1 },
            ),
            (
                json!({"id": null}),
                CardPropertyError::MissingId { position: 1 },
            ),
            (
                json!({"id": "p1", "name": 7}),
                CardPropertyError::MemberNotString {
                    id: p1(),
                    member: "name",
                },
            ),
            (
                json!({"id": "p1", "type": ["select"]}),
                CardPropertyError::MemberNotString {
                    id: p1(),
                    member: "type",
                },
            ),
            (
                json!({"id": "p1", "name": null}),
                CardPropertyError::MemberNotString {
                    id: p1(),
                    member: "name",
                },
            ),
            (
                json!({"id": "p1", "options": "none"}),
                CardPropertyError::InvalidOptions { id: p1() },
            ),
            (
                json!({"id": "p1", "options": ["a"]}),
                CardPropertyError::InvalidOptions { id: p1() },
            ),
            (
                json!({"id": "p1", "options": [{"id": "o1", "value": 1}]}),
                CardPropertyError::InvalidOptions { id: p1() },
            ),
            (
                json!({"id": "p1", "options": [{"color": false}]}),
                CardPropertyError::InvalidOptions { id: p1() },
            ),
        ];
        for (definition, expected) in cases {
            assert_eq!(
                validate_card_property_definitions(&[object(definition.clone())]),
                Err(expected),
                "definition {definition}"
            );
        }
    }

    #[test]
    fn one_malformed_definition_among_good_ones_fails_the_list() {
        let definitions = [
            object(json!({"id": "p1"})),
            object(json!({"name": "no id"})),
        ];
        assert_eq!(
            validate_card_property_definitions(&definitions),
            Err(CardPropertyError::MissingId { position: 2 }),
            "the second definition, named by its place"
        );
    }

    #[test]
    fn two_definitions_with_one_id_are_refused() {
        let definitions = [
            object(json!({"id": "p1", "name": "first"})),
            object(json!({"id": "p2"})),
            object(json!({"id": "p1", "name": "again"})),
        ];
        assert_eq!(
            validate_card_property_definitions(&definitions),
            Err(CardPropertyError::DuplicateId {
                id: "p1".to_owned()
            }),
            "a value could not say which of the two it belongs to"
        );
    }
}
