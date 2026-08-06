# Send All Replays to ShipBuilds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add command palette actions that upload every unique replay in all open sources to ShipBuilds with cache-aware behavior, a shared ShipBuilds HTTP connection pool, and status-area progress.

**Architecture:** Add a domain-specific `ShipBuildsClient` wrapper owned by `TabState` and cloned into every ShipBuilds consumer so all clones share one blocking reqwest client. Snapshot and deduplicate paths from all workspaces on the UI thread, then give the immutable batch to a standard `BackgroundTask` whose worker parses and uploads sequentially, persists successful sends, and reports count progress over a channel.

**Tech Stack:** Rust 1.92, edition 2024, eframe/egui, reqwest blocking client, std channels and threads, sqlx SQLite, jj.

## Global Constraints

- Work in the existing jj-colocated working copy and use `jj`, not `git`.
- Make one focused commit per logical milestone and do not add AI attribution.
- Before each milestone commit, dispatch a fresh adversarial code-review subagent as required by `AGENTS.md`; resolve its findings and rerun verification.
- Use TDD for every production behavior: add a focused test, observe the expected failure, implement minimally, and observe the pass.
- Keep code, comments, UI strings, tests, and commit messages ASCII-only.
- Honor the configured `DataSharingMode`, game-type eligibility, and test-ship rules.
- Support old 0.6.x and current replay versions through the existing version-aware parser path.
- Deduplicate the same replay path across open sources.
- The ignore-cache action is visible only when runtime `debug_mode` is enabled.
- All ShipBuilds traffic shares one application-session blocking reqwest client and connection pool.

---

## File Structure

- Create `crates/wows-toolkit/src/data/shipbuilds.rs`: domain wrapper around the shared HTTP client and ShipBuilds endpoints.
- Modify `crates/wows-toolkit/src/data/mod.rs`: export the ShipBuilds client module.
- Modify `crates/wows-toolkit/src/data/match_stats.rs`: accept the shared wrapper instead of owning an independently constructed client.
- Modify `crates/wows-toolkit/src/data/wows_data.rs`: carry the shared client in replay dependencies and remove the per-player debug client construction.
- Modify `crates/wows-toolkit/src/tab_state.rs`: own the shared client, expose deduplicated open-workspace replay paths, and pass the client into all dependency bundles.
- Modify `crates/wows-toolkit/src/task/replays.rs`: pass the shared client to the background parser and implement the send-all worker.
- Modify `crates/wows-toolkit/src/task/replay_upload.rs`: centralize upload execution and expose testable cache/outcome behavior.
- Modify `crates/wows-toolkit/src/task/mod.rs`: define send progress, task kind, exports, completion, and status rendering.
- Modify `crates/wows-toolkit/src/ui/command_palette.rs`: add cache-policy actions and runtime-debug entry filtering.
- Modify `crates/wows-toolkit/src/app.rs`: construct palette entries with debug state and dispatch the batch snapshot.

### Task 1: One shared ShipBuilds HTTP client

**Files:**
- Create: `crates/wows-toolkit/src/data/shipbuilds.rs`
- Modify: `crates/wows-toolkit/src/data/mod.rs`
- Modify: `crates/wows-toolkit/src/data/match_stats.rs`
- Modify: `crates/wows-toolkit/src/data/wows_data.rs`
- Modify: `crates/wows-toolkit/src/tab_state.rs`
- Modify: `crates/wows-toolkit/src/task/replays.rs`
- Modify: `crates/wows-toolkit/src/profiling.rs`

**Interfaces:**
- Produces: `ShipBuildsClient::new() -> Result<ShipBuildsClient, reqwest::Error>`
- Produces: `ShipBuildsClient::http(&self) -> &reqwest::blocking::Client`
- Produces: `ShipBuildsClient: Clone`, with clones backed by the same `Arc<reqwest::blocking::Client>`.
- Consumes: `crate::util::http::blocking_client()` for standard client configuration.

- [ ] **Step 1: Write failing shared-identity and dependency tests**

In `data/shipbuilds.rs`, add a unit test that clones a `ShipBuildsClient` and verifies `std::ptr::eq(original.http(), clone.http())`. Update the existing `match_stats` test helper and `ReplayDependencies` fixtures to require a `ShipBuildsClient`; compilation must expose every call site that still constructs or expects a raw client.

