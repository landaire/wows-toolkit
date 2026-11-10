import { effect, keymap, signal, Text, View, windowSize } from "nsh";
import { Command, Dialog } from "nsh/base";
import { Palette, type AppTab, type PaletteLayer, type ShipSearch } from "wows-toolkit/native";
import { Surface } from "./components";
import { searchFor } from "./search";
import { colors, controlRadius } from "./theme";

// How long typing rests before an index-backed layer asks the index.
const LAYER_DEBOUNCE = 100;

/** What confirming a palette entry asks the app to do: one kind per GPUI action. */
export type PaletteAction =
  | { kind: "goTo"; tab: string }
  | { kind: "advancedSearch" }
  | { kind: "enterLayer"; layer: PaletteLayer }
  | { kind: "searchFor"; query: string }
  | { kind: "openReplayFile" }
  | { kind: "openReplayDirectory" }
  | { kind: "indexAllReplays" }
  | { kind: "refreshPersistedData" }
  | { kind: "importConstants" }
  | { kind: "contributeAllReplays"; ignoreLedger: boolean }
  | { kind: "copyLatestLog" }
  | { kind: "copyBugReportInfo" }
  | { kind: "setTheme"; theme: "system" | "dark" | "light" }
  | { kind: "viewArmor"; paramIndex: string; displayName: string }
  | { kind: "tacticsBoard" }
  | { kind: "checkUpdates" }
  | { kind: "about" };

type ActionOf<K extends PaletteAction["kind"]> = Extract<PaletteAction, { kind: K }>;

/**
 * Who carries out each action. The palette lists an entry only when its kind
 * has a handler here; `enterLayer` and `searchFor` are the palette's own.
 */
export type PaletteHandlers = { [K in Exclude<PaletteAction["kind"], "enterLayer" | "searchFor">]?: (action: ActionOf<K>) => void };

/** The loaded build's ships, as the ship layer sees them. */
export type ShipListState =
  | { kind: "pending" }
  | { kind: "ready"; ships: ShipSearch }
  | { kind: "unavailable"; reason: string };

type Entry = { label: string; action: PaletteAction };
type Layer = { layer: PaletteLayer; title: string; placeholder: string };

let tabs: AppTab[] = [];
let handlers: PaletteHandlers = {};
let armorShips: (() => ShipListState) | null = null;
let activeTab: () => string = () => "";
let shellLabels = { tacticsBoard: "", checkUpdates: "", about: "" };
const paletteOpen = signal(false);
const layer = signal<Layer | null>(null);
const query = signal("");
const layerEntries = signal<Entry[]>([]);
const layerLoading = signal(false);
const layerError = signal<string | null>(null);
let layerGeneration = 0;
let layerTimer: number | undefined;

/** Wires the palette to the app: its tabs, who carries out each action, the ship list and the active tab. */
export function bindPalette(options: {
  tabs: AppTab[];
  handlers: PaletteHandlers;
  activeTab: () => string;
  armorShips?: () => ShipListState;
  /** The words of the shell's own entries, which the GPUI palette has no strings for. */
  shellLabels: { tacticsBoard: string; checkUpdates: string; about: string };
}) {
  tabs = options.tabs;
  handlers = options.handlers;
  activeTab = options.activeTab;
  armorShips = options.armorShips ?? null;
  shellLabels = options.shellLabels;
}

function text() {
  return Palette.text(tabs.map((tab) => tab.label));
}

