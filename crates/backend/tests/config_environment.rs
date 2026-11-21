use std::{path::PathBuf, time::Duration};

use wireguard_client_gui_backend::BackendConfig;

#[test]
fn process_environment_is_loaded() {
    // This integration-test binary contains one test, making process-environment changes isolated.
    std::env::set_var("WIREGUARD_CLIENT_GUI_CONFIG_DIR", "/tmp/test-wireguard");
    std::env::set_var("WIREGUARD_CLIENT_GUI_WG_QUICK", "/bin/test-wg-quick");
    std::env::set_var("WIREGUARD_CLIENT_GUI_WG", "/bin/test-wg");
    std::env::set_var("WIREGUARD_CLIENT_GUI_TIMEOUT_SECONDS", "17");

    let config = BackendConfig::from_env().unwrap();
    assert_eq!(config.config_dir, PathBuf::from("/tmp/test-wireguard"));
    assert_eq!(config.wg_quick, PathBuf::from("/bin/test-wg-quick"));
    assert_eq!(config.wg, PathBuf::from("/bin/test-wg"));
    assert_eq!(config.command_timeout, Duration::from_secs(17));
}