```rust
#[test]
fn clones_share_one_http_client() {
    let original = ShipBuildsClient::new().expect("test HTTP client");
    let clone = original.clone();
    assert!(std::ptr::eq(original.http(), clone.http()));
}
```

- [ ] **Step 2: Run tests to verify RED**

Run: `cargo test -p wows-toolkit data::shipbuilds::tests::clones_share_one_http_client`

Expected: FAIL because `data::shipbuilds` and `ShipBuildsClient` do not exist.

- [ ] **Step 3: Implement the wrapper and application ownership**

Implement the wrapper with no raw-client escape hatch other than a borrowed reference:

```rust
#[derive(Clone)]
pub struct ShipBuildsClient {
    http: Arc<reqwest::blocking::Client>,
}

impl ShipBuildsClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        crate::util::http::blocking_client().map(|http| Self { http: Arc::new(http) })
    }

    pub fn http(&self) -> &reqwest::blocking::Client {
        &self.http
    }
}
```

Add one field to `TabState`, initialize it once in `TabState::default` with `ShipBuildsClient::new().expect("failed to build ShipBuilds HTTP client")`, and clone it into `ReplayDependencies` and `BackgroundParserThread`. This preserves the existing fatal startup behavior for failure to build the background parser's HTTP client while moving construction to the single application-owned boundary. Change `MatchStatsClient` to store `ShipBuildsClient`. Replace the background parser's local client and the replay loader's per-player debug clients with the injected wrapper. Keep endpoint selection and request payloads unchanged.

- [ ] **Step 4: Verify GREEN and absence of stray ShipBuilds clients**

Run:

```powershell
cargo test -p wows-toolkit data::shipbuilds::tests::clones_share_one_http_client
cargo check -p wows-toolkit --all-features
rg -n "Client::new\(|blocking_client\(\)" crates/wows-toolkit/src/data/match_stats.rs crates/wows-toolkit/src/data/wows_data.rs crates/wows-toolkit/src/task/replays.rs
```

Expected: tests and check PASS; the search finds no HTTP-client construction in ShipBuilds request paths.

- [ ] **Step 5: Request adversarial review and commit the milestone**

Dispatch a fresh reviewer to challenge client ownership, initialization errors, thread safety, and whether every ShipBuilds call site really shares the same underlying client. Resolve findings, rerun Step 4, then commit:

```powershell
jj commit -m "refactor(toolkit): share one ShipBuilds HTTP client"
```

### Task 2: Palette actions and unique all-workspace snapshot

**Files:**
- Modify: `crates/wows-toolkit/src/ui/command_palette.rs`
- Modify: `crates/wows-toolkit/src/tab_state.rs`
- Modify: `crates/wows-toolkit/src/app.rs`

**Interfaces:**
- Produces: `SendReplayCachePolicy::{UseLedger, IgnoreLedger}`.
- Produces: `PaletteAction::SendAllReplaysToShipBuilds { cache_policy: SendReplayCachePolicy }`.
- Produces: `CommandPalette::root_entries(&self, debug_mode: bool)`.
- Produces: `TabState::open_replay_paths(&self) -> BTreeSet<PathBuf>`.

- [ ] **Step 1: Write failing palette visibility tests**

Replace the existing zero-argument root-entry fixture calls and add exact-label assertions:

```rust
#[test]
fn shipbuilds_entries_follow_runtime_debug_mode() {
    let palette = CommandPalette::default();
    let normal = palette.root_entries(false);
    assert!(normal.iter().any(|entry| entry.title == "Send all replays to ShipBuilds"));
    assert!(!normal.iter().any(|entry| entry.title == "Send all replays to ShipBuilds (ignore cache)"));

    let debug = palette.root_entries(true);
    assert!(debug.iter().any(|entry| entry.title == "Send all replays to ShipBuilds (ignore cache)"));
}
```

- [ ] **Step 2: Run the palette test to verify RED**

Run: `cargo test -p wows-toolkit ui::command_palette::tests::shipbuilds_entries_follow_runtime_debug_mode`

Expected: FAIL because `root_entries` has no debug argument and neither action exists.

- [ ] **Step 3: Implement the cache policy and palette entries**

