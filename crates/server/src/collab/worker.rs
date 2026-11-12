//! A worker process as the server sees it: started when it is first needed, asked one
//! request at a time, and killed and started again when it dies, breaks a frame or
//! takes longer than the time limit.
//!
//! Every start is a new generation. What a worker held is gone with it: a request
//! about a document opened in an earlier generation is not sent ([`CallError::Lost`]),
//! and whoever holds the document opens it again from what it stored. A worker that
//! dies again and again is started again after a wait that doubles, up to a ceiling.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until, timeout};

use crate::collab_worker::WORKER_ARGUMENT;
use crate::collab_worker::frame::{HEADER_BYTES, header, length_of};
use crate::collab_worker::wire::{Answer, Limits, Refused, Request, decode_response};

/// The longest wait before a worker that keeps dying is started again.
pub const LONGEST_RESTART_WAIT: Duration = Duration::from_secs(10);

/// The wait before the second start in a row that follows a death; it doubles with
/// each further one.
const FIRST_RESTART_WAIT: Duration = Duration::from_millis(100);

/// How long a worker that is asked to end is given before it is killed.
const END_WAIT: Duration = Duration::from_secs(2);

/// How a worker process is started: a program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerCommand {
    /// The program.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
}

impl WorkerCommand {
    /// The executable the server runs in, as the worker.
    ///
    /// # Errors
    ///
    /// Returns why the path of the running executable is not known.
    pub fn this_executable() -> std::io::Result<Self> {
        Ok(Self::of_executable(std::env::current_exe()?))
    }

    /// The server's executable at `program`, as the worker.
    #[must_use]
    pub fn of_executable(program: PathBuf) -> Self {
        Self {
            program,
            args: vec![WORKER_ARGUMENT.into()],
        }
    }
}

/// How the workers are started and held.
#[derive(Debug, Clone)]
pub struct WorkerSettings {
    /// How a worker is started.
    pub command: WorkerCommand,
    /// What a worker holds what it is sent to.
    pub limits: Limits,
    /// How long an answer may take.
    pub timeout: Duration,
    /// The address space of a worker, where the system limits it.
    pub memory_bytes: u64,
}

/// Why a request got no answer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CallError {
    /// The document was opened by a worker that is gone.
    #[error("the worker that held the document is gone")]
    Lost,
    /// The worker died, broke a frame or took too long, and was killed; it is started
    /// again for the next request.
    #[error("the worker died: {0}")]
    Died(Death),
    /// No worker could be started.
    #[error("no worker could be started: {0}")]
    Unavailable(String),
    /// The worker refused the request.
    #[error(transparent)]
    Refused(#[from] Refused),
}

/// How a worker ended.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Death {
    /// It did not answer within the time limit.
    #[error("it did not answer in time")]
    TimedOut,
    /// A frame could not be sent or read: the process ended, or wrote what is no
    /// answer.
    #[error("{0}")]
    Broken(String),
}

/// A worker process and what the server knows of it.
#[derive(Debug)]
pub struct Worker {
    settings: WorkerSettings,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    running: Option<Running>,
    /// The generation of the running worker, or of the next one if none runs.
    generation: u64,
    /// The deaths since the last answer.
    deaths: u32,
    /// When the next worker may be started.
    next_start: Instant,
}

#[derive(Debug)]
struct Running {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    /// A request was sent and its answer not read to the end: the call was given up
    /// half way, and the pipes are out of step.
    in_exchange: bool,
}

impl Worker {
    /// A worker that is started when it is first asked something.
    pub fn new(settings: WorkerSettings) -> Self {
        Self {
            settings,
            state: Mutex::new(State {
                running: None,
                generation: 0,
                deaths: 0,
                next_start: Instant::now(),
            }),
        }
    }

    /// Sends a request and waits for its answer, starting a worker first if none
    /// runs. `generation` is that of the worker that opened the document the request
    /// is about; `None` for a request that is about no open document. Gives the
    /// generation of the worker that answered.
    ///
    /// # Errors
    ///
    /// Returns [`CallError::Lost`] if the worker of `generation` is gone,
    /// [`CallError::Refused`] if the worker refused the request, and
    /// [`CallError::Died`] or [`CallError::Unavailable`] if it got no answer.
    ///
    /// # Panics
    ///
    /// Panics if no worker runs after one was started, which would be a defect.
    pub async fn call(
        &self,
        generation: Option<u64>,
        request: &Request,
    ) -> Result<(u64, Answer), CallError> {
        let mut state = self.state.lock().await;
        if state
            .running
            .as_ref()
            .is_some_and(|running| running.in_exchange)
        {
            let death = Death::Broken("a request was given up half way".to_owned());
            self.bury(&mut state, &death).await;
        }
        if generation.is_some_and(|generation| generation != state.generation) {
            return Err(CallError::Lost);
        }
        let generation = state.generation;
        if state.running.is_none() {
            state.running = Some(self.start(&mut state).await?);
        }
        let limit = self.settings.limits.frame_bytes();
        let running = state.running.as_mut().expect("a worker runs");
        let answered = timeout(self.settings.timeout, running.exchange(request, limit)).await;
        let death = match answered {
            Ok(Ok(response)) => {
                state.deaths = 0;
                return response
                    .map(|answer| (generation, answer))
                    .map_err(CallError::Refused);
            }
            Ok(Err(broken)) => Death::Broken(broken),
            Err(_) => Death::TimedOut,
        };
        self.bury(&mut state, &death).await;
        Err(CallError::Died(death))
    }

