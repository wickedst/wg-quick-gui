use ksni::{menu::StandardItem, Icon, MenuItem, ToolTip, Tray};
use tokio::sync::mpsc;

use crate::{Action, MenuEntry, SharedModel};

/// Toolkit-free StatusNotifierItem tray.
pub struct WireguardTray {
    model: SharedModel,
    actions: mpsc::UnboundedSender<Action>,
}

impl WireguardTray {
    /// Construct a tray backed by shared state and an action channel.
    #[must_use]
    pub fn new(model: SharedModel, actions: mpsc::UnboundedSender<Action>) -> Self {
        Self { model, actions }
    }

    fn send(&self, action: Action) {
        if self.actions.send(action).is_err() {
            eprintln!("Tray action ignored because the application is shutting down");
        }
    }
}

impl Tray for WireguardTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn id(&self) -> String {
        "wireguard-client-gui".to_owned()
    }

    fn title(&self) -> String {
        "WireGuard Client".to_owned()
    }

    fn icon_name(&self) -> String {
        "wireguard".to_owned()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        wireguard_icons()
    }

    fn tool_tip(&self) -> ToolTip {
        let model = self.model.lock().unwrap();
        ToolTip {
            icon_name: "wireguard".to_owned(),
            title: "WireGuard Client".to_owned(),
            description: model.tooltip(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let entries = self.model.lock().unwrap().menu_entries();
        let mut menu = Vec::with_capacity(entries.len() + 3);
        for entry in entries {
            match entry {
                MenuEntry::Message(label) => menu.push(
                    StandardItem {
                        label,
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                ),
                MenuEntry::Tunnel(row) => {
                    let action = Action::Toggle(row.name.clone());
                    let mut label = if row.connected {
                        format!("✓ {}", row.name)
                    } else {
                        format!("  {}", row.name)
                    }
                    .replace('_', "__");
                    if let Some(pending) = row.pending {
                        label.push_str(&format!(" ({})", pending.label()));
                    }
                    menu.push(
                        StandardItem {
                            label,
                            enabled: row.pending.is_none(),
                            ..Default::default()
                        }
                        .into(),
                    );
                    if let Some(MenuItem::Standard(item)) = menu.last_mut() {
                        item.activate = Box::new(move |tray: &mut Self| tray.send(action.clone()));
                    }
                }
            }
        }
        menu.push(MenuItem::Separator);
        menu.push(
            StandardItem {
                label: "Refresh".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(Action::Refresh)),
                ..Default::default()
            }
            .into(),
        );
        menu.push(
            StandardItem {
                label: "Quit".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        );
        menu
    }
}

const WIREGUARD_ICON_24: &[u8] = include_bytes!("../../../resources/icons/wireguard-24.argb");
const WIREGUARD_ICON_48: &[u8] = include_bytes!("../../../resources/icons/wireguard-48.argb");

/// Return the embedded WireGuard logo in common tray sizes, without requiring an icon theme.
#[must_use]
pub fn wireguard_icons() -> Vec<Icon> {
    vec![
        Icon {
            width: 24,
            height: 24,
            data: WIREGUARD_ICON_24.to_vec(),
        },
        Icon {
            width: 48,
            height: 48,
            data: WIREGUARD_ICON_48.to_vec(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use wireguard_client_gui_core::Tunnel;

    use super::*;
    use crate::TrayModel;

    fn fixture() -> (WireguardTray, mpsc::UnboundedReceiver<Action>, SharedModel) {
        let model = TrayModel::shared();
        let (sender, receiver) = mpsc::unbounded_channel();
        (WireguardTray::new(model.clone(), sender), receiver, model)
    }

    #[test]
    fn identity_and_left_click_behavior_are_stable() {
        let (tray, _receiver, _model) = fixture();
        assert_eq!(tray.id(), "wireguard-client-gui");
        assert_eq!(tray.title(), "WireGuard Client");
        const { assert!(<WireguardTray as Tray>::MENU_ON_ACTIVATE) };
    }

    #[test]
    fn loading_menu_contains_message_refresh_and_quit() {
        let (tray, _receiver, _model) = fixture();
        let menu = tray.menu();
        assert_eq!(menu.len(), 4);
        assert!(matches!(
            &menu[0],
            MenuItem::Standard(item) if item.label == "Loading…" && !item.enabled
        ));
        assert!(matches!(menu[1], MenuItem::Separator));
        assert!(
            matches!(&menu[2], MenuItem::Standard(item) if item.label == "Refresh" && item.icon_name.is_empty())
        );
        assert!(
            matches!(&menu[3], MenuItem::Standard(item) if item.label == "Quit" && item.icon_name.is_empty())
        );
    }

    #[test]
    fn tunnel_menu_reflects_connections_pending_state_and_escaped_names() {
        let (tray, _receiver, model) = fixture();
        {
            let mut model = model.lock().unwrap();
            model.apply_snapshot(vec![
                Tunnel::new("home_vpn", true),
                Tunnel::new("office", false),
            ]);
            assert_eq!(model.begin_toggle("office"), Some(true));
        }
        let menu = tray.menu();
        assert!(matches!(
            &menu[0],
            MenuItem::Standard(item)
                if item.label == "✓ home__vpn" && item.enabled && item.icon_name.is_empty()
        ));
        assert!(matches!(
            &menu[1],
            MenuItem::Standard(item)
                if item.label == "  office (connecting…)" && !item.enabled && item.icon_name.is_empty()
        ));
        assert_eq!(tray.icon_name(), "wireguard");
        assert_eq!(
            tray.tool_tip().description,
            "Active WireGuard tunnels: home_vpn"
        );
    }

    #[test]
    fn callbacks_emit_actions_without_blocking() {
        let (mut tray, mut receiver, model) = fixture();
        model
            .lock()
            .unwrap()
            .apply_snapshot(vec![Tunnel::new("home", false)]);
        let menu = tray.menu();
        if let MenuItem::Standard(item) = &menu[0] {
            (item.activate)(&mut tray);
        } else {
            panic!("first item should be a tunnel");
        }
        if let MenuItem::Standard(item) = &menu[2] {
            (item.activate)(&mut tray);
        }
        if let MenuItem::Standard(item) = &menu[3] {
            (item.activate)(&mut tray);
        }
        assert_eq!(
            receiver.try_recv().unwrap(),
            Action::Toggle("home".to_owned())
        );
        assert_eq!(receiver.try_recv().unwrap(), Action::Refresh);
        assert_eq!(receiver.try_recv().unwrap(), Action::Quit);
    }

    #[test]
    fn inactive_icon_and_empty_menu_have_fallbacks() {
        let (tray, _receiver, model) = fixture();
        model.lock().unwrap().apply_snapshot(Vec::new());
        assert_eq!(tray.icon_name(), "wireguard");
        assert_eq!(tray.tool_tip().description, "No active WireGuard tunnels");
        assert!(matches!(
            &tray.menu()[0],
            MenuItem::Standard(item) if item.label == "No configurations found"
        ));
    }

    #[test]
    fn wireguard_icons_are_embedded_in_common_sizes() {
        let icons = wireguard_icons();
        assert_eq!(icons.len(), 2);
        assert_eq!((icons[0].width, icons[0].height), (24, 24));
        assert_eq!((icons[1].width, icons[1].height), (48, 48));
        assert_eq!(icons[0].data.len(), 24 * 24 * 4);
        assert_eq!(icons[1].data.len(), 48 * 48 * 4);
        assert!(icons[0].data.chunks_exact(4).any(|pixel| pixel[0] == 255));
        assert!(icons[0].data.chunks_exact(4).any(|pixel| pixel[0] == 0));

        let (tray, _receiver, _model) = fixture();
        assert_eq!(tray.icon_pixmap().len(), 2);
    }

    #[test]
    fn actions_after_shutdown_are_non_fatal() {
        let (tray, receiver, _model) = fixture();
        drop(receiver);
        tray.send(Action::Refresh);
    }
}
