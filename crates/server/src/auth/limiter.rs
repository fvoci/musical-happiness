//! Limits on login attempts.
//!
//! Three things are counted, all in memory:
//!
//! * **A pair of account and address.** After a few failures in a row, each further
//!   failure makes that address wait longer before it may try that account again. Only
//!   whoever fails waits: nobody locks another user out by guessing at their account.
//! * **An account.** Failures in a row from anywhere, up to a number after which
//!   nobody signs in to the account with a password for a while. This is what stops
//!   guessing from many addresses.
//! * **An address.** How many attempts it makes per minute, whatever the account, and
//!   one attempt at a time: this is what stops one password tried on many accounts,
//!   and one address from filling the queue of password hashes.
//!
//! An account is whatever name the attempt gives: the user name, the email address, or
//! both, each counted for itself. Whether such an account exists makes no difference to
//! what is counted or answered, and nothing here is asked after the account was looked
//! up: a limit that knew the account from the name would tell, by who has to wait,
//! which names are accounts. The price is that an account that is tried by its name and
//! by its email address has the failures of each for itself.
//!
//! The records are forgotten when the process ends, and the oldest go when there are
//! too many.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};

use crate::config::Configuration;
use crate::metrics::{LoginLimit, Metrics};

/// How many records of each kind are kept at most.
const MAX_RECORDS: usize = 100_000;

/// The window of the count of attempts per address.
const ADDRESS_WINDOW: Duration = Duration::from_secs(60);

/// How long an address that has an attempt in progress is told to wait before the next.
const BUSY_RETRY: Duration = Duration::from_secs(1);

/// The longest wait and the longest lock: what is configured as more is taken as this.
/// A time this far ahead can always be added to the clock.
pub const LONGEST_WAIT: Duration = Duration::from_hours(10 * 365 * 24);

/// The numbers of the limits; see the configuration keys of the same names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Failures in a row that a pair of account and address has for free.
    pub pair_free_failures: u32,
    /// The wait after the first failure that is not free; it doubles with each further.
    pub pair_base_delay: Duration,
    /// The longest wait of a pair.
    pub pair_max_delay: Duration,
    /// Failures in a row, from anywhere, after which an account is locked.
    pub account_max_failures: u32,
    /// How long an account stays locked.
    pub account_lock: Duration,
    /// Attempts an address may make per minute; `None` for no limit by address.
    pub address_attempts_per_minute: Option<u32>,
}

impl Limits {
    /// The limits of a configuration.
    pub fn from_config(config: &Configuration) -> Self {
        Self {
            pair_free_failures: config.login_pair_free_failures,
            pair_base_delay: Duration::from_secs(config.login_pair_base_delay_seconds),
            pair_max_delay: Duration::from_secs(config.login_pair_max_delay_seconds)
                .min(LONGEST_WAIT),
            account_max_failures: config.login_account_max_failures,
            account_lock: Duration::from_secs(config.login_account_lock_seconds).min(LONGEST_WAIT),
            address_attempts_per_minute: (config.login_address_attempts_per_minute > 0)
                .then_some(config.login_address_attempts_per_minute),
        }
    }
}

/// An attempt was refused before it was looked at: try again after this long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAfter(pub Duration);

/// An account, by the name an attempt gave for it, in any letter case. Kept as a digest:
/// the name is of the attempt's choosing and may be long.
type AccountKey = [u8; 32];

/// An address as it is counted: an IPv4 address, or the /64 an IPv6 address is in,
/// which is what one subscriber usually has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum AddressKey {
    V4([u8; 4]),
    V6([u8; 8]),
}

impl From<IpAddr> for AddressKey {
    fn from(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V4(address) => Self::V4(address.octets()),
            IpAddr::V6(address) => {
                let octets = address.octets();
                let mut network = [0; 8];
                network.copy_from_slice(&octets[..8]);
                Self::V6(network)
            }
        }
    }
}

