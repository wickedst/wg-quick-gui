//! StatusNotifierItem tray client for `wg-quick`.

mod app;
mod client;
mod controller;
mod model;
mod notifier;
mod tray;

pub use app::{run, TrayError};
pub use client::{BackendClient, ClientError, ZbusBackendClient};
pub use controller::{Action, ActionOutcome, Controller};
pub use model::{MenuEntry, PendingOperation, SharedModel, TrayModel, TunnelRow};
pub use notifier::{FdoNotifier, Notifier, StderrNotifier};
pub use tray::{status_icon, WireguardTray};
