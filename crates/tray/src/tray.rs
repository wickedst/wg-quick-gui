use ksni::{
    menu::{CheckmarkItem, StandardItem},
    Icon, MenuItem, ToolTip, Tray,
};
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
        if self.model.lock().unwrap().any_connected() {
            "network-vpn".to_owned()
        } else {
            "network-offline".to_owned()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![status_icon(self.model.lock().unwrap().any_connected())]
    }

    fn tool_tip(&self) -> ToolTip {
        let model = self.model.lock().unwrap();
        ToolTip {
            icon_name: if model.any_connected() {
                "network-vpn".to_owned()
            } else {
                "network-offline".to_owned()
            },
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
                    let mut label = row.name.replace('_', "__");
                    if let Some(pending) = row.pending {
                        label.push_str(&format!(" ({})", pending.label()));
                    }
                    menu.push(
                        CheckmarkItem {
                            label,
                            checked: row.connected,
                            enabled: row.pending.is_none(),
                            ..Default::default()
                        }
                        .into(),
                    );
                    // Attach the callback after construction to keep the row data straightforward.
                    if let Some(MenuItem::Checkmark(item)) = menu.last_mut() {
                        item.activate = Box::new(move |tray: &mut Self| tray.send(action.clone()));
                    }
                }
            }
        }
        menu.push(MenuItem::Separator);
        menu.push(
            StandardItem {
                label: "Refresh".to_owned(),
                icon_name: "view-refresh".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(Action::Refresh)),
                ..Default::default()
            }
            .into(),
        );
        menu.push(
            StandardItem {
                label: "Quit".to_owned(),
                icon_name: "application-exit".to_owned(),
                activate: Box::new(|tray: &mut Self| tray.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        );
        menu
    }
}

/// Generate a small ARGB icon so hosts do not need a particular icon theme installed.
#[must_use]
pub fn status_icon(connected: bool) -> Icon {
    const SIZE: i32 = 24;
    let (red, green, blue) = if connected {
        (46, 204, 113)
    } else {
        (130, 130, 130)
    };
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x - 11;
            let dy = y - 11;
            let distance = dx * dx + dy * dy;
            let ring = (64..=100).contains(&distance);
            let keyhole_top = dx * dx + (y - 10) * (y - 10) <= 4;
            let keyhole_stem = (10..=12).contains(&x) && (12..=16).contains(&y);
            if ring || keyhole_top || keyhole_stem {
                data.extend_from_slice(&[255, red, green, blue]);
            } else {
                data.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon {
        width: SIZE,
        height: SIZE,
        data,
    }
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
        assert!(matches!(&menu[2], MenuItem::Standard(item) if item.label == "Refresh"));
        assert!(matches!(&menu[3], MenuItem::Standard(item) if item.label == "Quit"));
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
            MenuItem::Checkmark(item)
                if item.label == "home__vpn" && item.checked && item.enabled
        ));
        assert!(matches!(
            &menu[1],
            MenuItem::Checkmark(item)
                if item.label == "office (connecting…)" && !item.checked && !item.enabled
        ));
        assert_eq!(tray.icon_name(), "network-vpn");
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
        if let MenuItem::Checkmark(item) = &menu[0] {
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
        assert_eq!(tray.icon_name(), "network-offline");
        assert_eq!(tray.tool_tip().description, "No active WireGuard tunnels");
        assert!(matches!(
            &tray.menu()[0],
            MenuItem::Standard(item) if item.label == "No configurations found"
        ));
    }

    #[test]
    fn generated_icons_are_argb_and_reflect_connection_state() {
        let active = status_icon(true);
        let inactive = status_icon(false);
        assert_eq!((active.width, active.height), (24, 24));
        assert_eq!(active.data.len(), 24 * 24 * 4);
        assert_ne!(active.data, inactive.data);
        assert!(active.data.chunks_exact(4).any(|pixel| pixel[0] == 255));
        assert!(active.data.chunks_exact(4).any(|pixel| pixel[0] == 0));

        let (tray, _receiver, _model) = fixture();
        let rendered = tray.icon_pixmap();
        assert_eq!(rendered.len(), 1);
        assert_eq!(rendered[0].data, inactive.data);
    }

    #[test]
    fn actions_after_shutdown_are_non_fatal() {
        let (tray, receiver, _model) = fixture();
        drop(receiver);
        tray.send(Action::Refresh);
    }
}
