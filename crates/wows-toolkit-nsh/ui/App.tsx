import { keymap, mount, Scroll, signal, Storage, Text, View, windowSize } from "nsh";
import { Window } from "nsh/desktop";
import { Checkbox, Command, Dialog, Input, Menu, Radio, RadioGroup, Toolbar } from "nsh/base";
import { ReplayInspector as NativeReplayInspector, Toolkit, type PlayerTrackerState, type SearchState, type SettingsState, type StatisticsState } from "wows-toolkit/native";
import { Button, EmptyState, MenuRow, Surface } from "./components";
import { colors, controlRadius } from "./theme";

const startup = Toolkit.startup();
const activeTab = signal(startup.tabs[0].id);
const aboutOpen = signal(false);
const tacticsOpen = signal(false);
const paletteOpen = signal(false);
const paletteQuery = signal("");
const notice = signal<string | null>(null);
const settings = signal<SettingsState | null>(null);
const settingsBaseline = signal<SettingsState | null>(null);
const settingsLoading = signal(false);
const settingsSaving = signal(false);
const settingsError = signal<string | null>(null);
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
let settingsRequested = false;
let statisticsRequested = false;
let playerTrackerRequested = false;
let playerTrackerGeneration = 0;
let playerTrackerFilterTimer: number | undefined;
let playerTrackerRequestActive = false;
let playerTrackerRequestPending = false;
const STORAGE_TAB = "application.active-tab";

Window.setTitle(startup.title);
void Storage.getItem(STORAGE_TAB).then((saved) => {
  if (saved && startup.tabs.some((tab) => tab.id === saved)) selectTab(saved);
});

