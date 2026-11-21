use std::{error::Error, sync::Arc};

use wireguard_client_gui_backend::{serve, Backend, BackendConfig, PolicyKit};
use zbus::Connection;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("wireguard-client-gui-backend {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let config = BackendConfig::from_env()?;
    let connection = Connection::system().await?;
    let authorizer = Arc::new(PolicyKit::new(connection.clone()));
    let backend = Backend::new(config.wireguard(), authorizer);
    let _connection = serve(backend, connection).await?;
    std::future::pending::<()>().await;
    Ok(())
}
