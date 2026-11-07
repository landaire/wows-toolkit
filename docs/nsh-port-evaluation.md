# Evaluation: porting the GPUI app to Native Shell (nsh)

Evaluated 2026-11-06 against `crates/wows-toolkit-gpui` (81k LOC including inline tests) and `crates/wows-toolkit-viewmodel` (21k LOC). The target is [Native Shell](https://nsh.dev) 0.4.0 from crates.io, in pure Rust with no TSX and no QuickJS. nsh was read at tag `v0.4.0`. Facts that exist only on nsh `main` (`beade99a`, 2026-11-07) are marked "`main` only". Unless noted, nsh paths are relative to `crates/` in the nsh repository at `v0.4.0`. The spike in `crates/wows-toolkit-nsh` measured the riskiest paths on the Linux GTK 4 backend.

## Verdict

- A feature-equivalent port in pure Rust is feasible on nsh 0.4.0 without patching nsh. It is not a port, though. It is a rewrite of about 50k lines of views, plus three kinds of code we would own per backend:
  - a window host for the tactics board and the popped-out renderer;
  - a frame view, or a file-based frame route, for the replay renderer, minimap previews, tactics board and armor viewport;
  - the input that nsh views do not deliver: wheel, key-up, the pressed button on press, and modifier changes.
- nsh-base ships behavior without a look. Every GPUI Kit widget we use needs a Rust style layer that we would write and maintain.
- nsh is 3.5 weeks old (first commit 2026-10-14) and releases about weekly. Its compatibility promise covers the JS/TSX surface only (`docs/compatibility.md` in nsh), and the 0.4.0 release notes list breaking Rust changes.
- Our main platform is Windows, and the spike ran on Linux only.
- **Recommendation: do not start the port now.** The reason is cost and risk, not impossibility. Re-evaluate when the items under "What would change the verdict" land upstream.

## Method

1. Inventoried every user-visible feature of the GPUI app and the GPUI and GPUI Kit facilities each one uses.
2. Audited nsh's Rust API for each facility, with `path:line` evidence. TSX-only capabilities count as absent.
3. Built a spike on nsh 0.4.0 (Linux GTK 4.20 backend) covering four things:
   - a 5,000-row replay table built from real replay headers;
   - CPU frames at 30 fps, shown by four routes;
   - a line chart;
   - headless tests.
4. An adversarial review checked the verdicts, the citations and the spike's measurements. This version includes its corrections.

## What nsh is, in terms of this app

- **Threads.** Components run on an engine thread. Signals are bound to that thread: using one elsewhere panics (`nsh-core/src/element/reactive.rs:166`). Worker results return through `spawn` tasks (`element/ambient.rs:833`) that await `blocking`/`background` handles (`:1007`, `:1017`) or channels.
- **Rendering.** Each desktop backend draws with platform widgets: GTK 4 on Linux, Win32 and Direct2D on Windows, AppKit on macOS. A separate GPUI backend draws with `gpui-fast`.
- **No look.** nsh-base components "never style" (`nsh-base/src/lib.rs:13`). The only styled kit is TSX (`examples/nsh-gallery-ui`).
- **Custom drawing**, in four forms:
  - A `canvas` display list. A change re-records the drawing. Animated values update without a re-record.
  - A `shader` element. Its fragment function `shade(uv)` takes at most 4 KB of uniforms and has no vertex buffers or depth.
  - An `image` source. This is a URI or a file path, never raw pixels.
  - A native component per backend: a `gtk::Widget`, a Direct2D draw call or `HWND`, an `NSView`, or a GPUI `AnyElement`.
- **Windows.** An `AppView` is public on every backend and loads a Rust root (`nsh-platform-linux/src/app.rs:222,252,282`). A second window is host code that builds a platform window around a second `AppView`, which runs its own engine.

## Feature parity matrix

Status means the following:

- **Direct:** an nsh Rust API covers the facility.
- **Build:** nsh has the behavior, and we write the look or glue.
- **Workaround:** possible only with per-backend code, a file-based detour, or a change in behavior.

| Area (GPUI LOC) | GPUI facility used | nsh 0.4.0 Rust equivalent | Status |
|---|---|---|---|
| App shell (5.8k) | Main window, `TabBar`, title bar, dropdown menus | `run_app`, `nsh_base::tabs`, `set_title_bar`, `menu` | Build |
| App shell | OS file drop with a scrim that names the hovered file | `.on_file_drop` (`nsh-core/src/element/builder.rs:1380`). Drag enter and leave carry no paths (`:1364,1372`), so the scrim cannot name the file. The GTK and Win32 drop paths are "not compiled or exercised" (`docs/dev/file-drop.md:63-69` in nsh) | Workaround, unverified |
| App shell | Window geometry persistence | `desktop::set_frame` (`element/desktop.rs:93`). Linux sizes windows but cannot position them. Reactive `window_state` is `main` only | Build |
| App shell | UI zoom (rem size scaling) | `AppView::set_font_scale` scales text only (`nsh-platform-linux/src/app.rs:353`). Spacing has to scale through our style layer | Build |
| Tactics board, popped-out renderer | `cx.open_window` (`app.rs:1849`, `replay_inspector/view.rs:1607`) | Host code per backend: post to the platform main thread, build a window, load a second `AppView`. Window state is shared with the main window outside the reactive graph, because each window has its own engine. `DesktopCommand::OpenWindow` is `main` only and has no Rust wrapper | Workaround x3-4 backends |
| Replay browser (inspector total 16.3k) | Kit `Tree`, ctrl-click multi-select, context menu, hover preview after 300 ms | `nsh_base::tree` has single selection and takes the whole item list (`nsh-base/src/tree.rs:5-6`). Use `virtual_list` with our own rows and an anchored `overlay` | Build |
| Player table | Hand-built table on `list()`: sortable, resizable, expandable rows, per-row menus, selectable cells | `data_table` has one row height and a body with `pointer_events(None)` (`nsh-base/src/data_table/view.rs:513`). Expandable rows need a custom table on `virtual_list`, which the GPUI app already builds. A context menu goes to selectable text before the row when the text is nearer (`nsh-core/src/ops.rs:240`), so right-click on a cell behaves differently | Build |
| Alt-held and Ctrl-held modes | `on_modifiers_changed` (`replay_inspector/table.rs:1858`, `replay_renderer.rs:2850`) | Modifiers arrive only with pointer events over a listening view, or as declared `on_key` keystrokes. Without key-up, release is seen only on the next pointer event | Workaround |
| Dock tabs per replay, watched playback | Kit `DockArea` with dump/load. Watched playback is a dock tab (`replay_inspector/view.rs:1531-1535`) | `dock_area`, `DockAreaState` in gpui-base's frozen on-disk format (`nsh-base/src/dock/mod.rs:72`) | Build |
| Replay renderer (7.3k) | CPU raster to `RenderImage`, up to 120 frames/s at 60x | No raw-pixel source (`nsh-core/src/element/asset.rs`). See "Frame routes" for the four measured options. File path with two stacked images, or a native component per backend | Workaround |
| Replay renderer | Wheel zoom, drag pan, right-click menus, focused key handling | Right-click: `on_context_menu` (`builder.rs:1420`). Drag: press, move and pan events. Wheel: no wheel event on views (the listener list in `builder.rs` has none). On Linux, Ctrl+wheel becomes a pinch on views with `on_pinch_start` (`builder.rs:1661`, `nsh-platform-linux/src/input.rs:724-730`). A plain wheel reaches only native components and scroll views | Workaround |
| Minimap hover preview (1.6k) | 15 fps `img()` loop, explicit texture release | Same routes as the renderer, at 15 fps | Workaround |
| Tactics board (3.9k) | Separate window, CPU-rastered map in `img()`, wheel zoom, drag, context menu, per-view `on_key_down` | Window: as above. Map image changes on edit only, so a file or `data:` source suffices. Wheel: as the renderer | Workaround |
| Armor viewer (17k) plus `viewport/` (2.6k) | Own wgpu device, MSAA offscreen render, readback to `RenderImage`, gizmo painted in `canvas()` | The `shader` element cannot draw 3D (`nsh-core/src/shader.rs:1-30`). Keep our wgpu code and show frames through a frame route. Gizmo: `canvas` | Workaround |
| Armor viewer | WASD held at 60 Hz, middle-drag pan, wheel dolly | Middle drag: `MouseEvent.pressed_button` in `on_mouse_move` (`ops.rs:406`, `builder.rs:1409`). Held keys: impossible without key-up. Use a native component for this view on every backend, or change the controls | Workaround |
| Armor viewer | Tri-state visibility tree, ColorPicker, NumberInput, sliders, popovers, GLB export dialog | `tree`, `ColorPicker`, `number_input`, `slider`, `popover`, `dialog` | Build |
| Stats (5.5k) | Plot painted with quads, paths, shaped text and clip. Wheel zoom (`stats/chart_panel.rs:1030`). Dock layout persistence. Copy chart as image | `nsh_base::plot` or `canvas`. Wheel zoom as the renderer. Copy as image keeps the existing tiny-skia painter (`stats/plot_image.rs`) | Build, wheel Workaround |
| Player tracker (4.5k) | Dock panels, sortable lists, 2 s / 60 s / 120 s polling | `dock_area`, `virtual_list`, `set_interval` (`ambient.rs:878`) | Build |
| Unpacker (2.7k) | VFS trees, read-only highlighted editor, image viewer | `virtual_list` (the Tree builds only visible rows but flattens the whole list on change). `Editor` with `read_only` (`nsh-base/src/editor/mod.rs:87`). `image` from a file or `data:` URI | Build |
| Search (3.4k) | Query bar of pill buttons beside an `InputState`, anchored dropdowns, Calendar, caret queries | `input` with `selected_range`, `set_selection`, `on_caret_move`, `overlay` anchored to a point, `Calendar`. Pills stay views beside the input, as today | Build |
| Palette (0.8k) | Kit `Command` in a dialog, cascading modes | `command` with `filterable(false)` and our own ranking. The built-in filter matches substrings, ignoring case (`nsh-base/src/command.rs:9`) | Build |
| Collab (2.1k) | Popover forms, tokio session, waker-driven refresh, collab cursors in the renderer | `popover`, `input`, `spawn` awaiting the session channel | Build |
| Settings, first run, launcher, update | Kit `Form`, `Select`, `Slider`, `Checkbox`, `Textarea`, modifier-click on Kit controls, about 20 modal dialogs, rfd dialogs, child processes | `select`, `slider`, `checkbox`, multiline `input`, `alert_dialog`, `sheet`, `pick_paths`, `std::process`. nsh-base button and checkbox events carry no modifiers (`stats/ships.rs:468` and `armor_viewer/popover.rs:1762` use them) | Build |
| MP4 and batch export | Background encode, file-list clipboard | Unchanged code plus arboard | Build |
| Toasts and notices | Kit `Notification` | `Toast` and `ToastStack` views and `ToastManager`. No imperative `toast("...")` (`nsh-base/src/toast/mod.rs:15-18`) | Build |
| Clipboard | Text (GPUI), file lists and images (arboard) | `set_clipboard`, `set_clipboard_image(uri)` (`ambient.rs:471,494`). No file-list writer, so keep arboard | Direct plus arboard |
| Theme (0.6k) | Named palettes over Kit tokens, follow system appearance | `nsh_base::Theme` (shadcn tokens), `set_tokens` (`ambient.rs:341`), `set_color_scheme` (`:791`) | Build |
| Icons | Phosphor TTF, Kit SVG icons, game PNGs in memory | `Root::font` at startup (`nsh-core/src/element/component.rs:578`), `svg`, PNG from a file or `data:` URI | Direct |
| Localization | `rust-i18n` `t!()` with runtime language switching | Each translated text reads a locale signal | Build |
| Keyboard | Raw `on_key_down` per focused view | Global `on_key`/`on_key_with` (`ambient.rs:1097,1107`), gated by `focusable` plus `on_focus`/`on_blur` (`builder.rs:1761`). Keystrokes must be declared in advance. No key-up, no `tab_index` | Workaround |
| UI tests (`interaction_tests.rs` 2.6k plus about 30 inline modules) | Fork-specific `TestWindowExt` with element snapshots | `nsh_testing` headless steps and selectors (`press`, `type_text`, `drag`, `hover`, `id:`/`text:`). No pixels or screenshots from Rust, text measured approximately, native components absent | Build (rewrite) |

Everything not GPUI-bound carries over unchanged:

- the viewmodel crate;
- the wgpu viewport code, apart from `device.rs`'s `RenderImage` hand-off;
- replay loading, export, update, logging, CLI and hardening.

The inventory puts this framework-neutral code inside the GPUI crate at about 15-17k LOC. That leaves about 50-55k LOC of view code to rewrite [INFERENCE: from module boundaries, not audited line by line].

## Spike results

`crates/wows-toolkit-nsh` is excluded from the root workspace and has its own lock file, because it needs GTK 4 development files and builds only on Linux. It is a pure-Rust binary on nsh 0.4.0. `cargo tree -i rquickjs` finds no QuickJS. It has three tabs.

- **Replays:**
  - An nsh-base `data_table` over the headers of the 29 fixture replays, repeated to 5,000 rows.
  - Sorting happens in the app on `SortChange`. Columns are resizable. Pressing a row opens a detail panel.
  - Only the visible rows are built.
  - Without app classes there is no selection highlight or sort cue.
- **Frames:** a 768x768 tiny-skia scene (1,500 circles, 500 lines, 100 rectangles), produced at 30 fps on a worker thread and delivered to the engine through a `tokio::sync::watch` channel awaited in `spawn`.
- **Chart:** `nsh_base::plot` with 3 lines of 500 points, axes, and a hover index.
- **Tests:** `cargo test` passes 3 tests. They include a headless `nsh_testing` test that sorts the table and opens a row. Native components and image decoding do not exist headless.

### Frame routes

Release build. Runs of 22 s with the first 3 s dropped. Main-thread time is the nsh perf `frame` mean times GTK frames per second.

| Route | Encode or write ms | Engine ms/s | Main thread ms/s | Process CPU | RSS MB | Blank captures |
|---|---|---|---|---|---|---|
| Native GTK component (`gtk::Picture` + `gdk::MemoryTexture`) | none | 0.2 | 72.8 | 27.4% | 211 | 0/60 |
| `data:` BMP URI, one image | 1.32 | 221 | 34.2 | 58.0% | 331 | 20/60 |
| `data:` PNG URI, one image | 4.95 | 240 | 27.2 | 64.5% | 369 | 16/60 |
| `data:` BMP URI, two stacked images | 1.34 | 225 | 32.8 | 58.6% | 351 | 0/60 |
| BMP file in `/dev/shm`, one image | 1.07 | 0.3 | 31.0 | 36.6% | 380 | 4/60 |
| BMP file in `/dev/shm`, two stacked images | 1.07 | 0.6 | 28.4 | 36.1% | 391 | 0/60 |

All routes display 30.0 fps.

**How blanks were counted:**

- `grim` took 60 back-to-back captures (15-17 ms apart) of a 200x200 region inside the frame view.
- A capture counts as blank when every pixel equals the view's black background. Every rendered frame paints a non-black scene background.

**What the table shows:**

- **One image blanks between frames.** nsh clears the picture before the new source decodes (`nsh-platform-linux/src/image.rs:836-852`). Keeping the previous image stacked on top until the new one reports loaded removes the blanking.
- **`data:` URIs cost the engine thread.** About 7 ms per frame goes to base64-decoding and SHA-256-hashing the 3 MB URI there (`nsh-core/src/ops.rs:934-950`). That is 22-24% of the engine thread at 30 fps, and most of it at the renderer's 120 frames/s.
- **A file path avoids that cost.** The loader thread reads the file. Engine cost drops to under 1 ms/s and process CPU falls by 22 points. This route works on every backend that decodes the chosen format. BMP decoding on Windows and macOS was not checked. PNG works everywhere but costs 5 ms per frame to encode.
- **The native component is the cheapest on CPU, but it is per backend.** Its main-thread cost is the GSK texture upload during paint: 2.4 ms per frame, 7.3% of a core at 30 fps, so about 30% at 120 frames/s [INFERENCE: scaled linearly]. It must be written again for Direct2D, AppKit and GPUI. It is also the only place a plain wheel event arrives.
- **Both nsh and the GTK widget saw the same drag** in native mode, so a pan would be applied twice unless one side ignores it.
- **The file routes use more memory.** RSS was 380-391 MB against 211 MB for native. The cause was not found.

### Build cost

Measured on this machine (32 threads, `-j 16`).

| Build | Time |
|---|---|
| Clean debug | 91 s |
| Clean release (thin LTO) | 59-61 s |
| Incremental debug | 2.6-2.9 s |
| Incremental release | 44 s |

- **Binary size:** the release binary is 41.5 MB stripped.
- **Forced features:** `nsh-platform-linux` forces two sets of features (`nsh-platform-linux/Cargo.toml:31-32`):
  - nsh-engine's `net` and `shader`, which link wgpu 29 beside the workspace's wgpu 30;
  - nsh-base's `highlight` and `math`.

## Risks

- **API stability.**
  - The compatibility record excludes the host and Rust APIs.
  - Five releases shipped from 2026-10-17 to 2026-11-04.
  - In the 3 days from `v0.4.0` to `main`, nsh-core, nsh-base and the Linux and Windows backends gained 45,758 lines and lost 2,087 across 250 files. 21,430 of the added lines are in 46 new files. Most of that is growth, but it shows how fast the code we would build on moves.
- **Size and authorship.**
  - nsh is about 690k lines of Rust at `v0.4.0`. The crates a Rust desktop app depends on (core, base, engine, and the Linux, Windows and macOS backends) are about 457k of that.
  - 503 of the 792 commits up to `v0.4.0` carry a Claude or Codex co-author trailer.
  - Debugging into it is likely when a backend misbehaves. GPUI Kit, whose fork we already carry, does not have this profile.
- **Windows.**
  - It is our main platform, and the spike did not run there.
  - nsh's Windows backend has CI with real-window tests.
  - Our critical Windows paths have not been exercised by anyone: the frame route (BMP or PNG decode, or a Direct2D component), file drop and IME in the query bar.
- **Linux floor.** GTK 4.14+ excludes Ubuntu 22.04-era systems.
- **Buck2 and vendoring.**
  - The nsh crates are on crates.io and have no build scripts in the Rust-app path. Their dependencies do: `ring`, `zstd-sys`, and the gtk4, glib, pango and cairo `-sys` crates.
  - The platform crates default to `js`, which brings QuickJS C code and libclang. Every manifest and the reindeer third-party set must pin `default-features = false`.
- **Threading rewrite.** GPUI's `cx.spawn` / `entity.update` / `cx.notify` pattern becomes engine-thread signals plus channels. The spike shows that this pattern works. It still touches every async call site, and the inventory rates async use "very high".
- **Per-backend code.** Window hosting, and either a frame view or the file route, need a GTK, Win32, AppKit and possibly GPUI version. Each comes with its own input forwarding, DPI handling and texture or file lifetime.

## Effort

The GPUI port is the baseline. `crates/wows-toolkit-gpui` has 449 commits on `main` from 2026-07-16 to 2026-11-06, and its parity backlog is still open (`docs/gpui-port-parity.md`). Part of that work does not repeat, because it produced the viewmodel crate and other framework-neutral code that a second port reuses.

What an nsh port adds instead [INFERENCE]:

- A Rust style layer for about 25 components, which GPUI Kit provides today.
- Window hosting on 3-4 backends, with state shared between engines.
- Frame display and viewport input on 3-4 backends, or the file route plus the lost wheel and held-key behavior.
- A rewrite of the interaction tests (about 14k LOC with inline modules) on `nsh_testing`, which has no pixel checks from Rust.
- Tracking a Rust API that breaks every release.

The two roughly offset. Expect a duration on the order of the GPUI port before parity, with more risk on Windows.

## Alternatives considered

- **Embed nsh screens in the GPUI app.**
  - How it works: `nsh-platform-gpui`'s `AppView` is a GPUI view that hosts a Rust nsh root. That backend's `NativeComponent::render` returns any GPUI `AnyElement`, so our `RenderImage` paths would drop in.
  - Prerequisite: GPUI Kit's `gpui-fast` feature. The app builds on the default `gpui-pre` today. With `features = ["gpui-fast"]` on `gpui-kit`, `cargo check -p wows-toolkit-gpui` passes unchanged. The app was not run that way.
  - Maturity: the nsh GPUI backend is "not yet tried on macOS and Windows" (`docs/dev/gpui.md:279` in nsh).
  - Cost: every embedded screen needs a state bridge between GPUI entities and nsh's engine thread.
  - Benefit: none of nsh's native widgets, because it still draws with GPUI. It adds a second UI model inside the current one.
- **The nsh web target.** It renders DOM, and native components do not exist there. Minimap and armor frames would have to use `data:` URIs. It does not replace the existing egui WASM client.

## What would change the verdict

1. A public Rust wrapper for `DesktopCommand::OpenWindow`, which exists on `main` and is rendered on Linux, macOS and Windows. It must share state with the opening window or run in the same engine.
2. A public image source from raw pixels (RGBA or BGRA plus size), which needs code in `ImageSrc` and each backend's image path. It must replace the picture without blanking and skip base64 and hashing on the engine thread.
3. Wheel events on views, the pressed button on press events, key-up events, and modifier-change events. Each one is a new event kind dispatched on every backend.
4. A stated compatibility policy for the Rust API.
5. A Rust example of a styled component kit, or a decision to write and own one.

With items 1-4 upstream, the per-backend code disappears and the port becomes the view rewrite plus the style layer. The spike is the starting point for that re-evaluation.
