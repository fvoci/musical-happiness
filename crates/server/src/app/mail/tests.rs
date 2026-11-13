//! The queue in front of a fake way of sending: what it does with each outcome, the
//! pace of all mail, and recovery passing while invitations wait.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use tokio::time::Instant;

use super::*;

/// A way of sending that answers from a script, and the last answer once it runs out.
struct Fake {
    start: Instant,
    script: Mutex<VecDeque<Outcome>>,
    /// Each attempt: when, in whole seconds from the start, and to whom.
    attempts: Mutex<Vec<(u64, String)>>,
    /// While true, an attempt to a recipient at `held.test` is answered only once it
    /// is false again.
    held: tokio::sync::watch::Sender<bool>,
}

impl Fake {
    fn new(script: &[Outcome]) -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            script: Mutex::new(script.iter().copied().collect()),
            attempts: Mutex::new(Vec::new()),
            held: tokio::sync::watch::Sender::new(false),
        })
    }

    fn attempts(&self) -> Vec<(u64, String)> {
        self.attempts.lock().expect("attempts").clone()
    }
}

impl Outbound for Fake {
    fn name(&self) -> &'static str {
        "fake"
    }

    fn send<'a>(
        &'a self,
        message: &'a Outgoing,
    ) -> Pin<Box<dyn Future<Output = Outcome> + Send + 'a>> {
        self.attempts
            .lock()
            .expect("attempts")
            .push((self.start.elapsed().as_secs(), message.to.clone()));
        let mut script = self.script.lock().expect("script");
        let outcome = if script.len() > 1 {
            script.pop_front()
        } else {
            script.front().copied()
        };
        let outcome = outcome.unwrap_or(Outcome::Sent);
        let mut held = (message.to.ends_with("@held.test")).then(|| self.held.subscribe());
        Box::pin(async move {
            if let Some(held) = held.as_mut() {
                // The sender lives as long as the fake; it is never dropped first.
                let _ = held.wait_for(|held| !*held).await;
            }
            outcome
        })
    }
}

const LATER: Outcome = Outcome::Later {
    after: None,
    code: Some(451),
};

fn mailer_of(fake: &Arc<Fake>, metrics: Metrics) -> Mailer {
    mailer_with(fake, metrics, SmtpSettings::default())
}

fn mailer_with(fake: &Arc<Fake>, metrics: Metrics, settings: SmtpSettings) -> Mailer {
    let settings = SmtpSettings {
        smtp_host: "smtp.example.test".to_owned(),
        smtp_from: "sender@example.test".to_owned(),
        ..settings
    };
    let outbound: Arc<dyn Outbound> = Arc::<Fake>::clone(fake);
    Mailer::with_outbound(&settings, Some(outbound), metrics).expect("mailer")
}

/// Delivers one message to `to` in a task of its own.
fn send(mailer: &Mailer, queued: Queued, to: &str) -> tokio::task::JoinHandle<()> {
    let mailer = mailer.clone();
    let to = to.to_owned();
    tokio::spawn(async move {
        let written = queued
            .deliver(&mailer, || async {
                Ok::<_, ()>(Letter {
                    to,
                    content: Content::reset("https://example.test/reset", Language::English),
                })
            })
            .await;
        assert!(written.is_ok());
    })
}

fn reset(mailer: &Mailer, account: &str) -> Queued {
    mailer.queue_reset(account).expect("a place")
}

fn invitation(mailer: &Mailer) -> Queued {
    mailer
        .queue_invitations("workspace", "org", 300, 1)
        .expect("a place")
        .pop()
        .expect("one")
}

