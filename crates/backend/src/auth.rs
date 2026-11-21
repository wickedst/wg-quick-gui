use std::collections::HashMap;

use async_trait::async_trait;
use thiserror::Error;
use wireguard_client_gui_core::POLKIT_ACTION;
use zbus::{zvariant::Value, Connection};

const POLKIT_SERVICE: &str = "org.freedesktop.PolicyKit1";
const POLKIT_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const POLKIT_INTERFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const ALLOW_USER_INTERACTION: u32 = 1;

/// Authorization failure for a privileged operation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthorizationError {
    /// D-Bus did not supply a unique sender name.
    #[error("the request did not include a D-Bus sender")]
    MissingSender,
    /// Polkit could not complete the check.
    #[error("polkit authorization check failed: {0}")]
    CheckFailed(String),
    /// The caller was not authorized.
    #[error("administrator authorization is required to manage WireGuard connections")]
    Denied,
}

/// Authorization abstraction used by the D-Bus service.
#[async_trait]
pub trait Authorizer: Send + Sync + 'static {
    /// Authorize a unique D-Bus bus name.
    async fn authorize(&self, sender: Option<&str>) -> Result<(), AuthorizationError>;
}

/// Polkit-backed authorizer.
#[derive(Debug, Clone)]
pub struct PolicyKit {
    connection: Connection,
    action_id: String,
}

impl PolicyKit {
    /// Use the standard WireGuard management action.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self::with_action(connection, POLKIT_ACTION)
    }

    /// Use an explicit action, primarily for protocol tests.
    #[must_use]
    pub fn with_action(connection: Connection, action_id: impl Into<String>) -> Self {
        Self {
            connection,
            action_id: action_id.into(),
        }
    }
}

/// Validate the caller and polkit response separately from transport.
pub fn authorization_decision(
    sender: Option<&str>,
    authorized: bool,
) -> Result<(), AuthorizationError> {
    if sender.is_none() {
        return Err(AuthorizationError::MissingSender);
    }
    if authorized {
        Ok(())
    } else {
        Err(AuthorizationError::Denied)
    }
}

#[async_trait]
impl Authorizer for PolicyKit {
    async fn authorize(&self, sender: Option<&str>) -> Result<(), AuthorizationError> {
        let sender = sender.ok_or(AuthorizationError::MissingSender)?;
        let subject = (
            "system-bus-name",
            HashMap::from([("name", Value::from(sender))]),
        );
        let details: HashMap<&str, &str> = HashMap::new();
        let message = self
            .connection
            .call_method(
                Some(POLKIT_SERVICE),
                POLKIT_PATH,
                Some(POLKIT_INTERFACE),
                "CheckAuthorization",
                &(
                    subject,
                    self.action_id.as_str(),
                    details,
                    ALLOW_USER_INTERACTION,
                    "",
                ),
            )
            .await
            .map_err(|error| AuthorizationError::CheckFailed(error.to_string()))?;
        let (authorized, _is_challenge, _details): (bool, bool, HashMap<String, String>) = message
            .body()
            .deserialize()
            .map_err(|error| AuthorizationError::CheckFailed(error.to_string()))?;
        authorization_decision(Some(sender), authorized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_cover_authorized_denied_and_missing_senders() {
        assert_eq!(authorization_decision(Some(":1.2"), true), Ok(()));
        assert_eq!(
            authorization_decision(Some(":1.2"), false),
            Err(AuthorizationError::Denied)
        );
        assert_eq!(
            authorization_decision(None, true),
            Err(AuthorizationError::MissingSender)
        );
        assert_eq!(
            authorization_decision(None, false),
            Err(AuthorizationError::MissingSender)
        );
        assert!(AuthorizationError::Denied
            .to_string()
            .contains("administrator"));
        assert!(AuthorizationError::CheckFailed("failure".to_owned())
            .to_string()
            .contains("failure"));
        assert!(AuthorizationError::MissingSender
            .to_string()
            .contains("sender"));
    }
}
