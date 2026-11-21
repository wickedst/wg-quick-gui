{
  lib,
  rustPlatform,
  src,
  makeWrapper,
  dbus,
  wireguard-tools,
  iproute2,
  openresolv,
  iptables,
  nftables,
  procps,
  coreutils,
  gnugrep,
  gnused,
  gawk,
  bash,
}:
let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  runtimeTools = [
    wireguard-tools
    iproute2
    iptables
    nftables
    procps
    coreutils
    gnugrep
    gnused
    gawk
    bash
  ];
in
rustPlatform.buildRustPackage {
  pname = "wireguard-client-gui";
  inherit (cargoToml.workspace.package) version;
  inherit src;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];
  nativeCheckInputs = [
    dbus
    coreutils
  ];
  env.WIREGUARD_CLIENT_GUI_TEST_SHELL = "${bash}/bin/bash";

  cargoTestFlags = [ "--workspace" ];

  postInstall = ''
    mkdir -p "$out/libexec"
    mv "$out/bin/wireguard-client-gui-backend" "$out/libexec/wireguard-client-gui-backend"
    wrapProgram "$out/libexec/wireguard-client-gui-backend" \
      --set WIREGUARD_CLIENT_GUI_WG_QUICK ${lib.getExe' wireguard-tools "wg-quick"} \
      --set WIREGUARD_CLIENT_GUI_WG ${lib.getExe' wireguard-tools "wg"} \
      --prefix PATH : ${lib.makeBinPath runtimeTools} \
      --suffix PATH : ${lib.makeBinPath [ openresolv ]}

    install -Dm644 ${../resources/dev.wireguardclientgui.Backend.service} \
      "$out/share/dbus-1/system-services/dev.wireguardclientgui.Backend.service"
    install -Dm644 ${../resources/dev.wireguardclientgui.Backend.conf} \
      "$out/share/dbus-1/system.d/dev.wireguardclientgui.Backend.conf"
    install -Dm644 ${../resources/dev.wireguardclientgui.policy} \
      "$out/share/polkit-1/actions/dev.wireguardclientgui.policy"
    install -Dm644 ${../resources/wireguard-client-gui.desktop} \
      "$out/share/applications/wireguard-client-gui.desktop"
  '';

  meta = {
    description = "StatusNotifier tray client for wg-quick";
    homepage = "https://github.com/wickedst/wg-quick-gui";
    license = lib.licenses.mit;
    mainProgram = "wireguard-client-gui";
    platforms = lib.platforms.linux;
  };
}
