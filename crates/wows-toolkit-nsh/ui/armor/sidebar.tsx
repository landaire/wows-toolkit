import { effect, Image, Scroll, signal, Text, untracked, View } from "nsh";
import { ContextMenu, Input, Popover } from "nsh/base";
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

function toggle(key: string) {
  const open = expanded();
  expanded.set(open.includes(key) ? open.filter((k) => k !== key) : [...open, key]);
}

function Chevron(props: { open: boolean }) {
  return <Icon name={props.open ? "chevron-down" : "chevron-right"} size={12} color={colors.mutedForeground}/>;
}

function ShipRow(props: { ship: CatalogShip }) {
  const open = () => call(Armor.openShip(props.ship.paramIndex, props.ship.name));
  const selected = () => {
    const view = armor();
    return view?.panes.find((pane) => pane.id === view.activePane)?.paramIndex === props.ship.paramIndex;
  };
  return (
    <ContextMenu
      offset={2}
      margin={8}
      trigger={
        <View
          testID={`armor-ship-${props.ship.paramIndex}`}
          role="button"
          accessibilityLabel={props.ship.label}
          accessibilityState={{ selected: selected() }}
          onPress={open}
          hoverStyle={{ backgroundColor: colors.accent }}
          activeStyle={{ opacity: 0.7 }}
          style={{ minHeight: 22, paddingLeft: 36, paddingRight: 6, justifyContent: "center", borderRadius: 4, backgroundColor: selected() ? colors.accent : undefined }}
        >
          <Text style={{ fontSize: 12.5, color: colors.foreground, fontWeight: selected() ? "600" : "400" }}>{`${props.ship.roman} ${props.ship.name}`}</Text>
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

function ShipTree() {
  const searching = () => filtered() !== null;
  const tree = () => filtered() ?? catalog();
  const isOpen = (key: string) => expanded().includes(key);
  return (
    <Scroll accessibilityLabel={t("ui.armor.ships")} style={{ flex: 1 }}>
      <View style={{ paddingHorizontal: 4, paddingBottom: 8 }}>
        <Each of={tree()?.nations ?? []} key={(nation) => nation.nation}>
          {(nation) => {
            const nationKey = () => `n:${nation().nation}`;
            return (
              <View>
                <View
                  testID={`armor-nation-${nation().nation}`}
                  role="button"
                  accessibilityLabel={nation().label}
                  accessibilityState={{ expanded: isOpen(nationKey()) }}
                  onPress={() => toggle(nationKey())}
                  hoverStyle={{ backgroundColor: colors.accent }}
                  style={{ minHeight: 24, flexDirection: "row", alignItems: "center", gap: 4, paddingHorizontal: 4, borderRadius: 4 }}
                >
                  <Chevron open={isOpen(nationKey())}/>
                  <If when={nation().flag}>
                    <Image source={{ uri: nation().flag! }} style={{ width: 23, height: 16 }}/>
                  </If>
                  <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.foreground }}>{nation().label}</Text>
                </View>
                <If when={isOpen(nationKey())}>
                  <View style={{ marginLeft: 8, borderLeftWidth: 1, borderLeftColor: colors.border }}>
                    <Each of={nation().classes} key={(group) => group.species}>
                      {(group) => {
                        const classKey = () => `c:${nation().nation}:${group().species}`;
                        return (
                          <View>
                            <View
                              role="button"
                              accessibilityLabel={`${nation().label} ${group().label}`}
                              accessibilityState={{ expanded: isOpen(classKey()) }}
                              onPress={() => toggle(classKey())}
                              hoverStyle={{ backgroundColor: colors.accent }}
                              style={{ minHeight: 22, flexDirection: "row", alignItems: "center", gap: 4, paddingLeft: 8, borderRadius: 4 }}
                            >
                              <Chevron open={isOpen(classKey())}/>
                              <If when={group().icon}>
                                <Image source={{ uri: group().icon! }} style={{ width: 16, height: 16, tintColor: colors.foreground, transform: [{ rotate: "90deg" }] }}/>
                              </If>
                              <Text style={{ fontSize: 12.5, color: colors.foreground }}>{group().label}</Text>
                            </View>
                            <If when={isOpen(classKey())}>
                              <Each of={group().ships} key={(ship) => ship.paramIndex}>
                                {(ship) => <ShipRow ship={ship()}/>}
                              </Each>
                            </If>
                          </View>
                        );
                      }}
                    </Each>
                  </View>
                </If>
              </View>
            );
          }}
        </Each>
        <If when={!tree() || tree()!.nations.length === 0}>
          <Text style={{ padding: 8, fontSize: 12, color: colors.mutedForeground }}>
            {searching() ? "" : armor()?.status === "notConfigured" ? t("ui.armor.load_settings") : armor()?.status === "failed" ? t("ui.armor.catalog_failed", { reason: armor()?.statusReason ?? "" }) : t("ui.armor.loading_catalog")}
          </Text>
        </If>
      </View>
    </Scroll>
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
          <Text style={{ fontSize: 12, fontWeight: "600", color: colors.foreground }}>=</Text>
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
