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
}

impl Fake {
    fn new(script: &[Outcome]) -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            script: Mutex::new(script.iter().copied().collect()),
            attempts: Mutex::new(Vec::new()),
        })
    }

    fn attempts(&self) -> Vec<(u64, String)> {
        self.attempts.lock().expect("attempts").clone()
    }
}

impl Outbound for Fake {
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
        Box::pin(std::future::ready(outcome.unwrap_or(Outcome::Sent)))
    }
}

const LATER: Outcome = Outcome::Later {
    after: None,
    code: Some(451),
};

fn mailer_of(fake: &Arc<Fake>, metrics: Metrics) -> Mailer {
    let settings = SmtpSettings {
        smtp_host: "smtp.example.test".to_owned(),
        smtp_from: "sender@example.test".to_owned(),
        ..SmtpSettings::default()
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
                Ok::<_, ()>(Some(Letter {
                    to,
                    content: Content::reset("https://example.test/reset", Language::English),
                }))
            })
            .await;
        assert_eq!(written, Ok(()));
    })
}

fn reset(mailer: &Mailer, account: &str) -> Queued {
    mailer.queue_reset(account).expect("a place")
}

fn invitation(mailer: &Mailer) -> Queued {
    mailer
        .queue_invitations("workspace", 1)
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
        [0, 30, 90],
        "as long as the way asks, up to a minute"
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
