# Hermetic Buck2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build every desktop and CLI binary through native Buck2 targets for macOS ARM64, Linux x86_64, and Windows x86_64 MSVC without Cargo, dependency downloads, or ambient build tools in Buck actions.

**Architecture:** Buck is the direct build interface. Nix pins the macOS and Linux tool environments; Windows uses a committed manifest for a hash-verified offline Build Tools, Windows SDK, Rust MSVC, NASM, and WiX provisioner. Reindeer vendors Cargo dependencies and native first-party Buck rules consume those checked-in sources. Unsigned Windows MSI creation is a Buck output; signing is a separate publication action.

**Tech Stack:** Buck2 prelude Rust/C++ rules, Reindeer local registry vendoring, Nix flakes, Visual Studio Build Tools offline layout, Windows SDK, Rust MSVC, NASM, WiX 6, GitHub Actions.

## Buck2 pin

Buck2 is pinned to release `2026-08-01` (the binary reports `2026-07-31`) in
`flake.nix`, `toolchains/windows/toolchain-manifest.json`, and
`prelude/VENDORED_FROM`. All three must agree: the vendored prelude only loads
under the release it was expanded from. It does not come from nixpkgs, which
tracks a different release.

Moving the pin means running `nu scripts/vendor-prelude.nu` against the new
binary and fixing whatever the prelude renamed. Going from 2025-12-01 to this
release needed three: `buildscript_platform.bzl` moved under `rust/buildscript/`,
`CxxToolchainInfo` requires `runtime_dependency_handling` again, and the
`--env-set` splitting bug the vendor script patched is now fixed upstream.

## Status

Tasks 1-3 are done and verified on `x86_64-unknown-linux-gnu`: all nine aliases
build in debug and release, every `scripts/test-*.nu` passes, and
`check-buck-hermetic.nu` accepts each alias while rejecting all ten negative
fixtures.

Task 4 builds. All nine aliases build for `x86_64-pc-windows-msvc` in debug and
release, and `wows_toolkit.exe` carries its icon, its version resource, and the
hybrid-graphics exports. Two parts are still unproven: the build was driven from
an MSVC, SDK and Rust install already on the machine rather than from a
provisioned offline layout, so `provision-toolchain.ps1` has never run
end-to-end; and the MSI target has never been built, because WiX was not part of
that install. The NASM and WiX archive hashes in `toolchain-manifest.json` were
checked against the real downloads and match, as were the Buck2, zstd and Python
entries added while getting the build working. The Visual Studio, Windows SDK
and Rust MSVC hashes remain unverified.

Task 5 is partly verified: `toolchains/platforms` exists and explicit
`--target-platforms` selection is confirmed on Linux and Windows.
`.github/workflows/buck.yml` has never run. macOS has never been built, and the
Xcode pin in `build-support/check-xcode.nu` is unverified against any real Xcode
install.

## Global Constraints

- Buck2 is the direct build command. Nu may be used only for maintenance scripts.
- Native Buck actions never invoke Cargo, read a Cargo cache, resolve a tool through PATH, or download content.
- Supported native target platforms are `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`.
- macOS validates Xcode 26.6 build 17F113 and MacOSX26.5 SDK before the build.
- Windows produces reproducible unsigned `.exe`, `.pdb`, and `.msi` artifacts. Signing is outside the build graph.
- Reindeer vendor archives, generated Buck rules, fixups, and dependency lock data are committed together.
- Build scripts receive the checked-in empty game-data registry through `WOWS_GAME_DATA`.

---

### Task 1: Make Buck the direct build interface

**Files:**
- Modify: `.envrc`
- Modify: `.gitignore`
- Modify: `flake.nix`
- Modify: `toolchains/BUCK`
- Modify: `toolchains/hermetic_rust.bzl`
- Delete: `scripts/buck.nu`
- Create: `scripts/refresh-buck-toolchain.nu`

**Interfaces:**
- Consumes: Nix `.#buck-toolchain` output.
- Produces: `.buckconfig.local` containing only a validated toolchain-root configuration; direct `buck2 build` uses explicit tool paths.