Define the named cache-policy enum in `task/replay_upload.rs`, re-export it from `task/mod.rs`, add the palette action, and pass the current `persisted.settings.app.debug_mode` from the render call in `app.rs`. Add the standard entry unconditionally and the ignore-ledger entry only inside `if debug_mode`.

- [ ] **Step 4: Write failing workspace deduplication test**

In `tab_state.rs`, construct live and ad-hoc workspaces whose `replay_files` maps overlap and assert the returned set includes every distinct path exactly once:

```rust
#[test]
fn open_replay_paths_deduplicate_across_every_workspace() {
    let mut state = TabState::default();
    let shared = PathBuf::from("replays/shared.wowsreplay");
    let live_only = PathBuf::from("replays/live.wowsreplay");
    let ad_hoc_only = PathBuf::from("import/ad-hoc.wowsreplay");
    insert_listed_paths(&mut state.live_workspace, [&shared, &live_only]);
    let id = state.open_directory_workspace(PathBuf::from("import"));
    insert_listed_paths(state.workspace_mut(id).unwrap(), [&shared, &ad_hoc_only]);

    assert_eq!(state.open_replay_paths(), BTreeSet::from([shared, live_only, ad_hoc_only]));
}
```

- [ ] **Step 5: Run the workspace test to verify RED**

Run: `cargo test -p wows-toolkit tab_state::tests::open_replay_paths_deduplicate_across_every_workspace`

Expected: FAIL because `open_replay_paths` does not exist.

- [ ] **Step 6: Implement the immutable path snapshot**

Implement `open_replay_paths` over `all_workspaces()`, flattening only the keys of each populated `replay_files` map into a `BTreeSet<PathBuf>`. Do not walk roots or include replay tabs separately because listed files are the source of truth and open tabs already originate from listings.

- [ ] **Step 7: Verify GREEN**

Run:

```powershell
cargo test -p wows-toolkit ui::command_palette::tests
cargo test -p wows-toolkit tab_state::tests::open_replay_paths_deduplicate_across_every_workspace
```

Expected: PASS.

- [ ] **Step 8: Request adversarial review and commit the milestone**

Dispatch a fresh reviewer to challenge runtime debug visibility, source completeness, path identity, and overlap behavior. Resolve findings, rerun Step 7, then commit:

```powershell
jj commit -m "feat(toolkit): add ShipBuilds batch palette actions"
```

### Task 3: Cache-aware upload unit and batch worker

**Files:**
- Modify: `crates/wows-toolkit/src/task/replay_upload.rs`
- Modify: `crates/wows-toolkit/src/task/replays.rs`
- Modify: `crates/wows-toolkit/src/task/mod.rs`

**Interfaces:**
- Consumes: `ShipBuildsClient`, `ReplayDependencies`, `DataSharingMode`, sent ledger, SQLite pool/runtime, and `BTreeSet<PathBuf>`.
- Produces: `SendReplayCachePolicy::should_attempt(&self, ledger_contains: bool) -> bool`.
- Produces: `SendAllReplaysProgress { completed: ReplayCount, total: ReplayCount }` with a safe `fraction() -> f32`.
- Produces: `start_send_all_replays_to_shipbuilds(...) -> BackgroundTask`.
- Produces: `BackgroundTaskCompletion::ReplaysSentToShipBuilds { attempted: ReplayCount, sent: ReplayCount, total: ReplayCount }`.

- [ ] **Step 1: Write failing cache-policy and progress tests**

Use newtypes for counts and assert the exact boundary behavior:

```rust
#[test]
fn normal_policy_skips_ledger_entries_but_debug_policy_does_not() {
    assert!(!SendReplayCachePolicy::UseLedger.should_attempt(true));
    assert!(SendReplayCachePolicy::UseLedger.should_attempt(false));
    assert!(SendReplayCachePolicy::IgnoreLedger.should_attempt(true));
}

#[test]
fn empty_progress_has_a_safe_fraction() {
    let progress = SendAllReplaysProgress::new(ReplayCount(0), ReplayCount(0));
    assert_eq!(progress.fraction(), 0.0);
}
```

- [ ] **Step 2: Run focused tests to verify RED**

Run: `cargo test -p wows-toolkit task::replay_upload::tests`

Expected: FAIL because the cache-policy behavior and progress types do not exist.

- [ ] **Step 3: Extract one parsed-replay upload function**

