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
    /// OCI at the endpoint of `keys`, signing with the key its file holds, whose
    /// requests `client` makes.
    ///
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
        // What is signed is what is sent: the request target, the host and the length
        // are the ones reqwest writes.
        let headers = [
            ("date", date.as_str()),
            ("(request-target)", target.as_str()),
            ("host", self.host.as_str()),
            ("content-length", length.as_str()),
            ("content-type", "application/json"),
            ("x-content-sha256", hash.as_str()),
        ];
        let authorization = authorization(&self.key, &self.key_id, &headers);
        let request = headers
            .iter()
            .filter(|(name, _)| !matches!(*name, "(request-target)" | "host" | "content-length"))
            .fold(self.client.post(&self.url), |request, (name, value)| {
                request.header(*name, *value)
            })
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
/// attempt later. OCI answers what it does not let this server do with
/// `404 NotAuthorizedOrNotFound`: a sender it has not approved, but also a compartment
/// or a policy that is missing. The approval is the operator's to set up, and the
/// answer refuses the message as any other `404`, so each message is counted and its
/// guest link taken back while it lasts.
fn outcome_of(answer: &Answer) -> Outcome {
    let limited = answer.status == 400
        && serde_json::from_slice::<Value>(&answer.body)
            .is_ok_and(|body| body["code"] == "LimitExceeded");
    if limited {
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
