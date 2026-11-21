//! StatusNotifierItem tray client for `wg-quick`.

mod app;
mod client;
mod controller;
mod model;
mod notifier;
mod theme;
mod tray;

pub use app::{run, TrayError};
pub use client::{claim_tray_instance, BackendClient, ClientError, ZbusBackendClient};
pub use controller::{Action, ActionOutcome, Controller};
pub use model::{MenuEntry, PendingOperation, SharedModel, TrayModel, TunnelRow};
pub use notifier::{FdoNotifier, Notifier, StderrNotifier};
pub use theme::{parse_theme_args, Theme};
pub use tray::{wireguard_icons, WireguardTray};