Move the request-producing portion of `parse_replay_data_in_background` into a function with an outcome that distinguishes policy skip, completed send, and transient failure:

```rust
pub(crate) enum ShipBuildsUploadOutcome {
    Skipped,
    Sent,
    TransientFailure,
}

pub(crate) fn upload_parsed_replay(
    path: &Path,
    replay: &Replay,
    report: &BattleReport,
    metadata: &dyn GameMetadataProvider,
    mode: DataSharingMode,
    client: &ShipBuildsClient,
) -> ShipBuildsUploadOutcome;
```

Preserve `decide_upload_action`, battle-type checks, confirmed non-test handling, build payload generation, raw replay bytes, and feature-selected endpoints. Treat transport connection errors as transient exactly as the current parser does. Do not match formatted error strings.

- [ ] **Step 4: Keep the existing background parser green**

Change `parse_replay_data_in_background` to call `upload_parsed_replay` and map its result to the existing `ParseOutcome`. Run:

```powershell
cargo test -p wows-toolkit task::replay_upload::tests
cargo test -p wows-toolkit data::replay_reconcile::tests
```

Expected: PASS with unchanged existing upload decisions.

- [ ] **Step 5: Write failing batch orchestration tests**

Extract a synchronous `run_send_all_replays_to_shipbuilds` core that accepts an injected per-path operation in tests. Cover ordered progress and ledger behavior without sleeping or asserting thread timing:

```rust
#[test]
fn batch_advances_for_cached_skipped_failed_and_sent_paths() {
    let paths = BTreeSet::from([path("cached"), path("failed"), path("sent")]);
    let result = run_test_batch(paths, SendReplayCachePolicy::UseLedger, ledger(["cached"]), |path| {
        if path.ends_with("failed") { TestOutcome::TransientFailure } else { TestOutcome::Sent }
    });
    assert_eq!(result.progress.last(), Some(&progress(3, 3)));
    assert_eq!(result.sent_paths, set(["sent"]));
}

#[test]
fn ignore_ledger_attempts_and_records_an_already_cached_path() {
    let result = run_test_batch(
        BTreeSet::from([path("cached")]),
        SendReplayCachePolicy::IgnoreLedger,
        ledger(["cached"]),
        |_| TestOutcome::Sent,
    );
    assert_eq!(result.attempted, ReplayCount(1));
    assert_eq!(result.progress.last(), Some(&progress(1, 1)));
}
```

- [ ] **Step 6: Run batch tests to verify RED**

Run: `cargo test -p wows-toolkit task::replays::tests::send_all`

Expected: FAIL because the batch core and task constructor do not exist.

- [ ] **Step 7: Implement the worker and durable sent ledger updates**

For each unique path:

1. Read the ledger only long enough to decide whether to attempt.
2. If attempting, use `ReplayLoader::build_replay_from_path`, resolve versioned data, parse the replay, and call `upload_parsed_replay` with the shared client and current `DataSharingMode` snapshot.
3. On `Sent`, insert the path into the in-memory set and call `wows_toolkit_config::queries::insert_sent_replay` through the supplied runtime/pool. Attach path context to persistence errors; log and continue without claiming durable success if persistence fails.
4. Log path-level parse/read/upload failures and continue.
5. Send progress after every path regardless of outcome. Ignore progress-channel disconnection and continue processing.

The spawned wrapper sends the initial `0 / total` progress value, runs the synchronous core, and sends its completion result through the standard task receiver. It creates no HTTP client.

- [ ] **Step 8: Verify GREEN**

Run:

```powershell
cargo test -p wows-toolkit task::replays::tests::send_all
cargo test -p wows-toolkit task::replay_upload::tests
cargo check -p wows-toolkit --all-features
```

Expected: PASS.

- [ ] **Step 9: Request adversarial review and commit the milestone**

Dispatch a fresh reviewer to challenge consent enforcement, old-version parsing, test-ship safety, ledger race windows, persistence semantics, progress completeness, error classification, and shared-client reuse. Resolve findings, rerun Step 8, then commit:

```powershell
jj commit -m "feat(toolkit): upload open replay sources to ShipBuilds"
```

### Task 4: Dispatch, status UI, and completion handling

**Files:**
- Modify: `crates/wows-toolkit/src/app.rs`
- Modify: `crates/wows-toolkit/src/task/mod.rs`
- Modify: `crates/wows-toolkit/src/ui/command_palette.rs`

