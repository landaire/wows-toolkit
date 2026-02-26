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
      inherit (rustToolchainToml.toolchain) channel targets components;

      rustToolchain = pkgs.rust-bin.stable.${channel}.default.override {
        extensions = components;
        inherit targets;
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
        packages = {
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

          LD_LIBRARY_PATH = "${lib.makeLibraryPath buildInputs}";
        };
      });
}
