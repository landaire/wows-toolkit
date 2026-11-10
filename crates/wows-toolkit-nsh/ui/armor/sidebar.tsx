import { computed, effect, Image, signal, Text, untracked, View } from "nsh";
import { ContextMenu, Input, Popover, Tree, type TreeApi, type TreeEntry, type TreeItem } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { MenuRow, Surface } from "../components";
import { inputStyle } from "../shell/controls";
import { t } from "../i18n";
import { Icon } from "../icons";
import { colors, controlRadius } from "../theme";
import * as toast from "../shell/toast";
import { armor, call, catalog, type CatalogShip, type CatalogView, errorText } from "./state";
import { CheckRow, IconButton, tip, ToolButton } from "./widgets";

/** Whether the ships list is folded to its edge rail; per session, as in the other front ends. */
export const sidebarCollapsed = signal(false);

const query = signal("");
const filtered = signal<CatalogView | null>(null);
const expanded = signal<string[]>([]);
let filterGeneration = 0;

// A search opens the branches holding a match, which can still be folded;
// clearing it folds everything, as the GPUI tree does.
function setQuery(next: string) {
  query.set(next);
  const generation = ++filterGeneration;
  if (next.trim() === "") {
    filtered.set(null);
    expanded.set([]);
    return;
  }
  void Armor.filterCatalog(next).then((result: CatalogView) => {
    if (generation !== filterGeneration) return;
    filtered.set(result);
    expanded.set(result.nations.flatMap((nation) => [
      `n:${nation.nation}`,
      ...nation.classes.map((group) => `c:${nation.nation}:${group.species}`),
    ]));
  }).catch((error: unknown) => toast.failed(errorText(error)));
}

function Chevron(props: { open: boolean }) {
  return <Icon name={props.open ? "chevron-down" : "chevron-right"} size={12} color={colors.mutedForeground}/>;
}

/** What a tree row stands for, by its item id. */
type RowInfo =
  | { kind: "nation"; flag?: string | null }
  | { kind: "class"; icon?: string | null }
  | { kind: "ship"; ship: CatalogShip };

type TreeModel = { items: TreeItem[]; rows: Map<string, RowInfo> };

/** The row the tree's keys act on: a press on a ship row sets it, as the tree's own press does on the other rows. */
const selectedRow = signal<string | null>(null);
let treeApi: TreeApi | undefined;

function treeModel(view: CatalogView | null): TreeModel {
  const rows = new Map<string, RowInfo>();
  const items = (view?.nations ?? []).map((nation): TreeItem => {
    const nationId = `n:${nation.nation}`;
    rows.set(nationId, { kind: "nation", flag: nation.flag });
    return {
      id: nationId,
      label: nation.label,
      children: nation.classes.map((group): TreeItem => {
        const classId = `c:${nation.nation}:${group.species}`;
        rows.set(classId, { kind: "class", icon: group.icon });
        return {
          id: classId,
          label: group.label,
          children: group.ships.map((ship): TreeItem => {
            const shipId = `s:${ship.paramIndex}`;
            rows.set(shipId, { kind: "ship", ship });
            return { id: shipId, label: `${ship.roman} ${ship.name}` };
          }),
        };
      }),
    };
  });
  return { items, rows };
}

function openShip(ship: CatalogShip) {
  call(Armor.openShip(ship.paramIndex, ship.name));
}

/** A guide per level, so a row reads as belonging to the branch above it. */
function IndentGuides(props: { depth: number }) {
  return (
    <Each of={Array.from({ length: props.depth }, (_, level) => level)}>
      {() => <View style={{ width: 6, marginLeft: 6, alignSelf: "stretch", borderLeftWidth: 1, borderLeftColor: colors.border }}/>}
    </Each>
  );
}

/** The ship of the active pane, whose label is bold. */
function activeShip(): string | null {
  const view = armor();
  return view?.panes.find((pane) => pane.id === view.activePane)?.paramIndex ?? null;
}

