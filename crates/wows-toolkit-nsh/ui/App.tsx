import { keymap, mount, signal, Storage, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { Dialog, Menu, Toolbar } from "nsh/base";
import { Toolkit } from "wows-toolkit/native";
import { Button, EmptyState, MenuRow, Surface } from "./components";
import { colors, controlRadius } from "./theme";

const startup = Toolkit.startup();
const activeTab = signal(startup.tabs[0].id);
const aboutOpen = signal(false);
const tacticsOpen = signal(false);
const notice = signal<string | null>(null);
const STORAGE_TAB = "application.active-tab";

Window.setTitle(startup.title);
void Storage.getItem(STORAGE_TAB).then((saved) => {
  if (saved && startup.tabs.some((tab) => tab.id === saved)) activeTab.set(saved);
});

function selectTab(id: string) {
  activeTab.set(id);
  void Storage.setItem(STORAGE_TAB, id);
}

function showNotice(message: string) {
  notice.set(message);
  setTimeout(() => {
    if (notice() === message) notice.set(null);
  }, 4000);
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

function Content() {
  const tab = () => startup.tabs.find((candidate) => candidate.id === activeTab()) ?? startup.tabs[0];
  return (
    <EmptyState
      title={tab().label}
      description={descriptions[tab().id] ?? tab().description}
      action={tab().id === "replay-inspector" ? <Button variant="primary" label={startup.text.openReplay} onPress={() => showNotice(startup.text.openReplay)}/> : undefined}
    />
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
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <TabStrip/>
      <View style={{ flex: 1 }}><Content/></View>
      <View style={{ height: 26, flexDirection: "row", alignItems: "center", paddingHorizontal: 10, borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.card }}>
        <Text style={{ flex: 1, fontSize: 11.5, color: colors.mutedForeground }}>Ready</Text>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>WoWs Toolkit {startup.version}</Text>
      </View>
      <Notice/>
      <AboutDialog/>
      <TacticsDialog/>
    </View>
  );
}

mount(App);
