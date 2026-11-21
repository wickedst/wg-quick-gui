//! Shared, toolkit-independent WireGuard operations.

use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

use thiserror::Error;
use tokio::{process::Command, time::timeout};

/// System bus name owned by the privileged backend.
pub const DBUS_SERVICE: &str = "dev.wireguardclientgui.Backend";
/// Object path exposing the privileged backend.
pub const DBUS_PATH: &str = "/dev/wireguardclientgui/Backend";
/// Versioned backend interface.
pub const DBUS_INTERFACE: &str = "dev.wireguardclientgui.Backend";
/// Polkit action required for connection changes.
pub const POLKIT_ACTION: &str = "dev.wireguardclientgui.manage";

const MAX_INTERFACE_NAME_BYTES: usize = 15;

/// A configuration and its current kernel interface state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tunnel {
    /// Interface name, which is also the configuration filename without `.conf`.
    pub name: String,
    /// Whether a WireGuard interface with this name currently exists.
    pub connected: bool,
}

impl Tunnel {
    /// Construct a tunnel state value.
    #[must_use]
    pub fn new(name: impl Into<String>, connected: bool) -> Self {
        Self {
            name: name.into(),
            connected,
        }
    }
}

/// Errors produced while inspecting or changing WireGuard state.
#[derive(Debug, Error)]
pub enum WireguardError {
    /// The supplied name is not safe or valid as a WireGuard interface name.
    #[error("invalid WireGuard interface name: {0:?}")]
    InvalidName(String),
    /// The requested configuration does not exist or is not a regular file.
    #[error("WireGuard configuration not found: {0}")]
    ConfigNotFound(String),
    /// The configuration directory could not be read.
    #[error("failed to read WireGuard configuration directory {path}: {source}")]
    ReadDirectory {
        /// Directory being read.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A command could not be started or waited for.
    #[error("failed to execute {program}: {source}")]
    Execute {
        /// Executable that failed.
        program: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// A command returned an unsuccessful exit status.
    #[error("{operation} failed{status}: {stderr}", status = exit_status(.code))]
    CommandFailed {
        /// Human-readable operation.
        operation: String,
        /// Numeric exit status, if available.
        code: Option<i32>,
        /// Standard error, or standard output when standard error is empty.
        stderr: String,
    },
    /// A command exceeded its configured deadline and was killed.
    #[error("{operation} timed out after {} seconds", .duration.as_secs())]
    Timeout {
        /// Human-readable operation.
        operation: String,
        /// Configured command deadline.
        duration: Duration,
    },
}

fn exit_status(code: &Option<i32>) -> String {
    code.map_or_else(
        || " (terminated by signal)".to_owned(),
        |code| format!(" with exit status {code}"),
    )
}

/// Validate a WireGuard interface/configuration name.
///
/// Names are deliberately limited to the characters accepted by WireGuard tooling and to Linux's
/// 15-byte interface-name limit. Paths, whitespace, shell metacharacters, and empty names are not
/// accepted.
#[must_use]
pub fn is_valid_interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_INTERFACE_NAME_BYTES
        && name != "."
        && name != ".."
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'=' | b'+' | b'.' | b'-')
        })
}

/// Safe interface discovery and `wg-quick` execution.
#[derive(Debug, Clone)]
pub struct Wireguard {
    config_dir: PathBuf,
    wg_quick: PathBuf,
    wg: PathBuf,
    command_timeout: Duration,
}

impl Wireguard {
    /// Construct a runner using explicit paths and a command deadline.
    #[must_use]
    pub fn new(
        config_dir: impl Into<PathBuf>,
        wg_quick: impl Into<PathBuf>,
        wg: impl Into<PathBuf>,
        command_timeout: Duration,
    ) -> Self {
        Self {
            config_dir: config_dir.into(),
            wg_quick: wg_quick.into(),
            wg: wg.into(),
            command_timeout,
        }
    }

