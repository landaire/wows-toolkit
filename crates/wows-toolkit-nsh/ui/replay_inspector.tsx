import type { JSX, Signal } from "nsh";
import { effect, Image, Overlay, Scroll, signal, Text, untracked, View } from "nsh";
import {
  Button as BaseButton,
  Checkbox,
  ContextMenu,
  DockArea,
  Editor,
  Menu,
  MenuItem,
  Popover,
  Progress,
  ResizablePanel,
  ResizablePanelGroup,
  Tooltip,
} from "nsh/base";
import {
  GameData,
  PlayerTable,
  ReplayBrowser,
  ReplayInspectorService,
  Shell,
  type ChatLine,
  type GameDataSnapshot,
  type ReplaySummary,
} from "wows-toolkit/native";
import { Button, MenuRow, Surface } from "./components";
import { t } from "./i18n";
import { appSettings, editReplayPreferences, editSetting } from "./settings_store";
import { debugMode } from "./shell/status";
import * as toast from "./shell/toast";
import { Icon, type IconName } from "./icons";
import { colors, controlRadius, semantic } from "./theme";

type Grouping = "date" | "ship" | "none";
type RawJsonKind = "metadata" | "results" | "mapped";
type ExportFormat = "json" | "cbor" | "csv";
type NoticeLevel = "ok" | "info" | "warn" | "failed";

type DockNode =
  | { type: "split"; id?: number; axis: "horizontal" | "vertical"; children: DockNode[]; sizes?: (number | null)[] }
  | { type: "tabs"; id?: number; panels: string[]; activeIndex?: number };
type TabsNode = Extract<DockNode, { type: "tabs" }>;
type DockLayout = { center: DockNode; left?: unknown; right?: unknown; bottom?: unknown };

type PreviewFact = { label: string; value: string };
type PreviewHover = { path: string; mapName: string; x: number; y: number; facts: PreviewFact[] };
type ListedBuild = { build: number; version: string | null; replays: number; readable: boolean };

type SidePanel = { kind: "chat" } | { kind: "raw"; which: RawJsonKind } | { kind: "player"; json: string };

type TabState =
  | { kind: "loading" }
  | { kind: "failed"; error: string }
  | { kind: "loaded"; summary: ReplaySummary };

/** One replay in a workspace's dock. */
type ReplayTab = {
  id: string;
  path: string;
  state: Signal<TabState>;
  /** Other recordings of the same battle read with it. */
  alts: string[];
  /** Counts reads, so only the newest one lands. */
  generation: number;
  closed: boolean;
  reparsing: Signal<boolean>;
  side: Signal<SidePanel | null>;
  hideSelfStats: Signal<boolean>;
  exportStatus: Signal<string | null>;
};

/** A viewport docked beside the replays, as GPUI docks its renderer and watched-playback panels. */
type ViewportPanel =
  | { kind: "renderer"; path: string; alts: string[]; title: string }
  | { kind: "watched"; replayId: string; title: string };

/** A replay listing with its own dock of replays: the game's replays, or one opened directory. */
type Workspace = {
  key: string;
  directory: string | null;
  layout: Signal<DockLayout>;
  tabs: Signal<Record<string, ReplayTab>>;
  viewports: Signal<Record<string, ViewportPanel>>;
  current: Signal<string | null>;
  hover: Signal<PreviewHover | null>;
  rescan: Signal<number>;
};

export type RenderRequest = { path: string; alts: string[] };

/** What the app draws in a docked viewport: a replay on its minimap, or a session window another end plays. */
export type ViewportSlots = {
  renderer: (path: string, alts: string[]) => JSX.Element;
  watched: (replayId: string) => JSX.Element;
};

export type ReplayInspectorProps = {
  openPath: string | null;
  openRequestId: number;
  viewports?: ViewportSlots;
  onRenderMany?: (paths: string[], target: "video" | "clipboard") => void;
  previewSlot?: (path: string, mapName: string) => JSX.Element;
  sessionControl?: () => JSX.Element;
  onDirectoryStats?: (path: string) => void;
};

const MAIN = "main";
const CONSTANTS_UNFIT = "constants-unfit";
const AUTO_EXPORT_FAILED = "auto-export-failed";
const GROUPINGS: readonly { id: Grouping; key: string }[] = [
  { id: "date", key: "ui.replay.group.date" },
  { id: "ship", key: "ui.replay.group.ship" },
  { id: "none", key: "ui.replay.group.none" },
];

const main = makeWorkspace(MAIN, null);
const directoryWorkspaces = signal<Workspace[]>([]);
const activeWorkspace = signal(MAIN);
const gameData = signal<GameDataSnapshot | null>(null);
const listingCollapsed = signal(false);
const grouping = signal<Grouping>("date");
const columns = signal<string[]>([]);
const listingRead = signal(false);
/** Builds whose result-mapping mismatch was already reported this session. */
const recovering = new Set<number>();
/** A directory asked for that the game-data feed has not listed yet; it is shown once it is. */
let pendingDirectory: { path: string; known: Set<string> } | null = null;
let nextTabId = 1;
let started = false;

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function makeWorkspace(key: string, directory: string | null): Workspace {
  return {
    key,
    directory,
    layout: signal<DockLayout>({ center: { type: "split", axis: "horizontal", children: [], sizes: [] } }),
    tabs: signal<Record<string, ReplayTab>>({}),
    viewports: signal<Record<string, ViewportPanel>>({}),
    current: signal<string | null>(null),
    hover: signal<PreviewHover | null>(null),
    rescan: signal(0),
  };
}

function allWorkspaces(): Workspace[] {
  return [main, ...directoryWorkspaces()];
}

function tabGroups(node: DockNode): TabsNode[] {
  const groups: TabsNode[] = [];
  const pending: DockNode[] = [node];
  while (pending.length > 0) {
    const next = pending.shift()!;
    if (next.type === "tabs") groups.push(next);
    else pending.push(...next.children);
  }
  return groups;
}

function mapGroups(node: DockNode, change: (group: TabsNode) => TabsNode): DockNode {
  if (node.type === "tabs") return change(node);
  return { ...node, children: node.children.map((child) => mapGroups(child, change)) };
}

function shownPanels(layout: DockLayout): string[] {
  return tabGroups(layout.center)
    .filter((group) => group.panels.length > 0)
    .map((group) => group.panels[Math.min(group.activeIndex ?? 0, group.panels.length - 1)]);
}

function withPanelReplaced(layout: DockLayout, old: string, id: string): DockLayout {
  return {
    ...layout,
    center: mapGroups(layout.center, (group) => (
      group.panels.includes(old) ? { ...group, panels: group.panels.map((panel) => (panel === old ? id : panel)) } : group
    )),
  };
}

function withPanelAdded(layout: DockLayout, near: string | null, id: string): DockLayout {
  const groups = tabGroups(layout.center);
  const target = groups.find((group) => near !== null && group.panels.includes(near)) ?? groups[0];
  if (!target) {
    return { ...layout, center: { type: "split", axis: "horizontal", children: [{ type: "tabs", panels: [id], activeIndex: 0 }], sizes: [null] } };
  }
  return {
    ...layout,
    center: mapGroups(layout.center, (group) => (
      group === target ? { ...group, panels: [...group.panels, id], activeIndex: group.panels.length } : group
    )),
  };
}

