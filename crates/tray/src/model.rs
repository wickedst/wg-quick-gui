use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use wireguard_client_gui_core::Tunnel;

/// Thread-safe tray state shared with menu callbacks.
pub type SharedModel = Arc<Mutex<TrayModel>>;

/// An in-flight connection change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingOperation {
    /// Waiting for `wg-quick up`.
    Connecting,
    /// Waiting for `wg-quick down`.
    Disconnecting,
}

impl PendingOperation {
    /// Menu suffix describing this operation.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "connecting…",
            Self::Disconnecting => "disconnecting…",
        }
    }
}

/// Pure representation of a tunnel row in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelRow {
    /// Configuration name.
    pub name: String,
    /// Last observed active state.
    pub connected: bool,
    /// Current operation, if any.
    pub pending: Option<PendingOperation>,
}

/// Menu content derived from the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// Informational, disabled row.
    Message(String),
    /// Selectable tunnel row.
    Tunnel(TunnelRow),
}

/// Observable state for the tray icon and menu.
#[derive(Debug, Default)]
pub struct TrayModel {
    tunnels: Vec<Tunnel>,
    pending: HashMap<String, PendingOperation>,
    loaded: bool,
    error: Option<String>,
}

impl TrayModel {
    /// Create an empty model wrapped for tray callbacks.
    #[must_use]
    pub fn shared() -> SharedModel {
        Arc::new(Mutex::new(Self::default()))
    }

    /// Replace the last known state with a backend snapshot.
    pub fn apply_snapshot(&mut self, mut tunnels: Vec<Tunnel>) {
        tunnels.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        self.tunnels = tunnels;
        self.loaded = true;
        self.error = None;
    }

    /// Record a refresh error while preserving the last usable snapshot.
    pub fn apply_error(&mut self, error: String) {
        self.loaded = true;
        self.error = Some(error);
    }

    /// Current error, used to suppress repeated notifications.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Mark a toggle in flight and return the desired state.
    pub fn begin_toggle(&mut self, name: &str) -> Option<bool> {
        if self.pending.contains_key(name) {
            return None;
        }
        let tunnel = self.tunnels.iter().find(|tunnel| tunnel.name == name)?;
        let connected = !tunnel.connected;
        self.pending.insert(
            name.to_owned(),
            if connected {
                PendingOperation::Connecting
            } else {
                PendingOperation::Disconnecting
            },
        );
        Some(connected)
    }

    /// Clear an in-flight operation after the backend responds.
    pub fn finish_toggle(&mut self, name: &str) {
        self.pending.remove(name);
    }

    /// Names of currently active tunnels.
    #[must_use]
    pub fn active_names(&self) -> Vec<&str> {
        self.tunnels
            .iter()
            .filter(|tunnel| tunnel.connected)
            .map(|tunnel| tunnel.name.as_str())
            .collect()
    }

    /// Whether any tunnel is active.
    #[must_use]
    pub fn any_connected(&self) -> bool {
        self.tunnels.iter().any(|tunnel| tunnel.connected)
    }

    /// Human-readable tooltip.
    #[must_use]
    pub fn tooltip(&self) -> String {
        if self.error.is_some() && self.tunnels.is_empty() {
            return "Backend unavailable — select Refresh to retry".to_owned();
        }
        if !self.loaded {
            return "Loading WireGuard configurations…".to_owned();
        }
        let active = self.active_names();
        if active.is_empty() {
            "No active WireGuard tunnels".to_owned()
        } else {
            format!("Active WireGuard tunnels: {}", active.join(", "))
        }
    }

    /// Tunnel or informational entries preceding Refresh and Quit.
    #[must_use]
    pub fn menu_entries(&self) -> Vec<MenuEntry> {
        if !self.loaded {
            return vec![MenuEntry::Message("Loading…".to_owned())];
        }
        if self.tunnels.is_empty() {
            let message = if self.error.is_some() {
                "Backend unavailable"
            } else {
                "No configurations found"
            };
            return vec![MenuEntry::Message(message.to_owned())];
        }
        self.tunnels
            .iter()
            .map(|tunnel| {
                MenuEntry::Tunnel(TunnelRow {
                    name: tunnel.name.clone(),
                    connected: tunnel.connected,
                    pending: self.pending.get(&tunnel.name).copied(),
                })
            })
            .collect()
    }

    /// Test and diagnostics access to the latest snapshot.
    #[must_use]
    pub fn tunnels(&self) -> &[Tunnel] {
        &self.tunnels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TrayModel {
        let mut model = TrayModel::default();
        model.apply_snapshot(vec![Tunnel::new("zeta", false), Tunnel::new("home", true)]);
        model
    }

    #[test]
    fn initial_state_has_loading_menu_and_tooltip() {
        let model = TrayModel::default();
        assert_eq!(
            model.menu_entries(),
            [MenuEntry::Message("Loading…".to_owned())]
        );
        assert_eq!(model.tooltip(), "Loading WireGuard configurations…");
        assert!(!model.any_connected());
    }

    #[test]
    fn snapshots_are_sorted_and_drive_active_state() {
        let model = model();
        assert_eq!(model.tunnels()[0].name, "home");
        assert_eq!(model.active_names(), ["home"]);
        assert_eq!(model.tooltip(), "Active WireGuard tunnels: home");
        assert!(model.any_connected());
        assert_eq!(model.error(), None);
    }

    #[test]
    fn empty_snapshots_have_explicit_menu_and_tooltip() {
        let mut model = TrayModel::default();
        model.apply_snapshot(Vec::new());
        assert_eq!(
            model.menu_entries(),
            [MenuEntry::Message("No configurations found".to_owned())]
        );
        assert_eq!(model.tooltip(), "No active WireGuard tunnels");
    }

    #[test]
    fn errors_preserve_usable_snapshots() {
        let mut model = model();
        model.apply_error("backend offline".to_owned());
        assert_eq!(model.error(), Some("backend offline"));
        assert_eq!(model.active_names(), ["home"]);
        assert_eq!(model.menu_entries().len(), 2);

        model.apply_snapshot(Vec::new());
        assert_eq!(model.error(), None);
        model.apply_error("still offline".to_owned());
        assert_eq!(
            model.menu_entries(),
            [MenuEntry::Message("Backend unavailable".to_owned())]
        );
        assert_eq!(
            model.tooltip(),
            "Backend unavailable — select Refresh to retry"
        );
    }

    #[test]
    fn toggles_choose_the_opposite_state_and_block_duplicates() {
        let mut model = model();
        assert_eq!(model.begin_toggle("home"), Some(false));
        assert_eq!(model.begin_toggle("home"), None);
        assert_eq!(model.begin_toggle("zeta"), Some(true));
        assert_eq!(model.begin_toggle("missing"), None);

        let entries = model.menu_entries();
        assert_eq!(
            entries[0],
            MenuEntry::Tunnel(TunnelRow {
                name: "home".to_owned(),
                connected: true,
                pending: Some(PendingOperation::Disconnecting),
            })
        );
        assert_eq!(PendingOperation::Connecting.label(), "connecting…");
        assert_eq!(PendingOperation::Disconnecting.label(), "disconnecting…");

        model.finish_toggle("home");
        model.finish_toggle("home");
        assert_eq!(model.begin_toggle("home"), Some(false));
    }

    #[test]
    fn shared_model_is_thread_safe() {
        let shared = TrayModel::shared();
        shared
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", true)]);
        assert!(shared.lock().unwrap().any_connected());
    }
}
