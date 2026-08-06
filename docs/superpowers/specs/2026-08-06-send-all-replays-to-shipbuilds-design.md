# Send All Replays to ShipBuilds Design

## Goal

Add command palette actions that send every unique replay represented by an open replay source to ShipBuilds while reporting batch progress in the application's status area.

## Command palette

The root command palette includes an action labeled exactly `Send all replays to ShipBuilds`.

When the runtime application setting `debug_mode` is enabled, the root palette also includes an action labeled exactly `Send all replays to ShipBuilds (ignore cache)`. The debug action is absent when `debug_mode` is disabled. This visibility is controlled by the runtime setting, not by Rust debug assertions or the `shipbuilds_debugging` Cargo feature.

Both entries dispatch the same batch operation with a distinct cache policy:

- The standard action skips replay paths present in the local sent-replay ledger.
- The debug action ignores the ledger when deciding what to send.
- Both actions record a replay in the ledger after its upload completes successfully.

## Replay selection

At dispatch time, the main thread snapshots replay paths represented by every currently open replay workspace. This includes the live workspace rooted at the configured game replay directory and all open ad-hoc directory workspaces.

Paths are deduplicated across workspaces before the worker starts. If the same replay is represented in more than one source, the batch considers it once. The snapshot defines the batch: sources opened, closed, or changed after dispatch do not alter an in-flight batch.

The operation uses the replay paths already represented by the open workspaces. It does not independently rescan workspace roots for files that have not entered the workspace listing.

## Background task and progress

The batch uses the application's standard `BackgroundTask` design. Dispatch creates a completion channel and a progress channel, spawns one named worker thread, and adds a dedicated background task kind to `TabState::background_tasks`.

Progress is a pair of domain counts: completed replay count and total replay count. The total is the number of unique paths in the dispatch snapshot. The worker sends an initial progress value and then advances completed once for each considered path, including paths skipped because they are cached, ineligible under upload policy, unreadable, or unparseable. This ensures that every non-aborted batch reaches its total.

The status area drains the progress channel without blocking and retains the newest value. While the total is known and nonzero, it renders an `egui::ProgressBar` with a `completed / total` ShipBuilds label. Before progress is available, it renders a spinner and label. An empty batch completes normally without division by zero.

Only one worker is created for an invocation. A second invocation may coexist as another standard background task; it receives its own immutable path snapshot and progress channel.

## Shared ShipBuilds HTTP client

The application creates one `reqwest::blocking::Client` for ShipBuilds communication and shares it as an `Arc` through the dependencies of every ShipBuilds caller. The client is constructed with the existing blocking HTTP client builder so application-wide user-agent, timeout, and transport configuration remain consistent.

The shared client covers the existing background build uploader, raw replay uploader, match-stats client, debug build-upload path, and the new send-all batch. No ShipBuilds call site creates its own client, and no loop creates a client per replay, player, or payload. Tests may inject a client through the same dependency boundary.

A named wrapper such as `ShipBuildsClient` owns the `Arc<reqwest::blocking::Client>` so the domain-specific dependency cannot be confused with unrelated HTTP clients. Clones of the wrapper share the same underlying reqwest client and connection pool. Endpoint selection remains at the request-producing boundary, including the existing `shipbuilds_debugging` endpoint behavior.

The shared client lifetime is the application session. This provides connection reuse between independent ShipBuilds features, not only within one batch. It is dedicated to ShipBuilds because all known consumers use the blocking API and share one remote service; unrelated HTTP traffic keeps its existing clients and policies.

## Upload behavior

The worker processes paths sequentially and receives the application-owned `ShipBuildsClient`. It passes that client through every replay upload and never constructs an HTTP client itself.

The worker reuses the existing replay parsing and ShipBuilds upload decision logic rather than introducing a second interpretation of data-sharing settings.

The configured data-sharing mode remains authoritative. Existing game-type eligibility, test-ship handling, build-data versus raw-replay selection, endpoint selection, and payload construction continue to apply. The explicit palette action does not override consent or eligibility rules.

The cache policy affects only the local sent-ledger gate. Ignoring cache does not change upload eligibility and does not disable recording successful sends.

The batch must not perform unrelated replay UI work such as opening tabs or changing the active workspace. Any indexing or player-tracker side effects are limited to behavior inseparable from the reused parsing path; the implementation should extract a narrower upload unit when practical so the batch has upload-focused behavior.

## Sent ledger and errors

A replay is added to the in-memory sent set and persisted to SQLite only when the existing upload outcome considers the send complete. Transient upload failures remain absent from the ledger so a later standard run can retry them.

A failure affecting one replay is logged with its path and does not abort the remaining batch. Setup failures that make the entire batch impossible are returned through the background task completion channel and use the application's existing task error handling. Channel disconnection must not block the worker.

## Testing

Tests use the existing command palette and background-task test patterns and cover:

- The standard entry is present with the exact requested label.
- The ignore-cache entry has the exact requested label, is hidden outside runtime debug mode, and is visible in runtime debug mode.
- Paths from the live workspace and all open ad-hoc workspaces are selected.
- Identical paths represented by multiple workspaces are selected once.
- The standard cache policy skips ledger entries.
- The ignore-cache policy considers ledger entries for upload while preserving normal eligibility rules.
- The background build uploader, raw replay uploader, match-stats client, debug build uploader, and send-all task receive clones backed by the same HTTP client instance.
- Repeated requests within a replay or batch use that same client instance.
- Successful sends are recorded under both cache policies, while transient failures are not.
- Progress begins with the deduplicated total, advances for every considered path, and reaches completion despite per-file skips or failures.
- Empty input completes without a divide-by-zero or stuck status task.

Tests for worker orchestration isolate network transport behind the narrowest existing or extracted upload interface. They assert observable upload decisions, ledger mutations, progress, and completion rather than implementation-specific thread timing.

## Constraints

All code, comments, UI strings, tests, and commit messages remain ASCII-only. Domain counts and cache policy use named types rather than ambiguous primitive parameters where they cross component boundaries. Missing data and upload errors are propagated until the worker has enough path-level context to log and continue or task-level context to fail the batch.
