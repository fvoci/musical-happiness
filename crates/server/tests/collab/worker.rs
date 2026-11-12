//! The worker process as the server holds it: what it answers, and what becomes of it
//! when it dies, takes too long or is given up half way.

use std::time::{Duration, Instant};

use fvoci_collab::testing::Editor;
use fvoci_server::collab::worker::{CallError, Death, Worker, WorkerCommand, WorkerSettings};
use fvoci_server::collab_worker::wire::{Answer, Limits, Refusal, Request, Source};
use serde_json::{Value, json};

const STATE_BYTES: u64 = 8 * 1024 * 1024;

fn limits(documents: u64) -> Limits {
    Limits {
        update_bytes: 64 * 1024,
        state_bytes: STATE_BYTES,
        body_bytes: 1024 * 1024,
        documents,
        held_bytes: 128 * 1024 * 1024,
    }
}

fn worker(timeout: Duration, documents: u64) -> Worker {
    Worker::new(WorkerSettings {
        command: WorkerCommand::of_executable(env!("CARGO_BIN_EXE_fvoci-server").into()),
        limits: limits(documents),
        timeout,
        memory_bytes: 1024 * 1024 * 1024,
    })
}

fn roomy() -> Worker {
    worker(Duration::from_secs(10), 100)
}

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

fn open_body(doc: u64, body: &Value) -> Request {
    Request::Open {
        doc,
        source: Source::Body(Some(serde_json::to_vec(body).expect("JSON"))),
    }
}

/// The bytes of an answer that carries them.
fn bytes(answer: Answer) -> Vec<u8> {
    match answer {
        Answer::Bytes(bytes) => bytes,
        other => panic!("no bytes: {other:?}"),
    }
}

async fn body_of(worker: &Worker, generation: u64, doc: u64) -> Value {
    let (_, answer) = worker
        .call(Some(generation), &Request::Body(doc))
        .await
        .expect("a body");
    let Answer::Body(Some(json)) = answer else {
        panic!("a body: {answer:?}");
    };
    serde_json::from_slice(&json).expect("JSON")
}

fn refusal(result: Result<(u64, Answer), CallError>) -> Refusal {
    match result {
        Err(CallError::Refused(refused)) => refused.refusal,
        other => panic!("a refusal: {other:?}"),
    }
}

#[tokio::test]
async fn two_editors_that_write_at_once_converge_through_the_worker() {
    let worker = roomy();
    let (generation, opened) = worker
        .call(None, &open_body(1, &paragraphs(&["first"])))
        .await
        .expect("opened");
    let Answer::Opened {
        state: Some(state), ..
    } = opened
    else {
        panic!("a document made from a body has its state: {opened:?}");
    };
    let one = Editor::join(101, &state);
    let other = Editor::join(102, &state);
    // Each writes before it has the other's update.
    let from_one = one.append_paragraph("one");
    let from_other = other.append_paragraph("other");
    for update in [&from_one, &from_other] {
        let (_, taken) = worker
            .call(
                Some(generation),
                &Request::Take {
                    doc: 1,
                    update: update.clone(),
                },
            )
            .await
            .expect("taken");
        assert!(
            matches!(taken, Answer::Taken { repair: None, .. }),
            "{taken:?}"
        );
    }
    let merged = body_of(&worker, generation, 1).await;
    assert_eq!(
        merged["content"].as_array().map(Vec::len),
        Some(3),
        "{merged}"
    );
    let (_, own) = worker
        .call(Some(generation), &Request::StateVector(1))
        .await
        .expect("a state vector");
    assert_ne!(
        bytes(own),
        Vec::<u8>::new(),
        "a state vector of the document"
    );
    for editor in [&one, &other] {
        let (_, diff) = worker
            .call(
                Some(generation),
                &Request::Diff {
                    doc: 1,
                    state_vector: editor.state_vector(),
                },
            )
            .await
            .expect("what the editor has not");
        editor.receive(&bytes(diff));
        let body = editor.body().expect("a body");
        assert_eq!(
            body.document(),
            &merged,
            "the editor has what the worker has"
        );
    }
    worker.end().await;
}