fn account_key(name: &str) -> AccountKey {
    Sha256::digest(name.trim().to_lowercase().as_bytes()).into()
}

#[derive(Debug, Clone, Copy)]
struct PairRecord {
    failures: u32,
    blocked_until: Instant,
    last_failure: Instant,
}

#[derive(Debug, Clone, Copy)]
struct AccountRecord {
    failures: u32,
    locked_until: Option<Instant>,
    last_failure: Instant,
}

#[derive(Debug, Default)]
struct AddressRecord {
    /// When the attempts of the last minute were made.
    attempts: VecDeque<Instant>,
    /// Whether an attempt of this address is in progress.
    busy: bool,
}

#[derive(Debug, Default)]
struct Records {
    pairs: HashMap<(AccountKey, AddressKey), PairRecord>,
    accounts: HashMap<AccountKey, AccountRecord>,
    addresses: HashMap<AddressKey, AddressRecord>,
}

/// Counts login attempts and says which may go ahead.
#[derive(Debug, Clone)]
pub struct LoginLimiter {
    limits: Limits,
    records: Arc<Mutex<Records>>,
    /// Where the refusals are counted, by the limit that refused.
    metrics: Metrics,
}

/// An attempt that may go ahead. Whoever holds it says how it ended; an attempt that is
/// dropped without that counts as neither a failure nor a success, and frees its
/// address for the next attempt either way.
#[derive(Debug)]
pub struct Attempt {
    limiter: LoginLimiter,
    /// The names the attempt gave for the account; none for a registration.
    accounts: Vec<AccountKey>,
    address: Option<AddressKey>,
}

impl LoginLimiter {
    /// A limiter with these limits and no records. A wait or a lock longer than
    /// [`LONGEST_WAIT`] is taken as that.
    pub fn new(mut limits: Limits) -> Self {
        limits.pair_max_delay = limits.pair_max_delay.min(LONGEST_WAIT);
        limits.account_lock = limits.account_lock.min(LONGEST_WAIT);
        Self {
            limits,
            records: Arc::default(),
            metrics: Metrics::none(),
        }
    }