/// The count of `fvoci_mail_failed_total` of `kind` and `class`.
fn failed(metrics: &Metrics, kind: MailKind, class: MailFailure) -> u64 {
    let text = String::from_utf8(metrics.render().expect("kept").expect("rendered")).expect("text");
    let kind = format!("kind=\"{}\"", kind.as_str());
    let class = format!("class=\"{}\"", class.as_str());
    text.lines()
        .find(|line| {
            line.starts_with("fvoci_mail_failed_total{")
                && line.contains(&kind)
                && line.contains(&class)
        })
        .and_then(|line| line.rsplit(' ').next())
        .and_then(|value| value.parse().ok())
        .expect("a counter of each kind and class")
}

#[tokio::test(start_paused = true)]
async fn a_deferred_message_is_tried_again_and_a_refused_one_is_not() {
    let metrics = Metrics::kept().expect("metrics");
    let fake = Fake::new(&[LATER, Outcome::Sent]);
    let mailer = mailer_of(&fake, metrics.clone());
    send(&mailer, reset(&mailer, "a"), "a@example.test")
        .await
        .expect("sent");
    assert_eq!(
        fake.attempts(),
        [
            (0, "a@example.test".to_owned()),
            (1, "a@example.test".to_owned())
        ]
    );

    let fake = Fake::new(&[Outcome::Refused { code: Some(550) }]);
    let mailer = mailer_of(&fake, metrics.clone());
    send(&mailer, reset(&mailer, "b"), "b@example.test")
        .await
        .expect("given up");
    assert_eq!(fake.attempts().len(), 1, "a refusal is final");
    assert_eq!(
        failed(&metrics, MailKind::PasswordReset, MailFailure::Permanent),
        1
    );
    assert_eq!(
        failed(&metrics, MailKind::PasswordReset, MailFailure::Temporary),
        0
    );
}

#[tokio::test(start_paused = true)]
async fn a_message_deferred_at_every_attempt_is_given_up_after_the_third() {
    let metrics = Metrics::kept().expect("metrics");
    let fake = Fake::new(&[LATER]);
    let mailer = mailer_of(&fake, metrics.clone());
    send(&mailer, invitation(&mailer), "a@example.test")
        .await
        .expect("given up");
    let times: Vec<u64> = fake.attempts().into_iter().map(|(at, _)| at).collect();
    assert_eq!(times, [0, 1, 5], "after one and four seconds");
    assert_eq!(
        failed(&metrics, MailKind::Invitation, MailFailure::Temporary),
        1
    );

    let fake = Fake::new(&[
        Outcome::Later {
            after: Some(Duration::from_secs(30)),
            code: Some(421),
        },
        Outcome::Later {
            after: Some(Duration::from_hours(5)),
            code: Some(421),
        },
        Outcome::Sent,
    ]);
    let mailer = mailer_of(&fake, Metrics::none());
    send(&mailer, reset(&mailer, "a"), "a@example.test")
        .await
        .expect("sent");
    let times: Vec<u64> = fake.attempts().into_iter().map(|(at, _)| at).collect();
    assert_eq!(
        times,
        [0, 30],
        "as long as the way asks; given up when it asks for more than a minute"
    );
}

#[tokio::test(start_paused = true)]
async fn all_mail_keeps_the_pace_of_mail_per_second() {
    let fake = Fake::new(&[Outcome::Sent]);
    let mailer = mailer_of(&fake, Metrics::none());
    let sending: Vec<_> = (0..3)
        .map(|n| send(&mailer, reset(&mailer, &format!("{n}")), "a@example.test"))
        .collect();
    for task in sending {
        task.await.expect("sent");
    }
    let times: Vec<u64> = fake.attempts().into_iter().map(|(at, _)| at).collect();
    assert_eq!(times, [0, 1, 2], "the default is one a second");
}

