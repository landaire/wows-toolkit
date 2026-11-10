import type { JSX } from "nsh";
import { signal, Text, View } from "nsh";
import { Checkbox, Dialog } from "nsh/base";
import { GameData, type GameDataSnapshot } from "wows-toolkit/native";
import { Button, centeredDialog, Surface } from "./components";
import { t } from "./i18n";
import { colors } from "./theme";

const snapshot = signal<GameDataSnapshot | null>(null);
const actionError = signal<string | null>(null);
const ticked = signal<number[]>([]);
let watching = false;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/// Starts the service and the change feed once, whichever view mounts first.
function ensureWatching() {
  if (watching) {
    return;
  }
  watching = true;
  void GameData.start()
    .then((next) => {
      snapshot.set(next);
      return GameData.watch((changed) => snapshot.set(changed));
    })
    .catch((error) => actionError.set(errorMessage(error)));
}

/// Runs one service call, putting its failure on the section's error line.
async function run<T>(call: () => Promise<T>): Promise<T | undefined> {
  actionError.set(null);
  try {
    return await call();
  } catch (error) {
    actionError.set(errorMessage(error));
    return undefined;
  }
}

function formatBytes(bytes: number): string {
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${value} ${units[unit]}` : `${value.toFixed(2)} ${units[unit]}`;
}

function statusText(state: GameDataSnapshot | null): string {
  if (!state) {
    return actionError() ? t("ui.game_data.status_unavailable", { error: actionError()! }) : t("ui.game_data.status_starting");
  }
  switch (state.phase) {
    case "notConfigured":
      return state.reason ? t("ui.game_data.status_reason", { reason: state.reason }) : t("ui.game_data.status_not_configured");
    case "loading":
      return state.version ? t("ui.replay.loading_game_data_version", { version: state.version }) : t("ui.replay.loading_game_data");
    case "ready":
      return t("ui.game_data.status_ready", { version: state.version ?? t("ui.game_data.unknown_version"), build: state.build ?? "?" });
    case "failed":
      return t("ui.replay.game_data_failed", { error: state.reason ?? t("ui.game_data.unknown_reason") });
  }
}

/// The status-bar indicator: what game data is loaded, and the index pass
/// while one runs.
export function GameDataStatus() {
  ensureWatching();
  const state = () => snapshot();
  const indexing = () => {
    const index = state()?.index;
    return index?.running ? `${t("ui.app.status_indexing")} ${t("ui.app.status_of", { done: index.done, total: index.total })}` : null;
  };
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 12 }}>
      <Text
        accessibilityLabel={t("ui.game_data.status_label")}
        style={{ fontSize: 11.5, color: state()?.phase === "failed" ? colors.destructive : colors.mutedForeground }}
      >
        {statusText(state())}
      </Text>
      <If when={indexing()}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{indexing()}</Text>
      </If>
    </View>
  );
}

function Section(props: { title: string; description: string; children: JSX.Element }) {
  return (
    <Surface style={{ padding: 16, gap: 12 }}>
      <View style={{ gap: 3 }}>
        <Text style={{ fontSize: 16, fontWeight: "600", color: colors.cardForeground }}>{props.title}</Text>
        <Text style={{ fontSize: 12, lineHeight: 17, color: colors.mutedForeground }}>{props.description}</Text>
      </View>
      {props.children}
    </Surface>
  );
}

function Line(props: { text: string; tone?: "muted" | "error" | "normal"; label?: string }) {
  const color = () => props.tone === "error" ? colors.destructive : props.tone === "muted" ? colors.mutedForeground : colors.cardForeground;
  return <Text accessibilityLabel={props.label} style={{ fontSize: 12.5, lineHeight: 18, color: color() }}>{props.text}</Text>;
}

function Row(props: { children: JSX.Element }) {
  return <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>{props.children}</View>;
}

function cacheJobText(job: string): string {
  switch (job) {
    case "checking": return t("ui.app.status_cache_checking");
    case "validating": return t("ui.app.status_cache_validating");
    case "downloading": return t("ui.app.status_cache_downloading");
    case "planning": return t("ui.app.status_cache_planning");
    default: return job;
  }
}

/// The Settings section for the game-data cache: where it is, what it holds,
/// and the maintenance that keeps it matching the published repository.
export function GameDataCacheSection() {
  ensureWatching();
  const cache = () => snapshot()?.cache;
  const busy = () => cache()?.job != null;
  const builds = () => cache()?.buildCount ?? 0;
  return (
    <Section title={t("ui.settings.wows.cache.heading")} description={t("ui.settings.wows.cache.description")}>
      <Checkbox
        accessibilityLabel={t("ui.settings.wows.cache.auto_dump")}
        checked={cache()?.autoDump ?? false}
        class="group flex-row items-start gap-3"
        indicatorClass="mt-0.5 w-[18px] h-[18px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
        indicator={<Text class="text-[11px] text-primary-foreground">{cache()?.autoDump ? "x" : ""}</Text>}
        onChange={({ checked }) => void run(() => GameData.setAutoDump(checked))}
      >
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{t("ui.settings.wows.cache.auto_dump")}</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{t("ui.game_data.auto_dump_hint")}</Text>
        </View>
      </Checkbox>
      <Row>
        <Line label={t("ui.game_data.cache_directory_label")} text={`${t("ui.settings.wows.cache.directory_label")} ${cache()?.directory ?? t("ui.game_data.unavailable")}`}/>
        <Button label={t("ui.settings.wows.cache.browse")} variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.pickCacheDirectory())}/>
      </Row>
      <If when={cache()?.directory}>
        <Row>
          <Line
            label={t("ui.game_data.cache_size_label")}
            text={cache()?.sizeBytes == null ? t("ui.game_data.measuring") : t("ui.settings.wows.cache.stats", { size: formatBytes(cache()!.sizeBytes!), count: builds() })}
          />
          <Button label={t("ui.settings.wows.cache.open_folder")} variant="ghost" onPress={() => void run(() => GameData.openCacheFolder())}/>
          <If when={builds() > 1}>
            <Button label={t("ui.settings.wows.cache.delete_old")} variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.deleteOldCacheBuilds())}/>
          </If>
        </Row>
        <Row>
          <Button label={t("ui.settings.wows.cache.check_updates")} disabled={busy() || builds() === 0} onPress={() => void run(() => GameData.checkCacheUpdates())}/>
          <Button label={t("ui.settings.wows.cache.validate")} disabled={busy() || builds() === 0} onPress={() => void run(() => GameData.validateCache())}/>
          <Button label={t("ui.game_data.check_replays")} disabled={busy()} onPress={() => void run(() => GameData.checkMissingBuilds())}/>
        </Row>
      </If>
      <If when={cache()?.job}>
        <Line
          tone="muted"
          text={`${cacheJobText(cache()!.job!)}${cache()?.progressTotal ? ` ${cache()!.progressDone}/${cache()!.progressTotal}` : ""}`}
        />
      </If>
      <If when={(cache()?.updates.length ?? 0) > 0}>
        <Row>
          <Line text={t("ui.settings.wows.cache.updates_available", { count: cache()!.updates.length })}/>
          <Button label={t("ui.settings.wows.cache.update_all")} variant="primary" disabled={busy()} onPress={() => void run(() => GameData.updateCacheBuilds())}/>
        </Row>
      </If>
      <If when={(cache()?.repair.length ?? 0) > 0}>
        <Row>
          <Line tone="error" text={t("ui.settings.wows.cache.repair_needed", { count: cache()!.repair.length })}/>
          <Button label={t("ui.settings.wows.cache.repair")} variant="danger" disabled={busy()} onPress={() => void run(() => GameData.repairCacheBuilds())}/>
        </Row>
      </If>
      <If when={cache()?.message}>
        <Line label={t("ui.game_data.cache_message_label")} text={cache()!.message!}/>
      </If>
      <If when={cache()?.failure}>
        <Line tone="error" text={cache()!.failure!}/>
      </If>
      <If when={actionError()}>
        <Line tone="error" text={actionError()!}/>
      </If>
      <MissingBuildsDialog/>
    </Section>
  );
}

function toggleBuild(build: number, checked: boolean) {
  const current = ticked().filter((candidate) => candidate !== build);
  ticked.set(checked ? [...current, build] : current);
}

/// The offer to download the game data the listed replays need.
function MissingBuildsDialog() {
  const offer = () => snapshot()?.offer;
  let seeded = "";
  const rows = () => {
    const current = offer()?.rows ?? [];
    // Every eligible build starts ticked once the repository has answered.
    const key = current.map((row) => `${row.build}:${row.eligible}`).join(",");
    if (key !== seeded) {
      seeded = key;
      ticked.set(current.filter((row) => row.eligible).map((row) => row.build));
    }
    return current;
  };
  return (
    <Dialog style={centeredDialog} open={offer()?.open ?? false} onOpenChange={({ open }) => { if (!open) GameData.dismissMissingBuilds(); }}>
      <Surface style={{ width: "100%", maxWidth: 560, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 18, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.download_game_data")}</Text>
        <Text style={{ fontSize: 13, lineHeight: 19, color: colors.mutedForeground }}>
          {t("ui.dialogs.download_game_data_intro")}
        </Text>
        <If when={offer()?.planning}>
          <Line tone="muted" text={t("ui.dialogs.download_plan_pending")}/>
        </If>
        <Each of={rows()} key={(row) => row.build}>
          {(row) => (
            <Checkbox
              accessibilityLabel={t("ui.game_data.build_label", { build: row().build })}
              checked={ticked().includes(row().build)}
              disabled={!row().eligible}
              class="group flex-row items-start gap-3"
              indicatorClass="mt-0.5 w-[18px] h-[18px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
              indicator={<Text class="text-[11px] text-primary-foreground">{ticked().includes(row().build) ? "x" : ""}</Text>}
              onChange={({ checked }) => toggleBuild(row().build, checked)}
            >
              <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>
                {`${t("ui.dialogs.download_build_row", { version: row().version ?? "?", build: row().build })} -- ${t("ui.dialogs.download_replays_needing", { count: row().replays })} -- ${row().availability}`}
              </Text>
            </Checkbox>
          )}
        </Each>
        <If when={offer()?.objects != null}>
          <Line tone="muted" text={t("ui.dialogs.download_objects_to_fetch", { count: offer()!.objects! })}/>
        </If>
        <If when={offer()?.failure}>
          <Line tone="error" text={offer()!.failure!}/>
        </If>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.cancel")} onPress={() => GameData.dismissMissingBuilds()}/>
          <Button
            label={t("ui.buttons.download")}
            variant="primary"
            disabled={offer()?.planning || ticked().length === 0}
            onPress={() => void run(() => GameData.downloadMissingBuilds(ticked()))}
          />
        </View>
      </Surface>
    </Dialog>
  );
}

/// The Settings section for the replay index: building it, rebuilding it, and
/// how far the pass has got.
export function ReplayIndexSection() {
  ensureWatching();
  const index = () => snapshot()?.index;
  const configured = () => snapshot()?.wowsDir != null;
  const running = () => index()?.running ?? false;
  return (
    <Section title={t("ui.settings.index.heading")} description={t("ui.settings.index.description")}>
      <Row>
        <Button label={t("ui.settings.index.build")} disabled={!configured() || running()} onPress={() => void run(() => GameData.indexReplays("fillGaps"))}/>
        <Button label={t("ui.settings.index.rebuild")} disabled={!configured() || running()} onPress={() => void run(() => GameData.indexReplays("refreshAll"))}/>
        <If when={running()}>
          <Button label={t("ui.buttons.stop")} variant="ghost" onPress={() => GameData.cancelIndexing()}/>
        </If>
      </Row>
      <Line tone="muted" text={t("ui.settings.index.rebuild_hint")}/>
      <If when={!configured()}>
        <Line tone="muted" text={t("ui.game_data.index_needs_dir")}/>
      </If>
      <If when={running()}>
        <Line
          label={t("ui.game_data.index_progress_label")}
          text={index()!.skipped > 0
            ? t("ui.settings.index.progress_skipped", { done: index()!.done, total: index()!.total, skipped: index()!.skipped, failed: index()!.failed })
            : t("ui.settings.index.progress", { done: index()!.done, total: index()!.total, failed: index()!.failed })}
        />
      </If>
      <If when={index()?.outcome}>
        <Line label={t("ui.game_data.index_outcome_label")} tone={index()!.outcomeFailed ? "error" : "normal"} text={index()!.outcome!}/>
      </If>
      <If when={actionError()}>
        <Line tone="error" text={actionError()!}/>
      </If>
    </Section>
  );
}

/// The Settings section for the replay directories open beside the game's
/// own: adding, pinning so they reopen at startup, and closing them.
export function ReplayDirectoriesSection() {
  ensureWatching();
  const directories = () => snapshot()?.directories ?? [];
  return (
    <Section title={t("ui.game_data.directories_heading")} description={t("ui.game_data.directories_description")}>
      <If when={directories().length === 0}>
        <Line tone="muted" text={t("ui.game_data.directories_none")}/>
      </If>
      <Each of={directories()} key={(entry) => entry.path}>
        {(entry) => (
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
            <View style={{ flexGrow: 1, flexBasis: 320, minWidth: 240, gap: 1 }}>
              <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>
                {entry().pinned ? t("ui.game_data.directory_pinned", { label: entry().label }) : entry().label}
              </Text>
              <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{entry().path}</Text>
            </View>
            <Button
              label={t(entry().pinned ? "ui.game_data.unpin_directory" : "ui.game_data.pin_directory", { label: entry().label })}
              variant="ghost"
              onPress={() => void run(() => GameData.pinDirectory(entry().path, !entry().pinned))}
            />
            <Button label={t("ui.game_data.show_directory", { label: entry().label })} variant="ghost" onPress={() => void run(() => GameData.revealDirectory(entry().path))}/>
            <Button
              label={t("ui.game_data.close_directory", { label: entry().label })}
              variant="ghost"
              disabled={entry().pinned}
              onPress={() => void run(async () => GameData.closeDirectory(entry().path))}
            />
          </View>
        )}
      </Each>
      <Row>
        <Button label={t("ui.game_data.add_directory")} onPress={() => void run(() => GameData.pickDirectory())}/>
      </Row>
      <If when={snapshot()?.directoriesError}>
        <Line tone="error" text={snapshot()!.directoriesError!}/>
      </If>
    </Section>
  );
}
