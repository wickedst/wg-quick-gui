use std::collections::HashMap;

use async_trait::async_trait;
use zbus::{proxy, zvariant::Value, Connection};

/// Desktop notification abstraction.
#[async_trait]
pub trait Notifier: Send + Sync + 'static {
    /// Display an error notification. Delivery failures are intentionally non-fatal.
    async fn notify(&self, summary: &str, body: &str);
}

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications",
    gen_blocking = false
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::fdo::Result<u32>;
}

/// Last-resort notifier used when no session notification service is available.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrNotifier;

#[async_trait]
impl Notifier for StderrNotifier {
    async fn notify(&self, summary: &str, body: &str) {
        eprintln!("{summary}: {body}");
    }
}

/// Freedesktop notification client.
#[derive(Debug, Clone)]
pub struct FdoNotifier {
    connection: Connection,
}

impl FdoNotifier {
    /// Wrap an existing session-bus connection.
    #[must_use]
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Connect to the user's session bus.
    pub async fn session() -> zbus::Result<Self> {
        Ok(Self {
            connection: Connection::session().await?,
        })
    }

    async fn send(&self, summary: &str, body: &str) -> zbus::Result<()> {
        let proxy = NotificationsProxy::new(&self.connection).await?;
        proxy
            .notify(
                "WireGuard Client",
                0,
                "network-vpn",
                summary,
                body,
                Vec::new(),
                HashMap::new(),
                -1,
            )
            .await?;
        Ok(())
    }
}

#[async_trait]
impl Notifier for FdoNotifier {
    async fn notify(&self, summary: &str, body: &str) {
        if let Err(error) = self.send(summary, body).await {
            eprintln!("Unable to display desktop notification: {error}");
        }
    }
}
