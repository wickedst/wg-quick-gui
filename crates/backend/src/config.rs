use std::{ffi::OsString, path::PathBuf, time::Duration};

use thiserror::Error;
use wireguard_client_gui_core::Wireguard;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;

/// Runtime paths and limits for the privileged process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendConfig {
    /// Directory containing WireGuard configurations.
    pub config_dir: PathBuf,
    /// `wg-quick` executable.
    pub wg_quick: PathBuf,
    /// `wg` executable.
    pub wg: PathBuf,
    /// Maximum duration of each external command.
    pub command_timeout: Duration,
}

/// Invalid backend configuration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    /// The timeout was not a positive integer.
    #[error("WIREGUARD_CLIENT_GUI_TIMEOUT_SECONDS must be a positive integer, got {0:?}")]
    InvalidTimeout(String),
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            config_dir: PathBuf::from("/etc/wireguard"),
            wg_quick: PathBuf::from("wg-quick"),
            wg: PathBuf::from("wg"),
            command_timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECONDS),
        }
    }
}

impl BackendConfig {
    /// Read configuration from the process environment.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var_os(key))
    }

    /// Read configuration using a deterministic lookup function.
    pub fn from_lookup<F>(mut lookup: F) -> Result<Self, ConfigError>
    where
        F: FnMut(&str) -> Option<OsString>,
    {
        let defaults = Self::default();
        let timeout = match lookup("WIREGUARD_CLIENT_GUI_TIMEOUT_SECONDS") {
            Some(value) => {
                let value = value.to_string_lossy();
                let seconds = value
                    .parse::<u64>()
                    .ok()
                    .filter(|seconds| *seconds > 0)
                    .ok_or_else(|| ConfigError::InvalidTimeout(value.into_owned()))?;
                Duration::from_secs(seconds)
            }
            None => defaults.command_timeout,
        };
        Ok(Self {
            config_dir: lookup("WIREGUARD_CLIENT_GUI_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or(defaults.config_dir),
            wg_quick: lookup("WIREGUARD_CLIENT_GUI_WG_QUICK")
                .map(PathBuf::from)
                .unwrap_or(defaults.wg_quick),
            wg: lookup("WIREGUARD_CLIENT_GUI_WG")
                .map(PathBuf::from)
                .unwrap_or(defaults.wg),
            command_timeout: timeout,
        })
    }

    /// Build the shared WireGuard runner.
    #[must_use]
    pub fn wireguard(&self) -> Wireguard {
        Wireguard::new(
            &self.config_dir,
            &self.wg_quick,
            &self.wg,
            self.command_timeout,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn defaults_are_secure_and_conventional() {
        let config = BackendConfig::from_lookup(|_| None).unwrap();
        assert_eq!(config, BackendConfig::default());
        assert_eq!(config.config_dir, PathBuf::from("/etc/wireguard"));
        assert_eq!(config.command_timeout, Duration::from_secs(60));
    }

    #[test]
    fn environment_values_override_defaults() {
        let values = HashMap::from([
            (
                "WIREGUARD_CLIENT_GUI_CONFIG_DIR",
                OsString::from("/tmp/configs"),
            ),
            (
                "WIREGUARD_CLIENT_GUI_WG_QUICK",
                OsString::from("/bin/wg-quick"),
            ),
            ("WIREGUARD_CLIENT_GUI_WG", OsString::from("/bin/wg")),
            ("WIREGUARD_CLIENT_GUI_TIMEOUT_SECONDS", OsString::from("12")),
        ]);
        let config = BackendConfig::from_lookup(|key| values.get(key).cloned()).unwrap();
        assert_eq!(config.config_dir, PathBuf::from("/tmp/configs"));
        assert_eq!(config.wg_quick, PathBuf::from("/bin/wg-quick"));
        assert_eq!(config.wg, PathBuf::from("/bin/wg"));
        assert_eq!(config.command_timeout, Duration::from_secs(12));
        let _runner = config.wireguard();
    }

    #[test]
    fn invalid_timeouts_are_rejected() {
        for invalid in ["0", "-1", "abc", ""] {
            let error = BackendConfig::from_lookup(|key| {
                (key == "WIREGUARD_CLIENT_GUI_TIMEOUT_SECONDS").then(|| OsString::from(invalid))
            })
            .unwrap_err();
            assert_eq!(error, ConfigError::InvalidTimeout(invalid.to_owned()));
            assert!(error.to_string().contains("positive integer"));
        }
    }
}
