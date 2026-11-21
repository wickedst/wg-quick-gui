use std::collections::HashSet;

use async_trait::async_trait;
use thiserror::Error;
use wireguard_client_gui_core::Tunnel;
use zbus::{proxy, Connection};

/// Failure returned by the backend client.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ClientError(pub String);

impl From<zbus::Error> for ClientError {
    fn from(error: zbus::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<zbus::fdo::Error> for ClientError {
    fn from(error: zbus::fdo::Error) -> Self {
        Self(error.to_string())
    }
}

/// Backend operations needed by the tray application.
#[async_trait]
pub trait BackendClient: Send + Sync + 'static {
    /// Obtain an atomic-enough view of configurations and active interfaces.
    async fn snapshot(&self) -> Result<Vec<Tunnel>, ClientError>;
    /// Connect or disconnect one named configuration.
    async fn set_connected(&self, name: &str, connected: bool) -> Result<(), ClientError>;
}

#[proxy(
    interface = "dev.wireguardclientgui.Backend",
    default_service = "dev.wireguardclientgui.Backend",
    default_path = "/dev/wireguardclientgui/Backend",
    gen_blocking = false
)]
pub trait BackendApi {
    fn list_configurations(&self) -> zbus::fdo::Result<Vec<String>>;
    fn get_active_interfaces(&self) -> zbus::fdo::Result<Vec<String>>;

    #[zbus(allow_interactive_auth)]
    fn connect(&self, name: &str) -> zbus::fdo::Result<()>;

    #[zbus(allow_interactive_auth)]
    fn disconnect(&self, name: &str) -> zbus::fdo::Result<()>;

    #[zbus(signal)]
    fn changed(&self) -> zbus::fdo::Result<()>;
}

/// System D-Bus backend client.
#[derive(Debug, Clone)]
pub struct ZbusBackendClient {
    connection: Connection,
}

impl ZbusBackendClient {
    /// Connect to the system bus.
    pub async fn system() -> Result<Self, ClientError> {
        Ok(Self::new(Connection::system().await?))
    }

    /// Wrap an existing bus connection.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    async fn proxy(&self) -> Result<BackendApiProxy<'_>, ClientError> {
        Ok(BackendApiProxy::new(&self.connection).await?)
    }

    /// Subscribe to successful backend mutations.
    pub async fn changed_stream(&self) -> Result<ChangedStream, ClientError> {
        Ok(self.proxy().await?.receive_changed().await?)
    }
}

#[async_trait]
impl BackendClient for ZbusBackendClient {
    async fn snapshot(&self) -> Result<Vec<Tunnel>, ClientError> {
        let proxy = self.proxy().await?;
        let (configurations, active) =
            tokio::try_join!(proxy.list_configurations(), proxy.get_active_interfaces())?;
        let active: HashSet<String> = active.into_iter().collect();
        Ok(configurations
            .into_iter()
            .map(|name| {
                let connected = active.contains(&name);
                Tunnel { name, connected }
            })
            .collect())
    }

    async fn set_connected(&self, name: &str, connected: bool) -> Result<(), ClientError> {
        let proxy = self.proxy().await?;
        if connected {
            proxy.connect(name).await?;
        } else {
            proxy.disconnect(name).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_errors_preserve_the_dbus_diagnostic() {
        let error = ClientError::from(zbus::fdo::Error::AccessDenied("denied".to_owned()));
        assert!(error.to_string().contains("denied"));
    }
}
