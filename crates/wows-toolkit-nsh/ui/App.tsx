import { keymap, mount, Scroll, signal, Storage, Text, View, windowSize } from "nsh";
import { Window } from "nsh/desktop";
import { Command, Dialog, Input, Menu } from "nsh/base";
import { ReplayInspector as NativeReplayInspector, Shell, Toolkit, type PlayerTrackerState, type SearchState, type StatisticsState } from "wows-toolkit/native";
import { Button, EmptyState, MenuRow, Surface } from "./components";
import { colors, controlRadius } from "./theme";
import { applyLocale, t } from "./i18n";
import { appSettings, loadAppSettings } from "./settings_store";
import { SettingsView } from "./settings";
import { AboutDialog, openAbout } from "./shell/about";
import { ApplicationMenu, applyZoom, DropScrim, hoveringFiles, TabStrip, wowsDirInvalid } from "./shell/chrome";
import { copyLatestLog, CrashDialog, reportLastCrash } from "./shell/diagnostics";
import { askFirstRun, FirstRunDialog } from "./shell/first_run";
import { ErrorDialog, WarningDialog } from "./shell/notices";
import { DebugNotice, debugMode, StatusBar } from "./shell/status";
import * as toast from "./shell/toast";
import { restartTwitch } from "./shell/twitch";
import { checkForUpdate, UpdateDialog } from "./shell/update";
import { contributeAll } from "./shell/uploads";
import { GameDataStatus } from "./game_data";

const startup = Toolkit.startup();
const activeTab = signal(startup.tabs[0].id);
const tacticsOpen = signal(false);
const paletteOpen = signal(false);
const paletteQuery = signal("");
const statistics = signal<StatisticsState | null>(null);
const statisticsLoading = signal(false);
const statisticsError = signal<string | null>(null);
const playerTracker = signal<PlayerTrackerState | null>(null);
const playerTrackerLoading = signal(false);
const playerTrackerError = signal<string | null>(null);
const playerTrackerPeriod = signal("day");
const playerTrackerQuery = signal("");
const searchQuery = signal("");
const searchState = signal<SearchState | null>(null);
const searchLoading = signal(false);
const searchError = signal<string | null>(null);
const searchHasRun = signal(false);
const searchSortColumn = signal("date");
const searchSortDescending = signal(true);
const replayOpenPath = signal<string | null>(null);
const replayOpenRequestId = signal(0);
let statisticsRequested = false;
let playerTrackerRequested = false;
let playerTrackerGeneration = 0;
let playerTrackerFilterTimer: number | undefined;
let playerTrackerRequestActive = false;
let playerTrackerRequestPending = false;
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
});

