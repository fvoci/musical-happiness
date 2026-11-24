//! Webhooks: every inserted or changed block is posted, as JSON, to the addresses in the
//! `block_change_webhooks` setting.

use std::time::Duration;

use fvoci_model::Block;

/// How long one webhook request may take, so an unresponsive receiver cannot stall shutdown.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Posts block changes to the configured addresses.
#[derive(Debug, Clone)]
pub struct Webhook {
    urls: Vec<String>,
    /// `None` if there are no addresses: no client is needed then, and building one
    /// can fail (for example on a system without certificate authorities).
    http: Option<reqwest::Client>,
}

impl Webhook {
    /// Creates a client that notifies the given addresses.
    ///
    /// # Errors
    ///
    /// Returns an error if there are addresses but the HTTP client for them cannot be
    /// built, which means the TLS backend cannot be initialized.
    pub fn new(urls: Vec<String>) -> Result<Self, reqwest::Error> {
        let http = if urls.is_empty() {
            None
        } else {
            Some(
                reqwest::Client::builder()
                    .timeout(REQUEST_TIMEOUT)
                    .build()?,
            )
        };
        Ok(Self { urls, http })
    }

    /// Posts the block to every address, one after the other. A request that fails is
    /// logged and does not stop delivery to the remaining addresses.
    pub async fn notify_update(&self, block: &Block) {
        let Some(http) = &self.http else {
            return;
        };
        for url in &self.urls {
            // Webhook addresses often carry a secret in their path or query; the log
            // names the host only.
            let host = reqwest::Url::parse(url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default();
            match http.post(url).json(block).send().await {
                Ok(response) => {
                    // Read the answer to the end so the connection can be reused.
                    let _ = response.bytes().await;
                    tracing::debug!(host, "webhook.NotifyUpdate");
                }
                Err(error) => {
                    let error = error.without_url();
                    tracing::warn!(host, %error, "webhook.NotifyUpdate failed");
                }
            }
        }
    }
}
