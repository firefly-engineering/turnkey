//! Service installation for launchd (macOS) and systemd (Linux)
//!
//! Generates and installs service files that run `turnkey-composed serve`.
//! The service is user-level (no root required for installation).

use std::path::{Path, PathBuf};

use log::info;

/// Generate the macOS launchd plist content
pub fn launchd_plist(binary_path: &Path, config_path: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.firefly.turnkey-composed</string>

    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>serve</string>
        <string>--config</string>
        <string>{config}</string>
    </array>

    <key>RunAtLoad</key>
    <true/>

    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>

    <key>StandardOutPath</key>
    <string>/tmp/turnkey-composed.stdout.log</string>

    <key>StandardErrorPath</key>
    <string>/tmp/turnkey-composed.stderr.log</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/nix/var/nix/profiles/default/bin</string>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        binary = binary_path.display(),
        config = config_path.display(),
    )
}

/// Generate the Linux systemd user unit content
pub fn systemd_unit(binary_path: &Path, config_path: &Path) -> String {
    format!(
        r#"[Unit]
Description=Turnkey FUSE Composition Daemon
After=nix-daemon.service

[Service]
Type=simple
ExecStart={binary} serve --config {config}
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info
Environment=PATH=/usr/local/bin:/usr/bin:/bin:%h/.nix-profile/bin:/nix/var/nix/profiles/default/bin

[Install]
WantedBy=default.target
"#,
        binary = binary_path.display(),
        config = config_path.display(),
    )
}

/// Get the installation path for the service file
pub fn service_install_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .join("Library/LaunchAgents/com.firefly.turnkey-composed.plist")
    } else {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("systemd/user/turnkey-composed.service")
    }
}

/// Install the service file and optionally start it
pub fn install_service(
    binary_path: &Path,
    config_path: &Path,
) -> Result<PathBuf, ServiceError> {
    let install_path = service_install_path();

    // Ensure parent directory exists
    if let Some(parent) = install_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ServiceError::Io {
            message: format!("failed to create {:?}: {}", parent, e),
        })?;
    }

    // Generate the service content
    let content = if cfg!(target_os = "macos") {
        launchd_plist(binary_path, config_path)
    } else {
        systemd_unit(binary_path, config_path)
    };

    // Write the service file
    std::fs::write(&install_path, &content).map_err(|e| ServiceError::Io {
        message: format!("failed to write {:?}: {}", install_path, e),
    })?;

    info!("Installed service file at {:?}", install_path);
    Ok(install_path)
}

/// Load the service (start it via the system service manager)
pub fn load_service() -> Result<(), ServiceError> {
    let install_path = service_install_path();

    if cfg!(target_os = "macos") {
        let output = std::process::Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&install_path)
            .output()
            .map_err(|e| ServiceError::Io {
                message: format!("failed to run launchctl: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ServiceError::Load {
                message: format!("launchctl load failed: {}", stderr.trim()),
            });
        }
    } else {
        let output = std::process::Command::new("systemctl")
            .args(["--user", "enable", "--now", "turnkey-composed.service"])
            .output()
            .map_err(|e| ServiceError::Io {
                message: format!("failed to run systemctl: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ServiceError::Load {
                message: format!("systemctl enable failed: {}", stderr.trim()),
            });
        }
    }

    info!("Service loaded and started");
    Ok(())
}

/// Unload (stop) the service
pub fn unload_service() -> Result<(), ServiceError> {
    let install_path = service_install_path();

    if cfg!(target_os = "macos") {
        let _ = std::process::Command::new("launchctl")
            .args(["unload"])
            .arg(&install_path)
            .output();
    } else {
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "disable", "--now", "turnkey-composed.service"])
            .output();
    }

    Ok(())
}

#[derive(Debug)]
pub enum ServiceError {
    Io { message: String },
    Load { message: String },
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServiceError::Io { message } => write!(f, "{}", message),
            ServiceError::Load { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for ServiceError {}