    /// This limiter, counting its refusals in `metrics`.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Metrics) -> Self {
        self.metrics = metrics;
        self
    }

    fn records(&self) -> std::sync::MutexGuard<'_, Records> {
        self.records
            .lock()
            .expect("the login records are never held across a panic")
    }

    /// Asks whether an attempt to sign in from `address` may go ahead at `now`, and
    /// counts it against the address if it may. `names` are the names the attempt gives
    /// for the account, as it gives them: its user name, its email address, or both.
    /// This is asked before the account is looked up, so that the answer cannot depend
    /// on whether there is one.
    ///
    /// # Errors
    ///
    /// Returns how long to wait if the address has made too many attempts or has one in
    /// progress, if the address is made to wait at one of the names, or if one of the
    /// names is locked. Of several waits it is the longest.
    pub fn admit_login(
        &self,
        names: &[&str],
        address: Option<IpAddr>,
        now: Instant,
    ) -> Result<Attempt, RetryAfter> {
        let mut accounts: Vec<AccountKey> = names.iter().map(|name| account_key(name)).collect();
        accounts.sort_unstable();
        accounts.dedup();
        let address = address.map(AddressKey::from);
        let mut records = self.records();

        let waits = accounts.iter().flat_map(|account| {
            let locked = records
                .accounts
                .get(account)
                .and_then(|record| record.locked_until)
                .map(|until| (until, LoginLimit::AccountLock));
            let blocked = address
                .and_then(|address| records.pairs.get(&(*account, address)))
                .map(|record| (record.blocked_until, LoginLimit::PairDelay));
            [locked, blocked]
        });
        // The longest wait is the one answered, and the limit counted; of two as long,
        // the lock of the account.
        if let Some((until, limit)) = waits
            .flatten()
            .filter(|(until, _)| *until > now)
            .max_by_key(|(until, limit)| (*until, *limit == LoginLimit::AccountLock))
        {
            self.metrics.login_refused(limit);
            return Err(RetryAfter(until - now));
        }
        self.count_against_address(&mut records, address, now)?;
        Ok(Attempt {
            limiter: self.clone(),
            accounts,
            address,
        })
    }

    /// Asks whether a registration from `address` may go ahead at `now`: a registration
    /// costs a password hash like a login, and is counted against its address like one.
    ///
    /// # Errors
    ///
    /// Returns how long to wait if the address has made too many attempts or has one in
    /// progress.
    pub fn admit_registration(
        &self,
        address: Option<IpAddr>,
        now: Instant,
    ) -> Result<Attempt, RetryAfter> {
        self.admit_from_address(address, now)
    }

    /// Asks whether a request from `address` that names no account to lock may go ahead
    /// at `now`, and counts it against the address: it is counted on admission, so the
    /// attempt needs no ending but being dropped.
    ///
    /// # Errors
    ///
    /// Returns how long to wait if the address has made too many attempts or has one in
    /// progress.
    pub fn admit_from_address(
        &self,
        address: Option<IpAddr>,
        now: Instant,
    ) -> Result<Attempt, RetryAfter> {
        let address = address.map(AddressKey::from);
        self.count_against_address(&mut self.records(), address, now)?;
        Ok(Attempt {
            limiter: self.clone(),
            accounts: Vec::new(),
            address,
        })
    }

    /// Ends what was counted at `names`, from every address: the lock of each name and
    /// the waits of every address at it. For the owner who proved the account another
    /// way (a reset link from its mailbox) and chose the only password that opens it
    /// now: the guesses counted so far were at the old one.
    pub fn forget_account(&self, names: &[&str]) {
        let accounts: Vec<AccountKey> = names.iter().map(|name| account_key(name)).collect();
        let mut records = self.records();
        for account in &accounts {
            records.accounts.remove(account);
        }
        records
            .pairs
            .retain(|(account, _), _| !accounts.contains(account));
    }

    /// Counts an attempt against its address and marks the address as busy.
    fn count_against_address(
        &self,
        records: &mut Records,
        address: Option<AddressKey>,
        now: Instant,
    ) -> Result<(), RetryAfter> {
        let (Some(address), Some(per_minute)) = (address, self.limits.address_attempts_per_minute)
        else {
            return Ok(());
        };
        if !records.addresses.contains_key(&address) {
            make_room(
                &mut records.addresses,
                |record| {
                    !record.busy
                        && record
                            .attempts
                            .back()
                            .is_none_or(|last| now.duration_since(*last) >= ADDRESS_WINDOW)
                },
                // An address with an attempt in progress is as young as can be.
                |record| match record.attempts.back() {
                    Some(last) if !record.busy => *last,
                    _ => now,
                },
            );
        }
        let record = records.addresses.entry(address).or_default();
        while record
            .attempts
            .front()
            .is_some_and(|oldest| now.duration_since(*oldest) >= ADDRESS_WINDOW)
        {
            record.attempts.pop_front();
        }
        if record.busy {
            self.metrics.login_refused(LoginLimit::AddressBusy);
            return Err(RetryAfter(BUSY_RETRY));
        }
        let allowed = usize::try_from(per_minute).unwrap_or(usize::MAX);
        if record.attempts.len() >= allowed {
            // Room again when the oldest attempt of the window is a minute old.
            let oldest = record.attempts.front().copied().unwrap_or(now);
            self.metrics.login_refused(LoginLimit::AddressRate);
            return Err(RetryAfter(
                ADDRESS_WINDOW.saturating_sub(now.duration_since(oldest)),
            ));
        }
        record.attempts.push_back(now);
        record.busy = true;
        Ok(())
    }
}

impl Attempt {
    /// The attempt failed: the password was wrong, or there is no such account. It
    /// counts at every name the attempt gave.
    pub fn failed(self, now: Instant) {
        let limits = self.limiter.limits;
        let mut records = self.limiter.records();
        for account in &self.accounts {
            if let Some(address) = self.address {
                count_pair_failure(&mut records.pairs, (*account, address), limits, now);
            }
            count_account_failure(&mut records.accounts, *account, limits, now);
        }
    }