#[tokio::test(start_paused = true)]
async fn while_the_way_keeps_deferring_invitations_wait_and_recovery_is_tried() {
    let fake = Fake::new(&[LATER]);
    let mailer = mailer_of(&fake, Metrics::none());
    send(&mailer, invitation(&mailer), "first@example.test")
        .await
        .expect("given up");
    // Three deferrals in a row: invitations wait for a minute from the third.
    let waiting = send(&mailer, invitation(&mailer), "second@example.test");
    let recovery = send(&mailer, reset(&mailer, "a"), "owner@example.test");
    recovery.await.expect("tried and given up");
    waiting.await.expect("tried and given up");
    let attempts = fake.attempts();
    let of = |to: &str| -> Vec<u64> {
        attempts
            .iter()
            .filter(|(_, recipient)| recipient == to)
            .map(|(at, _)| *at)
            .collect()
    };
    assert_eq!(of("owner@example.test"), [6, 7, 11], "{attempts:?}");
    let second = of("second@example.test");
    assert!(
        second[0] >= 11 + 60,
        "the waiting invitation goes after the pause: {attempts:?}"
    );
}

#[test]
fn a_root_on_this_machine_only_is_told_apart() {
    for root in [
        "http://localhost:8000",
        "http://127.0.0.1/boards",
        "http://127.0.0.2",
        "https://[::1]:8443",
        "http://LOCALHOST",
        "http://boards.localhost",
        "http://0.0.0.0:8000",
        "http://[::]:8000",
        "http://[::ffff:127.0.0.1]:8000",
        "http://127.1",
        "http://localhost.:8000",
        "HTTP://LocalHost",
    ] {
        assert!(is_loopback_root(root), "{root}");
    }
    for root in [
        "https://boards.example.test",
        "http://10.0.0.5:8000",
        "https://[2001:db8::1]",
        "http://127.0.0.1.example.test",
        "not a url",
    ] {
        assert!(!is_loopback_root(root), "{root}");
    }
}

#[tokio::test(start_paused = true)]
async fn recovery_keeps_the_pace_while_invitations_wait_and_another_backs_off() {
    let fake = Fake::new(&[
        LATER,
        LATER,
        LATER,
        Outcome::Later {
            after: Some(Duration::from_secs(30)),
            code: Some(421),
        },
        Outcome::Sent,
    ]);
    let mailer = mailer_of(&fake, Metrics::none());
    send(&mailer, invitation(&mailer), "first@example.test")
        .await
        .expect("given up");
    // Invitations wait now; more of them than they may send at once.
    let waiting: Vec<_> = (0..4)
        .map(|n| send(&mailer, invitation(&mailer), &format!("i{n}@example.test")))
        .collect();
    let backing_off = send(&mailer, reset(&mailer, "a"), "a@example.test");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let next = send(&mailer, reset(&mailer, "b"), "b@example.test");
    for task in waiting.into_iter().chain([backing_off, next]) {
        task.await.expect("sent");
    }
    let attempts = fake.attempts();
    let of = |to: &str| -> Vec<u64> {
        attempts
            .iter()
            .filter(|(_, recipient)| recipient == to)
            .map(|(at, _)| *at)
            .collect()
    };
    assert_eq!(of("a@example.test"), [6, 36], "{attempts:?}");
    assert_eq!(
        of("b@example.test"),
        [7],
        "the next recovery message has the next turn while another waits: {attempts:?}"
    );
    assert!(
        of("i0@example.test")[0] < 36,
        "a message sent lets the invitations go: {attempts:?}"
    );
}