    /// The process identifier of the running worker, if one runs.
    pub async fn pid(&self) -> Option<u32> {
        let state = self.state.lock().await;
        state
            .running
            .as_ref()
            .and_then(|running| running.child.id())
    }

    /// Ends the worker: closes its input, which ends it, and kills it if it does not
    /// end in time.
    pub async fn end(&self) {
        let mut state = self.state.lock().await;
        if let Some(mut running) = state.running.take() {
            drop(running.input);
            if timeout(END_WAIT, running.child.wait()).await.is_err() {
                let _ = running.child.kill().await;
            }
            state.generation += 1;
        }
    }

    /// Starts a worker and gives it its limits, after the wait that the deaths before
    /// it call for.
    async fn start(&self, state: &mut State) -> Result<Running, CallError> {
        sleep_until(state.next_start).await;
        let started = self.spawn().map_err(|error| error.to_string());
        let mut running = match started {
            Ok(running) => running,
            Err(error) => {
                Self::count_death(state);
                tracing::error!(%error, "cannot start a collab worker");
                return Err(CallError::Unavailable(error));
            }
        };
        let configure = Request::Configure(self.settings.limits);
        let limit = self.settings.limits.frame_bytes();
        let configured = timeout(self.settings.timeout, running.exchange(&configure, limit)).await;
        match configured {
            Ok(Ok(Ok(Answer::Done))) => {
                tracing::info!(pid = running.child.id(), "started a collab worker");
                Ok(running)
            }
            other => {
                let _ = running.child.kill().await;
                Self::count_death(state);
                let error = format!("the worker did not take its limits: {other:?}");
                tracing::error!(%error, "cannot start a collab worker");
                Err(CallError::Unavailable(error))
            }
        }
    }

    fn spawn(&self) -> std::io::Result<Running> {
        let WorkerCommand { program, args } = &self.settings.command;
        let mut command = Command::new(program);
        command
            .args(args)
            // The worker needs nothing from the environment, which may hold the
            // database's password.
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        #[cfg(unix)]
        super::child::limit(&mut command, self.settings.memory_bytes);
        let mut child = command.spawn()?;
        let input = child.stdin.take().ok_or_else(no_pipe)?;
        let output = child.stdout.take().ok_or_else(no_pipe)?;
        Ok(Running {
            child,
            input,
            output: BufReader::new(output),
            in_exchange: false,
        })
    }

    /// Kills the running worker and waits for its end, so that it leaves no zombie;
    /// what it held is gone with it.
    async fn bury(&self, state: &mut State, death: &Death) {
        if let Some(mut running) = state.running.take() {
            let _ = running.child.start_kill();
            let status = running.child.wait().await;
            tracing::warn!(%death, ?status, "a collab worker died; the next request starts another");
        }
        state.generation += 1;
        Self::count_death(state);
    }

    fn count_death(state: &mut State) {
        state.deaths = state.deaths.saturating_add(1);
        state.next_start = Instant::now() + restart_wait(state.deaths);
    }
}

fn no_pipe() -> std::io::Error {
    std::io::Error::other("the worker's pipe was not made")
}

/// The wait before a worker is started after `deaths` deaths in a row: none after the
/// first, then [`FIRST_RESTART_WAIT`], doubling up to [`LONGEST_RESTART_WAIT`].
fn restart_wait(deaths: u32) -> Duration {
    if deaths <= 1 {
        return Duration::ZERO;
    }
    let doublings = deaths.saturating_sub(2).min(16);
    FIRST_RESTART_WAIT
        .saturating_mul(1 << doublings)
        .min(LONGEST_RESTART_WAIT)
}

impl Running {
    /// Writes a request and reads its answer.
    async fn exchange(
        &mut self,
        request: &Request,
        limit: u64,
    ) -> Result<Result<Answer, Refused>, String> {
        self.in_exchange = true;
        let payload = request.encode();
        let head = header(payload.len(), limit).map_err(|error| error.to_string())?;
        let write = async {
            self.input.write_all(&head).await?;
            self.input.write_all(&payload).await?;
            self.input.flush().await
        };
        write
            .await
            .map_err(|error| format!("cannot write a request: {error}"))?;
        let mut head = [0; HEADER_BYTES];
        self.output
            .read_exact(&mut head)
            .await
            .map_err(|error| format!("cannot read an answer: {error}"))?;
        let length = length_of(head, limit).map_err(|error| error.to_string())?;
        let mut answer = vec![0; length];
        self.output
            .read_exact(&mut answer)
            .await
            .map_err(|error| format!("cannot read an answer: {error}"))?;
        self.in_exchange = false;
        decode_response(&answer).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_before_a_restart_doubles_up_to_its_ceiling() {
        assert_eq!(restart_wait(0), Duration::ZERO);
        assert_eq!(
            restart_wait(1),
            Duration::ZERO,
            "a first death costs no wait"
        );
        assert_eq!(restart_wait(2), FIRST_RESTART_WAIT);
        assert_eq!(restart_wait(3), FIRST_RESTART_WAIT * 2);
        assert_eq!(restart_wait(9), LONGEST_RESTART_WAIT, "12.8 s is past it");
        assert_eq!(restart_wait(u32::MAX), LONGEST_RESTART_WAIT);
    }
}
