use std::{error::Error, sync::Arc};

use wireguard_client_gui::{run, FdoNotifier, Notifier, StderrNotifier, ZbusBackendClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("wireguard-client-gui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let client = ZbusBackendClient::system().await?;
    let notifier: Arc<dyn Notifier> = match FdoNotifier::session().await {
        Ok(notifier) => Arc::new(notifier),
        Err(error) => {
            eprintln!("Desktop notifications unavailable: {error}");
            Arc::new(StderrNotifier)
        }
    };
    run(client, notifier).await?;
    Ok(())
}
