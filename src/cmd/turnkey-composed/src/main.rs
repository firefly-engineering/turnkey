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
use std::path::{Path, PathBuf};
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
        /// - fuse: Use FUSE filesystem (Linux: fuse3; macOS: macFUSE)
        /// - symlink: Use symlinks (always available, no daemon needed)
        #[arg(long, default_value = "auto")]
        backend: String,

        /// Run in foreground (don't daemonize)
        #[arg(long, short)]
        foreground: bool,

        /// Files/directories to exclude from the source pass-through (repeatable)
        #[arg(long)]
        exclude: Vec<String>,

        /// Output directories to mount: name:real_path (repeatable)
        /// Example: --output build:/tmp/buck-out --output cargo-target:/tmp/cargo-target
        #[arg(long)]
        output: Vec<String>,

        /// VCS tools to wrap with transparent redirect (repeatable)
        /// Example: --vcs-wrap jj --vcs-wrap git
        #[arg(long)]
        vcs_wrap: Vec<String>,

        /// Disable manifest file watching
        #[arg(long)]
        no_watch: bool,
    },
    /// Run as a service, mounting all entries from config file
    ///
    /// Reads ~/.config/turnkey/composed.toml (or --config path) and manages
    /// all mount entries concurrently. This is the subcommand that launchd/systemd runs.
    Serve {
        /// Path to service config file
        #[arg(long, default_value_os_t = composition::serve_config::ServeConfig::default_path())]
        config: PathBuf,
    },

    /// Install as a system service (launchd on macOS, systemd on Linux)
    ///
    /// Creates a service file that runs `turnkey-composed serve` on login.
    /// The service reads mount configuration from ~/.config/turnkey/composed.toml.
    Install {
        /// Also start the service immediately after installing
        #[arg(long)]
        start: bool,
    },

    /// Uninstall the system service
    Uninstall,

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
            exclude,
            output,
            vcs_wrap,
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
                let mut cfg = discover::build_and_configure(&nix, &mp, &rr)
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                if !exclude.is_empty() {
                    cfg = cfg.with_excludes(exclude);
                }
                for o in &output {
                    if let Some((name, path)) = o.split_once(':') {
                        cfg = cfg.with_output_mount(name, path);
                    } else {
                        warn!("Invalid --output format '{}', expected name:path", o);
                    }
                }
                cfg
            };

            // Generate VCS wrappers if requested
            if !vcs_wrap.is_empty() {
                let mount_map = std::collections::HashMap::from([(
                    composition_config.mount_point.clone(),
                    composition_config.repo_root.clone(),
                )]);
                match composition::vcs_wrappers::generate_wrappers(
                    &vcs_wrap, &mount_map, &composition_config.source_dir_name,
                ) {
                    Ok(dir) => info!("VCS wrappers at {}", dir.display()),
                    Err(e) => warn!("Failed to generate VCS wrappers: {}", e),
                }
            }

            if !foreground {
                warn!("Daemonizing not yet implemented, running in foreground");
            }
            run_daemon(&cli.socket, composition_config, backend_type, !no_watch)
        }
        Commands::Serve { config } => run_serve(&config),
        Commands::Install { start } => {
            use composition::serve_config::ServeConfig;
            use composition::service;

            // Find the turnkey-composed binary path
            let binary = std::env::current_exe()
                .context("Failed to determine binary path")?;
            let config_path = ServeConfig::default_path();

            // Ensure config file exists with a helpful template
            if !config_path.exists() {
                if let Some(parent) = config_path.parent() {
                    std::fs::create_dir_all(parent).ok();
                }
                std::fs::write(
                    &config_path,
                    "# Turnkey composition daemon configuration\n\
                     # Add mount entries below:\n\
                     #\n\
                     # [[mounts]]\n\
                     # repo = \"/path/to/your/project\"\n\
                     # mount_point = \"/firefly/project\"\n",
                )
                .ok();
                info!("Created config template at {:?}", config_path);
            }

            let install_path = service::install_service(&binary, &config_path)
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            println!("Service installed at {}", install_path.display());
            println!("Config file: {}", config_path.display());

            if start {
                service::load_service()
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                println!("Service started");
            } else {
                println!("Run with --start to also start the service, or:");
                if cfg!(target_os = "macos") {
                    println!("  launchctl load -w {}", install_path.display());
                } else {
                    println!("  systemctl --user enable --now turnkey-composed.service");
                }
            }
            Ok(())
        }
        Commands::Uninstall => {
            use composition::service;
            service::unload_service()
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            let path = service::service_install_path();
            if path.exists() {
                std::fs::remove_file(&path).ok();
                println!("Service uninstalled from {}", path.display());
            } else {
                println!("No service file found at {}", path.display());
            }
            Ok(())
        }
        Commands::Stop => send_command(&cli.socket, IpcRequest::Stop),
        Commands::Status => send_command(&cli.socket, IpcRequest::Status),
        Commands::Refresh => send_command(&cli.socket, IpcRequest::Refresh),
    }
}

