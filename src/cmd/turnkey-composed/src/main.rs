//! turnkey-composed - Composition daemon
//!
//! This daemon manages the composition backend lifecycle for the Turnkey composition layer.
//! It provides:
//! - Start/stop commands for mounting/unmounting the composition view
//! - Unix socket IPC for status queries and control
//! - Graceful shutdown handling via SIGTERM/SIGINT
//! - Manifest file watching for automatic refresh
//! - Automatic backend selection (FUSE or symlinks based on platform)

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use composition::watcher::{ManifestWatcher, WatcherConfig, WatcherEvent};
use composition::compose_config::ComposeFile;
use composition::discover;
use composition::{create_backend, BackendType, CompositionConfig};
use nix_eval::CliNixClient;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};

/// Default socket path for IPC
fn default_socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"));
    runtime_dir.join("turnkey-composed.sock")
}

/// FUSE composition daemon for Turnkey
#[derive(Parser)]
#[command(name = "turnkey-composed")]
#[command(about = "FUSE composition daemon for Turnkey")]
struct Cli {
    /// Socket path for IPC
    #[arg(long, default_value_os_t = default_socket_path())]
    socket: PathBuf,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon and mount the composition view
    Start {
        /// Path to compose.toml config file
        ///
        /// If provided, mount point, repo root, and cells are read from this file.
        /// CLI flags --mount-point and --repo-root override the config file values.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Mount point for the composition view
        #[arg(long)]
        mount_point: Option<PathBuf>,

        /// Repository root path
        #[arg(long)]
        repo_root: Option<PathBuf>,

        /// Backend type: auto, fuse, or symlink
        ///
        /// - auto: Automatically select best available backend (default)
        /// - fuse: Use FUSE filesystem (requires FUSE/FUSE-T installation)
        /// - symlink: Use symlinks (always available, no daemon needed)
        #[arg(long, default_value = "auto")]
        backend: String,

        /// Run in foreground (don't daemonize)
        #[arg(long, short)]
        foreground: bool,

        /// Disable manifest file watching
        #[arg(long)]
        no_watch: bool,
    },
    /// Stop the daemon and unmount the composition view
    Stop,
    /// Query the daemon status
    Status,
    /// Refresh the composition view
    Refresh,
}

/// IPC request message
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum IpcRequest {
    Status,
    Stop,
    Refresh,
}

