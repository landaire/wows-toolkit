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
            ];
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
              trunk
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

          # wgpu loads Vulkan by dlopen-ing libvulkan.so.1 at runtime, and the
          # loader then discovers the machine's ICDs. The nix vulkan-loader
          # cannot see a non-NixOS host's system ICDs, so GUI rendering needs the
          # host's own Vulkan stack. Append the host graphics lib dir (NixOS:
          # /run/opengl-driver/lib; FHS distros: /usr/lib64) so libvulkan.so.1
          # and the system drivers resolve. Appended, not prefixed, so the nix
          # glibc and libstdc++ still win.
          shellHook =
            # bin/buck2 runs the Buck2 release toolchains/buck2 pins; ./bootstrap.sh
            # installs the DotSlash it runs through.
            ''
              if [ -x "$PWD/bin/buck2" ]; then
                export PATH="$PWD/bin:$PATH"
              fi
            ''
            + lib.optionalString stdenv.hostPlatform.isLinux ''
              for _gfx in /run/opengl-driver/lib /usr/lib64 /usr/lib; do
                if [ -e "$_gfx/libvulkan.so.1" ]; then
                  export LD_LIBRARY_PATH="''${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$_gfx"
                fi
              done
              unset _gfx
            '';

        };
      });
}