/// What a worker that dies leaves: it is reaped, the documents it held are gone, and
/// the next request starts another worker.
#[cfg(unix)]
#[tokio::test]
async fn a_worker_that_is_killed_is_started_again_without_its_documents() {
    let worker = roomy();
    let (generation, _) = worker
        .call(None, &open_body(1, &paragraphs(&["kept"])))
        .await
        .expect("opened");
    let first = worker.pid().await.expect("a worker runs");
    signal(first, "KILL");
    // The worker's task notices the death while nobody asks anything, and reaps it.
    let began = Instant::now();
    while is_alive(first) {
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "no zombie is left of it"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let lost = worker.call(Some(generation), &Request::State(1)).await;
    assert_eq!(lost, Err(CallError::Lost), "its documents are gone");
    let (again, _) = worker
        .call(None, &open_body(1, &paragraphs(&["kept"])))
        .await
        .expect("a document opens in the next worker");
    assert_ne!(again, generation, "a new generation");
    let second = worker.pid().await.expect("a worker runs");
    assert_ne!(second, first, "another process");
    assert_eq!(body_of(&worker, again, 1).await, paragraphs(&["kept"]));
    worker.end().await;
}

#[tokio::test]
async fn a_worker_that_takes_too_long_is_killed_and_the_request_fails() {
    let worker = worker(Duration::from_millis(300), 10);
    let (generation, _) = worker
        .call(None, &Request::Stall(0))
        .await
        .expect("a stall of nothing");
    let first = worker.pid().await.expect("a worker runs");
    let began = Instant::now();
    let stalled = worker.call(None, &Request::Stall(30_000)).await;
    assert_eq!(stalled, Err(CallError::Died(Death::TimedOut)));
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "{:?}",
        began.elapsed()
    );
    #[cfg(unix)]
    assert!(!is_alive(first), "killed, and no zombie is left");
    let (next, _) = worker
        .call(None, &Request::Stall(0))
        .await
        .expect("the next worker answers");
    assert_ne!(next, generation);
    assert_ne!(worker.pid().await, Some(first));
    worker.end().await;
}

/// A caller that gives up waiting leaves the worker to finish the request: the next
/// request is answered by the same worker, with its own answer.
#[tokio::test]
async fn a_call_given_up_half_way_neither_kills_the_worker_nor_puts_the_answers_out_of_step() {
    let worker = roomy();
    let (generation, _) = worker
        .call(None, &open_body(1, &paragraphs(&["before"])))
        .await
        .expect("opened");
    let first = worker.pid().await.expect("a worker runs");
    let given_up = tokio::time::timeout(
        Duration::from_millis(100),
        worker.call(None, &Request::Stall(500)),
    )
    .await;
    assert!(given_up.is_err(), "given up");
    assert_eq!(
        body_of(&worker, generation, 1).await,
        paragraphs(&["before"]),
        "the same worker, and the answer to this request"
    );
    assert_eq!(worker.pid().await, Some(first));
    worker.end().await;
}

/// A request larger than a frame is refused before it is sent, and the worker lives.
#[tokio::test]
async fn a_request_larger_than_a_frame_is_refused_without_harm_to_the_worker() {
    let worker = roomy();
    let (generation, _) = worker
        .call(None, &open_body(1, &paragraphs(&["kept"])))
        .await
        .expect("opened");
    let first = worker.pid().await;
    let past_a_frame = usize::try_from(STATE_BYTES).expect("small") + 2048;
    let huge = worker
        .call(
            Some(generation),
            &Request::Take {
                doc: 1,
                update: vec![0; past_a_frame],
            },
        )
        .await;
    assert_eq!(refusal(huge), Refusal::UpdateTooLarge);
    let stored = worker
        .call(
            None,
            &Request::Open {
                doc: 2,
                source: Source::State(vec![0; past_a_frame]),
            },
        )
        .await;
    assert_eq!(refusal(stored), Refusal::DocumentTooLarge);
    assert_eq!(worker.pid().await, first, "the same worker");
    assert_eq!(body_of(&worker, generation, 1).await, paragraphs(&["kept"]));
    worker.end().await;
}

/// A worker holds the states of its documents within the bytes it is given for them.
#[tokio::test]
async fn a_worker_holds_no_more_bytes_of_documents_than_it_may() {
    let texts: Vec<String> = (0..100)
        .map(|line| format!("line {line} of a body"))
        .collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    let body = paragraphs(&texts);
    let probe = roomy();
    let (_, opened) = probe
        .call(None, &open_body(1, &body))
        .await
        .expect("opened");
    probe.end().await;
    let Answer::Opened { state_bytes, .. } = opened else {
        panic!("{opened:?}");
    };
    let worker = Worker::new(WorkerSettings {
        command: WorkerCommand::of_executable(env!("CARGO_BIN_EXE_fvoci-server").into()),
        limits: Limits {
            held_bytes: state_bytes * 2,
            ..limits(100)
        },
        timeout: Duration::from_secs(10),
        memory_bytes: 1024 * 1024 * 1024,
    });
    for doc in 1..=2 {
        worker
            .call(None, &open_body(doc, &body))
            .await
            .expect("within");
    }
    let third = worker.call(None, &open_body(3, &body)).await;
    assert_eq!(refusal(third), Refusal::TooManyDocuments, "past the bytes");
    worker.call(None, &Request::Close(1)).await.expect("closed");
    worker
        .call(None, &open_body(3, &body))
        .await
        .expect("room again once one is closed");
    worker.end().await;
}