function ShipRow(props: { entry: TreeEntry; ship: CatalogShip }) {
  const shown = () => activeShip() === props.ship.paramIndex;
  // The row takes the press from the tree, so it selects itself and gives the tree the keys.
  const press = () => {
    selectedRow.set(props.entry.id);
    treeApi?.focus();
    openShip(props.ship);
  };
  return (
    <ContextMenu
      offset={2}
      margin={8}
      style={{ flex: 1, alignSelf: "stretch" }}
      trigger={
        <View
          testID={`armor-ship-${props.ship.paramIndex}`}
          role="button"
          accessibilityLabel={props.ship.label}
          focusable={false}
          onPress={press}
          activeStyle={{ opacity: 0.7 }}
          style={{ flex: 1, minHeight: 22, flexDirection: "row", alignItems: "center", gap: 4, paddingRight: 4 }}
        >
          <IndentGuides depth={props.entry.depth}/>
          <View style={{ width: 12 }}/>
          <Text style={{ fontSize: 12.5, color: colors.foreground, fontWeight: shown() ? "600" : "400" }}>{props.entry.label}</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 200, padding: 4, backgroundColor: colors.popover }}>
        <MenuRow label={t("ui.armor.compare_split")} onSelect={() => call(Armor.compare(props.ship.paramIndex, props.ship.name))}/>
        <MenuRow label={t("ui.armor.export_model")} onSelect={() => call(Armor.openExportDialog(props.ship.paramIndex, props.ship.name))}/>
      </Surface>
    </ContextMenu>
  );
}

function TreeRow(props: { entry: TreeEntry; info: RowInfo | undefined }) {
  const ship = () => props.info?.kind === "ship" ? props.info.ship : null;
  const flag = () => props.info?.kind === "nation" ? props.info.flag : null;
  const icon = () => props.info?.kind === "class" ? props.info.icon : null;
  return (
    <If when={ship()} else={
      <View style={{ flex: 1, minHeight: 22, flexDirection: "row", alignItems: "center", gap: 4, paddingRight: 4 }}>
        <IndentGuides depth={props.entry.depth}/>
        <Chevron open={expanded().includes(props.entry.id)}/>
        <If when={flag()}>
          {(src) => <Image src={src()} style={{ width: 23, height: 16 }}/>}
        </If>
        <If when={icon()}>
          {(src) => <Image src={src()} style={{ width: 16, height: 16, tintColor: colors.foreground, transform: [{ rotate: "90deg" }] }}/>}
        </If>
        <Text style={{ fontSize: 12.5, fontWeight: props.info?.kind === "nation" ? "600" : "400", color: colors.foreground }}>{props.entry.label}</Text>
      </View>
    }>
      {(current) => <ShipRow entry={props.entry} ship={current()}/>}
    </If>
  );
}

function ShipTree() {
  const searching = () => filtered() !== null;
  const model = computed(() => treeModel(filtered() ?? catalog()));
  // A ship opened anywhere (a press, the palette, a replay, another pane) becomes the selected row.
  let followed: string | null = null;
  effect(() => {
    const ship = activeShip();
    if (ship === followed) return;
    followed = ship;
    if (ship) untracked(() => selectedRow.set(`s:${ship}`));
  });
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={model().items.length > 0} else={
        <Text style={{ padding: 8, fontSize: 12, color: colors.mutedForeground }}>
          {searching() ? "" : armor()?.status === "notConfigured" ? t("ui.armor.load_settings") : armor()?.status === "failed" ? t("ui.armor.catalog_failed", { reason: armor()?.statusReason ?? "" }) : t("ui.armor.loading_catalog")}
        </Text>
      }>
        <Tree
          testID="armor-ships-tree"
          ref={(api: TreeApi) => { treeApi = api; }}
          accessibilityLabel={t("ui.armor.ships")}
          items={model().items}
          expanded={expanded()}
          onExpandedChange={({ expanded: next }) => expanded.set(next)}
          selected={selectedRow()}
          onSelectedChange={({ selected }) => selectedRow.set(selected)}
          onAction={({ id }) => {
            const info = model().rows.get(id);
            if (info?.kind === "ship") openShip(info.ship);
          }}
          style={{ flex: 1, minHeight: 0 }}
          listStyle={{ paddingHorizontal: 4, paddingBottom: 8 }}
          itemClass="rounded-[4px] border border-transparent hover:bg-accent selected:bg-accent focus:border-ring"
          renderItem={(entry: TreeEntry) => <TreeRow entry={entry} info={model().rows.get(entry.id)}/>}
        />
      </If>
    </View>
  );
}

