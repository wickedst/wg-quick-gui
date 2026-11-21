# wireguard-client-gui

A little system tray app for managing your `wg-quick` tunnels. It lists the configs in `/etc/wireguard` and lets you connect or disconnect them without touching a terminal.

One tunnel can be active at a time; connecting another disconnects the current one. Quitting the app leaves the active tunnel alone. It works with Waybar's ordinary `tray` module, along with other StatusNotifier-compatible trays.

## NixOS

Add the flake input and module:

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    wg-quick-gui = {
      url = "github:wickedst/wg-quick-gui";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, wg-quick-gui, ... }: {
    nixosConfigurations.IFNAMSIZ = nixpkgs.lib.nixosSystem {
      modules = [
        wg-quick-gui.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

Swap `your-hostname` for your machine's name. The module uses the flake's own package, so there's no overlay to remember.

Enable the app in `configuration.nix`:

```nix
services.wireguard-client-gui = {
  enable = true;

  # Optional: start it automatically in graphical sessions.
  autostart = true;

  # Optional: use black/light-grey icons for a light panel (default: dark).
  # theme = "light";
};
```

Rebuild NixOS. If you've left autostart off, just run:

```sh
wireguard-client-gui
```

There's no need to install `wg` or `wg-quick` separately — Nix sorts all that out for you.

## If there's no icon

- **Already logged in?** New autostart entries won't magically start in your current session. Log out and back in, or run:
  ```sh
  systemctl --user daemon-reload
  systemctl --user start wireguard-client-gui.service
  ```
  On desktops that only use XDG autostart, log out/in or start `xdg-desktop-autostart.target` instead.
- **Check the tray host.** You need a StatusNotifierWatcher, such as Waybar's standard `tray` module. Without one, there's nowhere for the icon to go.
- **Check polkit.** A polkit authentication agent must be running, or connect/disconnect can't ask for admin authorisation.
- **Check the logs.** With the systemd user unit enabled, run `journalctl --user -u wireguard-client-gui.service`.

The app reads `/etc/wireguard/*.conf`. NixOS's `networking.wireguard.interfaces` writes to `/run/wireguard`, so those declaratively managed tunnels won't show up here.

## Checks

```sh
nix develop
cargo test --workspace
nix flake check -L
```

The tests use temporary configs and private D-Bus instances, so they won't muck about with your real tunnels.

## Licence

MIT
