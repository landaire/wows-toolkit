{
  description = "WoWs Toolkit - World of Warships tools monorepo";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    crane,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      overlays = [(import rust-overlay)];
      pkgs = import nixpkgs {inherit system overlays;};

      rustToolchainToml = fromTOML (builtins.readFile ./rust-toolchain);
      inherit (rustToolchainToml.toolchain) channel components;

      rustToolchain = pkgs.rust-bin.stable.${channel}.default.override {
        extensions = components;
      };

      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
        ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
          pkgs.vulkan-loader
        ];
      };

      # Build workspace deps once, share across packages
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in
      with pkgs; {
        packages = let
          # Runtime libraries needed by the GUI (X11, Wayland, GL, Vulkan)
          guiRuntimeLibs = lib.optionals stdenv.hostPlatform.isLinux [
            libxkbcommon
            libGL
            fontconfig
            wayland
            vulkan-loader
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.libX11
          ];

          guiBuildInputs = commonArgs.buildInputs ++ lib.optionals stdenv.hostPlatform.isLinux [
            libxkbcommon
            wayland
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.libX11
            fontconfig
          ];

          unwrapped = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p wows_toolkit";
            buildInputs = guiBuildInputs;
          });
        in {
          wows-toolkit = if stdenv.hostPlatform.isLinux then
            pkgs.symlinkJoin {
              name = "wows-toolkit-${unwrapped.version or "dev"}";
              paths = [unwrapped];
              nativeBuildInputs = [pkgs.makeWrapper];
              postBuild = ''
                wrapProgram $out/bin/wows_toolkit \
                  --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath guiRuntimeLibs}
              '';
            }
          else
            unwrapped;

          default = self.packages.${system}.wows-toolkit;

          wowsunpack = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p wowsunpack";
          });

          minimap-renderer = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p wows_minimap_renderer --features bin,gpu";
            buildInputs = commonArgs.buildInputs ++ lib.optionals stdenv.hostPlatform.isLinux [
              vulkan-loader
            ];
          });

          replayshark = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            cargoExtraArgs = "-p replayshark";
          });
        };

        devShells.default = mkShell rec {
          buildInputs = [
            # Rust
            rustToolchain

            # misc. libraries
            openssl
            pkg-config
          ] ++ lib.optionals stdenv.hostPlatform.isLinux [
            # GUI libs
            libxkbcommon
            libGL
            fontconfig

            # wayland libraries
            wayland

            # x11 libraries
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            xorg.libX11
          ];

          LD_LIBRARY_PATH = lib.optionalString stdenv.hostPlatform.isLinux
            "${lib.makeLibraryPath buildInputs}";
        };
      });
}
