use std::process::Command;

#[test]
fn version_does_not_require_system_bus_or_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_wireguard-client-gui-backend"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "wireguard-client-gui-backend {}\n",
            env!("CARGO_PKG_VERSION")
        )
    );
}
