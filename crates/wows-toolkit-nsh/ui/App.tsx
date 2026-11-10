import { keymap, mount, signal, Storage, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { Dialog } from "nsh/base";
import { GameData, Palette, Shell, Toolkit } from "wows-toolkit/native";
import { EmptyState, errorMessage } from "./components";
import { colors } from "./theme";
import { applyLocale, t } from "./i18n";
import { loadAppSettings } from "./settings_store";
import { SettingsView } from "./settings";
import { AboutDialog, openAbout } from "./shell/about";
import { ApplicationMenu, applyZoom, DropScrim, hoveringFiles, TabStrip, wowsDirInvalid } from "./shell/chrome";
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
import { activateStats, StatsView } from "./stats";
import { activatePlayerTracker, PlayerTrackerView, setPlayerTrackerShown } from "./player_tracker";
import { bindSearchRowActions, bindSearchTab, searchFor, SearchView } from "./search";
import { bindPalette, CommandPalette, openPalette } from "./palette";
import { openTacticsBoard, TacticsBoard } from "./tactics";
import { UnpackerView } from "./unpacker";
import { BatchRenderStatus, followForcedWindows, renderReplaysToVideo, ReplayRendererView, WatchedPlaybackView } from "./renderer";
import { armorShipList, ArmorView, onArmorReveal, viewArmorShip } from "./armor";
import { importConstants, ReplayInspectorView } from "./replay_inspector";

const startup = Toolkit.startup();
const activeTab = signal(startup.tabs[0].id);
const replayOpenPath = signal<string | null>(null);
const replayOpenRequestId = signal(0);
/** The replay shown in the renderer window, when one is open. */
const rendererPath = signal<string | null>(null);
/** The session window being watched, when one is open. */
const watchedReplay = signal<string | null>(null);
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
  if (!loaded.disableAutoOpenSessionWindows) void followForcedWindows((replayId) => watchedReplay.set(replayId));
});

function selectTab(id: string) {
  if (id === "statistics") activateStats();
  if (id === "player-tracker") activatePlayerTracker(startup.locale);
  setPlayerTrackerShown(id === "player-tracker");
  activeTab.set(id);
  void Storage.setItem(STORAGE_TAB, id);
}

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
  renderReplay: (path) => rendererPath.set(path),
  renderReplayToClipboard: (path) => void renderToClipboard(path),
});
bindPalette({
  tabs: startup.tabs,
  activeTab: () => activeTab(),
  armorShips: armorShipList,
  shellLabels: {
    tacticsBoard: startup.text.tacticsBoard,
    checkUpdates: startup.text.checkUpdates,
    about: `${startup.text.about} WoWs Toolkit`,
  },
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

const SHOWN_TABS = ["replay-inspector", "launcher", "statistics", "player-tracker", "search", "unpacker", "armor-viewer", "settings"];

function Content() {
  const tab = () => startup.tabs.find((candidate) => candidate.id === activeTab()) ?? startup.tabs[0];
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "replay-inspector" ? "flex" : "none" }}>
        <ReplayInspectorView openPath={replayOpenPath()} openRequestId={replayOpenRequestId()}/>
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
      <If when={!SHOWN_TABS.includes(tab().id)}>
        <EmptyState title={tab().label} description={tab().description}/>
      </If>
    </View>
  );
}

/** The renderer and watched playback fill the window, as GPUI's separate windows do. */
function ViewportWindows() {
  return (
    <>
      <Dialog open={rendererPath() !== null} onOpenChange={({ open }) => { if (!open) rendererPath.set(null); }}>
        <View style={{ width: "100%", height: "100%", backgroundColor: colors.background }}>
          <If when={rendererPath()}>
            <ReplayRendererView path={rendererPath()!}/>
          </If>
        </View>
      </Dialog>
      <Dialog open={watchedReplay() !== null} onOpenChange={({ open }) => { if (!open) watchedReplay.set(null); }}>
        <View style={{ width: "100%", height: "100%", backgroundColor: colors.background }}>
          <If when={watchedReplay()}>
            <WatchedPlaybackView replayId={watchedReplay()!}/>
          </If>
        </View>
      </Dialog>
    </>
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
        tabs={startup.tabs}
        active={activeTab()}
        attention={(id) => id === "settings" && wowsDirInvalid()}
        onSelect={selectTab}
        menu={
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <SessionButton onOpenTacticsBoard={openTacticsBoard} onWatchSharedWindow={(window) => watchedReplay.set(window.replayId)}/>
            <ApplicationMenu onCommandPalette={openPalette} onTacticsBoard={openTacticsBoard}/>
          </View>
        }
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
      <TacticsBoard title={startup.text.tacticsBoard}/>
      <ViewportWindows/>
      <CommandPalette/>
    </View>
  );
}

mount(App);
