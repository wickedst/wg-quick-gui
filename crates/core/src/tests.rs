#![cfg(unix)]

use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::{ffi::OsStringExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

use tempfile::TempDir;

use super::*;

fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    let shell =
        std::env::var("WIREGUARD_CLIENT_GUI_TEST_SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(format!("#!{shell}\n{body}\n").as_bytes())
        .unwrap();
    file.sync_all().unwrap();
    drop(file);
    // Some virtualized filesystems release the executable write lock shortly after close.
    std::thread::sleep(Duration::from_millis(10));
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    path
}

fn config(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(format!("{name}.conf"));
    fs::write(&path, "[Interface]\n").unwrap();
    path
}

fn runner(config_dir: &Path, executable: &Path) -> Wireguard {
    Wireguard::new(config_dir, executable, executable, Duration::from_secs(2))
}

#[test]
fn tunnel_constructor_sets_fields() {
    assert_eq!(
        Tunnel::new("home", true),
        Tunnel {
            name: "home".to_owned(),
            connected: true,
        }
    );
}

#[test]
fn validates_interface_names() {
    for valid in ["wg0", "home-vpn", "a_b=c+d.e", "123456789012345"] {
        assert!(is_valid_interface_name(valid), "{valid} should be valid");
    }
    for invalid in [
        "",
        ".",
        "..",
        "1234567890123456",
        "bad/name",
        "../wg0",
        "bad name",
        "wg0;id",
        "wg0\n",
        "vpnå",
    ] {
        assert!(
            !is_valid_interface_name(invalid),
            "{invalid:?} should be invalid"
        );
    }
}

#[test]
fn lists_only_valid_sorted_configuration_files() {
    let temp = TempDir::new().unwrap();
    config(temp.path(), "zeta");
    config(temp.path(), "home");
    config(temp.path(), "123456789012345");
    config(temp.path(), "bad name");
    config(temp.path(), "1234567890123456");
    fs::write(temp.path().join("notes.txt"), "ignored").unwrap();
    fs::create_dir(temp.path().join("directory.conf")).unwrap();
    fs::write(
        temp.path().join(OsString::from_vec(vec![
            b'x', 0xff, b'.', b'c', b'o', b'n', b'f',
        ])),
        "ignored",
    )
    .unwrap();
    std::os::unix::fs::symlink(
        temp.path().join("home.conf"),
        temp.path().join("linked.conf"),
    )
    .unwrap();

    let executable = script(temp.path(), "wg", "exit 0");
    let names = runner(temp.path(), &executable)
        .list_configurations()
        .unwrap();
    assert_eq!(names, ["123456789012345", "home", "linked", "zeta"]);
}

#[test]
fn missing_configuration_directory_is_an_error() {
    let temp = TempDir::new().unwrap();
    let executable = script(temp.path(), "wg", "exit 0");
    let missing = temp.path().join("missing");
    let error = runner(&missing, &executable)
        .list_configurations()
        .unwrap_err();
    assert!(matches!(&error, WireguardError::ReadDirectory { path, .. } if *path == missing));
    assert!(error.to_string().contains("failed to read"));
}

#[test]
fn config_path_rejects_invalid_and_missing_names() {
    let temp = TempDir::new().unwrap();
    let expected = config(temp.path(), "home");
    let executable = script(temp.path(), "wg", "exit 0");
    let runner = runner(temp.path(), &executable);
    assert_eq!(runner.config_path("home").unwrap(), expected);
    assert!(matches!(
        runner.config_path("../home"),
        Err(WireguardError::InvalidName(name)) if name == "../home"
    ));
    assert!(matches!(
        runner.config_path("missing"),
        Err(WireguardError::ConfigNotFound(name)) if name == "missing"
    ));
    assert!(runner
        .config_path("missing")
        .unwrap_err()
        .to_string()
        .contains("not found"));
}

#[tokio::test]
async fn active_interfaces_parses_filters_and_sorts_output() {
    let temp = TempDir::new().unwrap();
    let executable = script(
        temp.path(),
        "wg",
        "printf 'zeta home bad/name home 1234567890123456\\n'",
    );
    let active = runner(temp.path(), &executable)
        .active_interfaces()
        .await
        .unwrap();
    assert_eq!(active, ["home", "zeta"]);
}

#[tokio::test]
async fn tunnels_combine_configuration_and_kernel_state() {
    let temp = TempDir::new().unwrap();
    config(temp.path(), "home");
    config(temp.path(), "office");
    config(temp.path(), "unused");
    let executable = script(temp.path(), "wg", "printf 'office home\\n'");
    let tunnels = runner(temp.path(), &executable).tunnels().await.unwrap();
    assert_eq!(
        tunnels,
        [
            Tunnel::new("home", true),
            Tunnel::new("office", true),
            Tunnel::new("unused", false),
        ]
    );
}

#[tokio::test]
async fn connect_and_disconnect_use_wg_quick_with_absolute_config_paths() {
    let temp = TempDir::new().unwrap();
    let config_path = config(temp.path(), "home");
    let log = temp.path().join("arguments");
    let executable = script(
        temp.path(),
        "wg-quick",
        &format!("printf '%s\\n' \"$@\" > '{}'", log.display()),
    );
    let runner = runner(temp.path(), &executable);

    runner.connect("home").await.unwrap();
    let arguments = fs::read_to_string(&log).unwrap();
    assert_eq!(arguments, format!("up\n{}\n", config_path.display()));

    runner.disconnect("home").await.unwrap();
    let arguments = fs::read_to_string(&log).unwrap();
    assert_eq!(arguments, format!("down\n{}\n", config_path.display()));
}

#[tokio::test]
async fn connect_rejects_invalid_name_before_starting_executable() {
    let temp = TempDir::new().unwrap();
    let marker = temp.path().join("executed");
    let executable = script(
        temp.path(),
        "wg-quick",
        &format!("touch '{}'", marker.display()),
    );
    let error = runner(temp.path(), &executable)
        .connect("home;id")
        .await
        .unwrap_err();
    assert!(matches!(error, WireguardError::InvalidName(_)));
    assert!(error.to_string().contains("invalid WireGuard interface"));
    assert!(!marker.exists());
}

#[tokio::test]
async fn unsuccessful_commands_include_stderr_and_exit_code() {
    let temp = TempDir::new().unwrap();
    let executable = script(
        temp.path(),
        "wg",
        "printf 'permission denied\\n' >&2; exit 7",
    );
    let error = runner(temp.path(), &executable)
        .active_interfaces()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WireguardError::CommandFailed { code: Some(7), ref stderr, .. } if stderr == "permission denied"
    ));
    let display = error.to_string();
    assert!(display.contains("exit status 7"));
    assert!(display.contains("permission denied"));
}

