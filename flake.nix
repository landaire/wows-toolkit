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

      # rust-toolchain.toml names a patchless channel, because a patch-pinned
      # one fails to resolve cargo on some hosts (see the comment there).
      # rust-overlay is keyed by exact version, so take the newest matching
      # point release, which is the one rustup installs for that channel.
      resolvedChannel =
        if pkgs.rust-bin.stable ? ${channel}
        then channel
        else let
          matching =
            builtins.filter
            (version: pkgs.lib.hasPrefix "${channel}." version)
            (builtins.attrNames pkgs.rust-bin.stable);
        in
          if matching == []
          then throw "rust-overlay has no ${channel}.x release; update the rust-overlay input."
          else pkgs.lib.last (builtins.sort pkgs.lib.versionOlder matching);

      # minimal (rustc + cargo + rust-std) keeps CI lean — the default profile
      # pulls rust-docs (~140 MiB) on every fresh runner. The `components` list
      # from rust-toolchain.toml (rustfmt, clippy) is added as extensions.
      rustToolchain = pkgs.rust-bin.stable.${resolvedChannel}.minimal.override {
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
          # rav1e (AV1 CPU encoder) builds hand-tuned x86 asm via nasm.
          nasm
        ];

        buildInputs = with pkgs;
          [
            openssl
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux [
            pkgs.vulkan-loader
            # keyring's Linux Secret Service backend (dbus-secret-service ->
            # libdbus-sys) pkg-config-probes dbus-1 at build. Buck vendors
            # libdbus's C sources; the cargo/crane path needs the dev package.
            pkgs.dbus
          ];
      };

      # Build workspace deps once, share across packages
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      # The CLI refuses a module built against any other wasm-bindgen crate
      # version, so this tracks the one Cargo.lock pins.
      wasmBindgenCli = pkgs.buildWasmBindgenCli rec {
        src = pkgs.fetchCrate {
          pname = "wasm-bindgen-cli";
          version = "0.2.128";
          hash = "sha256-a7lcXJnnZkYReja+iUO7NqqrWyv3toxnUgQb8s4IS5s=";
        };
        cargoDeps = pkgs.rustPlatform.fetchCargoVendor {
          inherit src;
          inherit (src) pname version;
          hash = "sha256-R1Tas33Ursy8kqsxguAkG0ZhNed2n5uFTAhw1l2qlLY=";
        };
      };
    in
      with pkgs; {
        packages = let
          # Runtime libraries needed by the GUI (X11, Wayland, GL, Vulkan) plus
          # libdbus for the keyring Secret Service backend.
          guiRuntimeLibs = lib.optionals stdenv.hostPlatform.isLinux [
            libxkbcommon
            libGL
            fontconfig
            wayland
            vulkan-loader
            libxcursor
            libxrandr
            libxi
            libx11
            dbus
          ];

          guiBuildInputs =
            commonArgs.buildInputs
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              libxkbcommon
              wayland
              libxcursor
              libxrandr
              libxi
              libx11
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

          # Every tool a Buck action may invoke. Actions run with no PATH, so
          # anything missing here is not merely unpinned, it is unavailable.
          buck-toolchain = symlinkJoin {
            name = "wows-toolkit-buck-toolchain";
            paths = [
              rustToolchain
              clang
              lld
              llvmPackages.llvm
              nasm
              bash
              coreutils
              libiconv
              python3
              # Unpacking the downloaded crate archives. GNU tar spawns its
              # decompressor as a separate process, so each one is needed too.
              gnutar
              bzip2
              gzip
              unzip
              xz
              zstd
              # wt-web's site: wasm-bindgen and wasm-opt.
              wasmBindgenCli
              binaryen
            ];
            # The wrapper's hardening flags (e.g. -fzero-call-used-regs) are
            # invalid for wasm32, so C compiled for wt-web uses the unwrapped
            # clang. The target prefix in the name selects wasm32 by default.
            postBuild = ''
              ln -s ${llvmPackages.clang-unwrapped}/bin/clang $out/bin/wasm32-unknown-unknown-clang
            '';
          };

          wowsunpack = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs = "-p wowsunpack";
            });

          minimap-renderer = craneLib.buildPackage (commonArgs
            // {
              inherit cargoArtifacts;
              cargoExtraArgs =
                "-p wows_minimap_renderer --features bin,cpu,cpu-av1"
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
        }
        # The nix-pinned macOS SDK the Buck toolchain's wrapped clang bakes in.
        # scripts/refresh-buck-toolchain.nu passes its path to check-macos-sdk.nu
        # so an ambient DEVELOPER_DIR/SDKROOT is verified to be exactly this one.
        // lib.optionalAttrs stdenv.hostPlatform.isDarwin {
          macos-sdk = pkgs.apple-sdk;
        };

        devShells.default = mkShell rec {
          buildInputs =
            [
              # Rust
              rustToolchain

              # misc. libraries
              openssl
              pkg-config

              # rav1e (AV1 CPU encoder) needs nasm to build hand-tuned x86 asm.
              nasm

              # Development tools
              depotdownloader
              mise
              cargo-edit
              cargo-local-registry
              reindeer
              direnv
              nushell

              # WASM build (ring C crypto → wasm32)
              # Use unwrapped clang — the nix wrapper adds hardening flags
              # (e.g. -fzero-call-used-regs) that are invalid for wasm32.
              llvmPackages.clang-unwrapped
              llvmPackages.llvm
            ]
            ++ lib.optionals stdenv.hostPlatform.isLinux [
              # Linking through this toolchain records store paths as the ELF
              # interpreter and runpath, which no Flatpak runtime has. Packaging
              # repoints them before the binary is bundled.
              patchelf

              # GUI libs
              libxkbcommon
              libGL
              fontconfig

              # keyring Secret Service backend (wgc-auth): dbus-1.pc + libdbus
              dbus

              # wayland libraries
              wayland

              # x11 libraries
              libxcursor
              libxrandr
              libxi
              libx11
            ];

          # ring's cc crate needs clang + llvm-ar for wasm32-unknown-unknown
          CC_wasm32_unknown_unknown = "${llvmPackages.clang-unwrapped}/bin/clang";
          AR_wasm32_unknown_unknown = "${llvmPackages.llvm}/bin/llvm-ar";

          LD_LIBRARY_PATH =
            lib.optionalString stdenv.hostPlatform.isLinux
            "${lib.makeLibraryPath buildInputs}";

          shellHook =
            # bin/buck2 runs the Buck2 release toolchains/buck2 pins; ./bootstrap.sh
            # installs the DotSlash it runs through. bin/with-host-graphics starts
            # the GUI with the host's graphics drivers.
            ''
              if [ -x "$PWD/bin/buck2" ]; then
                export PATH="$PWD/bin:$PATH"
              fi
            '';

        };
      });
}
