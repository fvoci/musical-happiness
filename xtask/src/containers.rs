//! The two containers the tests use: PostgreSQL and an S3-compatible object store.
//!
//! A container that runs is used as it is; a stopped one is started; a missing one is
//! created. Nothing is ever removed here: the containers outlive the test run.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};

use crate::steps::{capture, say};

/// How long a container may take to accept requests after it was started.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// The pause between two readiness probes.
const PROBE_INTERVAL: Duration = Duration::from_millis(500);

/// Test-only credentials of the PostgreSQL container.
const POSTGRES_USER: &str = "fvoci";
const POSTGRES_PASSWORD: &str = "fvoci-test";
const POSTGRES_DATABASE: &str = "fvoci";

/// The bucket the S3 storage test writes into.
const S3_BUCKET: &str = "fvoci-test";

/// A container this checkout's tests use. The name and the host port can be changed
/// through the environment, so that two checkouts on one machine do not collide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Container {
    name: String,
    host_port: u16,
    image: &'static str,
    internal_port: u16,
    environment: Vec<String>,
    command: Vec<&'static str>,
}

impl Container {
    /// The PostgreSQL container: `hafgufa-pg` on 127.0.0.1:15433 unless
    /// `FVOCI_TEST_PG_CONTAINER` and `FVOCI_TEST_PG_PORT` say otherwise.
    ///
    /// # Errors
    ///
    /// Fails if the port variable is set to something that is no port.
    pub(crate) fn postgres() -> Result<Self> {
        Ok(Self {
            name: name_from_env("FVOCI_TEST_PG_CONTAINER", "hafgufa-pg"),
            host_port: port_from_env("FVOCI_TEST_PG_PORT", 15433)?,
            image: "postgres:18",
            internal_port: 5432,
            environment: vec![
                format!("POSTGRES_USER={POSTGRES_USER}"),
                format!("POSTGRES_PASSWORD={POSTGRES_PASSWORD}"),
                format!("POSTGRES_DB={POSTGRES_DATABASE}"),
            ],
            command: Vec::new(),
        })
    }

    /// The S3-compatible store: `hafgufa-s3` on 127.0.0.1:18333 unless
    /// `FVOCI_TEST_S3_CONTAINER` and `FVOCI_TEST_S3_PORT` say otherwise.
    ///
    /// # Errors
    ///
    /// Fails if the port variable is set to something that is no port.
    pub(crate) fn object_store() -> Result<Self> {
        Ok(Self {
            name: name_from_env("FVOCI_TEST_S3_CONTAINER", "hafgufa-s3"),
            host_port: port_from_env("FVOCI_TEST_S3_PORT", 18333)?,
            image: "chrislusf/seaweedfs:latest",
            internal_port: 8333,
            environment: Vec::new(),
            command: vec!["server", "-s3", "-s3.port=8333"],
        })
    }

    /// The PostgreSQL URL the tests connect with.
    pub(crate) fn postgres_url(&self) -> String {
        format!(
            "postgres://{POSTGRES_USER}:{POSTGRES_PASSWORD}@localhost:{}/{POSTGRES_DATABASE}?sslmode=disable",
            self.host_port
        )
    }

    /// The `host:port` the S3 storage test connects to.
    pub(crate) fn s3_endpoint(&self) -> String {
        format!("localhost:{}", self.host_port)
    }

    /// The arguments of the `docker run` that creates this container, published on the
    /// loopback interface only.
    fn run_arguments(&self) -> Vec<String> {
        let mut arguments = vec![
            "run".to_owned(),
            "--detach".to_owned(),
            "--name".to_owned(),
            self.name.clone(),
        ];
        for variable in &self.environment {
            arguments.push("--env".to_owned());
            arguments.push(variable.clone());
        }
        arguments.push("--publish".to_owned());
        arguments.push(format!(
            "127.0.0.1:{}:{}",
            self.host_port, self.internal_port
        ));
        arguments.push(self.image.to_owned());
        arguments.extend(self.command.iter().map(|part| (*part).to_owned()));
        arguments
    }

    /// Makes sure the container runs: starts it if it is stopped, creates it if it does
    /// not exist.
    fn ensure_running(&self) -> Result<()> {
        match self.state()? {
            State::Running => say(&format!("    {}: already running", self.name)),
            State::Stopped => {
                say(&format!("    {}: stopped; starting it", self.name));
                docker(&["start", &self.name])?;
            }
            State::Missing => {
                say(&format!(
                    "    {}: not found; creating it from {}",
                    self.name, self.image
                ));
                let arguments = self.run_arguments();
                let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
                docker(&arguments)?;
            }
        }
        Ok(())
    }

    fn state(&self) -> Result<State> {
        let running = capture(
            "docker",
            &["inspect", "--format", "{{.State.Running}}", &self.name],
        )
        .context("docker is needed to prepare the test containers")?;
        Ok(State::from_inspect(running.as_deref()))
    }
}

/// What `docker inspect` says about a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Stopped,
    Missing,
}

impl State {
    /// `running` is the output of `docker inspect --format {{.State.Running}}`, or
    /// `None` if the inspection failed because there is no such container.
    fn from_inspect(running: Option<&str>) -> Self {
        match running {
            Some("true") => Self::Running,
            Some(_) => Self::Stopped,
            None => Self::Missing,
        }
    }
}