function selectTab(id: string) {
  if (id === "statistics" && !statisticsRequested) {
    statisticsRequested = true;
    void loadStatistics();
  }
  if (id === "player-tracker" && !playerTrackerRequested) {
    playerTrackerRequested = true;
    void loadPlayerTracker();
  }
  activeTab.set(id);
  void Storage.setItem(STORAGE_TAB, id);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

async function loadStatistics() {
  statisticsLoading.set(true);
  statisticsError.set(null);
  try {
    statistics.set(await Toolkit.loadStatistics());
  } catch (error) {
    statisticsError.set(`Statistics could not be loaded: ${errorMessage(error)}`);
  } finally {
    statisticsLoading.set(false);
  }
}
async function loadPlayerTracker() {
  if (playerTrackerRequestActive) {
    playerTrackerRequestPending = true;
    return;
  }
  playerTrackerRequestActive = true;
  playerTrackerLoading.set(true);
  do {
    playerTrackerRequestPending = false;
    const generation = ++playerTrackerGeneration;
    const period = playerTrackerPeriod();
    const query = playerTrackerQuery();
    playerTrackerError.set(null);
    try {
      const loaded = await Toolkit.loadPlayerTracker(period, query);
      if (generation === playerTrackerGeneration) playerTracker.set(loaded);
    } catch (error) {
      if (generation === playerTrackerGeneration) {
        playerTrackerError.set(`Player tracker could not be loaded: ${errorMessage(error)}`);
      }
    }
  } while (playerTrackerRequestPending);
  playerTrackerRequestActive = false;
  playerTrackerLoading.set(false);
}

function selectPlayerTrackerPeriod(period: string) {
  if (period === playerTrackerPeriod()) return;
  clearTimeout(playerTrackerFilterTimer);
  playerTrackerFilterTimer = undefined;
  playerTrackerGeneration += 1;
  playerTrackerPeriod.set(period);
  playerTracker.set(null);
  void loadPlayerTracker();
}

function updatePlayerTrackerQuery(query: string) {
  playerTrackerQuery.set(query);
  playerTrackerGeneration += 1;
  playerTracker.set(null);
  playerTrackerError.set(null);
  playerTrackerLoading.set(true);
  clearTimeout(playerTrackerFilterTimer);
  if (playerTrackerRequestActive) {
    playerTrackerFilterTimer = undefined;
    playerTrackerRequestPending = true;
  } else {
    playerTrackerFilterTimer = setTimeout(() => {
      playerTrackerFilterTimer = undefined;
      void loadPlayerTracker();
    }, 200);
  }
}
type SearchSort = {
  column: string;
  descending: boolean;
};

async function runSearch(preserveResults = false, rollback?: SearchSort) {
  if (searchLoading()) return;
  searchLoading.set(true);
  if (!preserveResults) searchState.set(null);
  searchError.set(null);
  searchHasRun.set(true);
  try {
    searchState.set(await Toolkit.searchIndex(searchQuery(), searchSortColumn(), searchSortDescending()));
  } catch (error) {
    if (rollback) {
      searchSortColumn.set(rollback.column);
      searchSortDescending.set(rollback.descending);
    }
    searchError.set(`Search could not be completed: ${errorMessage(error)}`);
  } finally {
    searchLoading.set(false);
  }
}
function defaultSearchSortDescending(column: string) {
  return column !== "map" && column !== "mode";
}

function selectSearchSort(column: string) {
  if (searchLoading()) return;
  const rollback = { column: searchSortColumn(), descending: searchSortDescending() };
  const defaultDescending = defaultSearchSortDescending(column);
  if (column !== searchSortColumn()) {
    searchSortColumn.set(column);
    searchSortDescending.set(defaultDescending);
  } else if (searchSortDescending() === defaultDescending) {
    searchSortDescending.set(!defaultDescending);
  } else {
    searchSortColumn.set("date");
    searchSortDescending.set(true);
  }
  void runSearch(true, rollback);
}

function openReplayPath(path: string) {
  if (!path) {
    showNotice("This replay has no file path.");
    return;
  }
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

function searchColumnHeader(label: string, column: string, width: number) {
  const selected = () => searchSortColumn() === column;
  const sortLabel = () => selected() ? `${label} (${searchSortDescending() ? "desc" : "asc"})` : label;
  return (
    <View
      role="columnheader"
      accessibilityLabel={selected() ? `${label}, sorted ${searchSortDescending() ? "descending" : "ascending"}` : `Sort by ${label}`}
      accessibilityState={{ selected: selected() }}
      onPress={() => selectSearchSort(column)}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ width, minHeight: 34, justifyContent: "center" }}
    >
      <Text style={{ fontSize: 12, fontWeight: "600", color: selected() ? colors.foreground : colors.mutedForeground }}>{sortLabel()}</Text>
    </View>
  );
}

function showNotice(message: string) {
  toast.info(message);
}

function openPalette() {
  paletteQuery.set("");
  paletteOpen.set(true);
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
  "mod+k": openPalette,
  "mod+p": openPalette,
  // Flips debug detail for this session only, as the other front ends do.
  "mod+shift+d": () => debugMode.set(!debugMode()),
});

