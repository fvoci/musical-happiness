//! Optional SMTP delivery settings; passwords are never exposed in diagnostics.

use anyhow::bail;
use serde::Deserialize;
use serde_json::{Map, Value};

pub(super) const ENV_KEYS: [(&str, super::EnvKind); 6] = [
    ("smtp_host", super::EnvKind::Text),
    ("smtp_port", super::EnvKind::Number),
    ("smtp_tls", super::EnvKind::Text),
    ("smtp_user", super::EnvKind::Text),
    ("smtp_password", super::EnvKind::Text),
    ("smtp_from", super::EnvKind::Text),
];

/// The SMTP settings, flattened into the main configuration file.
#[derive(Clone, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default)]
pub struct SmtpSettings {
    /// SMTP host name or IP address, without a scheme or port; empty disables mail.
    pub smtp_host: String,
    /// TCP port of the SMTP server.
    pub smtp_port: u16,
    /// Required TLS mode: `starttls`, `tls`, or explicitly unencrypted `none`.
    pub smtp_tls: String,
    /// SMTP authentication user; empty for no authentication.
    pub smtp_user: String,
    /// SMTP authentication password; omitted from debug output.
    pub smtp_password: String,
    /// Sender email address, without a display name.
    pub smtp_from: String,
}

impl Default for SmtpSettings {
    fn default() -> Self {
        Self {
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_tls: "starttls".to_owned(),
            smtp_user: String::new(),
            smtp_password: String::new(),
            smtp_from: String::new(),
        }
    }
}

impl std::fmt::Debug for SmtpSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpSettings")
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_tls", &self.smtp_tls)
            .field("smtp_from", &self.smtp_from)
            .finish_non_exhaustive()
    }
}

pub(super) fn check_password_is_text(keys: &Map<String, Value>) -> anyhow::Result<()> {
    match keys.get("smtp_password") {
        None | Some(Value::String(_)) => Ok(()),
        Some(_) => bail!("smtp_password must be a string"),
    }
}

impl SmtpSettings {
    /// Whether this server has an outgoing SMTP transport configured.
    pub fn enabled(&self) -> bool {
        !self.smtp_host.is_empty()
    }

    /// Validates the transport settings without contacting the mail server.
    ///
    /// # Errors
    /// Returns an error naming invalid or incomplete settings, without credentials.
    pub fn check(&self) -> anyhow::Result<()> {
        if !["starttls", "tls", "none"].contains(&self.smtp_tls.as_str()) {
            bail!("smtp_tls must be starttls, tls or none");
        }
        if self.smtp_port == 0 {
            bail!("smtp_port must be 1 to 65535");
        }
        if self.smtp_user.is_empty() != self.smtp_password.is_empty() {
            bail!("smtp_user and smtp_password must be supplied together");
        }
        if !self.enabled() {
            if !self.smtp_from.is_empty() || !self.smtp_user.is_empty() {
                bail!("smtp_host is required when mail settings are supplied");
            }
            return Ok(());
        }
        if self
            .smtp_host
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '/' | '@' | '#' | '?'))
        {
            bail!("smtp_host must be a host name or IP address");
        }
        if !fvoci_model::is_email_valid(&self.smtp_from) {
            bail!("smtp_from must be a single email address");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Configuration;

    #[test]
    fn mail_is_optional_and_incomplete_or_injected_settings_are_refused() {
        assert!(!SmtpSettings::default().enabled());
        SmtpSettings::default().check().expect("disabled mail");
        for (key, value) in [
            ("smtp_tls", "optional"),
            ("smtp_from", "sender@example.test\r\nBcc:other@example.test"),
            ("smtp_host", "smtp.example.test\nEHLO evil"),
        ] {
            let mut json = serde_json::json!({"smtp_host":"smtp.example.test","smtp_from":"sender@example.test"});
            json[key] = Value::String(value.to_owned());
            assert!(
                Configuration::parse(&json.to_string(), |_| None)
                    .expect("parse")
                    .check()
                    .is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn smtp_credentials_never_appear_in_debug_or_type_errors() {
        let text = r#"{"smtp_password":1234567890}"#;
        let error = Configuration::parse(text, |_| None)
            .expect_err("wrong type")
            .to_string();
        assert!(!error.contains("1234567890"), "{error}");
        let settings = SmtpSettings {
            smtp_user: "private-user".to_owned(),
            smtp_password: "private-password".to_owned(),
            ..SmtpSettings::default()
        };
        let debug = format!("{settings:?}");
        assert!(!debug.contains("private-"), "{debug}");
    }
}
