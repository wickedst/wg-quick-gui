use std::sync::Arc;

use crate::{BackendClient, Notifier, SharedModel};

/// User or timer action handled by the tray controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Re-read configuration and interface state.
    Refresh,
    /// Toggle one named tunnel.
    Toggle(String),
    /// Exit only the tray process.
    Quit,
}

/// Result of handling an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    /// The tray should redraw.
    Updated,
    /// The action was invalid or already in flight.
    Ignored,
    /// The application should exit.
    Quit,
}

/// Coordinates backend calls, model transitions, and error notifications.
pub struct Controller {
    client: Arc<dyn BackendClient>,
    notifier: Arc<dyn Notifier>,
    model: SharedModel,
}

impl Controller {
    /// Construct a controller.
    #[must_use]
    pub fn new(
        client: Arc<dyn BackendClient>,
        notifier: Arc<dyn Notifier>,
        model: SharedModel,
    ) -> Self {
        Self {
            client,
            notifier,
            model,
        }
    }

    /// Handle one action.
    pub async fn handle(&self, action: Action) -> ActionOutcome {
        match action {
            Action::Refresh => self.refresh().await,
            Action::Toggle(name) => self.toggle(&name).await,
            Action::Quit => ActionOutcome::Quit,
        }
    }

    /// Refresh state and notify only when a new error occurs.
    pub async fn refresh(&self) -> ActionOutcome {
        match self.client.snapshot().await {
            Ok(tunnels) => {
                self.model.lock().unwrap().apply_snapshot(tunnels);
            }
            Err(error) => {
                let message = format!("Unable to refresh WireGuard state: {error}");
                let repeated = self.model.lock().unwrap().error() == Some(message.as_str());
                self.model.lock().unwrap().apply_error(message.clone());
                if !repeated {
                    self.notifier
                        .notify("WireGuard backend unavailable", &message)
                        .await;
                }
            }
        }
        ActionOutcome::Updated
    }

    async fn toggle(&self, name: &str) -> ActionOutcome {
        let Some(connected) = self.model.lock().unwrap().begin_toggle(name) else {
            return ActionOutcome::Ignored;
        };

        let result = self.client.set_connected(name, connected).await;
        self.model.lock().unwrap().finish_toggle(name);
        if let Err(error) = result {
            let operation = if connected { "connect" } else { "disconnect" };
            self.notifier
                .notify(
                    "WireGuard connection failed",
                    &format!("Unable to {operation} {name}: {error}"),
                )
                .await;
        }

        // Re-read both lists after every mutation, including failures. This also recovers from a
        // tunnel changed outside this application while authentication was in progress.
        self.refresh().await
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex as StdMutex};

    use async_trait::async_trait;
    use tokio::sync::Mutex;
    use wireguard_client_gui_core::Tunnel;

    use super::*;
    use crate::{ClientError, TrayModel};

    struct MockClient {
        snapshots: StdMutex<VecDeque<Result<Vec<Tunnel>, ClientError>>>,
        mutations: StdMutex<VecDeque<Result<(), ClientError>>>,
        calls: StdMutex<Vec<(String, bool)>>,
    }

    impl MockClient {
        fn new() -> Self {
            Self {
                snapshots: StdMutex::new(VecDeque::new()),
                mutations: StdMutex::new(VecDeque::new()),
                calls: StdMutex::new(Vec::new()),
            }
        }

        fn with_snapshot(self, snapshot: Result<Vec<Tunnel>, ClientError>) -> Self {
            self.snapshots.lock().unwrap().push_back(snapshot);
            self
        }

        fn with_mutation(self, result: Result<(), ClientError>) -> Self {
            self.mutations.lock().unwrap().push_back(result);
            self
        }
    }

    #[async_trait]
    impl BackendClient for MockClient {
        async fn snapshot(&self) -> Result<Vec<Tunnel>, ClientError> {
            self.snapshots
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing mock snapshot")
        }