const descriptions: Record<string, string> = {
  "replay-inspector": "Open a replay file or drag one into this window. Replay metadata, players, battle events, chat, damage, achievements, and export controls appear here.",
  "launcher": "Install, update, and start World of Warships builds from one place.",
  "statistics": "Choose a replay directory to aggregate ships, maps, modes, players, and battle results.",
  "player-tracker": "Track selected players and refresh their recent battle records.",
  "search": "Search ships, upgrades, consumables, maps, and other game parameters.",
  "armor-viewer": "Select a ship and inspect its armor model, sections, thicknesses, and visibility controls.",
  "unpacker": "Browse game packages, preview files, and extract selected content.",
  "settings": "Configure game paths, replay loading, updates, sharing, collaboration, language, theme, and diagnostics.",
};

function metric(label: string, value: () => string) {
  return (
    <Surface style={{ minWidth: 145, flex: 1, padding: 14, gap: 5 }}>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{label}</Text>
      <Text style={{ fontSize: 23, fontWeight: "600", color: colors.cardForeground }}>{value()}</Text>
    </Surface>
  );
}

function StatisticsView() {
  const current = () => statistics();
  return (
    <View disabled={statisticsLoading()} style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Stats</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Recorded battles from the shared session database</Text>
        </View>
        <Button label={statisticsLoading() ? "Loading..." : "Refresh"} variant="ghost" disabled={statisticsLoading()} onPress={() => void loadStatistics()}/>
      </View>
      <If when={statisticsLoading() && !current()}>
        <EmptyState title="Loading statistics" description="Reading recorded battles."/>
      </If>
      <If when={!statisticsLoading() && statisticsError() && (!current() || current()!.games === 0)}>
        <EmptyState title="Statistics unavailable" description={statisticsError()!} action={<Button label="Try Again" onPress={() => void loadStatistics()}/>}/>
      </If>
      <If when={current() && current()!.games === 0 && !statisticsError()}>
        <EmptyState title="No recorded battles" description="Battle results appear here after replays have been processed." action={<Button label="Refresh" onPress={() => void loadStatistics()}/>}/>
      </If>
      <If when={current() && current()!.games > 0}>
        <Scroll style={{ flex: 1 }}>
          <View style={{ width: "100%", maxWidth: 1100, alignSelf: "center", padding: 18, gap: 14 }}>
            <If when={statisticsError()}>
              <Surface style={{ padding: 12, borderColor: colors.destructive }}>
                <Text style={{ fontSize: 12, color: colors.destructive }}>{statisticsError()}</Text>
              </Surface>
            </If>
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 10 }}>
              {metric("Battles", () => current()!.games.toFixed(0))}
              {metric("Win rate", () => `${current()!.winRate.toFixed(1)}%`)}
              {metric("Average damage", () => current()!.averageDamage.toFixed(0))}
              {metric("Average base XP", () => current()!.averageBaseXp.toFixed(0))}
              {metric("Average frags", () => current()!.averageFrags.toFixed(2))}
            </View>
            <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 650 }}>
              <Surface style={{ minWidth: 650, overflow: "hidden" }}>
                <View role="table" accessibilityLabel="Statistics by ship">
                <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.muted }}>
                  <Text role="columnheader" style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Ship</Text>
                  <Text role="columnheader" style={{ width: 90, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Battles</Text>
                  <Text role="columnheader" style={{ width: 100, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Win rate</Text>
                  <Text role="columnheader" style={{ width: 130, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Avg damage</Text>
                  <Text role="columnheader" style={{ width: 110, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Avg XP</Text>
                </View>
                <Each of={current()!.ships}>
                  {(ship) => (
                    <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                      <View role="cell" style={{ flex: 1 }}>
                        <Text style={{ fontSize: 13, color: colors.cardForeground }}>{ship().shipName}</Text>
                        <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{ship().shipId}</Text>
                      </View>
                      <Text role="cell" style={{ width: 90, fontSize: 12, color: colors.cardForeground }}>{ship().games.toFixed(0)}</Text>
                      <Text role="cell" style={{ width: 100, fontSize: 12, color: colors.cardForeground }}>{ship().winRate.toFixed(1)}%</Text>
                      <Text role="cell" style={{ width: 130, fontSize: 12, color: colors.cardForeground }}>{ship().averageDamage.toFixed(0)}</Text>
                      <Text role="cell" style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{ship().averageBaseXp.toFixed(0)}</Text>
                    </View>
                  )}
                </Each>
                </View>
              </Surface>
            </Scroll>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

const playerTrackerPeriods = [
  { id: "hour", label: "Past hour" },
  { id: "six_hours", label: "Past six hours" },
  { id: "day", label: "Past day" },
  { id: "week", label: "Past week" },
  { id: "month", label: "Past month" },
  { id: "all", label: "All time" },
];

function PlayerTrackerView() {
  const current = () => playerTracker();
  const periodLabel = () => playerTrackerPeriods.find((period) => period.id === playerTrackerPeriod())?.label ?? "Past day";
  const visiblePlayers = () => current()?.players ?? [];
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Player Tracker</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Players met in indexed battles</Text>
        </View>
        <Menu
          align="end"
          offset={5}
          margin={8}
          trigger={
            <View accessibilityLabel="Select player tracker period" hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 32, minWidth: 112, paddingHorizontal: 10, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}>
              <Text style={{ fontSize: 12.5, color: colors.foreground }}>{periodLabel()}</Text>
            </View>
          }
        >
          <Surface style={{ minWidth: 190, padding: 5, backgroundColor: colors.popover }}>
            <Each of={playerTrackerPeriods}>
              {(period) => <MenuRow label={period().label} note={period().id === playerTrackerPeriod() ? "Selected" : undefined} onSelect={() => selectPlayerTrackerPeriod(period().id)}/>}
            </Each>
          </Surface>
        </Menu>
        <Button label={playerTrackerLoading() ? "Loading..." : "Refresh"} variant="ghost" disabled={playerTrackerLoading()} onPress={() => void loadPlayerTracker()}/>
      </View>
      <View style={{ paddingHorizontal: 18, paddingTop: 14 }}>
        <Input
          accessibilityLabel="Filter tracked players"
          value={playerTrackerQuery()}
          placeholder="Filter by player, clan, or account ID"
          onChange={({ value }) => updatePlayerTrackerQuery(value)}
          style={inputStyle}
        />
      </View>
      <If when={playerTrackerLoading() && !current()}>
        <EmptyState title="Loading player history" description="Reading indexed battles."/>
      </If>
      <If when={!playerTrackerLoading() && playerTrackerError() && !current()}>
        <EmptyState title="Player tracker unavailable" description={playerTrackerError()!} action={<Button label="Try Again" onPress={() => void loadPlayerTracker()}/>}/>
      </If>
      <If when={current()}>
        <View style={{ flex: 1, minHeight: 0, padding: 18, gap: 12 }}>
          <If when={playerTrackerError()}>
            <Surface style={{ padding: 12, borderColor: colors.destructive }}>
              <Text style={{ fontSize: 12, color: colors.destructive }}>{playerTrackerError()}</Text>
            </Surface>
          </If>
          <If when={current()!.players.length === 0}>
            <EmptyState
              title={playerTrackerQuery().trim() ? "No matching players" : "No players found"}
              description={playerTrackerQuery().trim() ? "Change the filter to show tracked players." : "Index replays to build encounter history."}
            />
          </If>
          <If when={visiblePlayers().length > 0}>
            <View style={{ flex: 1, minHeight: 0, gap: 6 }}>
              <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>
                Showing {visiblePlayers().length.toFixed(0)} of {current()!.totalPlayers.toFixed(0)} players
              </Text>
              <Scroll style={{ flex: 1 }}>
                <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 620 }}>
                  <Surface style={{ minWidth: 620, overflow: "hidden" }}>
                  <View role="table" accessibilityLabel="Tracked players">
                    <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.muted }}>
                      <Text role="columnheader" style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Player</Text>
                      <Text role="columnheader" style={{ width: 180, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Clan</Text>
                      <Text role="columnheader" style={{ width: 110, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Encounters</Text>
                    </View>
                    <Each of={visiblePlayers()}>
                      {(player) => (
                        <View role="row" style={{ minHeight: 38, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                          <View role="cell" style={{ flex: 1 }}>
                            <Text style={{ fontSize: 13, color: colors.cardForeground }}>{player().name}</Text>
                            <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{player().accountId}</Text>
                          </View>
                          <Text role="cell" style={{ width: 180, fontSize: 12, color: colors.cardForeground }}>{player().clan || "No clan"}</Text>
                          <Text role="cell" style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{player().encounters.toFixed(0)}</Text>
                        </View>
                      )}
                    </Each>
                  </View>
                  </Surface>
                </Scroll>
              </Scroll>
            </View>
          </If>
        </View>
      </If>
    </View>
  );
}

function SearchView() {
  const current = () => searchState();
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Search</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Query the shared replay index</Text>
        </View>
        <Button label={searchLoading() ? "Searching..." : "Run Search"} disabled={searchLoading()} onPress={() => void runSearch()}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, padding: 18, gap: 12 }}>
        <Input
          accessibilityLabel="Replay search query"
          disabled={searchLoading()}
          value={searchQuery()}
          placeholder="map:Ocean and outcome:win"
          onChange={({ value }) => searchQuery.set(value)}
          style={inputStyle}
        />
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>
          An empty query matches every indexed replay. Use fields such as map, mode, outcome, player, clan, ship, tier, and damage.
        </Text>
        <If when={!searchHasRun()}>
          <EmptyState title="Search indexed replays" description="Enter a query or run an empty query to list recent indexed battles."/>
        </If>
        <If when={searchLoading() && !current()}>
          <EmptyState title="Searching replays" description="Reading the shared replay index."/>
        </If>
        <If when={!searchLoading() && searchError() && !current()}>
          <EmptyState title="Search unavailable" description={searchError()!} action={<Button label="Try Again" onPress={() => void runSearch()}/>}/>
        </If>
        <If when={!searchLoading() && !searchError() && current() && current()!.results.length === 0}>
          <EmptyState title="No matching replays" description="Change the query and run the search again."/>
        </If>
        <If when={current() && current()!.results.length > 0}>
          <View style={{ flex: 1, minHeight: 0, gap: 6 }}>
            <If when={searchError()}>
              <Surface style={{ padding: 12, borderColor: colors.destructive }}>
                <Text style={{ fontSize: 12, color: colors.destructive }}>{searchError()}</Text>
              </Surface>
            </If>
            <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>
              {current()!.truncated ? "Showing the first 500 matches" : `${current()!.results.length.toFixed(0)} matches`}
            </Text>
            <Scroll style={{ flex: 1 }}>
              <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 1070 }}>
                <Surface style={{ minWidth: 1070, overflow: "hidden" }}>
                  <View role="table" accessibilityLabel="Replay search results">
                    <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.muted }}>
                      {searchColumnHeader("Date", "date", 180)}
                      {searchColumnHeader("Map", "map", 150)}
                      {searchColumnHeader("Mode", "mode", 150)}
                      <Text role="columnheader" style={{ width: 150, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Ship</Text>
                      {searchColumnHeader("Outcome", "outcome", 90)}
                      {searchColumnHeader("Damage", "damage", 110)}
                      {searchColumnHeader("Kills", "kills", 80)}
                      {searchColumnHeader("PR", "pr", 90)}
                    </View>
                    <Each of={current()!.results}>
                      {(result) => (
                        <View
                          role="row"
                          accessibilityLabel={`Open replay ${result().replayPath}`}
                          onPress={() => openReplayPath(result().replayPath)}
                          hoverStyle={{ backgroundColor: colors.accent }}
                          activeStyle={{ opacity: 0.72 }}
                          style={{ minHeight: 38, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}
                        >
                          <Text role="cell" style={{ width: 180, fontSize: 11.5, color: colors.cardForeground }}>{result().timestamp}</Text>
                          <Text role="cell" style={{ width: 150, fontSize: 12, color: colors.cardForeground }}>{result().map}</Text>
                          <View role="cell" style={{ width: 150 }}>
                            <Text style={{ fontSize: 12, color: colors.cardForeground }}>{result().mode}</Text>
                            <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{result().gameType}</Text>
                          </View>
                          <Text role="cell" style={{ width: 150, fontSize: 12, color: colors.cardForeground }}>{result().ship}</Text>
                          <Text role="cell" style={{ width: 90, fontSize: 12, color: colors.cardForeground }}>{result().outcome}</Text>
                          <Text role="cell" style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{result().damage}</Text>
                          <Text role="cell" style={{ width: 80, fontSize: 12, color: colors.cardForeground }}>{result().kills}</Text>
                          <Text role="cell" style={{ width: 90, fontSize: 12, color: colors.cardForeground }}>{result().pr}</Text>
                        </View>
                      )}
                    </Each>
                  </View>
                </Surface>
              </Scroll>
            </Scroll>
          </View>
        </If>
      </View>
    </View>
  );
}

const inputStyle = {
  minHeight: 34,
  paddingHorizontal: 9,
  borderWidth: 1,
  borderColor: colors.input,
  borderRadius: controlRadius(),
  backgroundColor: colors.background,
  color: colors.foreground,
};

function Content() {
  const tab = () => startup.tabs.find((candidate) => candidate.id === activeTab()) ?? startup.tabs[0];
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "replay-inspector" ? "flex" : "none" }}>
        <NativeReplayInspector openPath={replayOpenPath()} openRequestId={replayOpenRequestId()}/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "statistics" ? "flex" : "none" }}>
        <StatisticsView/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "player-tracker" ? "flex" : "none" }}>
        <PlayerTrackerView/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "search" ? "flex" : "none" }}>
        <SearchView/>
      </View>
      <View style={{ flex: 1, minHeight: 0, display: tab().id === "settings" ? "flex" : "none" }}>
        <SettingsView/>
      </View>
      <If when={tab().id !== "replay-inspector" && tab().id !== "statistics" && tab().id !== "player-tracker" && tab().id !== "search" && tab().id !== "settings"}>
        <EmptyState title={tab().label} description={descriptions[tab().id] ?? tab().description}/>
      </If>
    </View>
  );
}