function withPanelSelected(layout: DockLayout, id: string): DockLayout {
  return {
    ...layout,
    center: mapGroups(layout.center, (group) => {
      const index = group.panels.indexOf(id);
      return index < 0 ? group : { ...group, activeIndex: index };
    }),
  };
}

function withPanelRemoved(layout: DockLayout, id: string): DockLayout {
  return {
    ...layout,
    center: mapGroups(layout.center, (group) => {
      const index = group.panels.indexOf(id);
      if (index < 0) return group;
      const panels = group.panels.filter((panel) => panel !== id);
      const active = group.activeIndex ?? 0;
      const activeIndex = index < active ? active - 1 : Math.max(0, Math.min(active, panels.length - 1));
      return { ...group, panels, activeIndex };
    }),
  };
}

function makeTab(path: string): ReplayTab {
  return {
    id: `replay-${nextTabId++}`,
    path,
    state: signal<TabState>({ kind: "loading" }),
    alts: [],
    generation: 0,
    closed: false,
    reparsing: signal(false),
    side: signal<SidePanel | null>(null),
    hideSelfStats: signal(false),
    exportStatus: signal<string | null>(null),
  };
}

function tabTitle(tab: ReplayTab): string {
  const state = tab.state();
  switch (state.kind) {
    case "loading": return t("ui.replay.loading");
    case "failed": return t("ui.messages.replay_load_failed");
    case "loaded": return state.summary.title;
  }
}

/** The tab a plain open replaces: the replay on screen, else the first shown. */
function replaceableTab(ws: Workspace): string | null {
  const shown = shownPanels(ws.layout()).filter((id) => ws.tabs()[id] !== undefined);
  const current = ws.current();
  if (current !== null && shown.includes(current)) return current;
  return shown[0] ?? null;
}

/**
 * Opens `path` in `ws`. A replay already open there is brought forward; a
 * plain open takes the place of the replay on screen, `newTab` adds a tab.
 */
function openReplay(ws: Workspace, path: string, newTab: boolean) {
  const existing = Object.values(ws.tabs()).find((tab) => tab.path === path);
  if (existing) {
    selectTab(ws, existing.id);
    return;
  }
  const tab = makeTab(path);
  ws.tabs.set({ ...ws.tabs(), [tab.id]: tab });
  const replaced = newTab ? null : replaceableTab(ws);
  ws.layout.set(replaced !== null ? withPanelReplaced(ws.layout(), replaced, tab.id) : withPanelAdded(ws.layout(), ws.current(), tab.id));
  ws.current.set(tab.id);
  if (replaced !== null) discardTab(ws, replaced);
  void readTab(tab, [], null);
}

function selectTab(ws: Workspace, id: string) {
  ws.layout.set(withPanelSelected(ws.layout(), id));
  ws.current.set(id);
}

function closeTab(ws: Workspace, id: string) {
  ws.layout.set(withPanelRemoved(ws.layout(), id));
  discardTab(ws, id);
}

/** Forgets a tab or viewport that left the dock, and the document a tab showed. */
function discardTab(ws: Workspace, id: string) {
  if (ws.viewports()[id]) {
    const { [id]: _closed, ...open } = ws.viewports();
    ws.viewports.set(open);
    if (ws.current() === id) ws.current.set(null);
    return;
  }
  const tab = ws.tabs()[id];
  if (!tab) return;
  tab.closed = true;
  const state = tab.state();
  if (state.kind === "loaded") ReplayInspectorService.close(state.summary.document);
  const { [id]: _gone, ...rest } = ws.tabs();
  ws.tabs.set(rest);
  if (ws.current() === id) ws.current.set(null);
}

function sameViewport(left: ViewportPanel, right: ViewportPanel): boolean {
  if (left.kind === "renderer" && right.kind === "renderer") return left.path === right.path;
  if (left.kind === "watched" && right.kind === "watched") return left.replayId === right.replayId;
  return false;
}

/**
 * Docks `panel` beside the replay on screen. A viewport already open for the
 * same replay or window is brought forward; a renderer asked for with other
 * recordings than it baked is replaced, since those are not on its map.
 */
function openViewport(ws: Workspace, panel: ViewportPanel) {
  const existing = Object.entries(ws.viewports()).find(([, open]) => sameViewport(open, panel));
  if (existing) {
    const [id, open] = existing;
    const stale = open.kind === "renderer" && panel.kind === "renderer" && open.alts.join("\n") !== panel.alts.join("\n");
    if (stale) {
      const fresh = `viewport-${nextTabId++}`;
      ws.viewports.set({ ...ws.viewports(), [fresh]: panel });
      ws.layout.set(withPanelReplaced(ws.layout(), id, fresh));
      discardTab(ws, id);
      ws.current.set(fresh);
    } else {
      selectTab(ws, id);
    }
    return;
  }
  const id = `viewport-${nextTabId++}`;
  ws.viewports.set({ ...ws.viewports(), [id]: panel });
  ws.layout.set(withPanelAdded(ws.layout(), ws.current(), id));
  ws.current.set(id);
}

function replayStem(path: string): string {
  return (path.split(/[\\/]/).pop() ?? path).replace(/\.wowsreplay$/i, "");
}

/** Docks a session window another end is playing in the game's own workspace. */
export function watchSharedWindow(replayId: string, title: string) {
  activeWorkspace.set(MAIN);
  openViewport(main, { kind: "watched", replayId, title });
}

/** Docks a renderer for `path` in the game's own workspace, for search results and the palette. */
export function renderInInspector(path: string) {
  activeWorkspace.set(MAIN);
  openViewport(main, { kind: "renderer", path, alts: [], title: replayStem(path) });
}

/**
 * Reads the tab's replay with `alts`. A replay already shown stays on screen
 * while it is read again; `trialAlt` names an alt just added, which is
 * dropped again if the battle cannot be read with it.
 */
async function readTab(tab: ReplayTab, alts: string[], trialAlt: string | null) {
  const generation = ++tab.generation;
  if (tab.state().kind === "loaded") tab.reparsing.set(true);
  else tab.state.set({ kind: "loading" });

  let summary: ReplaySummary;
  try {
    summary = await ReplayInspectorService.open(tab.path, alts);
  } catch (error) {
    if (tab.closed || generation !== tab.generation) return;
    tab.reparsing.set(false);
    const message = errorText(error);
    const shown = tab.state();
    if (trialAlt !== null && shown.kind === "loaded") {
      toast.failed(t("ui.replay.load_alt_perspective_failed", { error: message }));
      return;
    }
    tab.state.set({ kind: "failed", error: message });
    if (shown.kind === "loaded") ReplayInspectorService.close(shown.summary.document);
    toast.failed(t("ui.messages.replay_load_failed"));
    return;
  }

  if (tab.closed || generation !== tab.generation) {
    ReplayInspectorService.close(summary.document);
    return;
  }
  const shown = tab.state();
  tab.alts = summary.alts;
  tab.reparsing.set(false);
  if (summary.exportStatus != null) tab.exportStatus.set(summary.exportStatus);
  if (tab.side()?.kind === "chat" && summary.chat.length === 0) tab.side.set(null);
  tab.state.set({ kind: "loaded", summary });
  if (shown.kind === "loaded") ReplayInspectorService.close(shown.summary.document);
  if (summary.constantsMismatch != null) void recoverConstants(summary.constantsMismatch.build, summary.constantsMismatch.version ?? null);
}