- [x] Write a failing shell test that deletes `.buckconfig.local` and verifies `buck2 audit execution-platform-resolution //:wgcheck` reports the missing toolchain configuration.
- [x] Run the test and record the failing diagnostic.
- [x] Replace all `system_*_toolchain` uses with custom Rust, C++, and Python toolchain rules that construct every `RunInfo` from `read_root_config("nix_toolchain", "root", ...)`.
- [x] Keep `scripts/refresh-buck-toolchain.nu` as an explicit setup command. Remove the Nu Buck wrapper and make `.envrc` load only the Nix shell.
- [x] Run `nu scripts/refresh-buck-toolchain.nu`, `buck2 audit execution-platform-resolution //:wgcheck`, and `buck2 build //:wgcheck`.
- [ ] Have a fresh reviewer inspect tool actions for bare compiler, linker, Python, or shell paths.
- [x] Commit `build: make Buck the direct build interface`.

### Task 2: Finish offline third-party dependency generation

**Files:**
- Modify: `reindeer.toml`
- Modify: `scripts/update-buck-deps.nu`
- Modify: `third-party/rust/BUCK`
- Modify: `third-party/rust/fixups/**`
- Modify: `third-party/rust/vendor/**`

**Interfaces:**
- Consumes: root `Cargo.toml`, `Cargo.lock`, and the Nix-pinned Reindeer and cargo-local-registry executables.
- Produces: `//third-party/rust:*` targets backed only by checked-in `.crate` archives and fixups.

- [x] Write a failing `nu scripts/check-buck-hermetic.nu //:wgcheck` assertion against a fixture containing `http_archive`, `http_file`, `git_fetch`, `git_repository`, or a URL in an action command.
- [x] Run the assertion and verify each fixture fails.
- [x] Configure Reindeer local-registry vendoring for all three target platforms and make the update script reject every remote-rule pattern after buckify.
- [x] Add explicit build-script fixups for every dependency reachable from each public binary. Use `run = false` for scripts that discover host state or invoke Cargo; model generated sources and native libraries as declared inputs instead.
- [x] Run dependency regeneration, verify `third-party/rust/BUCK` has no remote fetch rule, and run the hermetic action check on `//:wgcheck`.
- [ ] Have a fresh reviewer inspect every new fixup for host reads and undeclared network use.
- [x] Commit `build: finish offline Rust dependency generation`.

### Task 3: Generate and validate native workspace targets

**Files:**
- Modify: `BUCK`
- Create or modify: `crates/*/BUCK`
- Create: `build-support/buck/workspace_rules.bzl`
- Modify: `build-support/no-game-data/versions.toml`

**Interfaces:**
- Consumes: public `//third-party/rust:*` targets and platform toolchains.
- Produces: native libraries, build scripts, and binaries for `wows_toolkit`, `wowsunpack`, `wows_data_mgr`, `replayshark`, `minimap_renderer`, `wgcheck`, `dhat_load`, `profile_replay`, and `dhat_parse`.

- [x] Write a failing query test that asserts every public root alias has no `cargo_binaries` dependency and has a native `cargo.rust_binary` owner.
- [x] Run it and verify the legacy aliases fail.
- [x] Define first-party library and binary targets with their Cargo package environment, features, assets, translations, and direct third-party dependencies.
- [x] Define separate feature variants for `dhat_load`, `profile_replay`, and `dhat_parse` so profiling allocators do not affect normal binaries.
- [x] Model first-party build scripts with declared sources, `local_manifest_dir`, and `WOWS_GAME_DATA = $(location ...)`; wire their generated output into dependent Rust rules.
- [x] Replace the root Cargo genrule and retain every existing public alias name.
- [x] Build every alias and run `nu scripts/check-buck-hermetic.nu` for each alias.
- [ ] Have a fresh reviewer inspect feature isolation, resource declarations, and build-script inputs.
- [x] Commit `build: add native Buck workspace targets`.

