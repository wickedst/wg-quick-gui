#![cfg(unix)]

use std::{fs, process::Stdio, time::Duration};

use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    time::{sleep, timeout},
};
use wireguard_client_gui_core::{DBUS_INTERFACE, DBUS_PATH, DBUS_SERVICE};
use zbus::{connection::Builder, Connection};

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

#[tokio::test]
async fn real_backend_binary_starts_and_serves_configuration_names() {
    let bus = TestBus::new().await;
    let configs = TempDir::new().unwrap();
    fs::write(configs.path().join("home.conf"), "[Interface]\n").unwrap();

    let mut backend = Command::new(env!("CARGO_BIN_EXE_wireguard-client-gui-backend"))
        .env("DBUS_SYSTEM_BUS_ADDRESS", &bus.address)
        .env("WIREGUARD_CLIENT_GUI_CONFIG_DIR", configs.path())
        .env("WIREGUARD_CLIENT_GUI_WG", "/bin/false")
        .env("WIREGUARD_CLIENT_GUI_WG_QUICK", "/bin/false")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let client = bus.connect().await;
    let mut names: Vec<String> = Vec::new();
    for _ in 0..100 {
        assert!(
            backend.try_wait().unwrap().is_none(),
            "backend exited before registering its D-Bus service"
        );
        match client
            .call_method(
                Some(DBUS_SERVICE),
                DBUS_PATH,
                Some(DBUS_INTERFACE),
                "ListConfigurations",
                &(),
            )
            .await
        {
            Ok(message) => {
                names = message.body().deserialize().unwrap();
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }

    assert_eq!(names, ["home"]);
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(backend.id().unwrap().to_string())
        .status()
        .await
        .unwrap();
    assert!(status.success());
    assert!(backend.wait().await.unwrap().success());
}
