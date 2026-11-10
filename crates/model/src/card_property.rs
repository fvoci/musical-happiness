//! The shapes that card property definitions and card property values must have.
//!
//! A board defines the properties of its cards (`cardProperties`), and every card holds a
//! value per property (`fields.properties` of its block). Both are free-form JSON, and
//! clients render them as they are: a value is text or a list of texts, and a definition
//! names itself and its options with strings. Anything else is refused before it is
//! stored, so that one member cannot break the board for the others.

use serde_json::Value;
use thiserror::Error;

use crate::JsonMap;

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
    /// A property value is neither a string, nor a list of strings, nor `null`.
    #[error("the value of card property {id:?} must be a string or a list of strings")]
    InvalidValue {
        /// The identifier of the property.
        id: String,
    },
    /// A property definition has no `id`, or one that is not a non-empty string.
    #[error("a card property definition needs a non-empty string as id")]
    MissingId,
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

/// Checks the property values of a card: each is a string, a list of strings, or `null`.
///
/// # Errors
///
/// Returns [`CardPropertyError::InvalidValue`] for the first value of another shape.
pub fn validate_card_property_values(values: &JsonMap) -> Result<(), CardPropertyError> {
    for (id, value) in values {
        let valid = match value {
            Value::Null | Value::String(_) => true,
            Value::Array(items) => items.iter().all(Value::is_string),
            Value::Bool(_) | Value::Number(_) | Value::Object(_) => false,
        };
        if !valid {
            return Err(CardPropertyError::InvalidValue { id: id.clone() });
        }
    }
    Ok(())
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
/// Returns the [`CardPropertyError`] of the first definition that is malformed.
pub fn validate_card_property_definitions(
    definitions: &[JsonMap],
) -> Result<(), CardPropertyError> {
    definitions.iter().try_for_each(validate_definition)
}

fn validate_definition(definition: &JsonMap) -> Result<(), CardPropertyError> {
    let id = match definition.get("id") {
        Some(Value::String(id)) if !id.is_empty() => id,
        _ => return Err(CardPropertyError::MissingId),
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
        Ok(())
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
    fn other_shapes_are_no_property_values() {
        for value in [
            json!(3),
            json!(1.5),
            json!(true),
            json!({"nested": "x"}),
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
            (json!({"name": "x"}), CardPropertyError::MissingId),
            (json!({"id": ""}), CardPropertyError::MissingId),
            (json!({"id": 7}), CardPropertyError::MissingId),
            (json!({"id": null}), CardPropertyError::MissingId),
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
            Err(CardPropertyError::MissingId),
            "the second definition"
        );
    }
}