### Task 4: Add a hermetic Windows MSVC and WiX boundary

**Files:**
- Create: `toolchains/windows/toolchain-manifest.json`
- Create: `toolchains/windows/verify-toolchain.ps1`
- Create: `toolchains/windows/provision-toolchain.ps1`
- Create: `toolchains/windows/BUCK`
- Create: `toolchains/windows/hermetic_msvc.bzl`
- Create: `toolchains/windows/wix.bzl`
- Create: `toolchains/windows/windows-x86_64-msvc.bzl`
- Modify: `.buckconfig`
- Modify: `toolchains/BUCK`
- Modify: `scripts/build-msi.ps1`

**Interfaces:**
- Consumes: SHA-256-pinned offline archives for VS Build Tools C++ Desktop components, Windows SDK, Rust MSVC, NASM, and WiX 6.
- Produces: validated tool paths, the `x86_64-pc-windows-msvc` Buck platform, and unsigned MSI outputs.

- [ ] Write a failing PowerShell test that substitutes one archive hash in `toolchain-manifest.json` and expects `verify-toolchain.ps1` to exit nonzero before Buck starts.
- [ ] Run the test and record the hash-mismatch diagnostic.
- [ ] Specify exact archive URLs, SHA-256 values, extraction paths, Visual Studio component IDs, executable paths, and version strings in `toolchain-manifest.json`.
- [ ] Implement provisioning from the offline layout with `--noWeb`, then implement verification of every archive hash, executable version, SDK include/lib directory, and WiX executable.
- [ ] Implement custom MSVC Rust/C++ Buck toolchains with explicit `cl.exe`, `link.exe`, `lib.exe`, `rc.exe`, `midl.exe`, `ml64.exe`, `rustc.exe`, `rustdoc.exe`, and `nasm.exe` paths.
- [ ] Implement a WiX Buck rule that takes declared `.wxs`, assets, binaries, PDBs, and version input and emits an unsigned MSI with deterministic timestamps.
- [ ] Replace the direct Cargo invocation in `scripts/build-msi.ps1` with a call that consumes the Buck MSI output only.
- [ ] Run `powershell -File toolchains/windows/verify-toolchain.ps1`, `buck2 build --target-platforms //toolchains/windows:windows_x86_64_msvc //:wows_toolkit`, and the MSI target with network disabled after provisioning.
- [ ] Have a fresh reviewer inspect the Windows action graph for PATH, internet access, mutable install locations, and signing credentials.
- [ ] Commit `build: add hermetic Windows toolchain and MSI target`.

### Task 5: Define all platforms and enforce hermetic CI

**Files:**
- Modify: `.buckconfig`
- Modify: `toolchains/BUCK`
- Create: `toolchains/platforms/BUCK`
- Create: `.github/workflows/buck.yml`
- Modify: `.github/workflows/build.yml`
- Modify: `.github/workflows/build_tools.yml`

**Interfaces:**
- Consumes: native aliases and Darwin, Linux, and Windows toolchain manifests.
- Produces: platform-selectable Buck builds and no-network CI gates for every tool.

- [ ] Write a failing CI fixture that runs `buck2 aquery` for every public alias and fails when it finds Cargo, a download rule, a URL, or a bare tool name.
- [ ] Run the fixture against the legacy workflow and verify it fails.
- [ ] Add Darwin ARM64, Linux x86_64, and Windows x86_64 MSVC target and execution platforms with explicit toolchain compatibility.
- [ ] Add CI jobs that provision only their pinned environment, disable network access before `buck2 build`, build every alias, and run the action-query gate.
- [ ] Publish unsigned Windows MSI, executables, and PDBs from Buck outputs. Make signing a downstream workflow that takes those artifacts by digest and has no build permission.
- [ ] Deliberately change the Xcode version and a Windows toolchain hash in CI fixtures; verify both fail before a Buck action runs.
- [ ] Have a fresh reviewer inspect platform selection, artifact provenance, and the signing/build separation.
- [ ] Commit `ci: verify hermetic Buck builds on all platforms`.