fn docker(arguments: &[&str]) -> Result<()> {
    match capture("docker", arguments)? {
        Some(_) => Ok(()),
        None => bail!("`docker {}` failed", arguments.join(" ")),
    }
}

fn name_from_env(variable: &str, default: &str) -> String {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn port_from_env(variable: &str, default: u16) -> Result<u16> {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse()
            .with_context(|| format!("{variable}={value:?} is no port number")),
        _ => Ok(default),
    }
}

/// Repeats `probe` until it succeeds or [`READY_TIMEOUT`] has passed.
fn wait_until(what: &str, mut probe: impl FnMut() -> Result<()>) -> Result<()> {
    let started = Instant::now();
    loop {
        match probe() {
            Ok(()) => return Ok(()),
            Err(error) if started.elapsed() >= READY_TIMEOUT => {
                return Err(error.context(format!(
                    "{what} was not ready after {} seconds",
                    READY_TIMEOUT.as_secs()
                )));
            }
            Err(_) => std::thread::sleep(PROBE_INTERVAL),
        }
    }
}

/// Makes sure PostgreSQL runs and accepts connections. Returns the URL for the tests.
///
/// # Errors
///
/// Fails if docker is missing, the container cannot be started, or the database does
/// not accept connections in time.
pub(crate) fn prepare_postgres() -> Result<String> {
    let container = Container::postgres()?;
    container.ensure_running()?;
    wait_until(&container.name, || {
        // Over TCP: while the image initializes a new database it answers on the socket
        // only, and restarts once more before it is ready.
        docker(&[
            "exec",
            &container.name,
            "pg_isready",
            "--host",
            "127.0.0.1",
            "--username",
            POSTGRES_USER,
            "--dbname",
            POSTGRES_DATABASE,
        ])
    })?;
    say(&format!(
        "    {}: accepting connections on port {}",
        container.name, container.host_port
    ));
    Ok(container.postgres_url())
}

/// Makes sure the object store runs and has the test bucket. Returns the endpoint and
/// the bucket for the tests.
///
/// # Errors
///
/// Fails if docker is missing, the container cannot be started, or the bucket cannot be
/// created in time.
pub(crate) fn prepare_object_store() -> Result<(String, &'static str)> {
    let container = Container::object_store()?;
    container.ensure_running()?;
    wait_until(&container.name, || {
        create_bucket(container.host_port, S3_BUCKET)
    })?;
    say(&format!(
        "    {}: bucket {S3_BUCKET} is there, port {}",
        container.name, container.host_port
    ));
    Ok((container.s3_endpoint(), S3_BUCKET))
}

/// Creates a bucket with a plain `PUT /<bucket>`; an existing bucket is fine.
fn create_bucket(port: u16, bucket: &str) -> Result<()> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .with_context(|| format!("connecting to port {port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "PUT /{bucket} HTTP/1.1\r\nHost: localhost:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    )?;
    let mut reply = String::new();
    // The status line is all that matters; a reset after it is no failure.
    let _ = stream.read_to_string(&mut reply);
    let status = status_code(&reply)
        .with_context(|| format!("the bucket request got no HTTP answer: {reply:?}"))?;
    // 409: the bucket exists already.
    if status == 200 || status == 409 {
        Ok(())
    } else {
        bail!("creating the bucket {bucket} answered {status}")
    }
}

/// The status code of an HTTP/1.1 response.
fn status_code(response: &str) -> Option<u16> {
    let mut parts = response.lines().next()?.split_whitespace();
    if !parts.next()?.starts_with("HTTP/") {
        return None;
    }
    parts.next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> Container {
        Container {
            name: "probe-pg".to_owned(),
            host_port: 15999,
            image: "postgres:18",
            internal_port: 5432,
            environment: vec!["POSTGRES_USER=fvoci".to_owned()],
            command: vec!["server", "-s3"],
        }
    }

    #[test]
    fn a_container_is_published_on_the_loopback_interface_only() {
        assert_eq!(
            probe().run_arguments(),
            [
                "run",
                "--detach",
                "--name",
                "probe-pg",
                "--env",
                "POSTGRES_USER=fvoci",
                "--publish",
                "127.0.0.1:15999:5432",
                "postgres:18",
                "server",
                "-s3",
            ],
            "name, environment, loopback port, image, then the command"
        );
    }

    #[test]
    fn the_inspection_decides_between_running_stopped_and_missing() {
        assert_eq!(State::from_inspect(Some("true")), State::Running, "true");
        assert_eq!(State::from_inspect(Some("false")), State::Stopped, "false");
        assert_eq!(State::from_inspect(None), State::Missing, "no such object");
    }

    #[test]
    fn the_test_urls_name_the_host_port() {
        assert_eq!(
            probe().postgres_url(),
            "postgres://fvoci:fvoci-test@localhost:15999/fvoci?sslmode=disable",
            "the URL the store's test support reads"
        );
        assert_eq!(
            probe().s3_endpoint(),
            "localhost:15999",
            "host and port, no scheme"
        );
    }

    #[test]
    fn the_status_code_is_read_from_the_status_line() {
        assert_eq!(
            status_code("HTTP/1.1 409 Conflict\r\nServer: x\r\n\r\n"),
            Some(409),
            "an existing bucket"
        );
        assert_eq!(status_code(""), None, "no answer");
        assert_eq!(status_code("garbage 200"), None, "no HTTP status line");
    }
}
