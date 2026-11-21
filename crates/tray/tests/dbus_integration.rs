#![cfg(unix)]

use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::timeout,
};
use wireguard_client_gui::{
    claim_tray_instance, BackendClient, FdoNotifier, Notifier, StderrNotifier, ZbusBackendClient,
};
use wireguard_client_gui_backend::{serve, AuthorizationError, Authorizer, Backend, PolicyKit};
use wireguard_client_gui_core::{Wireguard, DBUS_PATH, POLKIT_ACTION};
use zbus::{connection::Builder, interface, zvariant::OwnedValue, Connection};

struct TestBus {
    address: String,
    child: Child,
    _directory: TempDir,
}

impl TestBus {
    async fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let config = directory.path().join("session.conf");
        fs::write(
            &config,
            r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:tmpdir=/tmp</listen>
  <policy context="default">
    <allow own="*"/>
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
  </policy>
</busconfig>
"#,
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--print-address=1", "--nofork", "--nopidfile"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut lines = BufReader::new(stdout).lines();
        let address = timeout(Duration::from_secs(5), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        Self {
            address,
            child,
            _directory: directory,
        }
    }

    async fn connect(&self) -> Connection {
        Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

struct Allow;

#[async_trait]
impl Authorizer for Allow {
    async fn authorize(&self, sender: Option<&str>) -> Result<(), AuthorizationError> {
        if sender.is_some() {
            Ok(())
        } else {
            Err(AuthorizationError::MissingSender)
        }
    }
}

fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    let shell =
        std::env::var("WIREGUARD_CLIENT_GUI_TEST_SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
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

#[tokio::test]
async fn backend_client_round_trip_and_change_signals() {
    let bus = TestBus::new().await;
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("home.conf"), "[Interface]\n").unwrap();
    let active = temp.path().join("active");
    let wg_quick = script(
        temp.path(),
        "wg-quick",
        &format!(
            "if [ \"$1\" = up ]; then touch '{}'; else rm -f '{}'; fi",
            active.display(),
            active.display()
        ),
    );
    let wg = script(
        temp.path(),
        "wg",
        &format!(
            "if [ -e '{}' ]; then printf 'home\\n'; fi",
            active.display()
        ),
    );
    let wireguard = Wireguard::new(temp.path(), wg_quick, wg, Duration::from_secs(2));
    let backend = Backend::new(wireguard, Arc::new(Allow));
    let server = bus.connect().await;
    let _server = serve(backend, server).await.unwrap();

    let client = ZbusBackendClient::new(bus.connect().await);
    let mut changes = client.changed_stream().await.unwrap();
    let initial = client.snapshot().await.unwrap();
    assert_eq!(initial.len(), 1);
    assert!(!initial[0].connected);

    client.set_connected("home", true).await.unwrap();
    timeout(Duration::from_secs(2), changes.next())
        .await
        .unwrap();
    assert!(client.snapshot().await.unwrap()[0].connected);

    client.set_connected("home", false).await.unwrap();
    timeout(Duration::from_secs(2), changes.next())
        .await
        .unwrap();
    assert!(!client.snapshot().await.unwrap()[0].connected);
}

struct MockAuthority {
    authorized: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<(String, u32)>>>,
}

#[interface(name = "org.freedesktop.PolicyKit1.Authority")]
impl MockAuthority {
    #[zbus(name = "CheckAuthorization")]
    #[allow(clippy::unused_self)]
    fn check_authorization(
        &self,
        _subject: (&str, HashMap<String, OwnedValue>),
        action_id: &str,
        _details: HashMap<String, String>,
        flags: u32,
        _cancellation_id: &str,
    ) -> (bool, bool, HashMap<String, String>) {
        self.calls
            .lock()
            .unwrap()
            .push((action_id.to_owned(), flags));
        (
            self.authorized.load(Ordering::SeqCst),
            false,
            HashMap::new(),
        )
    }
}

#[tokio::test]
async fn polkit_uses_system_bus_name_subject_and_interactive_checks() {
    let bus = TestBus::new().await;
    let authorized = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let authority = MockAuthority {
        authorized: authorized.clone(),
        calls: calls.clone(),
    };
    let server = bus.connect().await;
    server
        .object_server()
        .at("/org/freedesktop/PolicyKit1/Authority", authority)
        .await
        .unwrap();
    server
        .request_name("org.freedesktop.PolicyKit1")
        .await
        .unwrap();

    let policykit = PolicyKit::new(bus.connect().await);
    policykit.authorize(Some(":1.99")).await.unwrap();
    assert_eq!(*calls.lock().unwrap(), [(POLKIT_ACTION.to_owned(), 1)]);

    authorized.store(false, Ordering::SeqCst);
    assert_eq!(
        policykit.authorize(Some(":1.99")).await,
        Err(AuthorizationError::Denied)
    );
    assert_eq!(
        policykit.authorize(None).await,
        Err(AuthorizationError::MissingSender)
    );
    assert_eq!(calls.lock().unwrap().len(), 2);

    drop(server);
    let error = policykit.authorize(Some(":1.99")).await.unwrap_err();
    assert!(matches!(error, AuthorizationError::CheckFailed(_)));
}

struct MockNotifications {
    messages: Arc<Mutex<Vec<(String, String)>>>,
}

#[interface(name = "org.freedesktop.Notifications")]
impl MockNotifications {
    #[zbus(name = "Notify")]
    #[allow(clippy::too_many_arguments, clippy::unused_self)]
    fn notify(
        &self,
        _app_name: &str,
        _replaces_id: u32,
        _app_icon: &str,
        summary: &str,
        body: &str,
        _actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
    ) -> u32 {
        self.messages
            .lock()
            .unwrap()
            .push((summary.to_owned(), body.to_owned()));
        1
    }
}

#[tokio::test]
async fn notifications_use_the_standard_session_protocol() {
    let bus = TestBus::new().await;
    let messages = Arc::new(Mutex::new(Vec::new()));
    let service = MockNotifications {
        messages: messages.clone(),
    };
    let server = bus.connect().await;
    server
        .object_server()
        .at("/org/freedesktop/Notifications", service)
        .await
        .unwrap();
    server
        .request_name("org.freedesktop.Notifications")
        .await
        .unwrap();

    let notifier = FdoNotifier::new(bus.connect().await);
    notifier.notify("Summary", "Diagnostic").await;
    assert_eq!(
        *messages.lock().unwrap(),
        [("Summary".to_owned(), "Diagnostic".to_owned())]
    );

    drop(server);
    // Notification delivery errors and stderr fallback are both non-fatal.
    notifier.notify("Ignored", "Ignored").await;
    StderrNotifier.notify("Fallback", "Diagnostic").await;
}

#[tokio::test]
async fn tray_instance_name_is_exclusive_within_a_session() {
    let bus = TestBus::new().await;
    let first = bus.connect().await;
    let second = bus.connect().await;
    assert_ne!(first.unique_name().unwrap(), second.unique_name().unwrap());

    claim_tray_instance(&first).await.unwrap();
    let error = claim_tray_instance(&second).await.unwrap_err();
    assert!(matches!(error, zbus::Error::NameTaken));
}

#[test]
fn public_backend_path_is_stable() {
    assert_eq!(DBUS_PATH, "/dev/wireguardclientgui/Backend");
}
