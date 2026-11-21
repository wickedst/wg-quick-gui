//! Privileged D-Bus service for constrained `wg-quick` operations.

mod auth;
mod config;
mod service;

pub use auth::{authorization_decision, AuthorizationError, Authorizer, PolicyKit};
pub use config::{BackendConfig, ConfigError};
pub use service::{serve, Backend, BackendSignals};
