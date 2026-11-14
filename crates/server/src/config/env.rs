//! Reading the value of an environment variable as the kind of its key.

use anyhow::{Context as _, bail};
use serde_json::Value;

use super::EnvKind;

/// The value the environment's `text` for the variable `name` gives a key of `kind`.
///
/// # Errors
///
/// Returns an error naming the variable if the text is no value of that kind.
pub(super) fn env_value(name: &str, text: &str, kind: EnvKind) -> anyhow::Result<Value> {
    Ok(match kind {
        EnvKind::Text => Value::from(text),
        EnvKind::Number => {
            let number: i64 = text
                .trim()
                .parse()
                .with_context(|| format!("{name}={text:?} is not a whole number"))?;
            Value::from(number)
        }
        EnvKind::Flag => match text.trim().to_lowercase().as_str() {
            "true" | "1" | "t" | "yes" | "on" => Value::Bool(true),
            "false" | "0" | "f" | "no" | "off" => Value::Bool(false),
            _ => bail!("{name}={text:?} is not true or false"),
        },
        EnvKind::List => Value::from(
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .collect::<Vec<&str>>(),
        ),
    })
}