function CommonSettings() {
  const several = () => (armor()?.panes.length ?? 0) > 1;
  return (
    <Popover
      placement="bottom"
      align="end"
      offset={4}
      trigger={
        <View
          testID="armor-sidebar-common"
          role="button"
          accessibilityLabel={t("ui.armor.common_settings")}
          tooltip={tip(t("ui.armor.common_settings"))}
          hoverStyle={{ backgroundColor: colors.accent }}
          style={{ width: 26, height: 24, alignItems: "center", justifyContent: "center", borderRadius: controlRadius() }}
        >
          <Icon name="settings-2" size={14} color={colors.foreground}/>
        </View>
      }
    >
      <Surface style={{ padding: 10, gap: 8, backgroundColor: colors.popover }}>
        <CheckRow
          id="armor-pane-mirror-cameras"
          label={t("ui.armor.mirror_cameras")}
          tooltip={t("ui.armor.mirror_cameras_tooltip")}
          checked={armor()?.mirrorCameras ?? false}
          disabled={!several()}
          onChange={(on) => call(Armor.setMirrorCameras(on))}
        />
        <CheckRow
          id="armor-pane-stack"
          label={t("ui.armor.stack_panes")}
          tooltip={t("ui.armor.stack_panes_tooltip")}
          checked={armor()?.stackPanes ?? false}
          disabled={!several()}
          onChange={(on) => call(Armor.setStackPanes(on))}
        />
        <CheckRow
          id="armor-pane-sync-options"
          label={t("ui.armor.sync_options")}
          tooltip={t("ui.armor.sync_tooltip")}
          checked={armor()?.syncOptions ?? false}
          disabled={!several()}
          onChange={(on) => call(Armor.setSyncOptions(on))}
        />
      </Surface>
    </Popover>
  );
}

/** The ships list: its header, its search and the nation, class and ship tree. */
export function ShipsSidebar() {
  // A refreshed catalog (a new game build) is filtered again under the same query.
  effect(() => {
    if (!catalog()) return;
    const current = untracked(() => query());
    if (current.trim() !== "") setQuery(current);
  });
  return (
    <View testID="armor-sidebar" style={{ flex: 1, minWidth: 0, backgroundColor: colors.surface }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 6, paddingVertical: 6 }}>
        <IconButton id="armor-ships-collapse" icon="panel-left-close" label={t("ui.armor.collapse_ships")} onPress={() => sidebarCollapsed.set(true)}/>
        <Text style={{ flex: 1, fontSize: 13, fontWeight: "600", color: colors.foreground }}>{t("ui.armor.ships")}</Text>
        <ToolButton
          id="armor-sidebar-compare"
          label={t("ui.armor.compare")}
          leading="layout-dashboard"
          tooltip={t("ui.armor.compare_tooltip")}
          onPress={() => call(Armor.compare(null, null))}
        />
        <CommonSettings/>
      </View>
      <View style={{ paddingHorizontal: 6, paddingBottom: 6 }}>
        <Input
          testID="armor-sidebar-search"
          accessibilityLabel={t("ui.armor.search_ships")}
          placeholder={t("ui.armor.search_ships")}
          value={query()}
          onChange={({ value }) => setQuery(value)}
          style={inputStyle}
        />
      </View>
      <ShipTree/>
    </View>
  );
}

/** The edge rail the ships list folds to. */
export function ShipsEdge() {
  return (
    <View style={{ width: 26, alignItems: "center", paddingTop: 6, borderRightWidth: 1, borderRightColor: colors.border, backgroundColor: colors.surface }}>
      <IconButton id="armor-ships-expand" icon="panel-left-open" label={t("ui.armor.expand_ships")} onPress={() => sidebarCollapsed.set(false)}/>
    </View>
  );
}
