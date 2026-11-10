import { keymap, mount, signal, Storage, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { Collab, GameData, Palette, Shell, Toolkit } from "wows-toolkit/native";
import { errorMessage } from "./components";
import { colors } from "./theme";
import { applyLocale, t } from "./i18n";
import { loadAppSettings } from "./settings_store";
import { SettingsView } from "./settings";
import { AboutDialog, openAbout } from "./shell/about";
import { ApplicationMenu, applyZoom, DropScrim, hoveringFiles, wowsDirInvalid } from "./shell/chrome";
import { hostTab, TabStrip, type TabTarget } from "./shell/tabs";
import { CrashDialog, reportLastCrash } from "./shell/diagnostics";
import { contributeAll, copyBugReportInfo, copyLatestLog, setTheme } from "./shell/actions";
import { askFirstRun, FirstRunDialog } from "./shell/first_run";
import { ErrorDialog, WarningDialog } from "./shell/notices";
import { DebugNotice, debugMode, StatusBar } from "./shell/status";
import * as toast from "./shell/toast";
import { restartTwitch } from "./shell/twitch";
import { checkForUpdate, UpdateDialog } from "./shell/update";
import { GameDataStatus } from "./game_data";
import { SessionButton } from "./collab";
import { LauncherView } from "./launcher";
import { activateStats, openDirectoryStats, showStatsScope, StatsView } from "./stats";
import { activatePlayerTracker, PlayerTrackerView, setPlayerTrackerShown } from "./player_tracker";
import { bindSearchRowActions, bindSearchTab, searchFor, SearchView } from "./search";
import { bindPalette, CommandPalette, openPalette } from "./palette";
import { openTacticsBoard, TacticsBoard } from "./tactics";
import { UnpackerView } from "./unpacker";
import { BatchRenderStatus, followForcedWindows, MinimapPreview, renderReplaysToVideo, ReplayRendererView, WatchedPlaybackView } from "./renderer";
import { armorShipList, ArmorView, onArmorReveal, viewArmorShip } from "./armor";
import { bindWorkspaceTabs, importConstants, renderInInspector, ReplayInspectorView, showReplayWorkspace, watchSharedWindow } from "./replay_inspector";

const startup = Toolkit.startup();
const activeTab = signal<TabTarget>({ kind: "fixed", id: startup.tabs[0].id });
const replayOpenPath = signal<string | null>(null);
const replayOpenRequestId = signal(0);

/** Docks a session window in the inspector and brings the inspector forward. */
function watchWindow(replayId: string) {
  const shared = Collab.state().sharedWindows.find((window) => window.replayId === replayId);
  watchSharedWindow(replayId, shared?.replayName ?? replayId);
  selectTab("replay-inspector");
}
const STORAGE_TAB = "application.active-tab";

Window.setTitle(`${startup.title} v${startup.version}`);
void Storage.getItem(STORAGE_TAB).then((saved) => {
  if (saved && startup.tabs.some((tab) => tab.id === saved)) selectTab(saved);
});
applyLocale(startup.locale);
void loadAppSettings().then((loaded) => {
  if (!loaded) return;
  // What may be shared is the reader's to answer before anything is sent
  // under a default, so the first-run question comes first.
  askFirstRun(loaded, startup.locale);
  reportLastCrash();
  if (loaded.locale) applyLocale(loaded.locale);
  void Shell.applyTheme(loaded.theme);
  applyZoom(loaded.zoom);
  debugMode.set(loaded.debugMode);
  if (loaded.checkForUpdates) void checkForUpdate("startup");
  void restartTwitch();
  if (!loaded.disableAutoOpenSessionWindows) void followForcedWindows(watchWindow);
});

function selectTab(id: string) {
  selectTarget({ kind: "fixed", id });
}

function selectTarget(target: TabTarget) {
  const shown = hostTab(target);
  if (target.kind === "fixed" && shown === "statistics") activateStats();
  if (shown === "player-tracker") activatePlayerTracker(startup.locale);
  setPlayerTrackerShown(shown === "player-tracker");
  showReplayWorkspace(target.kind === "directory" ? target.path : null);
  showStatsScope(target.kind === "directoryStats" ? target.scope : "session");
  activeTab.set(target);
  // Directory tabs last one run, so only a fixed tab is restored.
  if (target.kind === "fixed") void Storage.setItem(STORAGE_TAB, target.id);
}

bindWorkspaceTabs((directory) => selectTarget(directory === null ? { kind: "fixed", id: "replay-inspector" } : { kind: "directory", path: directory }));

onArmorReveal(() => selectTab("armor-viewer"));

function openReplayPath(path: string) {
  if (replayOpenPath() === path) {
    replayOpenRequestId.set(replayOpenRequestId() + 1);
  } else {
    replayOpenPath.set(path);
  }
  selectTab("replay-inspector");
}

/** A replay dragged onto the window opens; only a replay, and only one, since the inspector opens a file, not a set. */
function openDroppedReplay(uris: string[]) {
  hoveringFiles.set(false);
  const replays: string[] = [];
  for (const uri of uris) {
    try {
      const url = new URL(uri);
      if (url.protocol === "file:" && url.pathname.toLowerCase().endsWith(".wowsreplay")) replays.push(decodeURIComponent(url.pathname));
    } catch {
      continue;
    }
  }
  if (replays.length === 0) {
    toast.warn(t("ui.messages.drop_not_a_replay"));
  } else if (replays.length > 1) {
    toast.warn(t("ui.messages.drop_one_at_a_time"));
  } else {
    openReplayPath(replays[0]);
  }
}

function showNotice(message: string) {
  toast.info(message);
}

async function renderToClipboard(path: string) {
  const failure = await renderReplaysToVideo([path], true);
  if (failure) toast.warn(failure);
}

keymap.bind({
  "mod+1": () => selectTab(startup.tabs[0].id),
  "mod+2": () => selectTab(startup.tabs[1].id),
  "mod+3": () => selectTab(startup.tabs[2].id),
  "mod+4": () => selectTab(startup.tabs[3].id),
  "mod+5": () => selectTab(startup.tabs[4].id),
  "mod+6": () => selectTab(startup.tabs[5].id),
  "mod+7": () => selectTab(startup.tabs[6].id),
  "mod+,": () => selectTab("settings"),
  // Flips debug detail for this session only, as the other front ends do.
  "mod+shift+d": () => debugMode.set(!debugMode()),
});

bindSearchTab(() => selectTab("search"));
bindSearchRowActions({
  renderReplay: (path) => {
    renderInInspector(path);
    selectTab("replay-inspector");
  },
  renderReplayToClipboard: (path) => void renderToClipboard(path),
});
bindPalette({
  tabs: startup.tabs,
  activeTab: () => hostTab(activeTab()),
  armorShips: armorShipList,
  handlers: {
    goTo: ({ tab }) => selectTab(tab),
    advancedSearch: () => selectTab("search"),
    openReplayFile: () => {
      void Palette.pickReplay().then(
        (path) => {
          if (path) openReplayPath(path);
        },
        (error) => showNotice(errorMessage(error)),
      );
    },
    indexAllReplays: () => void GameData.indexReplays("fillGaps"),
    refreshPersistedData: () => void GameData.indexReplays("refreshAll"),
    contributeAllReplays: ({ ignoreLedger }) => void contributeAll(ignoreLedger ? "ignore" : "consult"),
    copyLatestLog: () => void copyLatestLog(),
    copyBugReportInfo: () => void copyBugReportInfo(),
    importConstants: () => {
      selectTab("replay-inspector");
      void importConstants();
    },
    setTheme: ({ theme }) => setTheme(theme),
    viewArmor: ({ paramIndex, displayName }) => viewArmorShip(paramIndex, displayName),
    tacticsBoard: openTacticsBoard,
    checkUpdates: () => void checkForUpdate("byHand"),
    about: openAbout,
  },
});

function Content() {
  // A directory shows under the inspector, a directory's stats under Stats.
  const tab = () => ({ id: hostTab(activeTab()) });
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "replay-inspector" ? "flex" : "none" }}>
        <ReplayInspectorView
          openPath={replayOpenPath()}
          openRequestId={replayOpenRequestId()}
          viewports={{
            renderer: (path, alts) => <ReplayRendererView path={path} alts={alts}/>,
            watched: (replayId) => <WatchedPlaybackView replayId={replayId}/>,
          }}
          onRenderMany={(paths, target) => {
            void renderReplaysToVideo(paths, target === "clipboard").then((failure) => {
              if (failure) toast.warn(failure);
            });
          }}
          previewSlot={(path) => <MinimapPreview path={path}/>}
          sessionControl={() => <SessionButton onOpenTacticsBoard={openTacticsBoard} onWatchSharedWindow={(window) => watchWindow(window.replayId)}/>}
          onDirectoryStats={(path) => {
            void openDirectoryStats(path).then(
              (scope) => selectTarget({ kind: "directoryStats", scope }),
              (error) => toast.warn(errorMessage(error)),
            );
          }}
        />
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "launcher" ? "flex" : "none" }}>
        <LauncherView visible={tab().id === "launcher"}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "statistics" ? "flex" : "none" }}>
        <StatsView/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "player-tracker" ? "flex" : "none" }}>
        <PlayerTrackerView onSearch={searchFor} onNotice={showNotice}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "search" ? "flex" : "none" }}>
        <SearchView onOpenReplay={openReplayPath} onNotice={showNotice}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "unpacker" ? "flex" : "none" }}>
        <UnpackerView active={tab().id === "unpacker"} locale={startup.locale}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "armor-viewer" ? "flex" : "none" }}>
        <ArmorView shown={tab().id === "armor-viewer"}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "settings" ? "flex" : "none" }}>
        <SettingsView/>
      </View>
    </View>
  );
}