#[test]
fn an_organization_holds_half_of_the_invitation_queue_within_its_own_hour() {
    let fake = Fake::new(&[Outcome::Sent]);
    let mailer = mailer_with(
        &fake,
        Metrics::none(),
        SmtpSettings {
            mail_invitation_queue: 6,
            ..SmtpSettings::default()
        },
    );
    let held = mailer
        .queue_invitations("w1", "a", 300, 3)
        .expect("half of the queue");
    assert_eq!(held.len(), 3);
    assert_eq!(
        mailer.queue_invitations("w2", "a", 300, 1).err(),
        Some(InvitationRefusal::OrgShareFull),
        "another workspace of the same organization counts in its share"
    );
    let other = mailer
        .queue_invitations("w3", "b", 300, 3)
        .expect("another organization has its own half");
    assert_eq!(
        mailer.queue_invitations("w4", "c", 1, 1).err(),
        Some(InvitationRefusal::QueueFull)
    );
    assert_eq!(
        mailer.org_shares.messages_of("c"),
        None,
        "the share taken before the queue refused is given back"
    );
    drop(other);
    mailer
        .queue_invitations("w4", "c", 1, 1)
        .expect("the refusal took nothing of c's hour");
    drop(held);
    assert!(matches!(
        mailer.queue_invitations("w5", "d", 2, 3).err(),
        Some(InvitationRefusal::OrgHourlyLimit(_))
    ));
    mailer
        .queue_invitations("w5", "d", 3, 3)
        .expect("the organization's own limit");
    assert!(
        matches!(
            mailer.queue_invitations("w6", "d", 3, 1).err(),
            Some(InvitationRefusal::OrgHourlyLimit(_))
        ),
        "the hour counts every workspace of the organization"
    );
    assert!(
        mailer.queue_invitations("w6", "e", 300, 1).is_ok(),
        "nothing of the refusals was kept"
    );
}

#[tokio::test(start_paused = true)]
async fn invitations_never_take_the_last_sending_and_one_organization_takes_two() {
    let fake = Fake::new(&[Outcome::Sent]);
    fake.held.send_replace(true);
    let mailer = mailer_with(
        &fake,
        Metrics::none(),
        SmtpSettings {
            mail_per_second: 100,
            ..SmtpSettings::default()
        },
    );
    let mut sending = Vec::new();
    for (org, count) in [("a", 3), ("b", 1), ("c", 1)] {
        for (n, queued) in mailer
            .queue_invitations(org, org, 300, count)
            .expect("places")
            .into_iter()
            .enumerate()
        {
            sending.push(send(&mailer, queued, &format!("{org}{n}@held.test")));
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let attempts = fake.attempts();
    assert_eq!(attempts.len(), 3, "three of four sendings: {attempts:?}");
    let of_a = attempts
        .iter()
        .filter(|(_, to)| to.starts_with('a'))
        .count();
    assert_eq!(of_a, 2, "two of them one organization's: {attempts:?}");
    sending.push(send(&mailer, reset(&mailer, "owner"), "owner@example.test"));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        fake.attempts()
            .iter()
            .any(|(_, to)| to == "owner@example.test"),
        "recovery has the fourth"
    );
    fake.held.send_replace(false);
    for task in sending {
        task.await.expect("sent");
    }
    assert_eq!(fake.attempts().len(), 6);
}

#[tokio::test(start_paused = true)]
async fn an_invitation_waiting_for_a_sending_takes_no_turn_it_could_use_in_a_pause() {
    let fake = Fake::new(&[LATER]);
    fake.held.send_replace(true);
    let mailer = mailer_of(&fake, Metrics::none());
    let mut sending = Vec::new();
    let mut waiting = Vec::new();
    for org in ["a", "b", "c"] {
        let mut queued = mailer
            .queue_invitations(org, org, 300, 2)
            .expect("places")
            .into_iter();
        let first = queued.next().expect("one");
        sending.push(send(&mailer, first, &format!("{org}0@held.test")));
        waiting.push((org, queued.next().expect("two")));
    }
    for (org, queued) in waiting {
        sending.push(send(&mailer, queued, &format!("{org}1@example.test")));
    }
    tokio::time::sleep(Duration::from_secs(6)).await;
    for _ in 0..3 {
        mailer.pace.observe(LATER);
    }
    fake.held.send_replace(false);
    for task in sending {
        task.await.expect("given up");
    }
    let attempts = fake.attempts();
    let early: Vec<_> = attempts
        .iter()
        .filter(|(at, to)| to.ends_with("1@example.test") && *at < 66)
        .collect();
    assert!(
        early.is_empty(),
        "the invitations that waited for a sending went after the pause: {attempts:?}"
    );
}
