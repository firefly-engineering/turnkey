//! The local cache's server: reaching it, and starting it when nothing
//! answers

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::{Config, Getenv, max_size_gib, store_dir};

/// Stops a server nobody has used for this long, so no process is left
/// behind on a machine that stopped running tk test. The next tk test
/// starts it again; recorded results stay in the store.
const IDLE_TIMEOUT: &str = "24h";

/// How long `ensure` waits for a server to answer a probe
const PROBE: Duration = Duration::from_millis(200);

/// How long `ensure` waits for a server it started to come up
const STARTUP: Duration = Duration::from_secs(5);

/// Opens an HTTP/2 connection (RFC 9113 section 3.4): the client preface
/// followed by an empty SETTINGS frame
pub fn http2_preface() -> Vec<u8> {
    let mut preface = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n".to_vec();
    preface.extend_from_slice(&[
        0, 0, 0,   // payload length 0
        0x4, // type SETTINGS
        0,   // flags
        0, 0, 0, 0, // stream 0
    ]);
    preface
}

/// Connects to `host_port` within `timeout`, trying each of its addresses
/// in turn with what is left of it
pub(crate) fn dial(host_port: &str, timeout: Duration) -> io::Result<TcpStream> {
    let deadline = Instant::now() + timeout;
    let mut last = io::Error::new(io::ErrorKind::NotFound, "no address");
    for addr in host_port.to_socket_addrs()? {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "i/o timeout"));
        }
        match TcpStream::connect_timeout(&addr, left) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// The command line the server is started with, after its binary
pub fn server_args(store: &Path, max_size_gib: u64, host_port: &str) -> Vec<String> {
    vec![
        "--dir".into(),
        store.join("cas").to_string_lossy().into_owned(),
        "--max_size".into(),
        max_size_gib.to_string(),
        "--grpc_address".into(),
        host_port.into(),
        // bazel-remote always serves HTTP too; nothing uses it, so take
        // any free port rather than risk a clash
        "--http_address".into(),
        "127.0.0.1:0".into(),
        "--idle_timeout".into(),
        IDLE_TIMEOUT.into(),
    ]
}

/// Starts the server, and returns a receiver that is told (or hung up on)
/// when it exits
pub(crate) type Start<'a> =
    &'a mut dyn FnMut(&Config, Getenv) -> Result<mpsc::Receiver<()>, String>;

impl Config {
    /// Whether a gRPC server answers at the cache's address within
    /// `timeout`. gRPC runs over HTTP/2, whose servers must answer a client
    /// preface with a SETTINGS frame; a listener that doesn't, such as some
    /// other program holding the port, is not the cache.
    pub fn reachable(&self, timeout: Duration) -> bool {
        let Ok(host_port) = self.host_port() else {
            return false;
        };
        let Ok(mut conn) = dial(host_port, timeout) else {
            return false;
        };
        if conn.set_read_timeout(Some(timeout)).is_err()
            || conn.set_write_timeout(Some(timeout)).is_err()
        {
            return false;
        }
        if conn.write_all(&http2_preface()).is_err() {
            return false;
        }
        // A frame header is 9 bytes; the type is the fourth
        let mut header = [0u8; 9];
        if conn.read_exact(&mut header).is_err() {
            return false;
        }
        header[3] == 0x4
    }

    /// Makes sure the cache is up, starting it if nothing answers. The
    /// server runs detached and outlives tk. It fails fast when the server
    /// can't start, so tk test can fall back to running uncached without a
    /// noticeable delay.
    pub fn ensure(&self, env: Getenv) -> Result<(), String> {
        self.ensure_with(env, &mut start)
    }

    /// [`Config::ensure`] with how the server is started as a seam
    pub(crate) fn ensure_with(&self, env: Getenv, start: Start) -> Result<(), String> {
        if self.reachable(PROBE) {
            return Ok(());
        }

        // Several tk test runs (other checkouts, other terminals) may find
        // the cache down at once. Only the one holding the lock starts it;
        // the others find it up once they get the lock.
        let _lock = lock_store(env)?;
        if self.reachable(PROBE) {
            return Ok(());
        }

        let exited = start(self, env)?;
        let deadline = Instant::now() + STARTUP;
        while Instant::now() < deadline {
            if self.reachable(PROBE) {
                return Ok(());
            }
            match exited.recv_timeout(Duration::from_millis(50)) {
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                _ => {
                    let log = store_dir(env).unwrap_or_default().join("server.log");
                    return Err(format!(
                        "the test result cache exited on startup (see {})",
                        log.display()
                    ));
                }
            }
        }
        Err(format!(
            "test result cache did not come up at {}",
            self.address
        ))
    }
}

/// Takes an exclusive lock on the store, waiting for any other holder.
/// Dropping the file releases it.
fn lock_store(env: Getenv) -> Result<File, String> {
    let store = store_dir(env)?;
    mkdir_all(&store).map_err(|e| format!("creating the test result store: {e}"))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o644)
        .open(store.join("server.lock"))
        .map_err(|e| format!("opening the test result cache lock: {e}"))?;
    lock.lock()
        .map_err(|e| format!("locking the test result store: {e}"))?;
    Ok(lock)
}

/// `os.MkdirAll(path, 0o755)`
fn mkdir_all(path: &PathBuf) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(path)
}

/// Launches the server, detached from tk's session so that it survives tk
/// and the terminal, its output appended to the store's server.log
fn start(cache: &Config, env: Getenv) -> Result<mpsc::Receiver<()>, String> {
    let host_port = cache.host_port()?;
    let store = store_dir(env)?;
    let max_size = max_size_gib(env)?;
    mkdir_all(&store).map_err(|e| format!("creating the test result store: {e}"))?;
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o644)
        .open(store.join("server.log"))
        .map_err(|e| format!("opening the test result cache log: {e}"))?;
    let log_err = log
        .try_clone()
        .map_err(|e| format!("opening the test result cache log: {e}"))?;

    let mut command = Command::new(&cache.server);
    command
        .args(server_args(&store, max_size, host_port))
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log_err);
    // SAFETY: setsid is async-signal-safe, and nothing else runs between
    // fork and exec
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("starting the test result cache: {e}"))?;
    // Watch for an early exit (a taken port, a bad store). A server that
    // comes up keeps running after tk hands over to buck2.
    let (exited, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = child.wait();
        let _ = exited.send(());
    });
    Ok(rx)
}
