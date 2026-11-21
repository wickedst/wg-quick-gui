{
  description = "A toolkit-free StatusNotifier tray client for wg-quick";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      overlays.default = final: _prev: {
        wireguard-client-gui = final.callPackage ./nix/package.nix { src = self; };
      };

      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        rec {
          wireguard-client-gui = pkgs.callPackage ./nix/package.nix { src = self; };
          default = wireguard-client-gui;
        }
      );

      nixosModules.default = {
        _module.args.self = self;
        imports = [ ./nix/module.nix ];
      };

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = nixpkgs.lib.getExe self.packages.${system}.default;
          meta.description = "Run the WireGuard StatusNotifier tray client";
        };
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          llvm = pkgs.rustPlatform.rust.rustc.llvmPackages.llvm;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              rustc
              rustfmt
              clippy
              cargo-llvm-cov
              dbus
              llvm
            ];
            env = {
              WIREGUARD_CLIENT_GUI_TEST_SHELL = "${pkgs.bash}/bin/bash";
              LLVM_COV = "${llvm}/bin/llvm-cov";
              LLVM_PROFDATA = "${llvm}/bin/llvm-profdata";
            };
          };
        }
      );

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          llvm = pkgs.rustPlatform.rust.rustc.llvmPackages.llvm;
          package = self.packages.${system}.default;
        in
        {
          inherit package;

          module =
            let
              evaluated = import (nixpkgs + "/nixos/lib/eval-config.nix") {
                inherit system;
                modules = [
                  {
                    services.wireguard-client-gui = {
                      enable = true;
                      autostart = true;
                    };
                  }
                  self.nixosModules.default
                ];
              };
              service = evaluated.config.systemd.services."dev.wireguardclientgui.Backend";
              userService = evaluated.config.systemd.user.services.wireguard-client-gui;
            in
            assert evaluated.config.services.wireguard-client-gui.package == package;
            assert userService.serviceConfig.ExecStart == nixpkgs.lib.getExe package;
            assert userService.wantedBy == [ "graphical-session.target" ];
            assert builtins.elem "d /etc/wireguard 0700 root root -" evaluated.config.systemd.tmpfiles.rules;
            pkgs.writeText "wireguard-client-gui-module.json" (
              builtins.toJSON {
                backend = {
                  inherit (service.serviceConfig) Type BusName ExecStart;
                };
                autostart = {
                  inherit (userService.serviceConfig) ExecStart;
                  wantedBy = userService.wantedBy;
                };
                enabled = evaluated.config.services.wireguard-client-gui.enable;
                tmpfilesRules = evaluated.config.systemd.tmpfiles.rules;
              }
            );

          coverage = package.overrideAttrs (previous: {
            nativeCheckInputs = (previous.nativeCheckInputs or [ ]) ++ [
              pkgs.cargo-llvm-cov
              llvm
            ];
            env = (previous.env or { }) // {
              LLVM_COV = "${llvm}/bin/llvm-cov";
              LLVM_PROFDATA = "${llvm}/bin/llvm-profdata";
            };
            checkPhase = ''
              runHook preCheck
              cargo llvm-cov --workspace --fail-under-lines 95 --summary-only
              runHook postCheck
            '';
          });
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
