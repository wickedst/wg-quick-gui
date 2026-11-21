use wireguard_client_gui::{FdoNotifier, ZbusBackendClient};

#[tokio::test]
async fn bus_constructors_report_connection_errors() {
    // This integration-test binary contains one test, so changing its process environment cannot
    // race with another bus test. No connection is made to the host buses.
    std::env::set_var(
        "DBUS_SYSTEM_BUS_ADDRESS",
        "unix:path=/tmp/wireguard-client-gui-missing-system-bus",
    );
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        "unix:path=/tmp/wireguard-client-gui-missing-session-bus",
    );

    let client_error = ZbusBackendClient::system().await.unwrap_err();
    assert!(!client_error.to_string().is_empty());

    let notification_error = FdoNotifier::session().await.unwrap_err();
    assert!(!notification_error.to_string().is_empty());
}