    /// The attempt succeeded with `name` as the name of the account: the failures in a
    /// row at that name end here, and so do those of this address at it. What other
    /// addresses have to wait for at that name stays: the owner signing in is no reason
    /// to let a guesser go on. So does what was counted at another name the attempt
    /// gave: a right password for one account says nothing about that one.
    pub fn succeeded(self, name: &str) {
        let account = account_key(name);
        let mut records = self.limiter.records();
        records.accounts.remove(&account);
        if let Some(address) = self.address {
            records.pairs.remove(&(account, address));
        }
    }
}

/// Counts one more failure of an address at a name, and makes the address wait once the
/// free ones are used up.
fn count_pair_failure(
    pairs: &mut HashMap<(AccountKey, AddressKey), PairRecord>,
    pair: (AccountKey, AddressKey),
    limits: Limits,
    now: Instant,
) {
    if !pairs.contains_key(&pair) {
        make_room(
            pairs,
            |record| {
                record.blocked_until <= now
                    && now.duration_since(record.last_failure) >= limits.pair_max_delay
            },
            |record| record.last_failure,
        );
    }
    let record = pairs.entry(pair).or_insert(PairRecord {
        failures: 0,
        blocked_until: now,
        last_failure: now,
    });
    record.failures = record.failures.saturating_add(1);
    record.last_failure = now;
    if record.failures > limits.pair_free_failures {
        let doublings = record.failures - limits.pair_free_failures - 1;
        let delay = limits
            .pair_base_delay
            .checked_mul(2_u32.saturating_pow(doublings))
            .unwrap_or(limits.pair_max_delay)
            .min(limits.pair_max_delay);
        // No more than `LONGEST_WAIT` ahead: see `LoginLimiter::new`.
        record.blocked_until = now + delay;
    }
}

/// Counts one more failure at a name from anywhere, and locks the name at the number
/// that is too many.
fn count_account_failure(
    accounts: &mut HashMap<AccountKey, AccountRecord>,
    account: AccountKey,
    limits: Limits,
    now: Instant,
) {
    if !accounts.contains_key(&account) {
        make_room(
            accounts,
            |record| {
                record.locked_until.is_none_or(|until| until <= now)
                    && now.duration_since(record.last_failure) >= limits.account_lock
            },
            |record| record.last_failure,
        );
    }
    let record = accounts.entry(account).or_insert(AccountRecord {
        failures: 0,
        locked_until: None,
        last_failure: now,
    });
    record.failures = record.failures.saturating_add(1);
    record.last_failure = now;
    if record.failures >= limits.account_max_failures {
        // No more than `LONGEST_WAIT` ahead: see `LoginLimiter::new`.
        record.locked_until = Some(now + limits.account_lock);
        // The count starts over when the lock ends.
        record.failures = 0;
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if let Some(address) = self.address
            && let Ok(mut records) = self.limiter.records.lock()
            && let Some(record) = records.addresses.get_mut(&address)
        {
            record.busy = false;
        }
    }
}

/// Makes room for one more record where there are [`MAX_RECORDS`]: the records that say
/// nothing any more go, and if all still say something, the tenth of them that was
/// touched longest ago, by `last_touched`.
fn make_room<K: std::hash::Hash + Eq + Copy, V>(
    records: &mut HashMap<K, V>,
    says_nothing: impl Fn(&V) -> bool,
    last_touched: impl Fn(&V) -> Instant,
) {
    if records.len() < MAX_RECORDS {
        return;
    }
    records.retain(|_, record| !says_nothing(record));
    if records.len() >= MAX_RECORDS {
        for key in oldest(records, MAX_RECORDS / 10, last_touched) {
            records.remove(&key);
        }
    }
}