function App() {
  return (
    <View
      onFileDrop={({ uris }) => openDroppedReplay(uris)}
      onFileDragEnter={() => hoveringFiles.set(true)}
      onFileDragLeave={() => hoveringFiles.set(false)}
      style={{ flex: 1, backgroundColor: colors.background }}
    >
      <TabStrip
        fixed={startup.tabs}
        active={activeTab()}
        attention={(id) => id === "settings" && wowsDirInvalid()}
        onSelect={selectTarget}
        menu={<ApplicationMenu onCommandPalette={openPalette} onTacticsBoard={openTacticsBoard}/>}
      />
      <View style={{ flex: 1, minHeight: 0 }}><Content/></View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8, borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.surface }}>
        <View style={{ paddingHorizontal: 8 }}><GameDataStatus/></View>
        <BatchRenderStatus/>
        <View style={{ flex: 1 }}><StatusBar/></View>
      </View>
      <DebugNotice/>
      <DropScrim/>
      <toast.ToastLayer/>
      <WarningDialog/>
      <ErrorDialog/>
      <AboutDialog version={startup.version}/>
      <UpdateDialog/>
      <CrashDialog/>
      <FirstRunDialog/>
      <TacticsBoard title={t("ui.windows.tactics_board")}/>
      <CommandPalette/>
    </View>
  );
}

mount(App);
