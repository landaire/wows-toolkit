import type { JSX } from "nsh";
import { signal, Text, View } from "nsh";
import { Checkbox, Dialog, Menu, Progress } from "nsh/base";
import { GameData, type GameDataSnapshot, type JobReport } from "wows-toolkit/native";
import { Button, centeredDialog, MenuRow, Surface } from "./components";
import { t } from "./i18n";
import { CheckMark } from "./icons";
import { CommitInput } from "./shell/controls";
import { showError } from "./shell/notices";
import { clearStatusJob, setStatusJob } from "./shell/status";
import * as toast from "./shell/toast";
import { colors, controlRadius } from "./theme";

const snapshot = signal<GameDataSnapshot | null>(null);
const actionError = signal<string | null>(null);
const ticked = signal<number[]>([]);
let watching = false;
/** The newest job report already said; `null` until one has been. */
let lastSaid: number | null = null;

/** How long a failure may be before it is worth a window rather than a toast. */
const TOO_LONG_TO_TOAST = 160;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Says a finished job's report wherever the reader is. */
function say(job: JobReport) {
  switch (job.level) {
    case "ok":
      toast.ok(job.text);
      return;
    case "warn":
      toast.warn(job.text);
      return;
    case "failed":
      // A sentence is a toast; a chain of causes is read at length and copied.
      if (job.text.length > TOO_LONG_TO_TOAST || job.text.includes("\n")) showError(job.text);
      else toast.failed(job.text);
  }
}

/** Takes a new snapshot: the reports not yet said, and the running jobs onto the status strip. */
function adopt(next: GameDataSnapshot) {
  snapshot.set(next);

  for (const job of next.reports) {
    if (lastSaid !== null && job.id <= lastSaid) continue;
    lastSaid = job.id;
    say(job);
  }

  const cache = next.cache;
  if (cache.job) {
    const job = cache.job;
    setStatusJob("game-data-cache", { label: () => cacheJobText(job), done: cache.progressDone ?? undefined, total: cache.progressTotal ?? undefined });
  } else {
    clearStatusJob("game-data-cache");
  }
  if (next.index.running) {
    setStatusJob("replay-index", { label: () => t("ui.app.status_indexing"), done: next.index.done, total: next.index.total });
  } else {
    clearStatusJob("replay-index");
  }
}

/// Starts the service and the change feed once, whichever view mounts first.
function ensureWatching() {
  if (watching) {
    return;
  }
  watching = true;
  void GameData.start()
    .then((next) => {
      adopt(next);
      return GameData.watch(adopt);
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

/** The cache maintenance jobs, under one menu as the other front ends keep them. */
function CacheActions(props: { busy: boolean }) {
  const trigger = (
    <View
      role="button"
      accessibilityLabel={t("ui.settings.wows.cache.actions")}
      accessibilityState={{ disabled: props.busy }}
      tooltip={t("ui.settings.wows.cache.actions_tooltip")}
      hoverStyle={props.busy ? undefined : { backgroundColor: colors.accent }}
      style={{ minHeight: 30, paddingHorizontal: 10, justifyContent: "center", borderWidth: 1, borderColor: colors.border, borderRadius: controlRadius(), opacity: props.busy ? 0.5 : 1 }}
    >
      <Text style={{ fontSize: 12.5, color: colors.foreground }}>{t("ui.settings.wows.cache.actions")}</Text>
    </View>
  );
  return (
    <If when={!props.busy} else={trigger}>
      <Menu align="start" offset={4} margin={8} trigger={trigger}>
        <Surface style={{ minWidth: 200, padding: 4, backgroundColor: colors.popover }}>
          <MenuRow label={t("ui.settings.wows.cache.check_updates")} onSelect={() => void run(() => GameData.checkCacheUpdates())}/>
          <MenuRow label={t("ui.settings.wows.cache.validate")} onSelect={() => void run(() => GameData.validateCache())}/>
        </Surface>
      </Menu>
    </If>
  );
}

/// The Settings section for the game-data cache: where it is, what it holds,
/// and the maintenance that keeps it matching the published repository.
/// Everything below the directory needs a measured cache holding a build.
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
        indicator={<CheckMark checked={cache()?.autoDump ?? false}/>}
        onChange={({ checked }) => void run(() => GameData.setAutoDump(checked))}
      >
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{t("ui.settings.wows.cache.auto_dump")}</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{t("ui.game_data.auto_dump_hint")}</Text>
        </View>
      </Checkbox>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <Text style={{ fontSize: 12.5, color: colors.cardForeground }}>{t("ui.settings.wows.cache.directory_label")}</Text>
        <View style={{ flex: 1 }}>
          <CommitInput
            label={t("ui.game_data.cache_directory_label")}
            value={cache()?.directorySetting ?? ""}
            placeholder={cache()?.defaultDirectory ?? ""}
            onCommit={(value) => void run(() => GameData.setCacheDirectory(value))}
          />
        </View>
        <Button label={t("ui.settings.wows.cache.browse")} variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.pickCacheDirectory())}/>
      </View>
      <If when={cache()?.directory != null && builds() > 0}>
        <Row>
          <Line label={t("ui.game_data.cache_size_label")} text={t("ui.settings.wows.cache.stats", { size: formatBytes(cache()?.sizeBytes ?? 0), count: builds() })}/>
          <Button label={t("ui.settings.wows.cache.open_folder")} variant="ghost" onPress={() => void run(() => GameData.openCacheFolder())}/>
          <If when={builds() > 1}>
            <Button label={t("ui.settings.wows.cache.delete_old")} variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.deleteOldCacheBuilds())}/>
          </If>
        </Row>
        <Row>
          <CacheActions busy={busy()}/>
          <If when={busy()}>
            <View style={{ width: 48 }}><Progress value={0} indeterminate accessibilityLabel={cacheJobText(cache()?.job ?? "")}/></View>
          </If>
        </Row>
        <If when={(cache()?.progressTotal ?? 0) > 0}>
          <Line tone="muted" text={`${cache()?.progressDone ?? 0} / ${cache()?.progressTotal ?? 0}`}/>
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
        <If when={cache()?.failure}>
          <Line tone="error" text={cache()!.failure!}/>
        </If>
      </If>
      <If when={actionError()}>
        <Line tone="error" text={actionError()!}/>
      </If>
    </Section>
  );
}

function toggleBuild(build: number, checked: boolean) {
  const current = ticked().filter((candidate) => candidate !== build);
  ticked.set(checked ? [...current, build] : current);
}

/// The offer to download the game data the listed replays need, mounted once
/// over the whole window so it opens on whichever tab asked.
export function MissingBuildsDialog() {
  ensureWatching();
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
              indicator={<CheckMark checked={ticked().includes(row().build)}/>}
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
