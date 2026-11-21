use std::sync::Arc;

use tokio::sync::Mutex;
use wireguard_client_gui_core::{
    Wireguard, WireguardError, DBUS_INTERFACE, DBUS_PATH, DBUS_SERVICE,
};
use zbus::{fdo, interface, message::Header, object_server::SignalEmitter, Connection};

use crate::{AuthorizationError, Authorizer};

/// Constrained backend D-Bus API.
#[derive(Clone)]
pub struct Backend {
    wireguard: Arc<Wireguard>,
    authorizer: Arc<dyn Authorizer>,
    operation_lock: Arc<Mutex<()>>,
}

impl Backend {
    /// Construct a backend from a runner and authorization policy.
    #[must_use]
    pub fn new(wireguard: Wireguard, authorizer: Arc<dyn Authorizer>) -> Self {
        Self {
            wireguard: Arc::new(wireguard),
            authorizer,
            operation_lock: Arc::new(Mutex::new(())),
        }
    }

    /// List configurations without exposing their contents.
    pub fn list_configurations_inner(&self) -> fdo::Result<Vec<String>> {
        self.wireguard
            .list_configurations()
            .map_err(operation_error)
    }

    /// Query active interfaces without requiring authorization.
    pub async fn active_interfaces_inner(&self) -> fdo::Result<Vec<String>> {
        self.wireguard
            .active_interfaces()
            .await
            .map_err(operation_error)
    }

    /// Connect after authorizing the unique D-Bus sender.
    pub async fn connect_for_sender(&self, name: &str, sender: Option<&str>) -> fdo::Result<()> {
        self.change_connection(name, sender, true).await
    }

    /// Disconnect after authorizing the unique D-Bus sender.
    pub async fn disconnect_for_sender(&self, name: &str, sender: Option<&str>) -> fdo::Result<()> {
        self.change_connection(name, sender, false).await
    }

    async fn change_connection(
        &self,
        name: &str,
        sender: Option<&str>,
        connect: bool,
    ) -> fdo::Result<()> {
        let sender = sender.ok_or_else(|| auth_error(AuthorizationError::MissingSender))?;
        self.authorizer
            .authorize(Some(sender))
            .await
            .map_err(auth_error)?;
        let _operation = self.operation_lock.lock().await;
        if connect {
            self.wireguard.connect(name).await.map_err(operation_error)
        } else {
            self.wireguard
                .disconnect(name)
                .await
                .map_err(operation_error)
        }
    }
}

fn operation_error(error: WireguardError) -> fdo::Error {
    fdo::Error::Failed(error.to_string())
}

fn auth_error(error: AuthorizationError) -> fdo::Error {
    match error {
        AuthorizationError::CheckFailed(message) => fdo::Error::Failed(message),
        error @ (AuthorizationError::MissingSender | AuthorizationError::Denied) => {
            fdo::Error::AccessDenied(error.to_string())
        }
    }
}

#[interface(name = "dev.wireguardclientgui.Backend")]
impl Backend {
    /// Return valid configuration names.
    async fn list_configurations(&self) -> fdo::Result<Vec<String>> {
        self.list_configurations_inner()
    }

    /// Return active WireGuard interface names.
    async fn get_active_interfaces(&self) -> fdo::Result<Vec<String>> {
        self.active_interfaces_inner().await
    }

    /// Run `wg-quick up` for an authorized caller.
    async fn connect(
        &self,
        name: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.connect_for_sender(name, header.sender().map(|name| name.as_str()))
            .await?;
        BackendSignals::changed(&emitter).await?;
        Ok(())
    }

    /// Run `wg-quick down` for an authorized caller.
    async fn disconnect(
        &self,
        name: &str,
        #[zbus(header)] header: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.disconnect_for_sender(name, header.sender().map(|name| name.as_str()))
            .await?;
        BackendSignals::changed(&emitter).await?;
        Ok(())
    }

    /// Emitted after a successful connection change.
    #[zbus(signal)]
    async fn changed(signal_emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Register the API and own its well-known name on an existing system-bus connection.
pub async fn serve(backend: Backend, connection: Connection) -> zbus::Result<Connection> {
    connection.object_server().at(DBUS_PATH, backend).await?;
    connection.request_name(DBUS_SERVICE).await?;
    Ok(connection)
}

/// Keep compile-time checks on the public bus constants close to their use.
const _: &str = DBUS_INTERFACE;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        time::Duration,
    };

    use async_trait::async_trait;
    use tempfile::TempDir;

    use super::*;

    #[derive(Debug, Clone, Copy)]
    enum Decision {
        Allow,
        Deny,
        Fail,
    }

    struct TestAuthorizer {
        decision: Decision,
        callers: Mutex<Vec<Option<String>>>,
    }

