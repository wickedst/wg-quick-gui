# wireguard-client-gui

A little system tray app for managing your `wg-quick` tunnels. It lists the configs in `/etc/wireguard` and lets you connect or disconnect them without touching a terminal.

Multiple tunnels are fine, and quitting the app leaves them connected. It works with Waybar's ordinary `tray` module, along with other StatusNotifier-compatible trays.

## NixOS

Add the flake input and module, then pop the overlay in your NixOS configuration:

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    wireguard-client-gui.url = "github:OWNER/wireguard-client-gui";
  };

  outputs = { nixpkgs, wireguard-client-gui, ... }: {
    nixosConfigurations.IFNAMSIZ = nixpkgs.lib.nixosSystem {
      modules = [
        ({ ... }: {
          nixpkgs.overlays = [ wireguard-client-gui.overlays.default ];
        })
        wireguard-client-gui.nixosModules.default
        ./configuration.nix
      ];
    };
  };
}
```

Swap `OWNER` for the GitHub owner and `your-hostname` for your machine's name.

Enable the app in `configuration.nix`:

```nix
services.wireguard-client-gui = {
  enable = true;

  # Optional: start it automatically when you log in.
  autostart = true;
};
```

Rebuild NixOS. If you've left autostart off, just run:

```sh
wireguard-client-gui
```

You'll need a polkit authentication agent running; it'll ask for admin authorisation when you change a tunnel. Waybar users only need its standard `tray` module enabled.

There's no need to install `wg` or `wg-quick` separately — Nix sorts all that out for you.

## Checks

```sh
nix develop
cargo test --workspace
nix flake check -L
```

The tests use temporary configs and private D-Bus instances, so they won't muck about with your real tunnels.

## Licence

MIT
