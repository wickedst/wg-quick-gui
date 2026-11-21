use std::{error::Error, sync::Arc};

use wireguard_client_gui::{
    parse_theme_args, run, FdoNotifier, Notifier, StderrNotifier, ZbusBackendClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|argument| argument == "--version") {
        println!("wireguard-client-gui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let theme = match parse_theme_args(std::env::args()) {
        Ok(theme) => theme,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };

    let client = ZbusBackendClient::system().await;
    let notifier: Arc<dyn Notifier> = match FdoNotifier::session().await {
        Ok(notifier) => Arc::new(notifier),
        Err(error) => {
            eprintln!("Desktop notifications unavailable: {error}");
            Arc::new(StderrNotifier)
        }
    };
    run(client, notifier, theme).await?;
    Ok(())
}
