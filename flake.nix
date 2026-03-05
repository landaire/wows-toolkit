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

      rustToolchainToml = fromTOML (builtins.readFile ./rust-toolchain.toml);
      inherit (rustToolchainToml.toolchain) channel components targets;

      rustToolchain = pkgs.rust-bin.stable.${channel}.default.override {
        extensions = components;
        inherit targets;
      };

      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      # Include embedded assets in addition to standard Cargo sources
      srcFilter = path: type:
        (craneLib.filterCargoSources path type)
        || (builtins.match ".*embedded_resources.*" path != null)
        || (builtins.match ".*assets.*" path != null);

      commonArgs = {
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = srcFilter;
        };
        strictDeps = true;

        nativeBuildInputs = with pkgs; [
          pkg-config
        ];

        buildInputs = with pkgs;
          [
            openssl
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
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

          guiBuildInputs =
            commonArgs.buildInputs
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              libxkbcommon
              wayland
              xorg.libXcursor
              xorg.libXrandr
              xorg.libXi
              xorg.libX11
              fontconfig
            ];

          unwrapped = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs = "-p wows_toolkit";
              buildInputs = guiBuildInputs;
              meta.mainProgram = "wows_toolkit";
            });
        in {
          wows-toolkit =
            if stdenv.hostPlatform.isLinux
            then
              (pkgs.symlinkJoin {
                name = "wows-toolkit-${unwrapped.version or "dev"}";
                paths = [unwrapped];
                nativeBuildInputs = [pkgs.makeWrapper];
                postBuild = ''
                  wrapProgram $out/bin/wows_toolkit \
                    --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath guiRuntimeLibs}
                '';
              }).overrideAttrs {meta.mainProgram = "wows_toolkit";}
            else unwrapped;

          default = self.packages.${system}.wows-toolkit;

          wowsunpack = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs = "-p wowsunpack";
            });

          minimap-renderer = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs =
                "-p wows_minimap_renderer --features bin,cpu"
                + lib.optionalString stdenv.hostPlatform.isLinux ",vulkan"
                + lib.optionalString stdenv.hostPlatform.isDarwin ",videotoolbox";
              buildInputs =
                commonArgs.buildInputs
                ++ lib.optionals stdenv.hostPlatform.isLinux [
                  vulkan-loader
                ];
            });

          replayshark = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs = "-p replayshark";
            });
        };

        devShells.default = mkShell rec {
          buildInputs =
            [
              # Rust
              rustToolchain

              # misc. libraries
              openssl
              pkg-config

              # Development tools
              depotdownloader
              trunk

              # WASM build (ring C crypto → wasm32)
              # Use unwrapped clang — the nix wrapper adds hardening flags
              # (e.g. -fzero-call-used-regs) that are invalid for wasm32.
              llvmPackages.clang-unwrapped
              llvmPackages.llvm
            ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [
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

          # ring's cc crate needs clang + llvm-ar for wasm32-unknown-unknown
          CC_wasm32_unknown_unknown = "${llvmPackages.clang-unwrapped}/bin/clang";
          AR_wasm32_unknown_unknown = "${llvmPackages.llvm}/bin/llvm-ar";

          LD_LIBRARY_PATH =
            lib.optionalString stdenv.hostPlatform.isLinux
            "${lib.makeLibraryPath buildInputs}";
        };
      });
}