function TacticsDialog() {
  return (
    <Dialog open={tacticsOpen()} onOpenChange={({ open }) => tacticsOpen.set(open)}>
      <Surface style={{ width: "100%", maxWidth: 720, minHeight: 460, padding: 18, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 20, fontWeight: "600", color: colors.popoverForeground }}>{startup.text.tacticsBoard}</Text>
        <Text style={{ fontSize: 13, color: colors.mutedForeground }}>Plan ship positions, paths, and map annotations.</Text>
        <View style={{ flex: 1, borderWidth: 1, borderColor: colors.border, borderRadius: controlRadius(), backgroundColor: colors.muted, alignItems: "center", justifyContent: "center" }}>
          <Text style={{ color: colors.mutedForeground }}>Select a map to start a tactics board.</Text>
        </View>
        <View style={{ alignItems: "flex-end" }}>
          <Button label="Close" onPress={() => tacticsOpen.set(false)}/>
        </View>
      </Surface>
    </Dialog>
  );
}

const commandItems = [
  ...startup.tabs.map((tab, index) => ({
    id: `tab:${tab.id}`,
    label: `Go to ${tab.label}`,
    keywords: [tab.id, tab.description],
    group: "Navigate",
    shortcut: index < 7 ? `mod+${index + 1}` : tab.id === "settings" ? "mod+," : undefined,
  })),
  { id: "action:tactics", label: startup.text.tacticsBoard, keywords: ["map", "plan"], group: "Actions" },
  { id: "action:updates", label: startup.text.checkUpdates, keywords: ["release", "version"], group: "Actions" },
  { id: "action:about", label: `${startup.text.about} WoWs Toolkit`, keywords: ["version"], group: "Actions" },
  { id: "action:copy-log", label: t("ui.replay.copy_latest_log"), keywords: ["log", "bug", "report"], group: "Actions" },
  { id: "action:send-all", label: t("ui.palette.send_all_replays"), keywords: ["shipbuilds", "upload", "share"], group: "Actions" },
  { id: "action:send-all-again", label: t("ui.palette.send_all_replays_again"), keywords: ["shipbuilds", "upload", "share"], group: "Actions" },
];