/// The keys of the `count` records that were touched longest ago.
fn oldest<K: Copy, V>(
    records: &HashMap<K, V>,
    count: usize,
    last_touched: impl Fn(&V) -> Instant,
) -> Vec<K> {
    let mut by_age: Vec<(Instant, K)> = records
        .iter()
        .map(|(key, record)| (last_touched(record), *key))
        .collect();
    if count < by_age.len() {
        by_age.select_nth_unstable_by_key(count, |(touched, _)| *touched);
        by_age.truncate(count);
    }
    by_age.into_iter().map(|(_, key)| key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_is_counted_under_its_limit() {
        let metrics = Metrics::kept().expect("the metrics register");
        let limiter = LoginLimiter::new(Limits {
            pair_free_failures: 1,
            account_max_failures: 3,
            address_attempts_per_minute: Some(5),
            ..LIMITS
        })
        .with_metrics(metrics.clone());
        let now = Instant::now();
        let count = |limit| metrics.login_refusals(limit);

        // The second failure of a pair makes it wait.
        fail(&limiter, "alice", "203.0.113.7", now);
        fail(&limiter, "alice", "203.0.113.7", now);
        assert!(wait_of(&limiter, "alice", "203.0.113.7", now).is_some());
        assert_eq!(count(LoginLimit::PairDelay), 1);

        // A third failure, from elsewhere, locks the account for every address.
        fail(&limiter, "alice", "198.51.100.1", now);
        assert!(wait_of(&limiter, "alice", "192.0.2.1", now).is_some());
        assert_eq!(count(LoginLimit::AccountLock), 1);

        // An attempt in progress holds its address up.
        let attempt = limiter
            .admit_login(&["bob"], Some(address("192.0.2.9")), now)
            .expect("admitted");
        assert!(wait_of(&limiter, "carol", "192.0.2.9", now).is_some());
        assert_eq!(count(LoginLimit::AddressBusy), 1);
        attempt.failed(now);

        // Five attempts a minute from an address, then no more.
        for name in ["d1", "d2", "d3", "d4"] {
            fail(&limiter, name, "192.0.2.9", now);
        }
        assert!(wait_of(&limiter, "d5", "192.0.2.9", now).is_some());
        assert_eq!(count(LoginLimit::AddressRate), 1);
        assert_eq!(
            count(LoginLimit::PairDelay),
            1,
            "nothing else counted twice"
        );
        assert_eq!(count(LoginLimit::AccountLock), 1);
    }

    const LIMITS: Limits = Limits {
        pair_free_failures: 5,
        pair_base_delay: Duration::from_secs(30),
        pair_max_delay: Duration::from_secs(3600),
        account_max_failures: 100,
        account_lock: Duration::from_secs(3600),
        address_attempts_per_minute: None,
    };

    fn address(text: &str) -> IpAddr {
        text.parse().expect("an address")
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    /// One failed attempt; panics if it is not admitted.
    #[track_caller]
    fn fail(limiter: &LoginLimiter, account: &str, from: &str, now: Instant) {
        limiter
            .admit_login(&[account], Some(address(from)), now)
            .expect("the attempt is admitted")
            .failed(now);
    }

    #[track_caller]
    fn wait_of(limiter: &LoginLimiter, account: &str, from: &str, now: Instant) -> Option<u64> {
        limiter
            .admit_login(&[account], Some(address(from)), now)
            .err()
            .map(|RetryAfter(wait)| wait.as_secs())
    }

    #[test]
    fn a_pair_waits_longer_with_each_failure_past_the_free_ones() {
        let limiter = LoginLimiter::new(LIMITS);
        let mut now = Instant::now();
        for _ in 0..5 {
            fail(&limiter, "alice", "203.0.113.7", now);
        }
        assert_eq!(
            wait_of(&limiter, "alice", "203.0.113.7", now),
            None,
            "five are free"
        );

        // Each further failure: 30 s, 1 min, 2 min, ... up to an hour.
        for expected in [30, 60, 120, 240, 480, 960, 1920, 3600, 3600] {
            fail(&limiter, "alice", "203.0.113.7", now);
            assert_eq!(
                wait_of(&limiter, "alice", "203.0.113.7", now),
                Some(expected),
                "the wait after this failure"
            );
            assert_eq!(
                wait_of(&limiter, "alice", "203.0.113.7", now + secs(expected - 1)),
                Some(1),
                "still a second to go"
            );
            now += secs(expected);
            assert_eq!(wait_of(&limiter, "alice", "203.0.113.7", now), None, "over");
        }
    }

    #[test]
    fn only_who_fails_waits() {
        let limiter = LoginLimiter::new(LIMITS);
        let now = Instant::now();
        for _ in 0..6 {
            fail(&limiter, "alice", "203.0.113.7", now);
        }
        assert_eq!(wait_of(&limiter, "alice", "203.0.113.7", now), Some(30));
        assert_eq!(
            wait_of(&limiter, "alice", "198.51.100.1", now),
            None,
            "alice signs in from her own address"
        );
        assert_eq!(
            wait_of(&limiter, "bob", "203.0.113.7", now),
            None,
            "another account from the same address"
        );
        assert_eq!(
            wait_of(&limiter, "  ALICE ", "203.0.113.7", now),
            Some(30),
            "the name in other letters is the same account"
        );
    }

    #[test]
    fn a_success_ends_the_failures_of_the_pair_and_of_the_account_only() {
        let limiter = LoginLimiter::new(LIMITS);
        let now = Instant::now();
        for _ in 0..6 {
            fail(&limiter, "alice", "203.0.113.7", now);
        }
        for _ in 0..4 {
            fail(&limiter, "alice", "198.51.100.1", now);
        }
        limiter
            .admit_login(&["alice"], Some(address("198.51.100.1")), now)
            .expect("alice is admitted from her address")
            .succeeded("alice");

        assert_eq!(
            wait_of(&limiter, "alice", "203.0.113.7", now),
            Some(30),
            "the guesser still waits"
        );
        for _ in 0..5 {
            fail(&limiter, "alice", "198.51.100.1", now);
        }
        assert_eq!(
            wait_of(&limiter, "alice", "198.51.100.1", now),
            None,
            "her own failures started over: five are free again"
        );
    }

    #[test]
    fn an_account_is_locked_after_too_many_failures_from_anywhere() {
        let limiter = LoginLimiter::new(Limits {
            account_max_failures: 10,
            ..LIMITS
        });
        let now = Instant::now();
        // Ten addresses, one failure each: no pair has anything to wait for.
        for index in 0..9 {
            fail(&limiter, "alice", &format!("203.0.113.{index}"), now);
        }
        assert_eq!(
            wait_of(&limiter, "alice", "198.51.100.1", now),
            None,
            "nine"
        );
        fail(&limiter, "alice", "203.0.113.200", now);

        assert_eq!(
            wait_of(&limiter, "alice", "198.51.100.1", now),
            Some(3600),
            "locked for everybody, the owner's address too"
        );
        assert_eq!(
            wait_of(&limiter, "bob", "203.0.113.1", now),
            None,
            "not bob"
        );
        let later = now + secs(3600);
        assert_eq!(
            wait_of(&limiter, "alice", "198.51.100.1", later),
            None,
            "over"
        );
        // And the count started over: nine more are possible again.
        for index in 0..9 {
            fail(&limiter, "alice", &format!("203.0.113.{index}"), later);
        }
        assert_eq!(wait_of(&limiter, "alice", "198.51.100.1", later), None);
    }

    #[test]
    fn an_address_makes_so_many_attempts_a_minute_and_one_at_a_time() {
        let limiter = LoginLimiter::new(Limits {
            address_attempts_per_minute: Some(3),
            ..LIMITS
        });
        let start = Instant::now();
        let from = Some(address("203.0.113.7"));

        let first = limiter
            .admit_login(&["a"], from, start)
            .expect("the first attempt");
        assert_eq!(
            limiter.admit_login(&["b"], from, start).err(),
            Some(RetryAfter(BUSY_RETRY)),
            "one at a time"
        );
        assert!(
            limiter
                .admit_registration(Some(address("198.51.100.1")), start)
                .is_ok(),
            "another address is not held up"
        );
        drop(first);
        // The refused attempt was not counted; successes are.
        limiter
            .admit_login(&["b"], from, start + secs(10))
            .expect("the second attempt")
            .succeeded("b");
        drop(
            limiter
                .admit_registration(from, start + secs(20))
                .expect("a registration counts as the third"),
        );
        assert_eq!(
            limiter.admit_login(&["c"], from, start + secs(30)).err(),
            Some(RetryAfter(secs(30))),
            "three a minute: room when the first is a minute old"
        );
        assert!(limiter.admit_login(&["c"], from, start + secs(60)).is_ok());
    }

    #[test]
    fn ipv6_addresses_of_one_network_are_one_address() {
        let limiter = LoginLimiter::new(Limits {
            address_attempts_per_minute: Some(1),
            ..LIMITS
        });
        let now = Instant::now();
        drop(limiter.admit_registration(Some(address("2001:db8:1:2::1")), now));
        assert!(
            limiter
                .admit_registration(Some(address("2001:db8:1:2:ffff::9")), now)
                .is_err(),
            "the same /64"
        );
        assert!(
            limiter
                .admit_registration(Some(address("2001:db8:1:3::1")), now)
                .is_ok(),
            "another /64"
        );
        // A pair is counted by the network too.
        let limiter = LoginLimiter::new(LIMITS);
        for host in 1..=6 {
            fail(&limiter, "alice", &format!("2001:db8:1:2::{host}"), now);
        }
        assert_eq!(
            wait_of(&limiter, "alice", "2001:db8:1:2::99", now),
            Some(30)
        );
    }

    #[test]
    fn an_attempt_without_an_address_is_counted_by_its_account_only() {
        let limiter = LoginLimiter::new(Limits {
            account_max_failures: 3,
            address_attempts_per_minute: Some(1),
            ..LIMITS
        });
        let now = Instant::now();
        for _ in 0..3 {
            limiter
                .admit_login(&["alice"], None, now)
                .expect("no address, no limit by address")
                .failed(now);
        }
        assert!(
            limiter.admit_login(&["alice"], None, now).is_err(),
            "locked"
        );
    }

    #[test]
    fn room_is_made_by_dropping_what_says_nothing_first_and_then_the_oldest() {
        let start = Instant::now();
        let count = u32::try_from(MAX_RECORDS).expect("fits");
        // The value: whether the record says nothing, and when it was touched.
        let touched = |key: u32| start + Duration::from_millis(u64::from(key));
        let mut records: HashMap<u32, (bool, Instant)> = (0..count)
            .map(|key| (key, (key % 2 == 0, touched(key))))
            .collect();
        make_room(&mut records, |record| record.0, |record| record.1);
        assert_eq!(records.len(), MAX_RECORDS / 2, "half said nothing");
        assert!(records.values().all(|record| !record.0));

        // The records were touched in the order of their keys, the highest key last.
        let mut full: HashMap<u32, (bool, Instant)> = (0..count)
            .map(|key| (key, (false, touched(count - key))))
            .collect();
        make_room(&mut full, |record| record.0, |record| record.1);
        let tenth = u32::try_from(MAX_RECORDS / 10).expect("fits");
        assert_eq!(
            full.len(),
            MAX_RECORDS - MAX_RECORDS / 10,
            "all say something: a tenth goes"
        );
        assert!(
            (count - tenth..count).all(|key| !full.contains_key(&key)),
            "the tenth that was touched longest ago is the one that went"
        );
        assert!(full.contains_key(&0), "the youngest stays");
    }

    #[test]
    fn what_is_counted_does_not_depend_on_whether_there_is_an_account() {
        // The names are counted as the attempt gave them, each for itself: a second
        // spelling of a name is the same name, and an email address is another.
        let limiter = LoginLimiter::new(LIMITS);
        let now = Instant::now();
        let from = Some(address("203.0.113.7"));
        for _ in 0..6 {
            limiter
                .admit_login(&["Alice ", "alice@example.com"], from, now)
                .expect("the attempt is admitted")
                .failed(now);
        }
        assert_eq!(
            wait_of(&limiter, "alice", "203.0.113.7", now),
            Some(30),
            "the name, however it is spelled"
        );
        assert_eq!(
            wait_of(&limiter, "ALICE@example.com", "203.0.113.7", now),
            Some(30),
            "the email address the same attempts gave"
        );
        assert_eq!(
            wait_of(&limiter, "bob", "203.0.113.7", now),
            None,
            "another name"
        );
        assert!(
            limiter
                .admit_login(&["bob", "alice@example.com"], from, now)
                .is_err(),
            "one name that waits is enough"
        );
    }

    #[test]
    fn a_success_ends_the_failures_at_the_name_that_signed_in_only() {
        let limiter = LoginLimiter::new(LIMITS);
        let now = Instant::now();
        let from = Some(address("203.0.113.7"));
        for _ in 0..6 {
            fail(&limiter, "victim@example.com", "203.0.113.7", now);
        }
        for _ in 0..3 {
            fail(&limiter, "mallory", "203.0.113.7", now);
        }
        // Mallory signs in to her own account and names the victim's address beside it.
        let later = now + secs(31);
        limiter
            .admit_login(&["mallory", "victim@example.com"], from, later)
            .expect("the wait is over")
            .succeeded("mallory");
        for _ in 0..5 {
            fail(&limiter, "mallory", "203.0.113.7", later);
        }
        assert_eq!(
            wait_of(&limiter, "mallory", "203.0.113.7", later),
            None,
            "her own failures in a row ended with the success"
        );
        fail(&limiter, "victim@example.com", "203.0.113.7", later);
        assert_eq!(
            wait_of(&limiter, "victim@example.com", "203.0.113.7", later),
            Some(60),
            "those at the other name went on: the seventh doubles the wait"
        );
    }

    #[test]
    fn a_wait_that_cannot_be_added_to_the_clock_is_the_longest_wait() {
        let limiter = LoginLimiter::new(Limits {
            pair_free_failures: 0,
            pair_base_delay: Duration::MAX,
            pair_max_delay: Duration::MAX,
            account_max_failures: 1,
            account_lock: Duration::MAX,
            ..LIMITS
        });
        let now = Instant::now();
        fail(&limiter, "alice", "203.0.113.7", now);
        assert_eq!(
            wait_of(&limiter, "alice", "203.0.113.7", now),
            Some(LONGEST_WAIT.as_secs()),
            "no panic, and the longest wait there is"
        );
    }

    #[test]
    fn forgetting_an_account_ends_its_lock_and_its_waits_at_those_names_only() {
        let limiter = LoginLimiter::new(Limits {
            pair_free_failures: 2,
            account_max_failures: 10,
            ..LIMITS
        });
        let now = Instant::now();
        for _ in 0..3 {
            fail(&limiter, "alice", "203.0.113.200", now);
            fail(&limiter, "bob", "203.0.113.200", now);
        }
        for last in 1..=7 {
            fail(&limiter, "alice", &format!("203.0.113.{last}"), now);
        }
        assert!(
            wait_of(&limiter, "alice", "198.51.100.1", now).is_some(),
            "locked"
        );
        limiter.forget_account(&["Alice", "alice@example.com"]);
        assert_eq!(wait_of(&limiter, "alice", "198.51.100.1", now), None);
        assert_eq!(wait_of(&limiter, "alice", "203.0.113.200", now), None);
        assert!(
            wait_of(&limiter, "bob", "203.0.113.200", now).is_some(),
            "another account keeps what was counted"
        );
    }
}