function rootEntries(): Entry[] {
  const words = text();
  const entries: Entry[] = [];
  const offer = (label: string, action: PaletteAction) => {
    const own = action.kind === "enterLayer" || action.kind === "searchFor";
    if (own || handlers[action.kind as keyof PaletteHandlers]) entries.push({ label, action });
  };
  // Tabs first because going somewhere is the common reason to open it.
  tabs.forEach((tab, index) => {
    if (tab.id !== "search") offer(words.goTo[index], { kind: "goTo", tab: tab.id });
  });
  offer(words.advancedSearch, { kind: "advancedSearch" });
  for (const each of words.layers) {
    if (each.layer === "armorShips" && (!armorShips || !handlers.viewArmor)) continue;
    offer(each.label, { kind: "enterLayer", layer: each.layer });
  }
  for (const seeded of words.seeded) offer(seeded.label, { kind: "searchFor", query: seeded.query });
  offer(words.openReplayFile, { kind: "openReplayFile" });
  offer(words.openReplayDirectory, { kind: "openReplayDirectory" });
  offer(words.indexAllReplays, { kind: "indexAllReplays" });
  offer(words.refreshPersistedData, { kind: "refreshPersistedData" });
  offer(words.importConstants, { kind: "importConstants" });
  offer(words.sendAllReplays, { kind: "contributeAllReplays", ignoreLedger: false });
  offer(words.sendAllReplaysAgain, { kind: "contributeAllReplays", ignoreLedger: true });
  offer(words.copyLatestLog, { kind: "copyLatestLog" });
  offer(words.copyBugReportInfo, { kind: "copyBugReportInfo" });
  offer(words.themeSystem, { kind: "setTheme", theme: "system" });
  offer(words.themeDark, { kind: "setTheme", theme: "dark" });
  offer(words.themeLight, { kind: "setTheme", theme: "light" });
  offer(shellLabels.tacticsBoard, { kind: "tacticsBoard" });
  offer(shellLabels.checkUpdates, { kind: "checkUpdates" });
  offer(shellLabels.about, { kind: "about" });
  return entries;
}

/** Opens the root list, replacing whatever the palette was showing. */
export function openPalette() {
  layer.set(null);
  query.set("");
  paletteOpen.set(true);
}

/** Opens the palette on one of its second layers. */
export function openPaletteLayer(chosen: PaletteLayer) {
  const words = text().layers.find((each) => each.layer === chosen);
  if (!words) return;
  layer.set({ layer: chosen, title: words.title, placeholder: words.placeholder });
  query.set("");
  paletteOpen.set(true);
  searchLayer("");
}

function closePalette() {
  paletteOpen.set(false);
  layer.set(null);
}

/** Does what an entry says. */
export function runPaletteAction(action: PaletteAction) {
  switch (action.kind) {
    case "enterLayer":
      openPaletteLayer(action.layer);
      return;
    case "searchFor":
      closePalette();
      searchFor(action.query);
      return;
    default: {
      closePalette();
      const handler = handlers[action.kind] as ((action: PaletteAction) => void) | undefined;
      handler?.(action);
    }
  }
}

/** Refills the open layer for `needle`. */
function searchLayer(needle: string) {
  const open = layer();
  if (!open) return;
  layerGeneration += 1;
  const asked = layerGeneration;
  layerError.set(null);
  clearTimeout(layerTimer);
  if (open.layer === "armorShips") {
    layerLoading.set(false);
    const ships = armorShips?.() ?? { kind: "unavailable", reason: "no game data" };
    layerEntries.set(ships.kind === "ready"
      ? ships.ships.rank(needle).map((ship) => ({
        label: ship.label,
        action: { kind: "viewArmor", paramIndex: ship.paramIndex, displayName: ship.displayName },
      }))
      : []);
    return;
  }
  layerEntries.set([]);
  layerLoading.set(true);
  layerTimer = setTimeout(() => {
    void Palette.layerRows(open.layer, needle).then(
      (rows) => {
        if (asked !== layerGeneration) return;
        layerEntries.set(rows.map((row) => ({ label: row.label, action: { kind: "searchFor", query: row.query } })));
        layerLoading.set(false);
      },
      (error) => {
        if (asked !== layerGeneration) return;
        layerError.set(error instanceof Error ? error.message : String(error));
        layerLoading.set(false);
      },
    );
  }, LAYER_DEBOUNCE);
}

function emptyMessage(): string {
  const open = layer();
  const words = text();
  if (open?.layer === "armorShips") {
    const ships = armorShips?.();
    if (ships?.kind === "pending") return words.shipsLoading;
    if (ships?.kind === "unavailable") return Palette.shipsUnavailable(ships.reason);
  }
  const error = layerError();
  return error ? Palette.searchFailed(error) : words.noMatches;
}