function executeCommand(id: string) {
  paletteOpen.set(false);
  if (id.startsWith("tab:")) {
    selectTab(id.slice(4));
  } else if (id === "action:tactics") {
    tacticsOpen.set(true);
  } else if (id === "action:updates") {
    void checkForUpdate("byHand");
  } else if (id === "action:about") {
    openAbout();
  } else if (id === "action:copy-log") {
    void copyLatestLog();
  } else if (id === "action:send-all") {
    void contributeAll("consult");
  } else if (id === "action:send-all-again") {
    void contributeAll("ignore");
  }
}

function CommandPalette() {
  return (
    <Dialog open={paletteOpen()} onOpenChange={({ open }) => paletteOpen.set(open)}>
      <Surface style={{ width: "100%", maxWidth: 640, maxHeight: Math.max(160, windowSize().height - 32), overflow: "hidden", backgroundColor: colors.popover }}>
        <Command
          items={commandItems}
          autoFocus
          query={paletteQuery()}
          placeholder="Type a command..."
          accessibilityLabel="Command palette"
          onSelect={({ id }) => executeCommand(id)}
          onQueryChange={({ query }) => paletteQuery.set(query)}
          onCancel={() => paletteOpen.set(false)}
          header={
            <Text style={{ paddingHorizontal: 14, paddingTop: 12, fontSize: 13, fontWeight: "600", color: colors.popoverForeground }}>
              Command Palette
            </Text>
          }
          empty={
            <Text style={{ padding: 18, textAlign: "center", fontSize: 13, color: colors.mutedForeground }}>
              No matching commands.
            </Text>
          }
          footer={
            <Text style={{ padding: 10, fontSize: 11.5, color: colors.mutedForeground }}>
              Enter to run. Escape clears, then closes.
            </Text>
          }
          searchBarStyle={{ minHeight: 46, paddingHorizontal: 14, borderBottomWidth: 1, borderBottomColor: colors.border }}
          searchStyle={{ fontSize: 14, color: colors.popoverForeground, placeholderColor: colors.mutedForeground }}
          listStyle={{ maxHeight: Math.min(420, Math.max(0, windowSize().height - 180)), padding: 6 }}
          itemClass="text-popover-foreground hover:bg-accent focus:bg-accent focus:text-accent-foreground"
          itemStyle={{ minHeight: 34, paddingHorizontal: 10, borderRadius: controlRadius(), fontSize: 13 }}
          groupStyle={{ paddingHorizontal: 10, paddingVertical: 6, fontSize: 11, fontWeight: "600", color: colors.mutedForeground }}
          separatorStyle={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}
        />
      </Surface>
    </Dialog>
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
        menu={<ApplicationMenu onCommandPalette={openPalette} onTacticsBoard={() => tacticsOpen.set(true)}/>}
      />
      <View style={{ flex: 1, minHeight: 0 }}><Content/></View>
      <View style={{ flexDirection: "row", alignItems: "center", borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.surface }}>
        <View style={{ paddingHorizontal: 8 }}><GameDataStatus/></View>
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
      <TacticsDialog/>
      <CommandPalette/>
    </View>
  );
}

mount(App);
