use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::{Stream, StreamExt};
use ksni::TrayMethods;
use thiserror::Error;
use tokio::{
    sync::mpsc,
    time::{interval_at, Instant},
};

use crate::{
    Action, ActionOutcome, ClientError, Controller, Notifier, SharedModel, TrayModel,
    WireguardTray, ZbusBackendClient,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

/// Fatal tray startup or runtime errors.
#[derive(Debug, Error)]
pub enum TrayError {
    /// System backend connection failed.
    #[error("unable to connect to the WireGuard backend: {0}")]
    Client(#[from] ClientError),
    /// StatusNotifierItem service failed.
    #[error("unable to start the system tray item: {0}")]
    Tray(#[from] ksni::Error),
    /// Ctrl-C handling failed.
    #[error("unable to listen for process termination: {0}")]
    Signal(#[from] std::io::Error),
}

#[async_trait]
trait TrayHandle: Send + 'static {
    async fn update(&mut self);
    async fn shutdown(&mut self);
}

struct KsniTrayHandle(ksni::Handle<WireguardTray>);

#[async_trait]
impl TrayHandle for KsniTrayHandle {
    async fn update(&mut self) {
        self.0.update(|_| ()).await;
    }

    async fn shutdown(&mut self) {
        self.0.shutdown().await;
    }
}

/// Run the tray until Quit or Ctrl-C. Existing tunnels are never changed during shutdown.
pub async fn run(client: ZbusBackendClient, notifier: Arc<dyn Notifier>) -> Result<(), TrayError> {
    let changed = client
        .changed_stream()
        .await
        .ok()
        .map(|stream| stream.map(|_| ()));
    let model = TrayModel::shared();
    let (actions, receiver) = mpsc::unbounded_channel();
    let tray = WireguardTray::new(model.clone(), actions.clone());
    let handle = tray.assume_sni_available(true).spawn().await?;
    run_with_handle(
        Arc::new(client),
        notifier,
        model,
        actions,
        receiver,
        KsniTrayHandle(handle),
        changed,
        REFRESH_INTERVAL,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_with_handle<S, H>(
    client: Arc<dyn crate::BackendClient>,
    notifier: Arc<dyn Notifier>,
    model: SharedModel,
    actions: mpsc::UnboundedSender<Action>,
    mut receiver: mpsc::UnboundedReceiver<Action>,
    mut handle: H,
    changed: Option<S>,
    refresh_interval: Duration,
) -> Result<(), TrayError>
where
    S: Stream<Item = ()> + Unpin + Send + 'static,
    H: TrayHandle,
{
    let controller = Controller::new(client, notifier, model);
    let mut changed = changed;
    let change_actions = actions.clone();
    let change_task = tokio::spawn(async move {
        if let Some(stream) = changed.as_mut() {
            while stream.next().await.is_some() {
                if change_actions.send(Action::Refresh).is_err() {
                    break;
                }
            }
        }
    });

    actions
        .send(Action::Refresh)
        .expect("new action receiver should be open");
    let signal_result = {
        let event_loop = async {
            let mut timer = interval_at(Instant::now() + refresh_interval, refresh_interval);
            loop {
                let action = tokio::select! {
                    action = receiver.recv() => action,
                    _ = timer.tick() => Some(Action::Refresh),
                };
                let Some(action) = action else {
                    break;
                };
                let outcome = controller.handle(action).await;
                handle.update().await;
                if outcome == ActionOutcome::Quit {
                    break;
                }
            }
        };

        tokio::pin!(event_loop);
        tokio::select! {
            () = &mut event_loop => Ok(()),
            result = tokio::signal::ctrl_c() => result,
        }
    };

    change_task.abort();
    let _ = change_task.await;
    handle.shutdown().await;
    signal_result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use wireguard_client_gui_core::Tunnel;

    use super::*;
    use crate::{BackendClient, StderrNotifier};

    #[derive(Default)]
    struct MockHandle {
        updates: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl TrayHandle for MockHandle {
        async fn update(&mut self) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }

        async fn shutdown(&mut self) {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Default)]
    struct StaticClient {
        snapshots: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl BackendClient for StaticClient {
        async fn snapshot(&self) -> Result<Vec<Tunnel>, ClientError> {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        async fn set_connected(&self, _name: &str, _connected: bool) -> Result<(), ClientError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn lifecycle_processes_refresh_signals_and_quit() {
        let model = TrayModel::shared();
        let (actions, receiver) = mpsc::unbounded_channel();
        let quit_actions = actions.clone();
        let quit_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            quit_actions.send(Action::Quit).unwrap();
        });
        let client = Arc::new(StaticClient::default());
        let handle = MockHandle::default();
        let updates = handle.updates.clone();
        let shutdowns = handle.shutdowns.clone();
        let snapshots = client.snapshots.clone();

        run_with_handle(
            client,
            Arc::new(StderrNotifier),
            model,
            actions,
            receiver,
            handle,
            Some(Box::pin(futures_util::stream::once(async {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }))),
            Duration::from_secs(60),
        )
        .await
        .unwrap();

        quit_task.await.unwrap();
        assert!(snapshots.load(Ordering::SeqCst) >= 2);
        assert!(updates.load(Ordering::SeqCst) >= 2);
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn lifecycle_refreshes_on_its_timer() {
        let model = TrayModel::shared();
        let (actions, receiver) = mpsc::unbounded_channel();
        let quit_actions = actions.clone();
        let quit_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(25)).await;
            quit_actions.send(Action::Quit).unwrap();
        });
        let client = Arc::new(StaticClient::default());
        let snapshots = client.snapshots.clone();

        run_with_handle(
            client,
            Arc::new(StderrNotifier),
            model,
            actions,
            receiver,
            MockHandle::default(),
            None::<futures_util::stream::Empty<()>>,
            Duration::from_millis(5),
        )
        .await
        .unwrap();

        quit_task.await.unwrap();
        assert!(snapshots.load(Ordering::SeqCst) >= 2);
    }
}