    /// List valid configurations directly inside the configured directory.
    ///
    /// Symlinks to regular files are followed, which supports NixOS `/etc` links. Subdirectories,
    /// non-UTF-8 filenames, non-`.conf` files, and invalid interface names are ignored.
    pub fn list_configurations(&self) -> Result<Vec<String>, WireguardError> {
        let entries = std::fs::read_dir(&self.config_dir).map_err(|source| {
            WireguardError::ReadDirectory {
                path: self.config_dir.clone(),
                source,
            }
        })?;

        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| WireguardError::ReadDirectory {
                path: self.config_dir.clone(),
                source,
            })?;
            let path = entry.path();
            if path.extension() != Some(OsStr::new("conf")) || !path.is_file() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if is_valid_interface_name(name) {
                names.push(name.to_owned());
            }
        }
        names.sort_unstable();
        names.dedup();
        Ok(names)
    }

    /// Return active WireGuard interface names reported by `wg show interfaces`.
    pub async fn active_interfaces(&self) -> Result<Vec<String>, WireguardError> {
        let output = self
            .run_command(
                &self.wg,
                &[OsStr::new("show"), OsStr::new("interfaces")],
                "query active WireGuard interfaces",
            )
            .await?;
        let mut interfaces: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .filter(|name| is_valid_interface_name(name))
            .map(str::to_owned)
            .collect();
        interfaces.sort_unstable();
        interfaces.dedup();
        Ok(interfaces)
    }

    /// Return all valid configurations with their active state.
    pub async fn tunnels(&self) -> Result<Vec<Tunnel>, WireguardError> {
        let names = self.list_configurations()?;
        let active: HashSet<String> = self.active_interfaces().await?.into_iter().collect();
        Ok(names
            .into_iter()
            .map(|name| {
                let connected = active.contains(&name);
                Tunnel { name, connected }
            })
            .collect())
    }

    /// Bring a configuration up using `wg-quick up`.
    pub async fn connect(&self, name: &str) -> Result<(), WireguardError> {
        self.run_wg_quick(name, "up").await
    }

    /// Bring a configuration down using `wg-quick down`.
    pub async fn disconnect(&self, name: &str) -> Result<(), WireguardError> {
        self.run_wg_quick(name, "down").await
    }

    /// Resolve a validated configuration name without allowing path traversal.
    pub fn config_path(&self, name: &str) -> Result<PathBuf, WireguardError> {
        if !is_valid_interface_name(name) {
            return Err(WireguardError::InvalidName(name.to_owned()));
        }
        let path = self.config_dir.join(format!("{name}.conf"));
        if !path.is_file() {
            return Err(WireguardError::ConfigNotFound(name.to_owned()));
        }
        Ok(path)
    }

    async fn run_wg_quick(&self, name: &str, action: &str) -> Result<(), WireguardError> {
        let path = self.config_path(name)?;
        let operation = format!("wg-quick {action} {name}");
        self.run_command(
            &self.wg_quick,
            &[OsStr::new(action), path.as_os_str()],
            &operation,
        )
        .await
        .map(|_| ())
    }

    async fn run_command(
        &self,
        program: &Path,
        args: &[&OsStr],
        operation: &str,
    ) -> Result<Output, WireguardError> {
        let command = async {
            Command::new(program)
                .args(args)
                .kill_on_drop(true)
                .output()
                .await
                .map_err(|source| WireguardError::Execute {
                    program: program.to_path_buf(),
                    source,
                })
        };
        let output = timeout(self.command_timeout, command).await.map_err(|_| {
            WireguardError::Timeout {
                operation: operation.to_owned(),
                duration: self.command_timeout,
            }
        })??;
        if output.status.success() {
            Ok(output)
        } else {
            let stderr = if output.stderr.is_empty() {
                output.stdout.clone()
            } else {
                output.stderr.clone()
            };
            let stderr = String::from_utf8_lossy(&stderr).trim().to_owned();
            Err(WireguardError::CommandFailed {
                operation: operation.to_owned(),
                code: output.status.code(),
                stderr: if stderr.is_empty() {
                    "no diagnostic output".to_owned()
                } else {
                    stderr
                },
            })
        }
    }
}

#[cfg(test)]
mod tests;
