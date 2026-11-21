{
  self,
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

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "wg-quick-gui.packages.<system>.default";
      description = "The wireguard-client-gui package to install.";
    };

    autostart = lib.mkEnableOption "XDG and systemd user autostart for graphical user sessions";
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    services.dbus.enable = true;
    services.dbus.packages = [ cfg.package ];
    security.polkit.enable = true;

    environment.etc."xdg/autostart/wireguard-client-gui.desktop" = lib.mkIf cfg.autostart {
      source = "${cfg.package}/share/applications/wireguard-client-gui.desktop";
    };

    systemd.tmpfiles.rules = [ "d /etc/wireguard 0700 root root -" ];

    systemd.services."dev.wireguardclientgui.Backend" = {
      description = "WireGuard Client privileged wg-quick backend";
      serviceConfig = {
        Type = "dbus";
        BusName = "dev.wireguardclientgui.Backend";
        ExecStart = "${cfg.package}/libexec/wireguard-client-gui-backend";
        Restart = "on-failure";
      };
    };

    systemd.user.services.wireguard-client-gui = lib.mkIf cfg.autostart {
      description = "WireGuard StatusNotifier tray client";
      after = [ "graphical-session.target" ];
      partOf = [ "graphical-session.target" ];
      wantedBy = [ "graphical-session.target" ];
      serviceConfig = {
        ExecStart = lib.getExe cfg.package;
        Restart = "on-failure";
      };
    };
  };
}