function reparseAll() {
  for (const ws of allWorkspaces()) {
    for (const tab of Object.values(ws.tabs())) void readTab(tab, tab.alts, null);
  }
}

/** New result mappings were written: the open replays are read again with them. */
function mappingsArrived(count: number) {
  recovering.clear();
  toast.resolved(CONSTANTS_UNFIT);
  toast.info(t("ui.replay.constants_written", { count }));
  reparseAll();
}

async function recoverConstants(build: number, version: string | null) {
  if (recovering.has(build)) return;
  recovering.add(build);
  toast.stuck(CONSTANTS_UNFIT, t("ui.messages.constants_version_mismatch"));
  try {
    const written = await ReplayInspectorService.recoverConstants(build, version);
    if (written > 0) mappingsArrived(written);
  } catch (error) {
    toast.warn(errorText(error));
  }
}

async function fetchMissingConstants(builds: ListedBuild[]) {
  if (builds.length === 0) return;
  try {
    const written = await ReplayInspectorService.fetchMissingConstants(builds.map((listed) => ({ build: listed.build, version: listed.version ?? null })));
    if (written > 0) mappingsArrived(written);
  } catch (error) {
    toast.warn(errorText(error));
  }
}

async function checkLatestConstants() {
  try {
    if (await ReplayInspectorService.checkLatestConstants()) {
      toast.info(t("ui.replay.constants_latest_written"));
      reparseAll();
    }
  } catch (error) {
    toast.warn(errorText(error));
  }
}

/** Builds the listing names that no game data on this machine reads are offered for download, as the other front ends offer them. */
function offerMissingBuilds(builds: ListedBuild[]) {
  if (!builds.some((listed) => !listed.readable)) return;
  // Without an install there is no game-data cache to download into.
  if (gameData()?.wowsDir == null) return;
  void GameData.checkMissingBuilds().catch((error) => toast.failed(errorText(error)));
}

/** A replay the game finished writing: exported, indexed and contributed, then opened when autoload is on. */
async function replayLanded(ws: Workspace, path: string) {
  try {
    const report = await ReplayInspectorService.processLanded(path);
    if (report.exportError != null) toast.stuck(AUTO_EXPORT_FAILED, report.exportError);
    else if (report.exported != null) toast.resolved(AUTO_EXPORT_FAILED);
  } catch (error) {
    toast.failed(errorText(error));
  }
  if (appSettings()?.autoLoadLatestReplay) openReplay(ws, path, false);
}

function replayChanged(ws: Workspace, path: string) {
  for (const tab of Object.values(ws.tabs())) {
    if (tab.path === path) void readTab(tab, tab.alts, null);
  }
}

