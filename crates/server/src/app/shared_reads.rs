//! The limit on reading shared boards with tokens that open nothing.
//!
//! A read token is unguessable, but a client that asks for many boards with many tokens
//! still tries. Each answer that a token opens nothing counts against the address of the
//! client, and an address with [`MAX_SHARED_READ_FAILURES`] of them within [`SHARED_READ_WINDOW`] is refused
//! (429) until the window that has them is over. Reads with a token that opens the board
//! count nothing, so a reader with a good link is never held back by its own reads.
//!
//! The counts are kept in memory and forgotten when the process ends; the oldest go
//! when there are too many.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::AppError;

/// How many reads that open nothing an address may have within [`SHARED_READ_WINDOW`].
pub const MAX_SHARED_READ_FAILURES: u32 = 30;

/// The window the failures of an address are counted in.
pub const SHARED_READ_WINDOW: Duration = Duration::from_secs(60);

/// How many addresses are kept at most.
const MAX_ADDRESSES: usize = 100_000;

/// The failures of one address within its current window.
#[derive(Debug, Clone, Copy)]
struct Count {
    /// When the window began: at the first failure after the last window ended.
    since: Instant,
    /// The failures since then.
    failures: u32,
}

/// The counts of the addresses, and the order they were first counted in.
#[derive(Debug, Default)]
struct Counts {
    by_address: HashMap<IpAddr, Count>,
    order: VecDeque<IpAddr>,
}

/// The limit on reads of shared boards that open nothing, by the client's address.
#[derive(Debug, Default)]
pub struct SharedReads {
    counts: Mutex<Counts>,
}

impl SharedReads {
    /// Lets a read in, or refuses it with 429 if its address has had too many reads that
    /// opened nothing within the window. A request without an address (the
    /// administration socket) is not limited.
    ///
    /// # Errors
    ///
    /// Returns the 429 of an address over the limit, with the wait until its window ends.
    pub fn admit(&self, address: Option<IpAddr>) -> Result<(), AppError> {
        self.admit_at(address, Instant::now())
    }

    fn admit_at(&self, address: Option<IpAddr>, now: Instant) -> Result<(), AppError> {
        let Some(address) = address else {
            return Ok(());
        };
        let counts = self.counts.lock().expect("the counts of shared reads");
        match counts.by_address.get(&address) {
            Some(count) if count.failures >= MAX_SHARED_READ_FAILURES => {
                let ends = count.since + SHARED_READ_WINDOW;
                if ends > now {
                    Err(AppError::rate_limited(ends - now))
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Counts a read of the address that opened nothing.
    pub fn failed(&self, address: Option<IpAddr>) {
        self.failed_at(address, Instant::now());
    }

    fn failed_at(&self, address: Option<IpAddr>, now: Instant) {
        let Some(address) = address else {
            return;
        };
        let mut counts = self.counts.lock().expect("the counts of shared reads");
        let Counts { by_address, order } = &mut *counts;
        if let Some(count) = by_address.get_mut(&address) {
            if now.duration_since(count.since) >= SHARED_READ_WINDOW {
                *count = Count {
                    since: now,
                    failures: 0,
                };
            }
            count.failures = count.failures.saturating_add(1);
            return;
        }
        if by_address.len() >= MAX_ADDRESSES
            && let Some(oldest) = order.pop_front()
        {
            by_address.remove(&oldest);
        }
        by_address.insert(
            address,
            Count {
                since: now,
                failures: 1,
            },
        );
        order.push_back(address);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7)));
    const OTHER: Option<IpAddr> = Some(IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 8)));

    #[test]
    fn an_address_is_refused_after_its_failures_until_the_window_ends() {
        let limit = SharedReads::default();
        let start = Instant::now();
        for _ in 0..MAX_SHARED_READ_FAILURES {
            assert!(limit.admit_at(ADDRESS, start).is_ok(), "within the limit");
            limit.failed_at(ADDRESS, start);
        }
        let refused = limit
            .admit_at(ADDRESS, start + Duration::from_secs(1))
            .expect_err("over the limit");
        assert_eq!(refused.status(), 429, "too many");
        assert!(limit.admit_at(OTHER, start).is_ok(), "another address");
        assert!(limit.admit_at(None, start).is_ok(), "no address");
        assert!(
            limit.admit_at(ADDRESS, start + SHARED_READ_WINDOW).is_ok(),
            "the window is over"
        );
        limit.failed_at(ADDRESS, start + SHARED_READ_WINDOW);
        assert!(
            limit.admit_at(ADDRESS, start + SHARED_READ_WINDOW).is_ok(),
            "a new window counts anew"
        );
    }

    #[test]
    fn the_oldest_address_goes_when_there_are_too_many() {
        let limit = SharedReads::default();
        let now = Instant::now();
        for _ in 0..MAX_SHARED_READ_FAILURES {
            limit.failed_at(ADDRESS, now);
        }
        assert!(limit.admit_at(ADDRESS, now).is_err(), "limited");
        for n in 0..MAX_ADDRESSES {
            let n = u32::try_from(n).expect("a small number");
            limit.failed_at(Some(IpAddr::V4(std::net::Ipv4Addr::from(n))), now);
        }
        assert!(limit.admit_at(ADDRESS, now).is_ok(), "forgotten");
    }
}
