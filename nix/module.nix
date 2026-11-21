{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.wireguard-client-gui;
in
{
  options.services.wireguard-client-gui = {
    enable = lib.mkEnableOption "the WireGuard StatusNotifier tray client and privileged backend";

    package = lib.mkPackageOption pkgs "wireguard-client-gui" { };

    autostart = lib.mkEnableOption "system-wide XDG autostart for graphical user sessions";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    services.dbus.enable = true;
    services.dbus.packages = [ cfg.package ];
    security.polkit.enable = true;

    environment.etc."xdg/autostart/wireguard-client-gui.desktop" = lib.mkIf cfg.autostart {
      source = "${cfg.package}/share/applications/wireguard-client-gui.desktop";
    };

    systemd.services."dev.wireguardclientgui.Backend" = {
      description = "WireGuard Client privileged wg-quick backend";
      serviceConfig = {
        Type = "dbus";
        BusName = "dev.wireguardclientgui.Backend";
        ExecStart = "${cfg.package}/libexec/wireguard-client-gui-backend";
        Restart = "on-failure";
      };
    };
  };
}
