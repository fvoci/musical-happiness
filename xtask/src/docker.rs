//! `cargo xtask docker`: builds the two images of `docker/`, starts them with
//! `docker/compose.yaml` as an installation would, and tries them from outside: sign
//! up, a board, a card, words in its body, and a search that finds them. Then it looks
//! at what the installation must not do (secrets in the images, a database role past
//! the wall, ports open to the host, processes as root) and takes everything down with
//! its volumes, whatever happened. `--push-plan` prints how the images go to GHCR.
//!
//! It needs `docker` with compose, `curl` and `git`. The images are built natively, for
//! the processor of the machine; the other one is built on a machine of that kind.

mod checks;
mod images;
mod scenario;

use std::fs;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context as _, Result, bail};

use crate::steps::{self, Captured, capture, say};

pub(crate) use images::print_push_plan;

/// The compose project of the trial: fixed, so that what a killed run left is found
/// and taken down by the next one.
const PROJECT: &str = "fvoci-smoke";

/// How long the services may take to become healthy, in seconds: the search server's
/// first start makes its certificates and its security index.
const WAIT_SECONDS: &str = "300";

/// How long `docker compose down` gives each container to stop, in seconds: the server
/// finishes its requests and lets go of its lock.
const STOP_SECONDS: &str = "40";

/// The installation under trial: where it answers and what it was given.
#[derive(Debug)]
pub(crate) struct Trial {
    root: PathBuf,
    env_file: PathBuf,
    /// The address of fvoci on the host.
    pub(crate) base: String,
    /// The secrets the installation was given, to be found nowhere they must not be.
    pub(crate) secrets: Vec<String>,
    pub(crate) images: images::Names,
}

impl Trial {
    /// Runs `docker compose` on the trial's project.
    pub(crate) fn compose(&self, arguments: &[&str]) -> Result<Captured> {
        let compose_file = self.root.join("docker/compose.yaml");
        let env_file = self.env_file.to_string_lossy();
        let compose_file = compose_file.to_string_lossy();
        let mut all = vec![
            "compose",
            "--project-name",
            PROJECT,
            "--env-file",
            &env_file,
            "--file",
            &compose_file,
        ];
        all.extend_from_slice(arguments);
        capture("docker", &all)
    }
}

/// Builds, starts, tries and takes down the installation.
///
/// # Errors
///
/// Fails if a step fails; the installation is taken down all the same.
pub(crate) fn run(root: &Path) -> Result<()> {
    let started = Instant::now();
    let names = images::Names::of_checkout(root)?;
    images::build(root, &names)?;
    let built = started.elapsed();

    let directory = root.join("target/docker-smoke");
    fs::create_dir_all(&directory).with_context(|| format!("creating {}", directory.display()))?;
    let port = free_port()?;
    let secrets = Secrets::new()?;
    let env_file = directory.join("smoke.env");
    write_private(&env_file, &secrets.env_file(&names, port))?;
    let trial = Trial {
        root: root.to_owned(),
        env_file,
        base: format!("http://localhost:{port}"),
        secrets: secrets.values(),
        images: names,
    };

    // What a killed run left behind is this trial's too.
    take_down(&trial)?;
    let outcome = try_installation(&trial, &directory);
    let down = take_down(&trial);
    let removed = fs::remove_file(&trial.env_file);
    // The session of a server that is gone; nothing to keep.
    let _ = fs::remove_file(directory.join("cookies.txt"));
    outcome?;
    down?;
    removed.context("removing the file of the trial's secrets")?;
    say(&format!(
        "\ndocker: passed in {:.0}s (of which building {:.0}s)",
        started.elapsed().as_secs_f64(),
        built.as_secs_f64()
    ));
    Ok(())
}

/// Starts the installation and puts it through the scenario and the checks.
fn try_installation(trial: &Trial, directory: &Path) -> Result<()> {
    say("\n==> starting the installation");
    let up = trial.compose(&[
        "up",
        "--detach",
        "--no-build",
        "--wait",
        "--wait-timeout",
        WAIT_SECONDS,
    ])?;
    if !up.succeeded {
        let logs = trial.compose(&["logs", "--no-color", "--tail", "60"])?;
        bail!(
            "the installation did not become healthy: {}\n{}",
            up.error,
            logs.output
        );
    }
    let checks = steps::run_all(
        "docker",
        vec![
            steps::Step::new("sign up, board, card, body, search", || {
                scenario::run(trial, directory)
            }),
            steps::Step::new("the server's database role", || {
                checks::database_role(trial)
            }),
            steps::Step::new("only fvoci's port is published", || {
                checks::published_ports(trial)
            }),
            steps::Step::new("no process runs as root", || checks::processes(trial)),
            steps::Step::new("no secret in the images or containers", || {
                checks::secrets(trial)
            }),
        ],
    );
    if checks.is_err() {
        let logs = trial.compose(&["logs", "--no-color", "--tail", "60", "fvoci"])?;
        say(&format!("\nfvoci's last lines:\n{}", logs.output));
    }
    checks
}

