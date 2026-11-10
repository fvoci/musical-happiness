//! The administration socket: `enablelocalmode`.

use std::fs::Permissions;
use std::io;
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, bail};
use axum::Router;
use tokio::net::UnixListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::request_log::Peer;

/// The mode of the socket file: only the user the server runs as may connect
/// (`server/server/server.go:398`).
const SOCKET_MODE: u32 = 0o600;

/// How long stopping waits for requests in flight before it cuts them off.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// The Unix socket that serves the administration API, and the task that serves it.
///
/// The socket file is removed when this is stopped or dropped.
#[derive(Debug)]
pub(crate) struct AdminSocket {
    path: PathBuf,
    stop: CancellationToken,
    serving: JoinHandle<()>,
}

impl AdminSocket {
    /// Creates the socket at `path`, replacing a socket file a previous run left
    /// behind, and starts serving `router` on it.
    pub(crate) fn start(path: &Path, router: Router) -> anyhow::Result<Self> {
        remove_stale_socket(path)?;
        let listener = UnixListener::bind(path).context("binding the socket")?;
        if let Err(error) = std::fs::set_permissions(path, Permissions::from_mode(SOCKET_MODE)) {
            let _ = std::fs::remove_file(path);
            return Err(error).context("restricting the socket to the server's user");
        }

        let stop = CancellationToken::new();
        let stopped = stop.clone();
        let serving = tokio::spawn(async move {
            tracing::info!("starting unix socket server");
            let service = router.into_make_service_with_connect_info::<Peer>();
            let served = axum::serve(listener, service)
                .with_graceful_shutdown(async move { stopped.cancelled().await })
                .await;
            if let Err(error) = served {
                tracing::error!(%error, "error serving the unix socket");
            }
        });
        Ok(Self {
            path: path.to_owned(),
            stop,
            serving,
        })
    }

    /// Stops serving and removes the socket file.
    pub(crate) async fn stop(mut self) {
        self.stop.cancel();
        if tokio::time::timeout(STOP_TIMEOUT, &mut self.serving)
            .await
            .is_err()
        {
            tracing::warn!("requests on the unix socket did not finish before shutdown");
        }
        // Dropping removes the file.
    }
}

impl Drop for AdminSocket {
    fn drop(&mut self) {
        self.stop.cancel();
        self.serving.abort();
        if let Err(error) = std::fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %self.path.display(), "unable to remove the unix socket");
        }
    }
}

/// Removes the socket file of a server that did not get to remove it, such as one
/// that was killed.
///
/// The original removes whatever file it finds at the path
/// (`server/server/server.go:386-390`); only a socket is removed here, so that a
/// mistyped path cannot cost a file.
fn remove_stale_socket(path: &Path) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(path).context("removing the socket file of an earlier run")
        }
        Ok(_) => bail!("the path exists and is not a socket; it is left alone"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("looking at the socket path"),
    }
}

#[cfg(test)]
mod tests {
    use fvoci_model::{IdKind, new_id};

    use super::*;

    /// A path short enough for a socket address, which the build directory is not.
    fn socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("fvoci-{}.socket", new_id(IdKind::None)))
    }

    fn mode(path: &Path) -> u32 {
        let metadata = std::fs::symlink_metadata(path).expect("the path exists");
        metadata.permissions().mode() & 0o777
    }

    fn is_socket(path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
    }

    #[tokio::test]
    async fn socket_is_private_and_removed_when_stopped() {
        let path = socket_path();

        let socket = AdminSocket::start(&path, Router::new()).expect("the socket opens");

        assert!(is_socket(&path), "a socket file exists");
        assert_eq!(mode(&path), 0o600, "only the owner may connect");
        socket.stop().await;
        assert!(!path.exists(), "the socket file is removed");
    }

    #[tokio::test]
    async fn socket_is_removed_when_dropped() {
        let path = socket_path();
        let socket = AdminSocket::start(&path, Router::new()).expect("the socket opens");

        drop(socket);

        assert!(!path.exists(), "the socket file is removed");
    }

    #[tokio::test]
    async fn stale_socket_is_replaced() {
        let path = socket_path();
        // A listener that goes away without removing its file, as a killed server's.
        drop(std::os::unix::net::UnixListener::bind(&path).expect("a socket is bound"));
        assert!(is_socket(&path), "the stale socket file exists");

        let socket = AdminSocket::start(&path, Router::new()).expect("the socket opens");

        assert!(is_socket(&path), "a socket file exists");
        assert_eq!(mode(&path), 0o600, "only the owner may connect");
        tokio::net::UnixStream::connect(&path)
            .await
            .expect("the new socket accepts connections");
        socket.stop().await;
    }

    #[tokio::test]
    async fn other_file_at_the_socket_path_is_left_alone() {
        let path = socket_path();
        std::fs::write(&path, "precious").expect("a file is written");

        let error = AdminSocket::start(&path, Router::new())
            .expect_err("a file that is not a socket is not replaced");

        assert!(error.to_string().contains("not a socket"), "{error:#}");
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file is still there"),
            "precious"
        );
        std::fs::remove_file(&path).expect("the file is removed");
    }
}