/// Run in service mode: manage multiple mounts from config file
fn run_serve(config_path: &Path) -> Result<()> {
    use composition::serve_config::ServeConfig;

    info!("Reading service config from {:?}", config_path);
    let config = ServeConfig::read(config_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    if config.mounts.is_empty() {
        anyhow::bail!("No mounts configured in {:?}", config_path);
    }

    info!("Managing {} mount(s)", config.mounts.len());

    // Set up signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set signal handler")?;

    // Start each mount in its own thread
    let mut handles: Vec<(String, thread::JoinHandle<()>)> = Vec::new();
    let mut backends: Vec<Arc<std::sync::Mutex<Box<dyn composition::CompositionBackend>>>> = Vec::new();

    for entry in &config.mounts {
        let backend_type = BackendType::from_str(&entry.backend).ok_or_else(|| {
            anyhow::anyhow!("Invalid backend type '{}' for {:?}", entry.backend, entry.repo)
        })?;

        info!(
            "Starting mount: {} -> {} (backend: {})",
            entry.repo.display(),
            entry.mount_point.display(),
            entry.backend
        );

        // Ensure mount point exists (creates synthetic firmlinks on macOS if needed)
        #[cfg(target_os = "macos")]
        composition::synthetic::ensure_mount_point(&entry.mount_point)
            .map_err(|e| anyhow::anyhow!("Failed to prepare mount point {:?}: {}", entry.mount_point, e))?;

        #[cfg(not(target_os = "macos"))]
        std::fs::create_dir_all(&entry.mount_point)
            .with_context(|| format!("Failed to create mount point {:?}", entry.mount_point))?;

        // Discover and build cells
        let nix = CliNixClient::new(&entry.repo);
        let mut composition_config = discover::build_and_configure(&nix, &entry.mount_point, &entry.repo)
            .map_err(|e| anyhow::anyhow!("Failed to discover cells for {:?}: {}", entry.repo, e))?;

        // Apply exclusion rules from config
        if !entry.exclude.is_empty() {
            composition_config = composition_config.with_excludes(entry.exclude.clone());
        }

        // Create and mount backend
        let mut backend = create_backend(backend_type, composition_config)
            .with_context(|| format!("Failed to create backend for {:?}", entry.repo))?;

        backend.mount()
            .with_context(|| format!("Failed to mount {:?}", entry.mount_point))?;

        backend.wait_ready(Some(Duration::from_secs(10)))
            .with_context(|| format!("Mount timed out for {:?}", entry.mount_point))?;

        info!("Mounted {:?} at {:?}", entry.repo, entry.mount_point);

        let backend = Arc::new(std::sync::Mutex::new(backend));
        backends.push(backend.clone());

        // Start manifest watcher in a thread
        let repo_root = entry.repo.clone();
        let mount_label = format!("{}", entry.mount_point.display());
        let running_clone = running.clone();
        let backend_clone = backend;

        let handle = thread::spawn(move || {
            let watcher_config = WatcherConfig::new(&repo_root).with_debounce(500);
            let watcher = match ManifestWatcher::new(watcher_config) {
                Ok(w) => Some(w),
                Err(e) => {
                    warn!("[{}] Failed to create watcher: {}", mount_label, e);
                    None
                }
            };

            while running_clone.load(Ordering::SeqCst) {
                if let Some(ref w) = watcher {
                    while let Some(event) = w.try_recv() {
                        match event {
                            WatcherEvent::ManifestChanged { manifest_name, .. } => {
                                info!("[{}] Manifest changed: {}, rebuilding cells", mount_label, manifest_name);
                                let nix = CliNixClient::new(&repo_root);
                                match discover::build_all_cells(&nix, nix_eval::current_system()) {
                                    Ok(cells) => {
                                        info!("[{}] Rebuilt {} cells", mount_label, cells.len());
                                        if let Ok(mut b) = backend_clone.lock() {
                                            if let Err(e) = b.refresh() {
                                                error!("[{}] Refresh failed: {}", mount_label, e);
                                            }
                                        }
                                    }
                                    Err(e) => error!("[{}] Cell rebuild failed: {}", mount_label, e),
                                }
                            }
                            WatcherEvent::Error { message } => {
                                warn!("[{}] Watcher error: {}", mount_label, message);
                            }
                        }
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
        });

        handles.push((entry.mount_point.display().to_string(), handle));
    }

    // Generate VCS wrappers if configured
    if !config.vcs_wrap.is_empty() {
        let mount_map: std::collections::HashMap<PathBuf, PathBuf> = config
            .mounts
            .iter()
            .map(|m| (m.mount_point.clone(), m.repo.clone()))
            .collect();
        match composition::vcs_wrappers::generate_wrappers(
            &config.vcs_wrap,
            &mount_map,
            "root", // source_dir_name
        ) {
            Ok(dir) => {
                info!(
                    "Generated VCS wrappers in {}. Add to PATH: export PATH=\"{}:$PATH\"",
                    dir.display(),
                    dir.display()
                );
            }
            Err(e) => warn!("Failed to generate VCS wrappers: {}", e),
        }
    }

    info!("All mounts ready. Watching config file for changes...");

    // Track mount state for hot-reload diffing
    let mut active_mount_points: std::collections::HashSet<PathBuf> = config
        .mounts
        .iter()
        .map(|m| m.mount_point.clone())
        .collect();

    // Watch the config file for changes
    let config_path_owned = config_path.to_path_buf();
    let config_mtime = std::fs::metadata(&config_path_owned)
        .and_then(|m| m.modified())
        .ok();
    let last_config_mtime = Arc::new(std::sync::Mutex::new(config_mtime));

    // Main event loop
    while running.load(Ordering::SeqCst) {
        // Check if config file changed
        if let Ok(meta) = std::fs::metadata(&config_path_owned) {
            if let Ok(mtime) = meta.modified() {
                let mut last = last_config_mtime.lock().unwrap();
                let changed = last.map(|l| mtime > l).unwrap_or(true);
                if changed {
                    *last = Some(mtime);
                    // Skip the initial check (first iteration)
                    if config_mtime.is_some() {
                        info!("Config file changed, reloading...");
                        match ServeConfig::read(&config_path_owned) {
                            Ok(new_config) => {
                                let new_mount_points: std::collections::HashSet<PathBuf> =
                                    new_config.mounts.iter().map(|m| m.mount_point.clone()).collect();

                                // Find removed mounts
                                for mp in active_mount_points.difference(&new_mount_points) {
                                    info!("Removing mount: {}", mp.display());
                                    // Find and unmount the backend
                                    // (simplified: we'd need a map of mount_point -> backend index)
                                    // TODO: implement proper removal with backend lookup
                                    warn!("Hot removal of mounts not yet implemented — restart the service");
                                }

                                // Find new mounts
                                for entry in &new_config.mounts {
                                    if !active_mount_points.contains(&entry.mount_point) {
                                        info!("Adding new mount: {} -> {}", entry.repo.display(), entry.mount_point.display());

                                        let backend_type = BackendType::from_str(&entry.backend)
                                            .unwrap_or(BackendType::Auto);

                                        #[cfg(target_os = "macos")]
                                        if let Err(e) = composition::synthetic::ensure_mount_point(&entry.mount_point) {
                                            error!("Failed to prepare mount point: {}", e);
                                            continue;
                                        }

                                        #[cfg(not(target_os = "macos"))]
                                        if let Err(e) = std::fs::create_dir_all(&entry.mount_point) {
                                            error!("Failed to create mount point: {}", e);
                                            continue;
                                        }

                                        let nix = CliNixClient::new(&entry.repo);
                                        match discover::build_and_configure(&nix, &entry.mount_point, &entry.repo) {
                                            Ok(comp_config) => {
                                                match create_backend(backend_type, comp_config) {
                                                    Ok(mut backend) => {
                                                        if let Err(e) = backend.mount() {
                                                            error!("Failed to mount {:?}: {}", entry.mount_point, e);
                                                        } else {
                                                            info!("Mounted new entry: {:?}", entry.mount_point);
                                                            backends.push(Arc::new(std::sync::Mutex::new(backend)));
                                                            active_mount_points.insert(entry.mount_point.clone());
                                                        }
                                                    }
                                                    Err(e) => error!("Failed to create backend: {}", e),
                                                }
                                            }
                                            Err(e) => error!("Failed to discover cells: {}", e),
                                        }
                                    }
                                }
                            }
                            Err(e) => error!("Failed to reload config: {}", e),
                        }
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(500));
    }

    // Cleanup: unmount all backends
    info!("Shutting down...");
    for backend in &backends {
        if let Ok(mut b) = backend.lock() {
            if let Err(e) = b.unmount() {
                error!("Failed to unmount: {}", e);
            }
        }
    }

    // Signal watcher threads to stop (they check `running`)
    // Wait for them
    for (label, handle) in handles {
        if let Err(_) = handle.join() {
            error!("Watcher thread for {} panicked", label);
        }
    }

    // Clean up VCS wrappers
    if !config.vcs_wrap.is_empty() {
        composition::vcs_wrappers::remove_wrappers(&config.vcs_wrap);
    }

    info!("All mounts stopped");
    Ok(())
}

/// Run the daemon process (single mount)
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