/// Takes the installation down with its volumes and makes sure nothing of it is left.
fn take_down(trial: &Trial) -> Result<()> {
    let down = trial.compose(&[
        "--profile",
        "s3",
        "--profile",
        "mail",
        "down",
        "--volumes",
        "--remove-orphans",
        "--timeout",
        STOP_SECONDS,
    ])?;
    if !down.succeeded {
        bail!("docker compose down failed: {}", down.error);
    }
    let label = format!("label=com.docker.compose.project={PROJECT}");
    let listings: [&[&str]; 3] = [
        &["container", "ls", "--all"],
        &["volume", "ls"],
        &["network", "ls"],
    ];
    for listing in listings {
        let mut arguments = listing.to_vec();
        arguments.extend(["--quiet", "--filter", &label]);
        let left = capture("docker", &arguments)?;
        if !left.succeeded {
            bail!("docker {} failed: {}", listing.join(" "), left.error);
        }
        if !left.output.is_empty() {
            bail!("the trial left a {}: {}", listing[0], left.output);
        }
    }
    Ok(())
}

/// A port of the loopback interface that nothing listens on now.
fn free_port() -> Result<u16> {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("finding a free local port")?;
    Ok(listener.local_addr()?.port())
}

/// Writes `text` to a file only its owner can read.
fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    file.write_all(text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

/// The secrets of one trial, made anew from the system's random source.
struct Secrets {
    postgres: String,
    database: String,
    secret_key: String,
    search: String,
    metrics: String,
}

impl Secrets {
    fn new() -> Result<Self> {
        Ok(Self {
            postgres: hex(&random::<24>()?),
            database: hex(&random::<24>()?),
            secret_key: base64(&random::<32>()?),
            search: hex(&random::<24>()?),
            metrics: hex(&random::<32>()?),
        })
    }

    fn values(&self) -> Vec<String> {
        vec![
            self.postgres.clone(),
            self.database.clone(),
            self.secret_key.clone(),
            self.search.clone(),
            self.metrics.clone(),
        ]
    }

    /// The `.env` of the trial, as an operator writes it from `.env.example`.
    fn env_file(&self, images: &images::Names, port: u16) -> String {
        format!(
            "FVOCI_IMAGE={}\nFVOCI_SEARCH_IMAGE={}\nFVOCI_HOST_PORT={port}\n\
             FVOCI_SERVERROOT=http://localhost:{port}\nFVOCI_ORG_CREATION=open\n\
             POSTGRES_PASSWORD={}\nFVOCI_DB_PASSWORD={}\nFVOCI_SECRET_KEY={}\n\
             FVOCI_SEARCH_PASSWORD={}\nFVOCI_METRICS_TOKEN={}\n\
             FVOCI_S3_ACCESS_KEY_ID=\nFVOCI_S3_SECRET_ACCESS_KEY=\n",
            images.server.local,
            images.search.local,
            self.postgres,
            self.database,
            self.secret_key,
            self.search,
            self.metrics
        )
    }
}

/// `N` bytes from the system's random source.
fn random<const N: usize>() -> Result<[u8; N]> {
    use std::io::Read as _;

    let mut bytes = [0; N];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .context("reading /dev/urandom")?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    bytes.iter().fold(String::new(), |mut text, byte| {
        // Writing to a String does not fail.
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Standard base64 with padding, as `FVOCI_SECRET_KEY` is written.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0_u32, |group, (at, byte)| {
            group | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                let index = (group >> (18 - 6 * at)) & 0x3f;
                text.push(char::from(ALPHABET[usize::try_from(index).unwrap_or(0)]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_the_standard_alphabet_with_padding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(&[0xfb, 0xff]), "+/8=");
        assert_eq!(base64(&[0; 32]).len(), 44);
    }

    #[test]
    fn hex_writes_two_digits_a_byte() {
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
    }
}
