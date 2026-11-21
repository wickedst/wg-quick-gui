use std::process::Command;

#[test]
fn version_does_not_require_a_tray_or_system_bus() {
    let output = Command::new(env!("CARGO_BIN_EXE_wireguard-client-gui"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("wireguard-client-gui {}\n", env!("CARGO_PKG_VERSION"))
    );
}