**Interfaces:**
- Consumes: `TabState::open_replay_paths()` and `start_send_all_replays_to_shipbuilds(...)`.
- Produces: `BackgroundTaskKind::SendingAllReplaysToShipBuilds { rx, last_progress }` status behavior.

- [ ] **Step 1: Write failing status-description tests**

Extract a pure formatter from the task kind so UI copy is testable without rendering egui:

```rust
#[test]
fn shipbuilds_progress_text_reports_completed_and_total() {
    let progress = SendAllReplaysProgress::new(ReplayCount(7), ReplayCount(12));
    assert_eq!(shipbuilds_progress_text(progress), "Sending replays to ShipBuilds: 7 / 12");
}
```

- [ ] **Step 2: Run the status test to verify RED**

Run: `cargo test -p wows-toolkit task::tests::shipbuilds_progress_text_reports_completed_and_total`

Expected: FAIL because the formatter and task kind do not exist.

- [ ] **Step 3: Implement status rendering and completion handling**

Add the task kind, drain all available progress updates each frame, retain the newest update, and render:

```rust
ui.add(
    egui::ProgressBar::new(progress.fraction())
        .text(shipbuilds_progress_text(*progress)),
);
```

Render a spinner plus `Sending replays to ShipBuilds` before progress arrives. For total zero, show the safe zero state until the completion receiver lands. Add an exhaustive no-op completion arm or a concise success toast for `ReplaysSentToShipBuilds`; do not show success for task errors.

- [ ] **Step 4: Dispatch the selected policy from the palette**

In `dispatch_palette_action`, snapshot `open_replay_paths`, clone the shared client, `wows_data_map`, sent ledger, database pool/runtime, personal rating dependencies, and the current configured `DataSharingMode`, then push the returned background task with `update_background_task!`. When `wows_data_map`, the database pool, or the Tokio runtime is absent, add an error toast reading `Cannot send replays to ShipBuilds before game data and application storage are ready` and do not start a task.

- [ ] **Step 5: Run focused and workspace verification**

Run:

```powershell
cargo test -p wows-toolkit ui::command_palette::tests
cargo test -p wows-toolkit task::tests::shipbuilds_progress_text_reports_completed_and_total
cargo test -p wows-toolkit task::replays::tests::send_all
cargo test -p wows-toolkit
cargo check --workspace --all-targets --all-features
cargo fmt --all -- --check
```

Expected: all commands PASS with no warnings introduced by the change.

- [ ] **Step 6: Request final adversarial review and commit**

Dispatch a fresh reviewer to challenge the end-to-end design, especially source completeness, duplicate tasks, UI-thread blocking, status cleanup, failure UX, and whether any ShipBuilds request still constructs a separate client. Resolve findings and rerun Step 5 plus:

```powershell
rg -n "Client::new\(|blocking_client\(\)" crates/wows-toolkit/src
jj diff --stat
jj status
```

Account for every remaining client constructor as non-ShipBuilds traffic or the single `ShipBuildsClient` constructor. Then commit:

```powershell
jj commit -m "feat(toolkit): show ShipBuilds batch upload progress"
```

### Task 5: Final verification

**Files:**
- Verify only; modify files only to resolve findings.

**Interfaces:**
- Consumes the completed feature and all prior milestone tests.
- Produces verification evidence suitable for completion handoff.

- [ ] **Step 1: Run the complete verification suite from a clean jj child**

Run:

```powershell
jj status
cargo fmt --all -- --check
cargo test -p wows-toolkit
cargo check --workspace --all-targets --all-features
```

Expected: clean working copy and all commands PASS.

- [ ] **Step 2: Inspect final history and requirements coverage**

Run:

```powershell
jj log -n 8 --no-graph
rg -n "Send all replays to ShipBuilds|SendingAllReplaysToShipBuilds|ShipBuildsClient" crates/wows-toolkit/src
```

Confirm both exact palette labels, runtime-only debug visibility, all-workspace deduplication, ledger bypass only for the debug action, persistent success recording, progress to total, and one shared ShipBuilds client.

- [ ] **Step 3: Use verification-before-completion and report evidence**

Invoke `superpowers:verification-before-completion`, rerun any command it requires, and report the milestone commit IDs and exact verification results. Do not claim completion based on earlier output.
