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

    theme = lib.mkOption {
      type = lib.types.enum [
        "dark"
        "light"
      ];
      default = "dark";
      description = "Panel colour scheme used for the transparent WireGuard icon.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    services.dbus.enable = true;
    services.dbus.packages = [ cfg.package ];
    security.polkit.enable = true;

    environment.etc."xdg/autostart/wireguard-client-gui.desktop" = lib.mkIf cfg.autostart {
      text = ''
        [Desktop Entry]
        Type=Application
        Name=WireGuard Client
        Comment=Manage wg-quick tunnels from the system tray
        Exec=${lib.getExe cfg.package} --theme ${cfg.theme}
        Icon=wireguard
        Terminal=false
        Categories=Network;
        StartupNotify=false
      '';
    };

    systemd.tmpfiles.rules = [ "d /etc/wireguard 0700 root root -" ];

    systemd.services."dev.wireguardclientgui.Backend" = {
      description = "WireGuard Client privileged wg-quick backend";
      # Start before graphical sessions instead of racing D-Bus activation on first login.
      wantedBy = [ "multi-user.target" ];
      after = [ "dbus.service" ];
      # Use openresolv or the systemd-resolved compatibility layer, matching wg-quick on NixOS.
      path = lib.mkBefore [ config.networking.resolvconf.package ];
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
        ExecStart = "${lib.getExe cfg.package} --theme ${cfg.theme}";
        Restart = "on-failure";
      };
    };
  };
}