    impl TestAuthorizer {
        fn new(decision: Decision) -> Self {
            Self {
                decision,
                callers: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl Authorizer for TestAuthorizer {
        async fn authorize(&self, sender: Option<&str>) -> Result<(), AuthorizationError> {
            self.callers.lock().await.push(sender.map(str::to_owned));
            match self.decision {
                Decision::Allow => Ok(()),
                Decision::Deny => Err(AuthorizationError::Denied),
                Decision::Fail => Err(AuthorizationError::CheckFailed("test failure".to_owned())),
            }
        }
    }

    fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        let shell = std::env::var("WIREGUARD_CLIENT_GUI_TEST_SHELL")
            .unwrap_or_else(|_| "/bin/sh".to_owned());
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(format!("#!{shell}\n{body}\n").as_bytes())
            .unwrap();
        file.sync_all().unwrap();
        drop(file);
        std::thread::sleep(Duration::from_millis(10));
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    fn setup(body: &str) -> (TempDir, Wireguard) {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("home.conf"), "[Interface]\n").unwrap();
        let executable = script(temp.path(), "tool", body);
        let wireguard = Wireguard::new(
            temp.path(),
            &executable,
            &executable,
            Duration::from_secs(2),
        );
        (temp, wireguard)
    }

    #[test]
    fn list_configurations_does_not_require_authorization() {
        let (_temp, wireguard) = setup("exit 0");
        let backend = Backend::new(wireguard, Arc::new(TestAuthorizer::new(Decision::Deny)));
        assert_eq!(backend.list_configurations_inner().unwrap(), ["home"]);
    }

    #[tokio::test]
    async fn active_interfaces_do_not_require_authorization() {
        let (_temp, wireguard) = setup("printf 'home office\\n'");
        let backend = Backend::new(wireguard, Arc::new(TestAuthorizer::new(Decision::Deny)));
        assert_eq!(
            backend.active_interfaces_inner().await.unwrap(),
            ["home", "office"]
        );
    }

    #[tokio::test]
    async fn authorized_connect_and_disconnect_invoke_runner() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("home.conf"), "[Interface]\n").unwrap();
        let log = temp.path().join("actions");
        let executable = script(
            temp.path(),
            "wg-quick",
            &format!("printf '%s\\n' \"$1\" >> '{}'", log.display()),
        );
        let wireguard = Wireguard::new(
            temp.path(),
            &executable,
            &executable,
            Duration::from_secs(2),
        );
        let authorizer = Arc::new(TestAuthorizer::new(Decision::Allow));
        let backend = Backend::new(wireguard, authorizer.clone());

        backend
            .connect_for_sender("home", Some(":1.42"))
            .await
            .unwrap();
        backend
            .disconnect_for_sender("home", Some(":1.42"))
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(log).unwrap(), "up\ndown\n");
        assert_eq!(
            *authorizer.callers.lock().await,
            [Some(":1.42".to_owned()), Some(":1.42".to_owned())]
        );
    }

    #[tokio::test]
    async fn denied_callers_cannot_execute_commands() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("home.conf"), "[Interface]\n").unwrap();
        let marker = temp.path().join("executed");
        let executable = script(
            temp.path(),
            "wg-quick",
            &format!("touch '{}'", marker.display()),
        );
        let wireguard = Wireguard::new(
            temp.path(),
            &executable,
            &executable,
            Duration::from_secs(2),
        );
        let backend = Backend::new(wireguard, Arc::new(TestAuthorizer::new(Decision::Deny)));
        let error = backend
            .connect_for_sender("home", Some(":1.42"))
            .await
            .unwrap_err();
        assert!(matches!(error, fdo::Error::AccessDenied(_)));
        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn missing_senders_and_polkit_failures_are_distinguished() {
        let (_temp, wireguard) = setup("exit 0");
        let denied = Backend::new(
            wireguard.clone(),
            Arc::new(TestAuthorizer::new(Decision::Allow)),
        );
        assert!(matches!(
            denied.connect_for_sender("home", None).await,
            Err(fdo::Error::AccessDenied(_))
        ));

        let failed = Backend::new(wireguard, Arc::new(TestAuthorizer::new(Decision::Fail)));
        let error = failed
            .disconnect_for_sender("home", Some(":1.42"))
            .await
            .unwrap_err();
        assert!(matches!(error, fdo::Error::Failed(message) if message == "test failure"));
    }

    #[tokio::test]
    async fn command_and_discovery_errors_become_dbus_failures() {
        let (_temp, wireguard) = setup("printf 'bad config' >&2; exit 2");
        let backend = Backend::new(wireguard, Arc::new(TestAuthorizer::new(Decision::Allow)));
        let error = backend
            .connect_for_sender("home", Some(":1.42"))
            .await
            .unwrap_err();
        assert!(matches!(error, fdo::Error::Failed(message) if message.contains("bad config")));

        let temp = TempDir::new().unwrap();
        let missing = Wireguard::new(
            temp.path().join("missing"),
            "wg-quick",
            "wg",
            Duration::from_secs(1),
        );
        let backend = Backend::new(missing, Arc::new(TestAuthorizer::new(Decision::Allow)));
        assert!(matches!(
            backend.list_configurations_inner(),
            Err(fdo::Error::Failed(_))
        ));
    }
}
