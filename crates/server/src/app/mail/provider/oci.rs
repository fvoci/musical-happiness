//! Sending through OCI Email Delivery's HTTPS submission (`SubmitEmail`), signed with
//! the user's API key.
//! <https://docs.oracle.com/en-us/iaas/api/#/en/emaildeliverysubmission/20220926/EmailSubmittedResponse/SubmitEmail>

use std::future::Future;
use std::pin::Pin;

use anyhow::Context as _;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::super::outbound::{Outbound, Outcome, Outgoing};
use super::clock::Moment;
use super::http_signature::{SigningKey, authorization};
use super::{Answer, host_of};
use crate::config::mail::OciKeys;

/// The path of `SubmitEmail`.
const PATH: &str = "/20220926/actions/submitEmail";

/// OCI's way of sending; constructing it reads the key's file and contacts nothing.
pub(in crate::app::mail) struct Oci {
    client: reqwest::Client,
    url: String,
    /// The `Host` header the requests are signed with.
    host: String,
    /// `tenancy/user/fingerprint`.
    key_id: String,
    key: SigningKey,
    compartment: String,
}

impl Oci {
    /// # Errors
    /// Returns an error naming `oci_private_key_file` if the key cannot be read or used,
    /// without its contents.
    pub(in crate::app::mail) fn new(
        keys: OciKeys,
        client: reqwest::Client,
    ) -> anyhow::Result<Self> {
        let pem = std::fs::read_to_string(&keys.private_key_file)
            .context("oci_private_key_file cannot be read")?;
        let key = SigningKey::from_pem(&pem).context("oci_private_key_file")?;
        let url = format!("{}{PATH}", keys.endpoint);
        Ok(Self {
            client,
            host: host_of(&reqwest::Url::parse(&url)?)?,
            url,
            key_id: format!("{}/{}/{}", keys.tenancy, keys.user, keys.fingerprint),
            key,
            compartment: keys.compartment,
        })
    }

    async fn deliver(&self, message: &Outgoing) -> Outcome {
        let body = json!({
            "sender": {
                "senderAddress": {"email": message.from},
                "compartmentId": self.compartment,
            },
            "recipients": {"to": [{"email": message.to}]},
            "subject": message.subject,
            "bodyText": message.plain,
            "bodyHtml": message.html,
        })
        .to_string();
        let date = Moment::now().http_date();
        let length = body.len().to_string();
        let hash = STANDARD.encode(Sha256::digest(body.as_bytes()));
        let target = format!("post {PATH}");
        let authorization = authorization(
            &self.key,
            &self.key_id,
            &[
                ("date", &date),
                ("(request-target)", &target),
                ("host", &self.host),
                ("content-length", &length),
                ("content-type", "application/json"),
                ("x-content-sha256", &hash),
            ],
        );
        let request = self
            .client
            .post(&self.url)
            .header("date", date)
            .header("content-type", "application/json")
            .header("x-content-sha256", hash)
            .header("authorization", authorization)
            .body(body);
        match super::send(request).await {
            Ok(answer) => outcome_of(&answer),
            Err(outcome) => outcome,
        }
    }
}

impl Outbound for Oci {
    fn name(&self) -> &'static str {
        "oci"
    }

    fn send<'a>(
        &'a self,
        message: &'a Outgoing,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + 'a>> {
        Box::pin(self.deliver(message))
    }
}

/// What OCI's answer makes of the attempt: by its status, except its `400` with the
/// code `LimitExceeded` (the messages a minute or a day), which is worth another
/// attempt later. A sender that is not approved (`404 NotAuthorizedOrNotFound`) is the
/// operator's to set up, and refuses the message as any other `404`.
fn outcome_of(answer: &Answer) -> Outcome {
    let limited = serde_json::from_slice::<Value>(&answer.body)
        .is_ok_and(|body| body["code"] == "LimitExceeded");
    if answer.status == 400 && limited {
        return Outcome::Later {
            after: answer.retry_after,
            code: Some(answer.status),
        };
    }
    super::outcome_of(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(status: u16, code: &str) -> Answer {
        Answer {
            status,
            retry_after: None,
            error_type: None,
            body: json!({"code": code, "message": "…"})
                .to_string()
                .into_bytes(),
        }
    }

    #[test]
    fn oci_errors_sort_by_status_and_its_limit_code() {
        assert_eq!(outcome_of(&answer(200, "")), Outcome::Sent);
        assert_eq!(
            outcome_of(&answer(400, "LimitExceeded")),
            Outcome::Later {
                after: None,
                code: Some(400)
            }
        );
        for (status, code) in [
            (400, "InvalidParameter"),
            (404, "NotAuthorizedOrNotFound"),
            (413, ""),
        ] {
            assert_eq!(
                outcome_of(&answer(status, code)),
                Outcome::Refused { code: Some(status) },
                "{code}"
            );
        }
        assert_eq!(
            outcome_of(&answer(401, "NotAuthenticated")),
            Outcome::PathDown { code: Some(401) }
        );
        assert_eq!(
            outcome_of(&answer(429, "TooManyRequests")),
            Outcome::Later {
                after: None,
                code: Some(429)
            }
        );
    }
}