function selectTab(id: string) {
  if (id === "settings" && !settingsRequested) {
    settingsRequested = true;
    void loadSettings();
  }
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

async function loadSettings() {
  settingsLoading.set(true);
  settingsError.set(null);
  try {
    const loaded = await Toolkit.loadSettings();
    settings.set(loaded);
    settingsBaseline.set({ ...loaded });
  } catch (error) {
    settingsError.set(`Settings could not be loaded: ${errorMessage(error)}`);
  } finally {
    settingsLoading.set(false);
  }
}

function updateSetting<K extends keyof SettingsState>(key: K, value: SettingsState[K]) {
  const current = settings();
  if (current) settings.set({ ...current, [key]: value });
}

async function saveSettings() {
  const current = settings();
  const baseline = settingsBaseline();
  if (!current || !baseline || settingsSaving()) return;
  const editable: (keyof SettingsState)[] = [
    "wowsDir",
    "locale",
    "checkForUpdates",
    "enableLogging",
    "debugMode",
    "autoLoadLatestReplay",
    "outputDir",
    "autoDumpGameData",
    "gameDataCacheDir",
    "dataSharing",
    "proxyUrl",
    "collabDisplayName",
    "disableAutoOpenSessionWindows",
  ];
  const changed = editable.filter((key) => current[key] !== baseline[key]);
  if (changed.length === 0) {
    showNotice("No settings changes.");
    return;
  }
  settingsSaving.set(true);
  settingsError.set(null);
  try {
    const committed = await Toolkit.saveSettings(current, changed);
    settings.set(committed);
    settingsBaseline.set({ ...committed });
    showNotice("Settings saved.");
    try {
      const refreshed = await Toolkit.loadSettings();
      settings.set(refreshed);
      settingsBaseline.set({ ...refreshed });
    } catch (error) {
      settingsError.set(`Settings were saved but could not be refreshed: ${errorMessage(error)}`);
    }
  } catch (error) {
    settingsError.set(`Settings could not be saved: ${errorMessage(error)}`);
  } finally {
    settingsSaving.set(false);
  }
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

function openDroppedReplay(uris: string[]) {
  for (const uri of uris) {
    try {
      const url = new URL(uri);
      if (url.protocol === "file:" && url.pathname.toLowerCase().endsWith(".wowsreplay")) {
        openReplayPath(decodeURIComponent(url.pathname));
        return;
      }
    } catch {
      continue;
    }
  }
  showNotice("Drop a World of Warships replay file.");
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
  notice.set(message);
  setTimeout(() => {
    if (notice() === message) notice.set(null);
  }, 4000);
}

function openPalette() {
  paletteQuery.set("");
  paletteOpen.set(true);
}

type Release = {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
};

function version(value: string): number[] | null {
  const match = value.replace(/^v/, "").match(/^(\d+)\.(\d+)\.(\d+)/);
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : null;
}

function compareVersions(left: number[], right: number[]): number {
  for (let index = 0; index < 3; index += 1) {
    if (left[index] !== right[index]) return left[index] - right[index];
  }
  return 0;
}

async function checkForUpdates() {
  showNotice("Checking for updates...");
  try {
    const response = await fetch("https://api.github.com/repos/landaire/wows-toolkit/releases?per_page=100", {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!response.ok) throw new Error(`release server returned ${response.status}`);
    const current = version(startup.version);
    if (!current) throw new Error(`invalid running version ${startup.version}`);
    const includePrerelease = startup.version.includes("-");
    const available = (await response.json() as Release[])
      .filter((release) => !release.draft && (includePrerelease || !release.prerelease))
      .map((release) => ({ release, version: version(release.tag_name) }))
      .filter((candidate): candidate is { release: Release; version: number[] } =>
        candidate.version !== null && compareVersions(candidate.version, current) > 0)
      .sort((left, right) => compareVersions(right.version, left.version))[0];
    showNotice(available ? `Version ${available.release.tag_name.replace(/^v/, "")} is available.` : startup.text.appUpToDate);
  } catch (error) {
    showNotice(`Update check failed: ${error instanceof Error ? error.message : String(error)}`);
  }
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
});

function ApplicationMenu() {
  return (
    <Toolbar accessibilityLabel="Application menu">
      <Menu
        align="end"
        offset={5}
        margin={8}
        trigger={
          <View
            accessibilityLabel="Open application menu"
            hoverStyle={{ backgroundColor: colors.accent }}
            activeStyle={{ opacity: 0.68 }}
            style={{ width: 32, height: 30, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}
          >
            <Text style={{ fontSize: 18, color: colors.foreground }}>...</Text>
          </View>
        }
      >
        <Surface style={{ minWidth: 220, padding: 5, backgroundColor: colors.popover }}>
          <MenuRow label="Command Palette" note={keymap.label("mod+k")} onSelect={openPalette}/>
          <MenuRow label={startup.text.tacticsBoard} onSelect={() => tacticsOpen.set(true)}/>
          <MenuRow label={startup.text.checkUpdates} onSelect={() => void checkForUpdates()}/>
          <MenuRow label={`${startup.text.about} WoWs Toolkit`} onSelect={() => aboutOpen.set(true)}/>
          <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>
          <MenuRow label={startup.text.quit} note={keymap.label("mod+q")} danger onSelect={() => Window.close()}/>
        </Surface>
      </Menu>
    </Toolbar>
  );
}

function TabStrip() {
  return (
    <View role="tablist" style={{ minHeight: 38, flexDirection: "row", alignItems: "center", gap: 2, paddingHorizontal: 8, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.card }}>
      <Each of={startup.tabs}>
        {(tab, index) => {
          const selected = () => activeTab() === tab().id;
          return (
            <View
              role="tab"
              accessibilityLabel={tab().label}
              accessibilityState={{ selected: selected() }}
              onPress={() => selectTab(tab().id)}
              hoverStyle={selected() ? undefined : { backgroundColor: colors.accent }}
              activeStyle={{ opacity: 0.7 }}
              style={{
                height: 30,
                paddingHorizontal: 10,
                borderRadius: controlRadius(),
                alignItems: "center",
                justifyContent: "center",
                backgroundColor: selected() ? colors.primary : "transparent",
              }}
            >
              <Text style={{ fontSize: 12.5, fontWeight: selected() ? "600" : "400", color: selected() ? colors.primaryForeground : colors.mutedForeground }}>
                {tab().label}
              </Text>
            </View>
          );
        }}
      </Each>
      <View style={{ flex: 1 }}/>
      <ApplicationMenu/>
    </View>
  );
}

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

function SettingsField(props: {
  label: string;
  description: string;
  value: string;
  placeholder?: string;
  masked?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <View style={{ gap: 5 }}>
      <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.label}</Text>
      <Input
        accessibilityLabel={props.label}
        value={props.value}
        placeholder={props.placeholder}
        masked={props.masked}
        onChange={({ value }) => props.onChange(value)}
        style={inputStyle}
      />
      <Text style={{ fontSize: 12, lineHeight: 17, color: colors.mutedForeground }}>{props.description}</Text>
    </View>
  );
}

function SettingsToggle(props: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      class="group flex-row items-start gap-3"
      indicatorClass="mt-0.5 w-[18px] h-[18px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<Text class="text-[11px] text-primary-foreground">{props.checked ? "x" : ""}</Text>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <View style={{ flex: 1, gap: 2 }}>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.label}</Text>
        <Text style={{ fontSize: 12, lineHeight: 17, color: colors.mutedForeground }}>{props.description}</Text>
      </View>
    </Checkbox>
  );
}

function SettingsSection(props: { title: string; children: JSX.Element }) {
  return (
    <Surface style={{ padding: 16, gap: 14 }}>
      <Text style={{ fontSize: 16, fontWeight: "600", color: colors.cardForeground }}>{props.title}</Text>
      {props.children}
    </Surface>
  );
}

function SharingOption(props: { value: SettingsState["dataSharing"]; label: string; description: string }) {
  const selected = () => settings()?.dataSharing === props.value;
  return (
    <Radio
      accessibilityLabel={props.label}
      checked={selected()}
      class="group flex-row items-start gap-3"
      onChange={() => updateSetting("dataSharing", props.value)}
    >
      <View class="mt-0.5 w-[18px] h-[18px] rounded-full border border-border items-center justify-center">
        <View class="w-[10px] h-[10px] rounded-full group-checked:bg-primary"/>
      </View>
      <View style={{ flex: 1, gap: 2 }}>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>{props.label}</Text>
        <Text style={{ fontSize: 12, lineHeight: 17, color: colors.mutedForeground }}>{props.description}</Text>
      </View>
    </Radio>
  );
}

function SettingsView() {
  const current = () => settings();
  return (
    <View disabled={settingsLoading() || settingsSaving()} style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Settings</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Shared with the egui and GPUI applications</Text>
        </View>
        <Button label="Reload" variant="ghost" disabled={settingsLoading() || settingsSaving()} onPress={() => void loadSettings()}/>
        <Button label={settingsSaving() ? "Saving..." : "Save Settings"} variant="primary" disabled={!current() || settingsLoading() || settingsSaving()} onPress={() => void saveSettings()}/>
      </View>
      <If when={settingsLoading() && !current()}>
        <EmptyState title="Loading settings" description="Opening the shared WoWs Toolkit settings database."/>
      </If>
      <If when={!settingsLoading() && !current()}>
        <EmptyState title="Settings unavailable" description={settingsError() ?? "The settings database could not be opened."} action={<Button label="Try Again" onPress={() => void loadSettings()}/>}/>
      </If>
      <If when={current()}>
        <Scroll style={{ flex: 1 }}>
          <View style={{ width: "100%", maxWidth: 980, alignSelf: "center", padding: 18, gap: 14 }}>
            <If when={settingsError()}>
              <Surface style={{ padding: 12, borderColor: colors.destructive }}>
                <Text style={{ fontSize: 12, color: colors.destructive }}>{settingsError()}</Text>
              </Surface>
            </If>
            <SettingsSection title="Appearance and language">
              <View style={{ gap: 13 }}>
                <View style={{ gap: 3 }}>
                  <Text style={{ fontSize: 13, fontWeight: "600", color: colors.cardForeground }}>Theme</Text>
                  <Text style={{ fontSize: 13, color: colors.foreground }}>NSH default</Text>
                  <Text style={{ fontSize: 12, lineHeight: 17, color: colors.mutedForeground }}>Uses NSH system tokens and follows the desktop appearance.</Text>
                </View>
                <SettingsField
                  label="Language code"
                  description="Leave empty to use the system language. A language change applies after restart."
                  value={current()!.locale}
                  placeholder={startup.locale}
                  onChange={(value) => updateSetting("locale", value)}
                />
              </View>
            </SettingsSection>
            <SettingsSection title="Game and files">
              <View style={{ gap: 14 }}>
                <SettingsField
                  label="World of Warships directory"
                  description="Installation directory used to load game data."
                  value={current()!.wowsDir}
                  placeholder="/path/to/World_of_Warships"
                  onChange={(value) => updateSetting("wowsDir", value)}
                />
                <SettingsField
                  label="Resource unpacker output directory"
                  description="Extracted game files are written here."
                  value={current()!.outputDir}
                  placeholder="/path/to/output"
                  onChange={(value) => updateSetting("outputDir", value)}
                />
                <SettingsField
                  label="Game data cache directory"
                  description="Leave empty to use the application data directory."
                  value={current()!.gameDataCacheDir}
                  placeholder={current()!.storageDir}
                  onChange={(value) => updateSetting("gameDataCacheDir", value)}
                />
                <Text style={{ fontSize: 11, color: colors.mutedForeground }}>Application data: {current()!.storageDir || "Unavailable"}</Text>
              </View>
            </SettingsSection>
            <SettingsSection title="Replay and diagnostics">
              <View style={{ gap: 14 }}>
                <SettingsToggle
                  label="Automatically load the latest replay"
                  description="Open the newest replay when replay monitoring starts."
                  checked={current()!.autoLoadLatestReplay}
                  onChange={(checked) => updateSetting("autoLoadLatestReplay", checked)}
                />
                <SettingsToggle
                  label="Cache game data automatically"
                  description="Keep data required to open replays after a game update."
                  checked={current()!.autoDumpGameData}
                  onChange={(checked) => updateSetting("autoDumpGameData", checked)}
                />
                <SettingsToggle
                  label="Check for updates at startup"
                  description="Query the release service when the application starts."
                  checked={current()!.checkForUpdates}
                  onChange={(checked) => updateSetting("checkForUpdates", checked)}
                />
                <SettingsToggle
                  label="Write diagnostic logs"
                  description="Write a rotated log file for diagnostics."
                  checked={current()!.enableLogging}
                  onChange={(checked) => updateSetting("enableLogging", checked)}
                />
                <SettingsToggle
                  label="Debug replay details"
                  description="Show replay data intended for diagnostic work."
                  checked={current()!.debugMode}
                  onChange={(checked) => updateSetting("debugMode", checked)}
                />
              </View>
            </SettingsSection>
            <SettingsSection title="Data sharing">
              <RadioGroup accessibilityLabel="Data sharing mode" class="gap-12">
                <SharingOption value="off" label="Off" description="Nothing is sent."/>
                <SharingOption value="build_data" label="Ship builds" description="Per-player ship builds are sent after a battle."/>
                <SharingOption value="replays" label="Replays" description="The replay file is sent once results are available."/>
              </RadioGroup>
            </SettingsSection>
            <SettingsSection title="Network and collaboration">
              <View style={{ gap: 14 }}>
                <SettingsField
                  label="Proxy URL"
                  description="Optional HTTP proxy. A host and port without a scheme uses http."
                  value={current()!.proxyUrl}
                  placeholder="proxy.example:8080"
                  masked
                  onChange={(value) => updateSetting("proxyUrl", value)}
                />
                <SettingsField
                  label="Collaboration display name"
                  description="Name shown to other participants in a collaboration session."
                  value={current()!.collabDisplayName}
                  onChange={(value) => updateSetting("collabDisplayName", value)}
                />
                <SettingsToggle
                  label="Do not open shared windows automatically"
                  description="Leave session windows closed until you choose to open them."
                  checked={current()!.disableAutoOpenSessionWindows}
                  onChange={(checked) => updateSetting("disableAutoOpenSessionWindows", checked)}
                />
              </View>
            </SettingsSection>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

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

function AboutDialog() {
  return (
    <Dialog open={aboutOpen()} onOpenChange={({ open }) => aboutOpen.set(open)}>
      <Surface style={{ width: "100%", maxWidth: 460, padding: 22, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 21, fontWeight: "600", color: colors.popoverForeground }}>WoWs Toolkit</Text>
        <Text style={{ fontSize: 13, color: colors.mutedForeground }}>Version {startup.version}</Text>
        <Text style={{ fontSize: 14, lineHeight: 21, color: colors.popoverForeground }}>Tools for World of Warships replays, game data, statistics, rendering, and collaboration.</Text>
        <View style={{ alignItems: "flex-end" }}>
          <Button label="Close" onPress={() => aboutOpen.set(false)}/>
        </View>
      </Surface>
    </Dialog>
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
];

function executeCommand(id: string) {
  paletteOpen.set(false);
  if (id.startsWith("tab:")) {
    selectTab(id.slice(4));
  } else if (id === "action:tactics") {
    tacticsOpen.set(true);
  } else if (id === "action:updates") {
    void checkForUpdates();
  } else if (id === "action:about") {
    aboutOpen.set(true);
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

function Notice() {
  return (
    <If when={notice()}>
      <Surface style={{ position: "absolute", right: 16, bottom: 36, minWidth: 280, maxWidth: 420, padding: 14, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{notice()}</Text>
      </Surface>
    </If>
  );
}

function App() {
  return (
    <View onFileDrop={({ uris }) => openDroppedReplay(uris)} style={{ flex: 1, backgroundColor: colors.background }}>
      <TabStrip/>
      <View style={{ flex: 1 }}><Content/></View>
      <View style={{ height: 26, flexDirection: "row", alignItems: "center", paddingHorizontal: 10, borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.card }}>
        <Text style={{ flex: 1, fontSize: 11.5, color: colors.mutedForeground }}>Ready</Text>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>WoWs Toolkit {startup.version}</Text>
      </View>
      <Notice/>
      <AboutDialog/>
      <TacticsDialog/>
      <CommandPalette/>
    </View>
  );
}

mount(App);