#[tokio::test]
async fn unsuccessful_commands_fall_back_to_stdout() {
    let temp = TempDir::new().unwrap();
    let executable = script(temp.path(), "wg", "printf 'stdout diagnostic\\n'; exit 1");
    let error = runner(temp.path(), &executable)
        .active_interfaces()
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            WireguardError::CommandFailed { ref stderr, .. } if stderr == "stdout diagnostic"
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn unsuccessful_commands_have_a_diagnostic_fallback() {
    let temp = TempDir::new().unwrap();
    let executable = script(temp.path(), "wg", "exit 1");
    let error = runner(temp.path(), &executable)
        .active_interfaces()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WireguardError::CommandFailed { ref stderr, .. } if stderr == "no diagnostic output"
    ));
}

#[tokio::test]
async fn signal_termination_is_described() {
    let temp = TempDir::new().unwrap();
    let executable = script(temp.path(), "wg", "kill -KILL $$");
    let error = runner(temp.path(), &executable)
        .active_interfaces()
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        WireguardError::CommandFailed { code: None, .. }
    ));
    assert!(error.to_string().contains("terminated by signal"));
}

#[tokio::test]
async fn missing_executable_is_an_execute_error() {
    let temp = TempDir::new().unwrap();
    let missing = temp.path().join("missing-wg");
    let error = runner(temp.path(), &missing)
        .active_interfaces()
        .await
        .unwrap_err();
    assert!(matches!(error, WireguardError::Execute { ref program, .. } if *program == missing));
    assert!(error.to_string().contains("failed to execute"));
}

#[tokio::test]
async fn commands_are_killed_at_the_timeout() {
    let temp = TempDir::new().unwrap();
    let executable = script(temp.path(), "wg", "sleep 5");
    let runner = Wireguard::new(
        temp.path(),
        &executable,
        &executable,
        Duration::from_millis(25),
    );
    let error = runner.active_interfaces().await.unwrap_err();
    assert!(matches!(
        error,
        WireguardError::Timeout { duration, .. } if duration == Duration::from_millis(25)
    ));
    assert!(error.to_string().contains("timed out after 0 seconds"));
}
