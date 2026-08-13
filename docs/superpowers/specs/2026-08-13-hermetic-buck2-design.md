# Hermetic Buck2 Build Design

## Goal

Build wows-toolkit and every supported CLI tool with native Buck2 targets on
aarch64-darwin, x86_64-linux, and x86_64-pc-windows-msvc. Every compiler, dependency source, SDK,
build flag, and execution environment is either declared and pinned or is
rejected before Buck starts.

## Boundary

The repository does not distribute Apple Xcode. The macOS execution image is
therefore a versioned external input. CI pins Xcode 26.6 build 17F113 and the
desktop entrypoint checks that build, the active developer directory, SDK
path, and architecture before it invokes Buck.

Nix provides the user-facing and CI execution envelope. flake.lock pins
Nixpkgs and the project tools. The checked-in toolchain manifest exposes the
matching Nix store paths to Buck as explicit toolchain inputs. Cargo remains
the dependency-authoring tool, but Buck actions never invoke Cargo or read an
ambient Cargo cache.

Windows compilation and unsigned MSI construction are inside this boundary.
They use a pinned, offline Visual Studio Build Tools layout, Windows SDK, Rust
MSVC toolchain, NASM, and WiX bundle. Bootstrap verifies every archive hash and
the installed tool manifest before Buck starts. The Buck build runs with network
access disabled and emits unsigned .exe, .pdb, and .msi artifacts.

Trusted signing is deliberately outside the build graph. A credentialed
publication step signs the exact unsigned artifacts produced by Buck and does
not rebuild them.

## Architecture

Reindeer consumes the workspace Cargo.toml and Cargo.lock during a controlled
update. It vendors the exact registry and git crate sources under
third-party/rust/vendor and generates checked-in Buck rules under
third-party/rust/BUCK. Checked-in fixups model features, proc macros, build
scripts, C/C++ linkage, resources, and platform constraints.

Each workspace crate receives native rust_library and rust_binary targets.
Root aliases preserve the existing Buck target names: wows_toolkit,
wowsunpack, wows_data_mgr, replayshark, minimap_renderer, wgcheck, dhat_load,
profile_replay, and dhat_parse. The cargo_binaries genrule is removed.

Dedicated Buck target and execution platforms select Rust, linker, C/C++, and
SDK toolchains for Darwin ARM64, Linux x86_64, and Windows x86_64 MSVC. Rust, C/C++, Buck2,
Reindeer, and their supporting tools come from flake.lock-pinned Nix inputs.
The macOS SDK is an explicitly validated Xcode boundary. Linux system
libraries are Nix store inputs. Windows uses an explicitly validated offline
toolchain boundary whose source archives and installation manifest are pinned
in the repository.

## Data Flow

Cargo.toml and Cargo.lock are dependency-authoring inputs. The update command
uses the pinned Reindeer executable to vendor and buckify the complete graph.
The resulting vendor tree, lock file, generated Buck rules, and fixups are
reviewed and committed together. Native Buck targets consume only repository
sources, vendored sources, and declared toolchain targets.

Game data is not an implicit build input. Buck build-script actions use a
checked-in empty data registry so their compile-time cfgs are deterministic.
Developers may continue to use local game data through Cargo builds and test
commands. Windows package inputs, including WiX sources, product version, and
application assets, are declared Buck inputs so unsigned MSI creation is
reproducible.

## Desktop Experience

Entering the checkout through direnv keeps the user's shell and loads the
matching flake dev shell. The entry hook validates the macOS external boundary
and makes `buck2 build //:wows_toolkit` the normal desktop build command. A
failed validation reports the expected and observed Xcode, SDK, and
architecture values.

## Verification

- Clean Linux, pinned macOS, and pinned Windows runners build all native Buck
  targets with network access disabled after checkout.
- Buck action inspection contains no Cargo executable or Cargo cache path.
- The macOS entry hook accepts the pinned Xcode contract and rejects a version,
  SDK, or architecture mismatch.
- The Windows bootstrap accepts only the pinned offline toolchain manifest and
  rejects missing, mismatched, or web-fetched Visual Studio, SDK, Rust, NASM,
  and WiX inputs.
- The Windows unsigned MSI is byte-reproducible from the same declared inputs;
  signing consumes that artifact in a separate credentialed publication step.
- Dependency updates fail when vendored sources, generated Buck metadata, or
  fixups are stale.