/// IPC response message
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum IpcResponse {
    Status {
        running: bool,
        mount_point: Option<String>,
        status: String,
        watching: bool,
    },
    Ok {
        message: String,
    },
    Error {
        message: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    match cli.command {
        Commands::Start {
            config,
            mount_point,
            repo_root,
            backend,
            foreground,
            no_watch,
        } => {
            // Parse backend type
            let backend_type = BackendType::from_str(&backend).ok_or_else(|| {
                anyhow::anyhow!(
                    "Invalid backend type '{}'. Valid options: {}",
                    backend,
                    BackendType::valid_names().join(", ")
                )
            })?;

            // Build composition config
            let composition_config = if let Some(config_path) = config {
                // Explicit config file
                let compose = ComposeFile::read(&config_path)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                let mut cfg = compose.into_composition_config();
                if let Some(mp) = mount_point {
                    cfg.mount_point = mp;
                }
                if let Some(rr) = repo_root {
                    cfg.repo_root = rr;
                }
                cfg
            } else {
                // Auto-discover: build cell derivations directly via nix build
                let mp = mount_point
                    .ok_or_else(|| anyhow::anyhow!("--mount-point is required"))?;
                let rr = repo_root
                    .ok_or_else(|| anyhow::anyhow!("--repo-root is required"))?;
                let nix = CliNixClient::new(&rr);
                discover::build_and_configure(&nix, &mp, &rr)
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            };

            if !foreground {
                warn!("Daemonizing not yet implemented, running in foreground");
            }
            run_daemon(&cli.socket, composition_config, backend_type, !no_watch)
        }
        Commands::Stop => send_command(&cli.socket, IpcRequest::Stop),
        Commands::Status => send_command(&cli.socket, IpcRequest::Status),
        Commands::Refresh => send_command(&cli.socket, IpcRequest::Refresh),
    }
}

/// Run the daemon process
fn run_daemon(
    socket_path: &PathBuf,
    config: CompositionConfig,
    backend_type: BackendType,
    enable_watch: bool,
) -> Result<()> {
    // Remove existing socket if present
    if socket_path.exists() {
        std::fs::remove_file(socket_path)
            .with_context(|| format!("Failed to remove existing socket: {:?}", socket_path))?;
    }

    // Create the Unix socket
    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("Failed to bind to socket: {:?}", socket_path))?;

    info!("Daemon listening on {:?}", socket_path);

    // Set up signal handling for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set signal handler")?;

    let mount_point = config.mount_point.clone();
    let repo_root = config.repo_root.clone();

    info!(
        "Composition config: mount={}, repo={}, cells={}",
        mount_point.display(),
        repo_root.display(),
        config.cells.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
    );

    // Create backend using automatic selection
    let mut backend = create_backend(backend_type, config)
        .context("Failed to create composition backend")?;

    info!("Mounting composition view at {:?}", mount_point);
    backend.mount().context("Failed to mount composition view")?;

    // Wait for backend to be ready
    backend
        .wait_ready(Some(Duration::from_secs(10)))
        .context("FUSE mount timed out")?;

    info!("FUSE filesystem mounted and ready");

    // Set up manifest watcher if enabled
    let watcher = if enable_watch {
        let watcher_config = WatcherConfig::new(&repo_root).with_debounce(500);

        match ManifestWatcher::new(watcher_config) {
            Ok(w) => {
                info!("Watching for manifest changes in {:?}", repo_root);
                Some(w)
            }
            Err(e) => {
                warn!("Failed to create manifest watcher: {}. Continuing without watching.", e);
                None
            }
        }
    } else {
        info!("Manifest watching disabled");
        None
    };

    // Set socket to non-blocking for the event loop
    listener
        .set_nonblocking(true)
        .context("Failed to set socket to non-blocking")?;

    // Main event loop
    while running.load(Ordering::SeqCst) {
        // Check for manifest changes
        if let Some(ref w) = watcher {
            while let Some(event) = w.try_recv() {
                match event {
                    WatcherEvent::ManifestChanged { path, manifest_name } => {
                        info!(
                            "Manifest changed: {} ({:?}), re-bootstrapping cells",
                            manifest_name, path
                        );
                        // Rebuild cells directly via nix build
                        let nix = CliNixClient::new(&repo_root);
                        match discover::build_all_cells(&nix, nix_eval::current_system()) {
                            Ok(cells) => {
                                info!("Rebuilt {} cells, refreshing backend", cells.len());
                                // TODO: update backend's cell paths with new store paths
                                if let Err(e) = backend.refresh() {
                                    error!("Failed to refresh backend: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to rebuild cells: {}", e);
                            }
                        }
                    }
                    WatcherEvent::Error { message } => {
                        warn!("Watcher error: {}", message);
                    }
                }
            }
        }

        // Check for incoming connections
        match listener.accept() {
            Ok((stream, _)) => {
                let status = backend.status();
                let mp = mount_point.clone();
                let watching = watcher.is_some();
                let running_clone = running.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &status, &mp, watching, &running_clone) {
                        error!("Error handling client: {}", e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No connection waiting, sleep briefly
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }

    // Cleanup
    info!("Shutting down...");
    drop(watcher); // Stop watcher first
    backend.unmount().context("Failed to unmount FUSE filesystem")?;

    // Remove socket
    if socket_path.exists() {
        std::fs::remove_file(socket_path).ok();
    }

    info!("Daemon stopped");
    Ok(())
}

/// Handle an IPC client connection
fn handle_client(
    mut stream: UnixStream,
    status: &composition::BackendStatus,
    mount_point: &PathBuf,
    watching: bool,
    running: &Arc<AtomicBool>,
) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .ok();

    let reader = BufReader::new(stream.try_clone()?);

    for line in reader.lines() {
        let line = line?;
        debug!("Received: {}", line);

        let response = match serde_json::from_str::<IpcRequest>(&line) {
            Ok(IpcRequest::Status) => IpcResponse::Status {
                running: true,
                mount_point: Some(mount_point.display().to_string()),
                status: status.to_string(),
                watching,
            },
            Ok(IpcRequest::Stop) => {
                // Signal shutdown (this is a simplified version)
                IpcResponse::Ok {
                    message: "Shutdown requested".into(),
                }
            }
            Ok(IpcRequest::Refresh) => IpcResponse::Ok {
                message: "Refresh triggered".into(),
            },
            Err(e) => IpcResponse::Error {
                message: format!("Invalid request: {}", e),
            },
        };

        let response_json = serde_json::to_string(&response)?;
        writeln!(stream, "{}", response_json)?;
        stream.flush()?;

        // For Stop command, signal the main loop to stop
        if matches!(
            serde_json::from_str::<IpcRequest>(&line),
            Ok(IpcRequest::Stop)
        ) {
            info!("Stop command received, signaling shutdown");
            running.store(false, Ordering::SeqCst);
        }

        break; // One request per connection
    }

    Ok(())
}

/// Send a command to the running daemon
fn send_command(socket_path: &PathBuf, request: IpcRequest) -> Result<()> {
    if !socket_path.exists() {
        if matches!(request, IpcRequest::Status) {
            println!("Daemon is not running");
            return Ok(());
        }
        anyhow::bail!("Daemon is not running (socket not found)");
    }

    let mut stream =
        UnixStream::connect(socket_path).context("Failed to connect to daemon socket")?;

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .ok();

    // Send request
    let request_json = serde_json::to_string(&request)?;
    writeln!(stream, "{}", request_json)?;
    stream.flush()?;

    // Read response
    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = line?;
        let response: IpcResponse = serde_json::from_str(&line)?;

        match response {
            IpcResponse::Status {
                running,
                mount_point,
                status,
                watching,
            } => {
                println!("Daemon status:");
                println!("  Running: {}", running);
                if let Some(mp) = mount_point {
                    println!("  Mount point: {}", mp);
                }
                println!("  Status: {}", status);
                println!("  Watching manifests: {}", watching);
            }
            IpcResponse::Ok { message } => {
                println!("{}", message);
            }
            IpcResponse::Error { message } => {
                eprintln!("Error: {}", message);
                std::process::exit(1);
            }
        }
        break;
    }

    Ok(())
}
