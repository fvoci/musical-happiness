//! What one organization may hold of the invitation mail: at most half of its queue,
//! and two of the three sendings invitations may use, so that another organization's
//! invitations are taken and go out whatever one organization queued.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;

/// How many messages one organization sends at once, of the invitations' sendings:
/// one is always left for the others.
pub(super) const SENDING_PER_ORG: usize = super::INVITATION_CONNECTIONS - 1;

/// The invitation messages each organization holds in the queue, kept in memory.
#[derive(Debug)]
pub(super) struct OrgShares {
    /// The most messages one organization holds: half of `mail_invitation_queue`.
    most: u32,
    held: Mutex<HashMap<Arc<str>, Held>>,
}

#[derive(Debug)]
struct Held {
    messages: u32,
    /// The organization's own sendings, while it holds messages.
    sending: Arc<Semaphore>,
}

/// One message's part of its organization's share, given back on drop.
#[derive(Debug)]
pub(super) struct OrgPlace {
    shares: Arc<OrgShares>,
    org: Arc<str>,
    /// The organization's sendings, which the message waits for before it is sent.
    pub(super) sending: Arc<Semaphore>,
}

impl OrgShares {
    /// Shares of which one organization holds at most `most` messages.
    pub(super) fn new(most: u32) -> Self {
        Self {
            most,
            held: Mutex::new(HashMap::new()),
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<Arc<str>, Held>> {
        self.held
            .lock()
            .expect("the organizations' mail shares are never held across a panic")
    }

    /// Takes the parts of `messages` for `org`, or `None` if they would pass its share.
    pub(super) fn take(self: &Arc<Self>, org: &str, messages: u32) -> Option<Vec<OrgPlace>> {
        let mut held = self.held();
        let org: Arc<str> = held
            .get_key_value(org)
            .map_or_else(|| Arc::from(org), |(key, _)| Arc::clone(key));
        let entry = held.entry(Arc::clone(&org)).or_insert_with(|| Held {
            messages: 0,
            sending: Arc::new(Semaphore::new(SENDING_PER_ORG)),
        });
        if entry.messages.saturating_add(messages) > self.most {
            if entry.messages == 0 {
                held.remove(&org);
            }
            return None;
        }
        entry.messages += messages;
        let sending = Arc::clone(&entry.sending);
        let places = (0..messages)
            .map(|_| OrgPlace {
                shares: Arc::clone(self),
                org: Arc::clone(&org),
                sending: Arc::clone(&sending),
            })
            .collect();
        Some(places)
    }
}

#[cfg(test)]
impl OrgShares {
    /// How many messages `org` holds.
    pub(super) fn messages_of(&self, org: &str) -> Option<u32> {
        self.held().get(org).map(|held| held.messages)
    }
}

impl Drop for OrgPlace {
    fn drop(&mut self) {
        // A drop does not panic: a poisoned lock leaves the count as it is.
        if let Ok(mut held) = self.shares.held.lock()
            && let Some(entry) = held.get_mut(&self.org)
        {
            entry.messages = entry.messages.saturating_sub(1);
            if entry.messages == 0 {
                held.remove(&self.org);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_organization_holds_its_share_and_another_its_own() {
        let shares = Arc::new(OrgShares::new(3));
        let first = shares.take("a", 2).expect("within the share");
        assert!(shares.take("a", 2).is_none(), "past the share of a");
        let other = shares.take("b", 3).expect("b has a share of its own");
        drop(first);
        let again = shares.take("a", 3).expect("given back as they leave");
        assert!(Arc::ptr_eq(&again[0].sending, &again[2].sending));
        assert!(!Arc::ptr_eq(&again[0].sending, &other[0].sending));
        drop((again, other));
        assert!(
            shares.held().is_empty(),
            "nothing is kept of who holds nothing"
        );
        assert!(shares.take("c", 4).is_none());
        assert!(shares.held().is_empty(), "nor of a refusal");
    }
}