#[tokio::test]
async fn what_is_past_the_limits_is_refused() {
    let worker = worker(Duration::from_secs(10), 1);
    let (generation, _) = worker
        .call(
            None,
            &Request::Open {
                doc: 1,
                source: Source::Body(None),
            },
        )
        .await
        .expect("opened");
    let second = worker
        .call(
            None,
            &Request::Open {
                doc: 2,
                source: Source::Body(None),
            },
        )
        .await;
    assert_eq!(refusal(second), Refusal::TooManyDocuments);
    let large = worker
        .call(
            Some(generation),
            &Request::Take {
                doc: 1,
                update: vec![0; 64 * 1024 + 1],
            },
        )
        .await;
    assert_eq!(refusal(large), Refusal::UpdateTooLarge);
    let garbage = worker
        .call(
            Some(generation),
            &Request::Take {
                doc: 1,
                update: vec![0xff; 16],
            },
        )
        .await;
    assert_eq!(refusal(garbage), Refusal::Unreadable);
    let vector = worker
        .call(
            Some(generation),
            &Request::Diff {
                doc: 1,
                state_vector: vec![0xff, 0xff, 0xff, 0xff, 0x0f],
            },
        )
        .await;
    assert_eq!(
        refusal(vector),
        Refusal::Unreadable,
        "a state vector that claims 2^32 entries"
    );
    let unknown = worker.call(Some(generation), &Request::State(7)).await;
    assert_eq!(refusal(unknown), Refusal::UnknownDocument);
    worker.end().await;
}

/// The worker is held to its limits by the system, has no environment and its own
/// process group.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_worker_is_started_with_its_limits() {
    let worker = roomy();
    worker
        .call(None, &Request::Stall(0))
        .await
        .expect("started");
    let pid = worker.pid().await.expect("a worker runs");
    let limits = std::fs::read_to_string(format!("/proc/{pid}/limits")).expect("its limits");
    let limit = |name: &str| {
        limits
            .lines()
            .find(|line| line.starts_with(name))
            .map_or_else(
                || panic!("{name} in {limits}"),
                |line| {
                    line.split_whitespace()
                        .rev()
                        .skip(1)
                        .take(2)
                        .collect::<Vec<_>>()
                },
            )
    };
    assert_eq!(limit("Max open files"), ["16", "16"], "{limits}");
    assert_eq!(limit("Max core file size"), ["0", "0"], "{limits}");
    assert_eq!(
        limit("Max address space"),
        ["1073741824", "1073741824"],
        "{limits}"
    );
    let environment = std::fs::read(format!("/proc/{pid}/environ")).expect("its environment");
    assert!(
        environment.is_empty(),
        "nothing of the server's environment"
    );
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("its state");
    let group = stat
        .rsplit(')')
        .next()
        .and_then(|rest| rest.split_whitespace().nth(2))
        .expect("the process group");
    assert_eq!(group, pid.to_string(), "a group of its own");
    worker.end().await;
}

/// The time of a request and its answer, and the memory of a worker that holds 20
/// documents, printed for the report.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_cost_of_a_request_and_of_held_documents_is_measured() {
    const ROUNDS: u32 = 1_000;
    let worker = roomy();
    let (generation, _) = worker
        .call(None, &open_body(1, &paragraphs(&["measured"])))
        .await
        .expect("opened");
    let began = Instant::now();
    for _ in 0..ROUNDS {
        worker
            .call(Some(generation), &Request::StateVector(1))
            .await
            .expect("a state vector");
    }
    let round_trip = began.elapsed() / ROUNDS;
    let pid = worker.pid().await.expect("a worker runs");
    let before = resident_kib(pid);
    let texts: Vec<String> = (0..200)
        .map(|line| format!("line {line} of a body"))
        .collect();
    let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
    let body = paragraphs(&texts);
    for doc in 2..22 {
        worker
            .call(None, &open_body(doc, &body))
            .await
            .expect("opened");
    }
    let after = resident_kib(pid);
    let body_bytes = serde_json::to_vec(&body).expect("JSON").len();
    report(&format!(
        "worker round trip {round_trip:?}; resident {before} KiB with 1 document, \
         {after} KiB with 21 (20 of {body_bytes} bytes of JSON each)"
    ));
    worker.end().await;
}

#[cfg(target_os = "linux")]
fn resident_kib(pid: u32) -> u64 {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).expect("its status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|kib| kib.parse().ok())
        .expect("its resident memory")
}

#[expect(clippy::print_stderr, reason = "the measurements are for the report")]
fn report(line: &str) {
    eprintln!("measured: {line}");
}

#[cfg(unix)]
fn signal(pid: u32, name: &str) {
    let status = std::process::Command::new("kill")
        .args([format!("-{name}"), pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(status.success(), "kill -{name} {pid}");
}

/// Whether a process of that identifier is there, a zombie included.
#[cfg(unix)]
fn is_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}