keymap.bind("mod+k", openPalette);
keymap.bind("mod+p", () => {
  // On the Armor Viewer the palette opens straight on its ship list.
  if (activeTab() === "armor-viewer" && armorShips && handlers.viewArmor) {
    openPaletteLayer("armorShips");
  } else {
    openPalette();
  }
});

function LayerHeader(props: { title: string }) {
  const words = text();
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 10, paddingVertical: 6, borderBottomWidth: 1, borderBottomColor: colors.border }}>
      <View
        testID="palette-back"
        accessibilityLabel={`Back to ${words.title}`}
        onPress={openPalette}
        hoverStyle={{ backgroundColor: colors.accent }}
        style={{ paddingHorizontal: 6, paddingVertical: 2, borderRadius: controlRadius() }}
      >
        <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{`< ${words.title}`}</Text>
      </View>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{">"}</Text>
      <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>{props.title}</Text>
      <Text style={{ fontSize: 11, color: colors.mutedForeground }}>{words.backHint}</Text>
    </View>
  );
}

/** The command palette over the window. */
export function CommandPalette() {
  const root = () => (paletteOpen() && !layer() ? rootEntries() : []);
  const entries = () => (layer() ? layerEntries() : root());
  const commandStyle = {
    searchBarStyle: { minHeight: 46, paddingHorizontal: 14, borderBottomWidth: 1, borderBottomColor: colors.border },
    searchStyle: { fontSize: 14, color: colors.popoverForeground, placeholderColor: colors.mutedForeground },
    itemClass: "text-popover-foreground hover:bg-accent focus:bg-accent focus:text-accent-foreground",
    itemStyle: { minHeight: 34, paddingHorizontal: 10, borderRadius: controlRadius(), fontSize: 13 },
  };
  // Backspace in an empty field leaves a layer for the root list. Bound only
  // then: a bound key is the binding's, so the field would lose it.
  effect(() => {
    if (paletteOpen() && layer() && query() === "") keymap.bind("backspace", openPalette, { when: "always" });
  });
  return (
    <Dialog open={paletteOpen()} onOpenChange={({ open }) => (open ? paletteOpen.set(true) : closePalette())}>
      <Surface style={{ width: "100%", maxWidth: 640, maxHeight: Math.max(160, windowSize().height - 32), overflow: "hidden", backgroundColor: colors.popover }}>
        <Command
          items={entries().map((entry, index) => ({ id: String(index), label: entry.label }))}
          autoFocus
          filterable={!layer()}
          loading={layer() !== null && layerLoading()}
          query={query()}
          placeholder={layer()?.placeholder ?? text().placeholder}
          accessibilityLabel="Command palette"
          onSelect={({ id }) => {
            const entry = entries()[Number(id)];
            if (entry) runPaletteAction(entry.action);
          }}
          onQueryChange={({ query: typed }) => {
            query.set(typed);
            if (layer()) searchLayer(typed);
          }}
          onCancel={() => (layer() ? openPalette() : closePalette())}
          header={
            layer()
              ? <LayerHeader title={layer()!.title}/>
              : <Text style={{ paddingHorizontal: 14, paddingTop: 12, fontSize: 13, fontWeight: "600", color: colors.popoverForeground }}>{text().title}</Text>
          }
          empty={
            <Text style={{ padding: 18, textAlign: "center", fontSize: 13, color: colors.mutedForeground }}>
              {layer() ? emptyMessage() : text().noMatches}
            </Text>
          }
          searchBarStyle={commandStyle.searchBarStyle}
          searchStyle={commandStyle.searchStyle}
          listStyle={{ maxHeight: Math.min(420, Math.max(0, windowSize().height - 180)), padding: 6 }}
          itemClass={commandStyle.itemClass}
          itemStyle={commandStyle.itemStyle}
        />
      </Surface>
    </Dialog>
  );
}