/** Shows the file dialog and opens the replay picked, in the workspace on screen. */
export async function openReplayFileDialog() {
  try {
    const path = await ReplayInspectorService.pickReplayFile();
    const shown = directoryWorkspaces().find((ws) => ws.key === activeWorkspace()) ?? main;
    if (path != null) openReplay(shown, path, false);
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/** Shows the directory dialog and opens the directory picked as a workspace of its own. */
export async function openReplayDirectoryDialog() {
  try {
    const path = await ReplayInspectorService.pickReplayDirectory();
    if (path == null) return;
    pendingDirectory = { path, known: new Set((gameData()?.directories ?? []).map((entry) => entry.path)) };
    await GameData.openDirectory(path, false);
    if ((gameData()?.directories ?? []).some((entry) => entry.path === path)) {
      activeWorkspace.set(path);
      pendingDirectory = null;
    }
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/** Imports a constants.json for the loaded build and reads the open replays again with it. */
export async function importConstants() {
  if (gameData()?.phase !== "ready") {
    toast.warn(t("ui.replay.constants_no_build"));
    return;
  }
  try {
    const imported = await ReplayInspectorService.importConstants();
    if (imported == null) return;
    toast.ok(imported);
    reparseAll();
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/** Follows the game-data feed: the directory workspaces, and listing again when the game data changes. */
function adoptGameData(next: GameDataSnapshot) {
  const before = gameData();
  gameData.set(next);

  const existing = new Map(directoryWorkspaces().map((ws) => [ws.key, ws]));
  const kept = next.directories.map((entry) => existing.get(entry.path) ?? makeWorkspace(entry.path, entry.path));
  for (const ws of existing.values()) {
    if (next.directories.some((entry) => entry.path === ws.key)) continue;
    for (const id of Object.keys(ws.tabs())) discardTab(ws, id);
  }
  const changed = kept.length !== existing.size || kept.some((ws, index) => directoryWorkspaces()[index] !== ws);
  if (changed) directoryWorkspaces.set(kept);
  if (activeWorkspace() !== MAIN && !kept.some((ws) => ws.key === activeWorkspace())) activeWorkspace.set(MAIN);
  if (pendingDirectory !== null) {
    const pending = pendingDirectory;
    const shown = next.directories.find((entry) => entry.path === pending.path) ?? next.directories.find((entry) => !pending.known.has(entry.path));
    if (shown) {
      activeWorkspace.set(shown.path);
      pendingDirectory = null;
    }
  }

  if (before === null) return;
  followGameDataEdges(before, next);
  if (next.phase === "ready" && before.phase !== "ready") void checkLatestConstants();
  // Rows are named from the game data, so a new install or build lists again.
  if (next.wowsDir !== before.wowsDir || next.phase !== before.phase || next.version !== before.version) {
    for (const ws of allWorkspaces()) ws.rescan.set(ws.rescan() + 1);
  }
}

function start() {
  if (started) return;
  started = true;
  void GameData.watch((next) => adoptGameData(next)).catch((error) => toast.failed(errorText(error)));
  void GameData.start().then((next) => adoptGameData(next)).catch((error) => toast.failed(errorText(error)));
  void ReplayInspectorService.listingState()
    .then((state) => {
      grouping.set(state.grouping);
      listingCollapsed.set(state.listingCollapsed);
      columns.set(state.columns);
      listingRead.set(true);
    })
    .catch((error) => toast.failed(errorText(error)));
}

function setGrouping(next: Grouping) {
  const before = grouping();
  if (before === next) return;
  grouping.set(next);
  void ReplayInspectorService.setGrouping(next).catch((error) => {
    grouping.set(before);
    toast.failed(errorText(error));
  });
}

function setListingCollapsed(collapsed: boolean) {
  listingCollapsed.set(collapsed);
  void ReplayInspectorService.setListingCollapsed(collapsed).catch((error) => {
    listingCollapsed.set(!collapsed);
    toast.failed(errorText(error));
  });
}

async function checkGameData() {
  try {
    const offer = await GameData.checkMissingBuilds();
    if (!offer.open && offer.rows.length === 0) toast.info("Game data is available for the listed replays.");
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/** Set when game data became ready, until the main listing next reports what it holds. */
let awaitingListedReplays = false;

/** What GPUI reports when the game data finishes loading, and the open replays rated once the PR table arrives. */
function followGameDataEdges(before: GameDataSnapshot, next: GameDataSnapshot) {
  if (next.phase === "ready" && before.phase !== "ready") {
    toast.ok(t("ui.messages.game_data_loaded"));
    awaitingListedReplays = true;
  }
  if (next.personalRatingLoaded && !before.personalRatingLoaded) rerateAll();
}

/** Warns once after game data became ready when the install's listing holds no replay. */
function noteListedReplays(ws: Workspace, builds: ListedBuild[]) {
  if (ws.key !== MAIN || !awaitingListedReplays) return;
  awaitingListedReplays = false;
  if (builds.every((build) => build.replays === 0)) toast.warn(t("ui.messages.no_replays_detected"));
}

/** Rates the rows of open replays read before the personal-rating table loaded. */
function rerateAll() {
  for (const ws of allWorkspaces()) {
    for (const tab of Object.values(ws.tabs())) void rerateTab(tab);
  }
}

async function rerateTab(tab: ReplayTab) {
  const state = tab.state();
  if (state.kind !== "loaded") return;
  const read = state.summary.document;
  let rated: ReplaySummary | null | undefined;
  try {
    rated = await ReplayInspectorService.rerate(read);
  } catch (error) {
    toast.failed(errorText(error));
    return;
  }
  if (rated == null) return;
  const shown = tab.state();
  // A tab closed or read again meanwhile does not take the rated copy.
  if (tab.closed || shown.kind !== "loaded" || shown.summary.document !== read) {
    ReplayInspectorService.close(rated.document);
    return;
  }
  tab.state.set({ kind: "loaded", summary: rated });
}

function closeDirectory(path: string) {
  try {
    GameData.closeDirectory(path);
  } catch (error) {
    toast.failed(errorText(error));
  }
}

async function pinDirectory(path: string, pinned: boolean) {
  try {
    await GameData.pinDirectory(path, pinned);
  } catch (error) {
    toast.failed(errorText(error));
  }
}

function IconButton(props: { label: string; glyph?: string; icon?: IconName; tooltip?: string; onPress?: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      onPress={props.onPress}
      tooltip={<Hint text={props.tooltip ?? props.label}/>}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.68 }}
      style={{ minWidth: 26, height: 24, paddingHorizontal: 5, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}
    >
      <If when={props.icon} else={<Text style={{ fontSize: 12.5, color: colors.foreground }}>{props.glyph ?? ""}</Text>}>
        {(icon) => <Icon name={icon()} size={15} color={colors.foreground}/>}
      </If>
    </View>
  );
}

/** Activity without a measure, beside the text that names it. */
function Spinner(props: { label: string }) {
  return <View style={{ width: 60 }}><Progress value={0} indeterminate accessibilityLabel={props.label}/></View>;
}

function Hint(props: { text: string }) {
  return (
    <Tooltip style={{ maxWidth: 320, paddingHorizontal: 8, paddingVertical: 5, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{props.text}</Text>
    </Tooltip>
  );
}

function Rule() {
  return <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>;
}

function SectionLabel(props: { text: string }) {
  return <Text style={{ paddingHorizontal: 10, paddingTop: 4, paddingBottom: 2, fontSize: 11, fontWeight: "600", color: colors.mutedForeground }}>{props.text}</Text>;
}

function OptionCheckbox(props: { label: string; accessibilityLabel?: string; tooltip?: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Checkbox
      accessibilityLabel={props.accessibilityLabel ?? props.label}
      checked={props.checked}
      tooltip={props.tooltip ? <Hint text={props.tooltip}/> : undefined}
      class="group flex-row items-center gap-2"
      indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<Text class="text-[10px] text-primary-foreground">{props.checked ? "x" : ""}</Text>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{props.label}</Text>
    </Checkbox>
  );
}

/** An item of a menu that can be checked or dimmed. */
function ActionItem(props: { label: string; checked?: boolean; disabled?: boolean; onSelect: () => void }) {
  return (
    <MenuItem
      onSelect={props.onSelect}
      checked={props.checked}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.72 }}
      style={{ height: 30, paddingHorizontal: 10, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 8, opacity: props.disabled ? 0.45 : 1 }}
    >
      <Text style={{ width: 12, fontSize: 12, color: colors.popoverForeground }}>{props.checked ? "x" : ""}</Text>
      <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>{props.label}</Text>
    </MenuItem>
  );
}

function ListingOptions() {
  const replay = () => appSettings()?.replay;
  return (
    <Popover
      align="end"
      offset={4}
      margin={8}
      trigger={<IconButton label={t("ui.replay.listing_options")} glyph="..."/>}
    >
      <Surface style={{ minWidth: 240, padding: 10, gap: 8, backgroundColor: colors.popover }}>
        <OptionCheckbox
          label={t("ui.replay.autoload_latest")}
          tooltip={t("ui.replay.autoload_latest_tooltip")}
          checked={appSettings()?.autoLoadLatestReplay ?? false}
          onChange={(checked) => void editSetting("autoLoadLatestReplay", checked)}
        />
        <Rule/>
        <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.replay.group_label")}</Text>
        <View style={{ flexDirection: "row", gap: 4 }}>
          <Each of={GROUPINGS}>
            {(choice) => (
              <BaseButton
                accessibilityLabel={`${t("ui.replay.group_label")}: ${t(choice().key)}`}
                selected={grouping() === choice().id}
                onPress={() => setGrouping(choice().id)}
                hoverStyle={{ opacity: 0.86 }}
                style={{
                  minHeight: 26,
                  paddingHorizontal: 10,
                  justifyContent: "center",
                  borderRadius: controlRadius(),
                  borderWidth: 1,
                  borderColor: colors.border,
                  backgroundColor: grouping() === choice().id ? colors.primary : colors.secondary,
                }}
              >
                <Text style={{ fontSize: 12.5, color: grouping() === choice().id ? colors.primaryForeground : colors.secondaryForeground }}>{t(choice().key)}</Text>
              </BaseButton>
            )}
          </Each>
        </View>
        <Rule/>
        <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.replay.column_filters")}</Text>
        <OptionCheckbox
          label={t("stat.raw_xp")}
          accessibilityLabel={`${t("ui.replay.column_filters")}: ${t("stat.raw_xp")}`}
          checked={replay()?.showRawXp ?? false}
          onChange={(checked) => void editReplayPreferences({ showRawXp: checked })}
        />
        <OptionCheckbox
          label={t("ui.replay.column.observed_damage")}
          accessibilityLabel={`${t("ui.replay.column_filters")}: ${t("ui.replay.column.observed_damage")}`}
          checked={replay()?.showObservedDamage ?? false}
          onChange={(checked) => void editReplayPreferences({ showObservedDamage: checked })}
        />
        <OptionCheckbox
          label={t("ui.replay.column.heals")}
          accessibilityLabel={`${t("ui.replay.column_filters")}: ${t("ui.replay.column.heals")}`}
          checked={replay()?.showHeals ?? false}
          onChange={(checked) => void editReplayPreferences({ showHeals: checked })}
        />
      </Surface>
    </Popover>
  );
}

function OpenMenu() {
  return (
    <Menu
      align="end"
      offset={4}
      margin={8}
      trigger={<IconButton label={t("ui.replay.open")} icon="folder-open"/>}
    >
      <Surface style={{ minWidth: 220, padding: 5, backgroundColor: colors.popover }}>
        <MenuRow label={t("ui.replay.open_file")} onSelect={() => void openReplayFileDialog()}/>
        <MenuRow label={t("ui.replay.open_directory")} onSelect={() => void openReplayDirectoryDialog()}/>
      </Surface>
    </Menu>
  );
}

function ListingHeader() {
  return (
    <View style={{ minHeight: 32, flexDirection: "row", alignItems: "center", gap: 4, paddingHorizontal: 6, borderBottomWidth: 1, borderBottomColor: colors.border }}>
      <IconButton label={t("ui.replay.collapse_listing")} glyph="<<" onPress={() => setListingCollapsed(true)}/>
      <Text numberOfLines={1} style={{ flex: 1, fontSize: 13, fontWeight: "700", color: colors.foreground }}>{t("ui.replay.listing_caption")}</Text>
      <ListingOptions/>
      <OpenMenu/>
    </View>
  );
}

function ListingRail() {
  return (
    <View style={{ width: 32, alignItems: "center", paddingTop: 4, borderRightWidth: 1, borderRightColor: colors.border }}>
      <IconButton label={t("ui.replay.expand_listing")} glyph=">>" onPress={() => setListingCollapsed(false)}/>
    </View>
  );
}

function DockTabLabel(props: { ws: Workspace; id: string }) {
  const tab = () => props.ws.tabs()[props.id];
  const viewport = () => props.ws.viewports()[props.id];
  const loaded = () => {
    const state = tab()?.state();
    return state?.kind === "loaded" ? state.summary : null;
  };
  const title = () => {
    const shown = tab();
    if (shown) return tabTitle(shown);
    return viewport()?.title ?? props.id;
  };
  const hint = () => {
    const shown = viewport();
    if (tab()) return tab()!.path;
    return shown?.kind === "renderer" ? shown.path : title();
  };
  return (
    <View
      tooltip={<Hint text={hint()}/>}
      style={{ maxWidth: 300, flexDirection: "row", alignItems: "center", gap: 6 }}
    >
      <If when={loaded()?.nationFlag}>
        {(flag) => <Image source={flag()} style={{ width: 23, height: 16 }}/>}
      </If>
      <If when={loaded()?.classIcon}>
        {(icon) => <Image source={icon()} style={{ width: 16, height: 16, tintColor: colors.foreground, transform: [{ rotate: "90deg" }] }}/>}
      </If>
      <Text numberOfLines={1} style={{ flexShrink: 1, fontSize: 12.5 }}>{title()}</Text>
      <View
        role="button"
        accessibilityLabel={`Close ${title()}`}
        onPress={() => closeTab(props.ws, props.id)}
        hoverStyle={{ backgroundColor: colors.accent }}
        style={{ paddingHorizontal: 4, borderRadius: controlRadius() }}
      >
        <Text style={{ fontSize: 11, color: colors.mutedForeground }}>x</Text>
      </View>
    </View>
  );
}

function outcomeLabel(outcome: ReplaySummary["outcome"]): { text: string; color: string; icon: IconName } | null {
  switch (outcome) {
    case "win": return { text: "Victory", color: semantic.win, icon: "trophy" };
    case "loss": return { text: "Defeat", color: semantic.loss, icon: "face-slightly-frowning" };
    case "draw": return { text: "Draw", color: semantic.draw, icon: "equal" };
    default: return null;
  }
}

function sideTitle(side: SidePanel): string {
  switch (side.kind) {
    case "chat": return t("ui.replay.game_chat");
    case "player": return t("ui.replay.debug.player_metadata");
    case "raw": {
      switch (side.which) {
        case "metadata": return t("ui.replay.debug.raw_metadata");
        case "results": return t("ui.replay.debug.raw_results");
        case "mapped": return t("ui.replay.debug.mapped_results");
      }
    }
  }
}

function sameSide(left: SidePanel | null, right: SidePanel): boolean {
  if (left === null || left.kind !== right.kind) return false;
  if (left.kind === "raw" && right.kind === "raw") return left.which === right.which;
  return right.kind !== "player";
}

function toggleSide(tab: ReplayTab, next: SidePanel) {
  tab.side.set(sameSide(tab.side(), next) ? null : next);
}

async function exportResults(tab: ReplayTab, summary: ReplaySummary, format: ExportFormat) {
  try {
    const written = await ReplayInspectorService.exportResults(summary.document, format, debugMode());
    if (written != null) tab.exportStatus.set(t("ui.replay.results_exported"));
  } catch (error) {
    tab.exportStatus.set(errorText(error));
  }
}

async function loadAltPerspective(tab: ReplayTab, summary: ReplaySummary) {
  let alt: string | null;
  try {
    alt = await ReplayInspectorService.pickAltPerspective(summary.document);
  } catch (error) {
    // The service words both refusals; the cap is a warning, a recording that does not match a failure.
    if (tab.alts.length >= 11) toast.warn(errorText(error));
    else toast.failed(errorText(error));
    return;
  }
  if (alt == null) return;
  void readTab(tab, [...tab.alts, alt], alt);
}

function ActionsMenu(props: { tab: ReplayTab; summary: ReplaySummary; onRender?: (request: RenderRequest) => void }) {
  const altCount = () => props.summary.alts.length;
  const side = () => props.tab.side();
  const showsRaw = (which: RawJsonKind) => {
    const shown = side();
    return shown !== null && shown.kind === "raw" && shown.which === which;
  };
  return (
    <Menu
      align="end"
      offset={4}
      margin={8}
      trigger={
        <View
          role="button"
          accessibilityLabel={t("ui.replay.actions")}
          hoverStyle={{ backgroundColor: colors.accent }}
          style={{ height: 26, paddingHorizontal: 10, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border }}
        >
          <Text style={{ fontSize: 12.5, color: colors.foreground }}>{t("ui.replay.actions")}</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 260, padding: 5, backgroundColor: colors.popover }}>
        <SectionLabel text={t("ui.replay.section_match")}/>
        <If when={props.onRender !== undefined}>
          <ActionItem
            label={t("ui.replay.context.render_replay")}
            onSelect={() => props.onRender?.({ path: props.tab.path, alts: [...props.tab.alts] })}
          />
        </If>
        <ActionItem
          label={altCount() > 0 ? `${t("ui.replay.load_alt_perspective")} (${altCount()})` : t("ui.replay.load_alt_perspective")}
          onSelect={() => void loadAltPerspective(props.tab, props.summary)}
        />
        <If when={props.summary.isTestShip}>
          <ActionItem
            label={t("ui.replay.hide_my_stats")}
            checked={props.tab.hideSelfStats()}
            onSelect={() => props.tab.hideSelfStats.set(!props.tab.hideSelfStats())}
          />
        </If>
        <Rule/>
        <SectionLabel text={t("ui.replay.section_export")}/>
        <ActionItem label={t("ui.replay.export_results_json")} onSelect={() => void exportResults(props.tab, props.summary, "json")}/>
        <ActionItem label={t("ui.replay.export_results_cbor")} onSelect={() => void exportResults(props.tab, props.summary, "cbor")}/>
        <ActionItem label={t("ui.replay.export_results_csv")} onSelect={() => void exportResults(props.tab, props.summary, "csv")}/>
        <If when={debugMode()}>
          <Rule/>
          <SectionLabel text={t("ui.replay.section_debug")}/>
          <ActionItem
            label={t("ui.replay.debug.raw_metadata")}
            checked={showsRaw("metadata")}
            onSelect={() => toggleSide(props.tab, { kind: "raw", which: "metadata" })}
          />
          <ActionItem
            label={t("ui.replay.debug.raw_results")}
            checked={showsRaw("results")}
            disabled={!props.summary.hasRawResults}
            onSelect={() => toggleSide(props.tab, { kind: "raw", which: "results" })}
          />
          <ActionItem
            label={t("ui.replay.debug.mapped_results")}
            checked={showsRaw("mapped")}
            disabled={!props.summary.hasMappedResults}
            onSelect={() => toggleSide(props.tab, { kind: "raw", which: "mapped" })}
          />
        </If>
      </Surface>
    </Menu>
  );
}

function HeaderRow(props: { tab: ReplayTab; summary: ReplaySummary; onRender?: (request: RenderRequest) => void }) {
  const outcome = () => outcomeLabel(props.summary.outcome);
  const noChat = () => props.summary.chat.length === 0;
  const chatShown = () => props.tab.side()?.kind === "chat";
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
      <View selectable style={{ flex: 1, flexDirection: "row", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
        <If when={props.summary.incompleteResults}>
          <View tooltip={<Hint text={t("ui.replay.incomplete_results_tooltip")}/>} style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <Text style={{ fontSize: 13, fontWeight: "700", color: semantic.warn }}>{`(i) ${t("ui.replay.incomplete_results")}`}</Text>
          </View>
        </If>
        <If when={outcome()}>
          {(shown) => (
            <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
              <Icon name={shown().icon} size={16} color={shown().color}/>
              <Text style={{ fontSize: 14, fontWeight: "700", color: shown().color }}>{shown().text}</Text>
            </View>
          )}
        </If>
        <If when={props.summary.personalRating}>
          {(rating) => (
            <View style={{ paddingHorizontal: 8, paddingVertical: 2, borderRadius: controlRadius(), backgroundColor: rating().tint }}>
              <Text style={{ fontSize: 12.5, fontWeight: "600", color: rating().color }}>{rating().label}</Text>
            </View>
          )}
        </If>
      </View>
      <If when={props.tab.reparsing()}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Spinner label={t("ui.replay.loading")}/>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{t("ui.replay.loading")}</Text>
        </View>
      </If>
      <If when={props.tab.exportStatus()}>
        <Text selectable style={{ fontSize: 12, color: colors.mutedForeground }}>{props.tab.exportStatus()!}</Text>
      </If>
      <ActionsMenu tab={props.tab} summary={props.summary} onRender={props.onRender}/>
      <BaseButton
        accessibilityLabel={t("ui.replay.chat")}
        selected={chatShown()}
        disabled={noChat()}
        tooltip={noChat() ? <Hint text={t("ui.replay.no_chat")}/> : undefined}
        onPress={() => toggleSide(props.tab, { kind: "chat" })}
        hoverStyle={{ opacity: 0.86 }}
        style={{
          height: 26,
          paddingHorizontal: 10,
          justifyContent: "center",
          borderRadius: controlRadius(),
          borderWidth: 1,
          borderColor: colors.border,
          backgroundColor: chatShown() ? colors.accent : "transparent",
          opacity: noChat() ? 0.45 : 1,
        }}
      >
        <Text style={{ fontSize: 12.5, color: colors.foreground }}>{t("ui.replay.chat")}</Text>
      </BaseButton>
    </View>
  );
}

function MatchContext(props: { summary: ReplaySummary }) {
  const damage = () => props.summary.teamDamage;
  return (
    <View selectable accessibilityLabel="Match context" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center" }}>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>
        {[...props.summary.matchContext, t("ui.replay.team_damage")].join(" - ")}
        <Text style={{ color: semantic.win }}>{damage().friendly}</Text>
        {" : "}
        <Text style={{ color: semantic.loss }}>{damage().enemy}</Text>
        {` (${damage().total})`}
      </Text>
    </View>
  );
}

async function copyChatLine(line: ChatLine) {
  await Shell.copyText(line.copyText);
  toast.ok(t("ui.replay.chat_copied"));
}

async function saveChat(summary: ReplaySummary) {
  try {
    const path = await ReplayInspectorService.saveChat(summary.document, summary.title);
    if (path != null) toast.ok(t("ui.replay.chat_saved", { path }));
  } catch (error) {
    toast.failed(t("ui.replay.chat_save_failed", { error: errorText(error) }));
  }
}

function copyChat(summary: ReplaySummary) {
  try {
    void Shell.copyText(ReplayInspectorService.chatTranscript(summary.document));
  } catch (error) {
    toast.failed(errorText(error));
  }
}

function ChatPanel(props: { summary: ReplaySummary }) {
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 6 }}>
      <View style={{ flexDirection: "row", gap: 6, paddingHorizontal: 8 }}>
        <Button label={t("ui.replay.chat_copy_all")} variant="ghost" onPress={() => copyChat(props.summary)}/>
        <Button label={t("ui.replay.chat_save_to_file")} variant="ghost" onPress={() => void saveChat(props.summary)}/>
      </View>
      <Scroll style={{ flex: 1, minHeight: 0 }}>
        <View selectable accessibilityLabel="Chat messages">
          <Each of={props.summary.chat}>
            {(line) => (
              <View class="group" style={{ paddingHorizontal: 8, paddingVertical: 5, gap: 2, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                  <If when={line().clan != null && line().clanColor != null}>
                    <Text style={{ fontSize: 12.5, fontWeight: "600", color: line().clanColor! }}>{`[${line().clan}]`}</Text>
                  </If>
                  <Text style={{ flex: 1, fontSize: 12.5, fontWeight: "600", color: line().senderColor ?? semantic.textDim }}>{`${line().sender}:`}</Text>
                  <View
                    role="button"
                    accessibilityLabel={t("ui.replay.copy_message")}
                    class="opacity-0 group-hover:opacity-100"
                    tooltip={<Hint text={t("ui.replay.copy_message")}/>}
                    onPress={() => void copyChatLine(line())}
                    hoverStyle={{ backgroundColor: colors.accent }}
                    style={{ paddingHorizontal: 5, borderRadius: controlRadius() }}
                  >
                    <Icon name="copy" size={13} color={colors.mutedForeground}/>
                  </View>
                </View>
                <Text style={{ fontSize: 12.5, color: line().messageColor }}>{line().message}</Text>
              </View>
            )}
          </Each>
        </View>
      </Scroll>
    </View>
  );
}

function RawJsonView(props: { title: string; json: string | null }) {
  return (
    <Editor
      value={props.json ?? ""}
      language="json"
      readOnly
      lineNumbers
      fontSize={12}
      accessibilityLabel={props.title}
      style={{ flex: 1, minHeight: 0 }}
    />
  );
}

function SidePanelView(props: { tab: ReplayTab; summary: ReplaySummary; side: SidePanel }) {
  const json = () => {
    const side = props.side;
    if (side.kind === "player") return side.json;
    if (side.kind !== "raw") return null;
    try {
      return ReplayInspectorService.rawJson(props.summary.document, side.which);
    } catch (error) {
      toast.failed(errorText(error));
      return null;
    }
  };
  return (
    <View style={{ width: 360, minHeight: 0, gap: 6, borderLeftWidth: 1, borderLeftColor: colors.border }}>
      <View style={{ minHeight: 30, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8 }}>
        <Text style={{ flex: 1, fontSize: 13, fontWeight: "700", color: colors.foreground }}>{sideTitle(props.side)}</Text>
        <IconButton label={t("ui.replay.close_side_panel")} glyph=">>" onPress={() => props.tab.side.set(null)}/>
      </View>
      <If when={props.side.kind === "chat"} else={<RawJsonView title={sideTitle(props.side)} json={json()}/>}>
        <ChatPanel summary={props.summary}/>
      </If>
    </View>
  );
}

function LoadedPanel(props: { tab: ReplayTab; summary: ReplaySummary; onRender?: (request: RenderRequest) => void }) {
  return (
    <View style={{ flex: 1, minHeight: 0, padding: 8, gap: 6 }}>
      <HeaderRow tab={props.tab} summary={props.summary} onRender={props.onRender}/>
      <MatchContext summary={props.summary}/>
      <View style={{ flex: 1, minHeight: 0, flexDirection: "row" }}>
        <PlayerTable
          style={{ flex: 1, minWidth: 0 }}
          document={props.summary.document}
          debug={debugMode()}
          columns={columns()}
          hideSelfStats={props.tab.hideSelfStats()}
          onViewRawJson={({ json }) => props.tab.side.set({ kind: "player", json })}
          onNotice={({ text }) => toast.info(text)}
        />
        <If when={props.tab.side()}>
          {(side) => <SidePanelView tab={props.tab} summary={props.summary} side={side()}/>}
        </If>
      </View>
    </View>
  );
}

function ReplayPanel(props: { ws: Workspace; id: string; onRender?: (request: RenderRequest) => void }) {
  const tab = () => props.ws.tabs()[props.id];
  return (
    <If when={tab()}>
      {(tab) => {
        const state = () => tab().state();
        const loaded = () => {
          const shown = state();
          return shown.kind === "loaded" ? shown.summary : null;
        };
        const failure = () => {
          const shown = state();
          return shown.kind === "failed" ? shown.error : null;
        };
        return (
          <View style={{ flex: 1, minHeight: 0 }}>
            <If when={loaded()}>
              {(summary) => <LoadedPanel tab={tab()} summary={summary()} onRender={props.onRender}/>}
            </If>
            <If when={failure()}>
              {(error) => (
                <View selectable style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8, padding: 24 }}>
                  <Text style={{ fontSize: 15, fontWeight: "700", color: colors.foreground }}>{t("ui.messages.replay_load_failed")}</Text>
                  <Text style={{ fontSize: 12.5, color: semantic.error }}>{error()}</Text>
                </View>
              )}
            </If>
            <If when={state().kind === "loading"}>
              <View style={{ flex: 1, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 8 }}>
                <Spinner label={t("ui.replay.loading")}/>
                <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{t("ui.replay.loading")}</Text>
              </View>
            </If>
          </View>
        );
      }}
    </If>
  );
}

function ReplayDock(props: { ws: Workspace; onRender?: (request: RenderRequest) => void; viewports?: ViewportSlots }) {
  const ws = props.ws;
  return (
    <DockArea
      style={{ flex: 1, minWidth: 0, minHeight: 0 }}
      layout={ws.layout()}
      onLayoutChange={({ layout }) => ws.layout.set(layout as DockLayout)}
      onPanelActiveChange={({ panel, active }) => {
        if (active) ws.current.set(panel);
      }}
      onPanelRemove={({ panel }) => discardTab(ws, panel)}
      renderPanel={(panel) => {
        const viewport = ws.viewports()[panel.id];
        if (viewport && props.viewports) {
          return (
            <View style={{ flex: 1, minHeight: 0 }}>
              {viewport.kind === "renderer" ? props.viewports.renderer(viewport.path, viewport.alts) : props.viewports.watched(viewport.replayId)}
            </View>
          );
        }
        return <ReplayPanel ws={ws} id={panel.id} onRender={props.onRender}/>;
      }}
      renderTab={(tab) => <DockTabLabel ws={ws} id={tab.panel}/>}
      renderEmpty={() => (
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{t("ui.replay.select_replay")}</Text>
        </View>
      )}
      groupStyle={{ flex: 1, minHeight: 0 }}
      tabBarClass="flex-row border-b border-border bg-card"
      tabClass="h-8 px-3 justify-center text-muted-foreground hover:bg-accent selected:bg-background selected:text-foreground"
      contentStyle={{ flex: 1, minHeight: 0 }}
      handleStyle={{ backgroundColor: colors.border }}
      dropIndicatorStyle={{ backgroundColor: colors.accent, opacity: 0.4 }}
    />
  );
}

function PreviewCard(props: { ws: Workspace; previewSlot?: (path: string, mapName: string) => JSX.Element }) {
  return (
    <If when={props.ws.hover()}>
      {(hover) => (
        <Overlay anchor={{ x: hover().x, y: hover().y }} placement="right" align="start" offset={24} margin={8}>
          <View
            pointerEvents="none"
            style={{ width: 392, padding: 4, gap: 6, borderWidth: 1, borderColor: colors.border, borderRadius: controlRadius(), backgroundColor: colors.popover }}
          >
            {props.previewSlot ? props.previewSlot(hover().path, hover().mapName) : null}
            <View accessibilityLabel="Replay preview facts" style={{ gap: 2, padding: 4 }}>
              <Each of={hover().facts}>
                {(fact) => (
                  <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 6 }}>
                    <Text style={{ width: 58, fontSize: 11.5, color: colors.mutedForeground }}>{fact().label}</Text>
                    <Text style={{ flexGrow: 1, flexShrink: 1, fontSize: 11.5, color: colors.popoverForeground }}>{fact().value}</Text>
                  </View>
                )}
              </Each>
            </View>
          </View>
        </Overlay>
      )}
    </If>
  );
}

function WorkspaceToolbar(props: { ws: Workspace; view: ReplayInspectorProps }) {
  const directory = props.ws.directory;
  const shown = () => props.view.sessionControl !== undefined || directory !== null;
  const statsReady = () => gameData()?.phase === "ready";
  return (
    <If when={shown()}>
      <View style={{ minHeight: 34, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8, paddingHorizontal: 8, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        {props.view.sessionControl ? props.view.sessionControl() : null}
        <If when={directory}>
          {(path) => (
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
              <View accessibilityLabel={path()} tooltip={<Hint text={path()}/>} style={{ maxWidth: 280 }}>
                <Text numberOfLines={1} style={{ fontSize: 12, color: colors.mutedForeground }}>{path()}</Text>
              </View>
              <View tooltip={<Hint text={t("ui.replay.check_game_data_tooltip")}/>}>
                <Button label={t("ui.replay.check_game_data")} variant="ghost" onPress={() => void checkGameData()}/>
              </View>
              <If when={props.view.onDirectoryStats !== undefined}>
                <View tooltip={<Hint text={statsReady() ? t("ui.stats.open_directory_stats_tooltip") : t("ui.stats.directory_stats_unavailable")}/>}>
                  <Button label={t("ui.stats.open_directory_stats")} variant="ghost" disabled={!statsReady()} onPress={() => props.view.onDirectoryStats?.(path())}/>
                </View>
              </If>
            </View>
          )}
        </If>
      </View>
    </If>
  );
}

function WorkspaceView(props: { ws: Workspace; view: ReplayInspectorProps }) {
  const ws = props.ws;
  const slots = props.view.viewports;
  const render = slots
    ? (request: RenderRequest) => openViewport(ws, { kind: "renderer", path: request.path, alts: request.alts, title: replayStem(request.path) })
    : undefined;
  const failure = () => (gameData()?.phase === "failed" ? gameData()?.reason ?? null : null);
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <WorkspaceToolbar ws={ws} view={props.view}/>
      <If when={failure()}>
        {(reason) => (
          <Text style={{ paddingHorizontal: 8, paddingVertical: 3, fontSize: 11.5, color: colors.mutedForeground }}>
            {t("ui.replay.game_data_failed", { error: reason() })}
          </Text>
        )}
      </If>
      <ResizablePanelGroup
        axis="horizontal"
        style={{ flex: 1, minHeight: 0 }}
        handleStyle={{ backgroundColor: colors.border }}
        handleAccessibilityLabel="Resize the replay listing"
      >
        <ResizablePanel size={280} minSize={180} maxSize={520} fixed visible={!listingCollapsed()}>
          <View style={{ flex: 1, minHeight: 0 }}>
            <ListingHeader/>
            <ReplayBrowser
              style={{ flex: 1, minHeight: 0 }}
              directory={ws.directory}
              grouping={grouping()}
              rescan={ws.rescan()}
              canRender={render !== undefined}
              onOpen={({ path }) => openReplay(ws, path, false)}
              onOpenInNewTab={({ path }) => openReplay(ws, path, true)}
              onRender={({ path }) => render?.({ path, alts: [] })}
              onRenderMany={({ paths, target }) => props.view.onRenderMany?.(paths, target)}
              onPreviewHover={({ hover }) => ws.hover.set(hover)}
              onPreviewLeave={() => ws.hover.set(null)}
              onReplayAppeared={({ path }) => void replayLanded(ws, path)}
              onReplayChanged={({ path }) => replayChanged(ws, path)}
              onBuildsListed={({ builds }) => {
                noteListedReplays(ws, builds);
                void fetchMissingConstants(builds);
                offerMissingBuilds(builds);
              }}
              onNotice={({ text, level }) => toast[level as NoticeLevel](text)}
            />
          </View>
        </ResizablePanel>
        <ResizablePanel minSize={240}>
          <View style={{ flex: 1, minHeight: 0, flexDirection: "row" }}>
            <If when={listingCollapsed()}>
              <ListingRail/>
            </If>
            <ReplayDock ws={ws} onRender={render} viewports={slots}/>
          </View>
        </ResizablePanel>
      </ResizablePanelGroup>
      <PreviewCard ws={ws} previewSlot={props.view.previewSlot}/>
    </View>
  );
}

function WorkspaceTab(props: { label: string; selected: boolean; onSelect: () => void; children?: JSX.Element }) {
  return (
    <View
      role="tab"
      accessibilityState={{ selected: props.selected }}
      onPress={props.onSelect}
      hoverStyle={props.selected ? undefined : { backgroundColor: colors.accent }}
      style={{
        height: 30,
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        paddingHorizontal: 10,
        borderBottomWidth: 2,
        borderBottomColor: props.selected ? colors.primary : "transparent",
      }}
    >
      <Text style={{ fontSize: 12.5, fontWeight: props.selected ? "600" : "400", color: props.selected ? colors.foreground : colors.mutedForeground }}>{props.label}</Text>
      {props.children}
    </View>
  );
}

/** The game's replays and every opened directory, one tab each; a directory is pinned or closed from its tab. */
function WorkspaceStrip() {
  const entries = () => gameData()?.directories ?? [];
  return (
    <View role="tablist" style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 2, paddingHorizontal: 6, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.surface }}>
      <WorkspaceTab label={t("ui.tabs.replay_parser")} selected={activeWorkspace() === MAIN} onSelect={() => activeWorkspace.set(MAIN)}/>
      <Each of={entries()} key={(entry) => entry.path}>
        {(entry) => (
          <ContextMenu
            trigger={
              <View tooltip={<Hint text={entry().path}/>}>
                <WorkspaceTab label={entry().label} selected={activeWorkspace() === entry().path} onSelect={() => activeWorkspace.set(entry().path)}>
                  <If
                    when={entry().pinned}
                    else={
                      <View
                        role="button"
                        accessibilityLabel={`Close directory ${entry().label}`}
                        tooltip={<Hint text="Close directory"/>}
                        onPress={() => closeDirectory(entry().path)}
                        hoverStyle={{ backgroundColor: colors.accent }}
                        style={{ paddingHorizontal: 4, borderRadius: controlRadius() }}
                      >
                        <Text style={{ fontSize: 11, color: colors.mutedForeground }}>x</Text>
                      </View>
                    }
                  >
                    <Text style={{ fontSize: 11, color: colors.mutedForeground }}>[Pinned]</Text>
                  </If>
                </WorkspaceTab>
              </View>
            }
          >
            <Surface style={{ minWidth: 200, padding: 5, backgroundColor: colors.popover }}>
              <ActionItem
                label={entry().pinned ? "Unpin directory" : "Pin directory"}
                onSelect={() => void pinDirectory(entry().path, !entry().pinned)}
              />
              <ActionItem label="Close directory" disabled={entry().pinned} onSelect={() => closeDirectory(entry().path)}/>
            </Surface>
          </ContextMenu>
        )}
      </Each>
    </View>
  );
}

/** The Replay Inspector tab: replay listings beside docks of open replays. */
export function ReplayInspectorView(props: ReplayInspectorProps) {
  start();

  effect(() => {
    const replay = appSettings()?.replay;
    if (!replay || !listingRead()) return;
    untracked(() => {
      try {
        columns.set(ReplayInspectorService.columnsFor(replay));
      } catch (error) {
        toast.failed(errorText(error));
      }
    });
  });

  // Debug detail goes with debug mode; a player's metadata asked for from a row stays.
  effect(() => {
    if (debugMode()) return;
    untracked(() => {
      for (const ws of allWorkspaces()) {
        for (const tab of Object.values(ws.tabs())) {
          if (tab.side()?.kind === "raw") tab.side.set(null);
        }
      }
    });
  });

  // Search, the palette and a dropped file open here, in the game's own workspace.
  effect(() => {
    const path = props.openPath;
    props.openRequestId;
    if (path == null) return;
    untracked(() => {
      activeWorkspace.set(MAIN);
      openReplay(main, path, false);
    });
  });

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={directoryWorkspaces().length > 0}>
        <WorkspaceStrip/>
      </If>
      <Each of={allWorkspaces()} key={(ws) => ws.key}>
        {(ws) => (
          <View style={{ flex: 1, minHeight: 0, display: activeWorkspace() === ws().key ? "flex" : "none" }}>
            <WorkspaceView ws={ws()} view={props}/>
          </View>
        )}
      </Each>
    </View>
  );
}
