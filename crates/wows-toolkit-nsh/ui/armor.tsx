import { effect, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { Armor, ShipSearch } from "wows-toolkit/native";
import { t } from "./i18n";
import type { ShipListState } from "./palette";
import { colors } from "./theme";
import { PenCheckPanel } from "./armor/analysis";
import { followLegendDrag, Legend, ViewportDock } from "./armor/dock";
import { ExportDialog } from "./armor/export";
import { OptionsEdge, OptionsRail } from "./armor/options";
import { ShipsEdge, ShipsSidebar, sidebarCollapsed } from "./armor/sidebar";
import { armor, call, catalog, ensureStarted, ensureWatching, onArmorReveal, revealArmor, type CatalogView } from "./armor/state";

export { onArmorReveal };

let shipSearch: { from: CatalogView; search: ShipSearch } | null = null;

/** Every ship of the catalog, as the palette's ship layer reads it. Asking starts the viewer so the catalog loads. */
export function armorShipList(): ShipListState {
  ensureWatching();
  ensureStarted();
  const read = catalog();
  if (read) {
    if (shipSearch?.from !== read) {
      const ships = read.nations.flatMap((nation) => nation.classes.flatMap((group) => group.ships.map((ship) => ({
        paramIndex: ship.paramIndex,
        displayName: ship.name,
        tier: ship.roman,
        class: group.label,
        nation: nation.label,
      }))));
      shipSearch = { from: read, search: new ShipSearch(ships) };
    }
    return { kind: "ready", ships: shipSearch.search };
  }
  const status = armor()?.status;
  if (status === "failed" || status === "notConfigured") return { kind: "unavailable", reason: armor()?.statusReason ?? t("ui.armor.load_settings") };
  return { kind: "pending" };
}

/** Shows the Armor Viewer with the chosen ship open. */
export function viewArmorShip(paramIndex: string, displayName: string) {
  revealArmor();
  call(Armor.openShip(paramIndex, displayName));
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
