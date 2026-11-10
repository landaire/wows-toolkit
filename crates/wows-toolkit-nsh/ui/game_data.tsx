import type { JSX } from "nsh";
import { signal, Text, View } from "nsh";
import { Checkbox, Dialog } from "nsh/base";
import { GameData, type GameDataSnapshot } from "wows-toolkit/native";
import { Button, Surface } from "./components";
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
  void GameData.watch((next) => snapshot.set(next)).catch((error) => actionError.set(errorMessage(error)));
  void GameData.start().then((next) => snapshot.set(next)).catch((error) => actionError.set(errorMessage(error)));
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
    return "Game data: starting";
  }
  switch (state.phase) {
    case "notConfigured":
      return state.reason ? `Game data: ${state.reason}` : "Game data: no World of Warships directory";
    case "loading":
      return state.version ? `Game data: loading ${state.version}` : "Game data: loading";
    case "ready":
      return `Game data: ${state.version ?? "unknown version"} (build ${state.build ?? "?"})`;
    case "failed":
      return `Game data failed: ${state.reason ?? "unknown reason"}`;
  }
}

/// The status-bar indicator: what game data is loaded, and the index pass
/// while one runs.
export function GameDataStatus() {
  ensureWatching();
  const state = () => snapshot();
  const indexing = () => {
    const index = state()?.index;
    return index?.running ? `Indexing replays ${index.done}/${index.total}` : null;
  };
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 12 }}>
      <Text
        accessibilityLabel="Game data status"
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
    case "checking": return "Checking for updates...";
    case "validating": return "Validating the cache...";
    case "downloading": return "Downloading game data...";
    case "planning": return "Asking the repository what it publishes...";
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
    <Section title="Game data cache" description="Builds the installed game no longer has are read from here, so older replays still open.">
      <Checkbox
        accessibilityLabel="Cache game data automatically"
        checked={cache()?.autoDump ?? false}
        class="group flex-row items-start gap-3"
        indicatorClass="mt-0.5 w-[18px] h-[18px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
        indicator={<Text class="text-[11px] text-primary-foreground">{cache()?.autoDump ? "x" : ""}</Text>}
        onChange={({ checked }) => void run(() => GameData.setAutoDump(checked))}
      >
        <View style={{ flex: 1, gap: 2 }}>
          <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>Cache game data automatically</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Write each build loaded from the game into the cache. A build takes several gigabytes.</Text>
        </View>
      </Checkbox>
      <Row>
        <Line label="Game data cache directory" text={`Directory: ${cache()?.directory ?? "Unavailable"}`}/>
        <Button label="Browse..." variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.pickCacheDirectory())}/>
      </Row>
      <If when={cache()?.directory}>
        <Row>
          <Line
            label="Game data cache size"
            text={cache()?.sizeBytes == null ? "Measuring..." : `${formatBytes(cache()!.sizeBytes!)} in ${builds()} cached builds`}
          />
          <Button label="Open Folder" variant="ghost" onPress={() => void run(() => GameData.openCacheFolder())}/>
          <If when={builds() > 1}>
            <Button label="Delete Old Builds" variant="ghost" disabled={busy()} onPress={() => void run(() => GameData.deleteOldCacheBuilds())}/>
          </If>
        </Row>
        <Row>
          <Button label="Check for Updates" disabled={busy() || builds() === 0} onPress={() => void run(() => GameData.checkCacheUpdates())}/>
          <Button label="Validate Cache" disabled={busy() || builds() === 0} onPress={() => void run(() => GameData.validateCache())}/>
          <Button label="Check Game Data for Replays" disabled={busy()} onPress={() => void run(() => GameData.checkMissingBuilds())}/>
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
          <Line text={`Updates are available for ${cache()!.updates.length} cached builds.`}/>
          <Button label="Update All" variant="primary" disabled={busy()} onPress={() => void run(() => GameData.updateCacheBuilds())}/>
        </Row>
      </If>
      <If when={(cache()?.repair.length ?? 0) > 0}>
        <Row>
          <Line tone="error" text={`${cache()!.repair.length} cached builds need repair.`}/>
          <Button label="Repair" variant="danger" disabled={busy()} onPress={() => void run(() => GameData.repairCacheBuilds())}/>
        </Row>
      </If>
      <If when={cache()?.message}>
        <Line label="Game data cache message" text={cache()!.message!}/>
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
    <Dialog open={offer()?.open ?? false} onOpenChange={({ open }) => { if (!open) GameData.dismissMissingBuilds(); }}>
      <Surface style={{ width: "100%", maxWidth: 560, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 18, fontWeight: "600", color: colors.popoverForeground }}>Download Game Data</Text>
        <Text style={{ fontSize: 13, lineHeight: 19, color: colors.mutedForeground }}>
          Some replays were recorded on builds this machine does not have. Choose the builds to download into the game data cache.
        </Text>
        <If when={offer()?.planning}>
          <Line tone="muted" text="Asking the repository what it publishes..."/>
        </If>
        <Each of={rows()} key={(row) => row.build}>
          {(row) => (
            <Checkbox
              accessibilityLabel={`Build ${row().build}`}
              checked={ticked().includes(row().build)}
              disabled={!row().eligible}
              class="group flex-row items-start gap-3"
              indicatorClass="mt-0.5 w-[18px] h-[18px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
              indicator={<Text class="text-[11px] text-primary-foreground">{ticked().includes(row().build) ? "x" : ""}</Text>}
              onChange={({ checked }) => toggleBuild(row().build, checked)}
            >
              <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>
                {`${row().version ?? "?"} (${row().build}) -- ${row().replays} replays -- ${row().availability}`}
              </Text>
            </Checkbox>
          )}
        </Each>
        <If when={offer()?.objects != null}>
          <Line tone="muted" text={`${offer()!.objects} objects to fetch.`}/>
        </If>
        <If when={offer()?.failure}>
          <Line tone="error" text={offer()!.failure!}/>
        </If>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label="Cancel" onPress={() => GameData.dismissMissingBuilds()}/>
          <Button
            label="Download"
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
    <Section title="Replay index" description="Search, statistics and the player tracker read every replay through this index.">
      <Row>
        <Button label="Build Index" disabled={!configured() || running()} onPress={() => void run(() => GameData.indexReplays("fillGaps"))}/>
        <Button label="Rebuild Index" disabled={!configured() || running()} onPress={() => void run(() => GameData.indexReplays("refreshAll"))}/>
        <If when={running()}>
          <Button label="Stop" variant="ghost" onPress={() => GameData.cancelIndexing()}/>
        </If>
      </Row>
      <Line tone="muted" text="Rebuild reads every replay again, for rows an older version decoded through the wrong mapping."/>
      <If when={!configured()}>
        <Line tone="muted" text="Set the World of Warships directory to index its replays."/>
      </If>
      <If when={running()}>
        <Line
          label="Replay index progress"
          text={`Indexing ${index()!.done} of ${index()!.total}${index()!.skipped > 0 ? `, ${index()!.skipped} skipped` : ""}, ${index()!.failed} could not be read`}
        />
      </If>
      <If when={index()?.outcome}>
        <Line label="Replay index outcome" tone={index()!.outcomeFailed ? "error" : "normal"} text={index()!.outcome!}/>
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
    <Section title="Replay directories" description="Other directories of replays to list and index. Pinned directories reopen at startup.">
      <If when={directories().length === 0}>
        <Line tone="muted" text="No replay directories are open."/>
      </If>
      <Each of={directories()} key={(entry) => entry.path}>
        {(entry) => (
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
            <View style={{ flex: 1, gap: 1 }}>
              <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>
                {entry().pinned ? `${entry().label} [Pinned]` : entry().label}
              </Text>
              <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{entry().path}</Text>
            </View>
            <Button
              label={entry().pinned ? `Unpin ${entry().label}` : `Pin ${entry().label}`}
              variant="ghost"
              onPress={() => void run(() => GameData.pinDirectory(entry().path, !entry().pinned))}
            />
            <Button label={`Show ${entry().label}`} variant="ghost" onPress={() => void run(() => GameData.revealDirectory(entry().path))}/>
            <Button
              label={`Close ${entry().label}`}
              variant="ghost"
              disabled={entry().pinned}
              onPress={() => void run(async () => GameData.closeDirectory(entry().path))}
            />
          </View>
        )}
      </Each>
      <Row>
        <Button label="Add Directory..." onPress={() => void run(() => GameData.pickDirectory())}/>
      </Row>
      <If when={snapshot()?.directoriesError}>
        <Line tone="error" text={snapshot()!.directoriesError!}/>
      </If>
    </Section>
  );
}
