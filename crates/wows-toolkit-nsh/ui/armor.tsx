import { effect, signal, Text, View, windowSize } from "nsh";
import { Command, Dialog, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { Surface } from "./components";
import { t } from "./i18n";
import { colors, controlRadius } from "./theme";
import { PenCheckPanel } from "./armor/analysis";
import { followLegendDrag, Legend, ViewportDock } from "./armor/dock";
import { ExportDialog } from "./armor/export";
import { OptionsEdge, OptionsRail } from "./armor/options";
import { ShipsEdge, ShipsSidebar, sidebarCollapsed } from "./armor/sidebar";
import { armor, call, catalog, ensureStarted, ensureWatching, fold, onArmorReveal, revealArmor } from "./armor/state";

export { onArmorReveal };

const paletteOpen = signal(false);
const paletteQuery = signal("");

/** Opens the palette layer that lists every ship for the Armor Viewer. */
export function openArmorShipPalette() {
  ensureWatching();
  ensureStarted();
  paletteQuery.set("");
  paletteOpen.set(true);
}

function shipItems() {
  const read = catalog();
  if (!read) return [];
  return read.nations.flatMap((nation) => nation.classes.flatMap((group) => group.ships.map((ship) => ({
    id: ship.paramIndex,
    label: t("ui.palette.armor_ship_row", { name: ship.name, tier: ship.roman, class: group.label, nation: nation.label }),
    keywords: [fold(ship.name), ship.paramIndex],
  }))));
}

/** The palette layer listing every ship; choosing one opens it in the Armor Viewer. `onBack` returns to the root palette. */
export function ArmorShipPalette(props: { onBack: () => void }) {
  const items = () => shipItems();
  const names = () => new Map(catalog()?.nations.flatMap((nation) => nation.classes.flatMap((group) => group.ships.map((ship) => [ship.paramIndex, ship.name] as const))) ?? []);
  const empty = () => {
    const status = armor()?.status;
    if (status === "failed" || status === "notConfigured") return t("ui.palette.ships_unavailable", { reason: armor()?.statusReason ?? t("ui.armor.load_settings") });
    if (!catalog()) return t("ui.palette.ships_loading");
    return t("ui.palette.matches_found", { count: 0 });
  };
  return (
    <Dialog open={paletteOpen()} onOpenChange={({ open }) => paletteOpen.set(open)}>
      <Surface style={{ width: "100%", maxWidth: 640, maxHeight: Math.max(160, windowSize().height - 32), overflow: "hidden", backgroundColor: colors.popover }}>
        <Command
          items={items()}
          autoFocus
          query={paletteQuery()}
          placeholder={t("ui.palette.armor_ships_placeholder")}
          accessibilityLabel={t("ui.palette.armor_ships_title")}
          onSelect={({ id }) => {
            paletteOpen.set(false);
            revealArmor();
            call(Armor.openShip(id, names().get(id) ?? id));
          }}
          onQueryChange={({ query }) => paletteQuery.set(query)}
          onCancel={() => {
            paletteOpen.set(false);
            props.onBack();
          }}
          header={
            <Text style={{ paddingHorizontal: 14, paddingTop: 12, fontSize: 13, fontWeight: "600", color: colors.popoverForeground }}>
              {t("ui.palette.armor_ships_title")}
            </Text>
          }
          empty={<Text style={{ padding: 18, textAlign: "center", fontSize: 13, color: colors.mutedForeground }}>{empty()}</Text>}
          searchBarStyle={{ minHeight: 46, paddingHorizontal: 14, borderBottomWidth: 1, borderBottomColor: colors.border }}
          searchStyle={{ fontSize: 14, color: colors.popoverForeground, placeholderColor: colors.mutedForeground }}
          listStyle={{ maxHeight: Math.min(420, Math.max(0, windowSize().height - 180)), padding: 6 }}
          itemClass="text-popover-foreground hover:bg-accent focus:bg-accent focus:text-accent-foreground"
          itemStyle={{ minHeight: 34, paddingHorizontal: 10, borderRadius: controlRadius(), fontSize: 13 }}
          footer={<Text style={{ padding: 10, fontSize: 11.5, color: colors.mutedForeground }}>{t("ui.palette.back_hint")}</Text>}
        />
      </Surface>
    </Dialog>
  );
}

/** The Armor Viewer tab: ships list, viewports, Pen Check and the options rail. `shown` starts the viewer on first show. */
export function ArmorView(props: { shown: boolean }) {
  ensureWatching();
  effect(() => {
    if (props.shown) ensureStarted();
  });
  const optionsOpen = () => armor()?.optionsOpen ?? true;
  const penOpen = () => armor()?.penCheckOpen ?? false;
  return (
    <View testID="armor-viewer" onMouseMove={followLegendDrag} style={{ flex: 1, minHeight: 0, flexDirection: "row", backgroundColor: colors.background }}>
      <If when={sidebarCollapsed()}>
        <ShipsEdge/>
      </If>
      <View style={{ flex: 1, minWidth: 0 }}>
        <ResizablePanelGroup axis="horizontal" handleClass="bg-border group-active:bg-primary">
          <ResizablePanel size={240} minSize={180} maxSize={480} visible={!sidebarCollapsed()}>
            <If when={!sidebarCollapsed()}>
              <ShipsSidebar/>
            </If>
          </ResizablePanel>
          <ResizablePanel minSize={200}>
            <ViewportDock/>
          </ResizablePanel>
          <ResizablePanel size={320} minSize={260} maxSize={480} visible={penOpen()}>
            <If when={penOpen()}>
              <PenCheckPanel/>
            </If>
          </ResizablePanel>
        </ResizablePanelGroup>
      </View>
      <If when={optionsOpen()} else={<OptionsEdge/>}>
        <OptionsRail/>
      </If>
      <Legend/>
      <ExportDialog/>
    </View>
  );
}
