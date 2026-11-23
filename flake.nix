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
      # from rust-toolchain.toml is added as extensions. rust-analyzer and the
      # rust-src it reads std from serve the editor alone, so only the devShell
      # carries them.
      editorComponents = ["rust-analyzer" "rust-src"];
      mkRustToolchain = extensions:
        pkgs.rust-bin.stable.${resolvedChannel}.minimal.override {
          inherit extensions targets;
        };
      rustToolchain = mkRustToolchain (pkgs.lib.subtractLists editorComponents components);
      devRustToolchain = mkRustToolchain components;

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
            # libdbus-sys) pkg-config-probes dbus-1 at build.
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

          # Rust's libc links libiconv on macOS. nixpkgs removes the SDK's stub
          # for it in favor of a store build, which a binary linked against it
          # loads from its store path. The SDK's own stubs name /usr/lib, which
          # every Mac has. libcharset is a re-export of libiconv.
          macosSystemLibStubs = stdenvNoCC.mkDerivation {
            pname = "macos-system-lib-stubs";
            inherit (apple-sdk) version src;
            dontConfigure = true;
            dontBuild = true;
            installPhase = ''
              mkdir -p $out/lib
              cp -d usr/lib/libiconv*.tbd usr/lib/libcharset*.tbd $out/lib
            '';
          };
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
            ]
            ++ lib.optionals stdenv.hostPlatform.isDarwin [macosSystemLibStubs];
            nativeBuildInputs = [makeWrapper];
            # The wrapper's hardening flags (e.g. -fzero-call-used-regs) are
            # invalid for wasm32, so C compiled for wt-web uses the unwrapped
            # clang. The target prefix in the name selects wasm32 by default.
            #
            # A Buck action inherits the environment of the shell that started
            # the daemon, and the Nix compiler and linker wrappers read that
            # shell's flags (an -rpath into the checkout among them) whenever
            # its role markers are set. Each wrapper here drops every NIX_
            # variable, then pins the store path (without it the linker
            # wrapper writes host directories such as /usr/lib64 into the
            # runpath), the hardening and the refusal of -march=native the dev
            # shell would have given, so no shell changes what a build makes.
            postBuild = ''
              ln -s ${llvmPackages.clang-unwrapped}/bin/clang $out/bin/wasm32-unknown-unknown-clang
              for tool in c++ cc clang clang++ cpp ld ld.bfd ld.gold; do
                [ -e "$out/bin/$tool" ] || continue
                wrapped=$(readlink -f "$out/bin/$tool")
                rm "$out/bin/$tool"
                makeWrapper "$wrapped" "$out/bin/$tool" \
                  --run 'for name in ''${!NIX_@}; do unset "$name"; done' \
                  --set NIX_STORE ${builtins.storeDir} \
                  --set NIX_HARDENING_ENABLE_${clang.suffixSalt} ${lib.escapeShellArg (lib.concatStringsSep " " clang.defaultHardeningFlags)} \
                  --set NIX_ENFORCE_NO_NATIVE_${clang.suffixSalt} 1
              done
            '';
          };

          # Shared libraries Buck links dynamically (toolchains//:dbus-1,
          # :fontconfig, :xkbcommon). Kept apart from buck-toolchain so adding
          # one leaves the toolchain path, and with it every cached action,
          # unchanged.
          buck-system-libs = symlinkJoin {
            name = "wows-toolkit-buck-system-libs";
            paths = lib.optionals stdenv.hostPlatform.isLinux [dbus.lib fontconfig.lib libxkbcommon];
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
              # Rust, with rust-analyzer from the same release
              devRustToolchain

              # misc. libraries
              openssl
              pkg-config

              # rav1e (AV1 CPU encoder) needs nasm to build hand-tuned x86 asm.
              nasm

              # Development tools. reindeer is left to `nu scripts/dev.nu
              # install-reindeer`: the revision nixpkgs packages rewrites much of
              # third-party/rust/BUCK.
              depotdownloader
              cargo-edit
              cargo-local-registry
              # THIRD_PARTY_NOTICES.txt. scripts/update_third_party_notices.nu
              # requires the version this nixpkgs revision provides.
              cargo-about
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

          # The Buck toolchain this shell was evaluated with, built on entering
          # it. bin/buck2 rewrites .buckconfig.local when it names others, so
          # a flake.lock change needs no manual refresh.
          WT_BUCK_TOOLCHAIN = self.packages.${system}.buck-toolchain;
          WT_BUCK_SYSTEM_LIBS = self.packages.${system}.buck-system-libs;
          WT_MACOS_SDK = lib.optionalString stdenv.hostPlatform.isDarwin "${self.packages.${system}.macos-sdk}";

          shellHook =
            # bin/buck2 runs the Buck2 release toolchains/buck2 pins; ./bootstrap.sh
            # installs the DotSlash it runs through. bin/with-host-graphics starts
            # the GUI with the host's graphics drivers.
            #
            # A shell that got this far evaluated, so it is not the older one
            # nix-direnv falls back to; bin/buck2 refuses to run under that one,
            # and `nix develop` from it would otherwise inherit its marker.
            ''
              if [ -x "$PWD/bin/buck2" ]; then
                export PATH="$PWD/bin:$PATH"
              fi
              unset NIX_DIRENV_DID_FALLBACK
            '';

        };
      });
}