        async fn set_connected(&self, name: &str, connected: bool) -> Result<(), ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_owned(), connected));
            self.mutations
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing mock mutation")
        }
    }

    #[derive(Default)]
    struct MockNotifier {
        messages: Mutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl Notifier for MockNotifier {
        async fn notify(&self, summary: &str, body: &str) {
            self.messages
                .lock()
                .await
                .push((summary.to_owned(), body.to_owned()));
        }
    }

    struct Fixture {
        controller: Controller,
        client: Arc<MockClient>,
        notifier: Arc<MockNotifier>,
        model: SharedModel,
    }

    fn fixture(client: MockClient) -> Fixture {
        let client = Arc::new(client);
        let notifier = Arc::new(MockNotifier::default());
        let model = TrayModel::shared();
        let controller = Controller::new(client.clone(), notifier.clone(), model.clone());
        Fixture {
            controller,
            client,
            notifier,
            model,
        }
    }

    #[tokio::test]
    async fn refresh_applies_a_snapshot() {
        let fixture = fixture(MockClient::new().with_snapshot(Ok(vec![Tunnel::new("home", true)])));
        assert_eq!(
            fixture.controller.handle(Action::Refresh).await,
            ActionOutcome::Updated
        );
        assert_eq!(fixture.model.lock().unwrap().active_names(), ["home"]);
        assert!(fixture.notifier.messages.lock().await.is_empty());
    }

    #[tokio::test]
    async fn repeated_refresh_errors_notify_only_once() {
        let fixture = fixture(
            MockClient::new()
                .with_snapshot(Err(ClientError("offline".to_owned())))
                .with_snapshot(Err(ClientError("offline".to_owned()))),
        );
        fixture.controller.refresh().await;
        fixture.controller.refresh().await;
        assert_eq!(fixture.notifier.messages.lock().await.len(), 1);
        assert_eq!(
            fixture.model.lock().unwrap().error(),
            Some("Unable to refresh WireGuard state: offline")
        );
    }

    #[tokio::test]
    async fn successful_connect_mutates_and_refreshes() {
        let fixture = fixture(
            MockClient::new()
                .with_mutation(Ok(()))
                .with_snapshot(Ok(vec![Tunnel::new("home", true)])),
        );
        fixture
            .model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", false)]);
        assert_eq!(
            fixture
                .controller
                .handle(Action::Toggle("home".to_owned()))
                .await,
            ActionOutcome::Updated
        );
        assert_eq!(
            *fixture.client.calls.lock().unwrap(),
            [("home".to_owned(), true)]
        );
        assert!(fixture.model.lock().unwrap().any_connected());
    }

    #[tokio::test]
    async fn successful_disconnect_mutates_and_refreshes() {
        let fixture = fixture(
            MockClient::new()
                .with_mutation(Ok(()))
                .with_snapshot(Ok(vec![Tunnel::new("home", false)])),
        );
        fixture
            .model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", true)]);
        fixture
            .controller
            .handle(Action::Toggle("home".to_owned()))
            .await;
        assert_eq!(
            *fixture.client.calls.lock().unwrap(),
            [("home".to_owned(), false)]
        );
        assert!(!fixture.model.lock().unwrap().any_connected());
    }

    #[tokio::test]
    async fn failed_mutations_clear_pending_state_and_notify() {
        let fixture = fixture(
            MockClient::new()
                .with_mutation(Err(ClientError("denied".to_owned())))
                .with_snapshot(Ok(vec![Tunnel::new("home", false)])),
        );
        fixture
            .model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", false)]);
        fixture
            .controller
            .handle(Action::Toggle("home".to_owned()))
            .await;
        let notifications = fixture.notifier.messages.lock().await;
        assert_eq!(notifications.len(), 1);
        assert!(notifications[0].1.contains("denied"));
        drop(notifications);
        assert_eq!(
            fixture.model.lock().unwrap().begin_toggle("home"),
            Some(true)
        );
    }

    #[tokio::test]
    async fn duplicate_and_unknown_toggles_are_ignored() {
        let fixture = fixture(MockClient::new());
        fixture
            .model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", false)]);
        {
            let mut model = fixture.model.lock().unwrap();
            assert_eq!(model.begin_toggle("home"), Some(true));
        }
        assert_eq!(
            fixture
                .controller
                .handle(Action::Toggle("home".to_owned()))
                .await,
            ActionOutcome::Ignored
        );
        assert_eq!(
            fixture
                .controller
                .handle(Action::Toggle("missing".to_owned()))
                .await,
            ActionOutcome::Ignored
        );
        assert!(fixture.client.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn quit_does_not_touch_backend_or_tunnels() {
        let fixture = fixture(MockClient::new());
        fixture
            .model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", true)]);
        assert_eq!(
            fixture.controller.handle(Action::Quit).await,
            ActionOutcome::Quit
        );
        assert!(fixture.model.lock().unwrap().any_connected());
        assert!(fixture.client.calls.lock().unwrap().is_empty());
    }
}
