import type { MouseEvent } from "nsh";
import { signal, Text, View } from "nsh";
import { Menu, MenuItem, MenuSeparator, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { Button, MenuRow, Surface } from "../components";
import { Frame } from "../frame";
import * as toast from "../shell/toast";
import { t } from "../i18n";
import { colors, controlRadius } from "../theme";
import { armor, call, errorText, type ExportResult, hover, type HoverView, type PaneView } from "./state";
import { IconButton, Swatch } from "./widgets";

/** Reports an export the way the other front ends do; a cancelled save says nothing. */
export async function runExport(command: () => Promise<ExportResult | null>) {
  try {
    const result = await command();
    if (!result) return;
    toast.ok(result.size
      ? t("ui.armor.export.exported_with_size", { ship: result.ship, size: result.size })
      : t("ui.armor.export.exported", { ship: result.ship }));
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/** Where the viewport's context menu was asked for, in the pane. */
const menuAt = signal<{ pane: number; x: number; y: number; plate: HoverView["plate"] } | null>(null);

function CheckItem(props: { label: string; checked: boolean; disabled?: boolean; onSelect: () => void }) {
  return (
    <MenuItem
      checked={props.checked}
      disabled={props.disabled}
      onSelect={props.onSelect}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ height: 28, paddingHorizontal: 10, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 8, opacity: props.disabled ? 0.45 : 1 }}
    >
      <Text style={{ width: 12, fontSize: 12, color: colors.popoverForeground }}>{props.checked ? "x" : ""}</Text>
      <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>{props.label}</Text>
    </MenuItem>
  );
}

function separator() {
  return <MenuSeparator style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>;
}

/** The viewport's context menu: the plate under the pointer, then the quick toggles. */
function ViewportMenu(props: { pane: PaneView }) {
  const at = () => menuAt();
  const open = () => at()?.pane === props.pane.id;
  const active = () => armor()?.active;
  const loaded = () => active()?.hasArmor ?? false;
  return (
    <If when={open()}>
      <View style={{ position: "absolute", left: at()!.x, top: at()!.y, width: 1, height: 1 }}>
        <Menu
          open={open()}
          offset={2}
          margin={8}
          align="start"
          onOpenChange={({ open: next }) => { if (!next) menuAt.set(null); }}
          trigger={<View testID="armor-viewport-menu-anchor" style={{ width: 1, height: 1 }}/>}
        >
          <Surface style={{ minWidth: 230, padding: 4, backgroundColor: colors.popover }}>
            <If when={at()?.plate}>
              {(plate) => (
                <View>
                  <MenuRow
                    label={`${plate().hidden ? "Show" : "Hide"} ${plate().mm} mm ${plate().materialLabel}`}
                    onSelect={() => call(Armor.togglePlate(plate().zone, plate().material, plate().tenths))}
                  />
                  <If when={props.pane.hiddenPlates > 0}>
                    <MenuRow label={`Show all hidden plates (${props.pane.hiddenPlates})`} onSelect={() => call(Armor.showAllPlates())}/>
                  </If>
                  {separator()}
                  <MenuRow label={`Disable ${plate().materialLabel}`} onSelect={() => call(Armor.disableMaterial(plate().zone, plate().material))}/>
                  {separator()}
                </View>
              )}
            </If>
            <CheckItem label={t("ui.armor.show_hidden")} checked={active()?.showHidden ?? false} disabled={!loaded()} onSelect={() => call(Armor.setShowHidden(!active()!.showHidden))}/>
            <CheckItem label={t("ui.armor.gaps")} checked={active()?.gaps.shown ?? false} disabled={!loaded()} onSelect={() => call(Armor.setShowGaps(!active()!.gaps.shown))}/>
            <CheckItem label={t("ui.armor.splash_mode")} checked={active()?.splash.shown ?? false} disabled={!active()?.splash.hasData} onSelect={() => call(Armor.setSplashMode(!active()!.splash.shown))}/>
            <CheckItem label={t("ui.armor.splash_boxes")} checked={active()?.splash.boxesShown ?? false} disabled={!active()?.splash.hasData} onSelect={() => call(Armor.setSplashBoxes(!active()!.splash.boxesShown))}/>
            <CheckItem label={t("ui.armor.trajectory")} checked={active()?.trajectory.shown ?? false} disabled={!loaded()} onSelect={() => call(Armor.setTrajectoryMode(!active()!.trajectory.shown))}/>
            <CheckItem label={t("ui.armor.pen_check")} checked={armor()?.penCheckOpen ?? false} onSelect={() => call(Armor.setPenCheckOpen(!armor()?.penCheckOpen))}/>
            <CheckItem label={t("ui.armor.options_panel")} checked={armor()?.optionsOpen ?? false} onSelect={() => call(Armor.setOptionsOpen(!armor()?.optionsOpen))}/>
            {separator()}
            <CheckItem label={t("ui.armor.export_model")} checked={false} disabled={!loaded()} onSelect={() => call(Armor.openExportConfirm())}/>
          </Surface>
        </Menu>
      </View>
    </If>
  );
}

/** The plate or camera ring under the pointer, beside it. */
function HoverTip(props: { pane: PaneView }) {
  const here = () => {
    const current = hover();
    return current && current.paneId === props.pane.id && (current.plate || current.ring?.length) ? current : null;
  };
  return (
    <If when={here()}>
      {(current) => (
        <View style={{ position: "absolute", left: current().x + 16, top: current().y + 16, maxWidth: 280, padding: 8, gap: 3, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}>
          <If when={current().ring?.length} else={
            <If when={current().plate}>
              {(plate) => (
                <View style={{ gap: 3 }}>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                    <Swatch color={plate().color}/>
                    <Text style={{ fontSize: 13, fontWeight: "700", color: colors.popoverForeground }}>{`${plate().mm} mm`}</Text>
                  </View>
                  <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{`${plate().zoneLabel} / ${plate().materialLabel}`}</Text>
                  <If when={plate().layers.length > 1}>
                    <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{`${plate().layers.length} layers for this part:`}</Text>
                    <Each of={plate().layers} key="index">
                      {(layer) => (
                        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                          <Swatch color={layer().color}/>
                          <Text style={{ fontSize: 12, fontWeight: layer().current ? "700" : "400", color: colors.popoverForeground }}>{`${layer().mm} mm`}</Text>
                        </View>
                      )}
                    </Each>
                  </If>
                </View>
              )}
            </If>
          }>
            <Each of={current().ring ?? []} key="index">
              {(line) => <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{line()}</Text>}
            </Each>
          </If>
        </View>
      )}
    </If>
  );
}

/** The confirmation an Export from the rail or the context menu asks for. */
function ExportConfirm(props: { pane: PaneView }) {
  return (
    <View style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center", backgroundColor: "rgba(0,0,0,0.45)" }}>
      <Surface style={{ width: 360, padding: 16, gap: 10, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 15, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.armor.export_model")}</Text>
        <Text style={{ fontSize: 12.5, lineHeight: 18, color: colors.popoverForeground }}>
          {`Export ${props.pane.ship ?? ""} to a glTF Binary (.glb) file. ${t("ui.armor.export_disclaimer")}`}
        </Text>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <View testID="armor-export-confirm-cancel"><Button label={t("ui.buttons.cancel")} onPress={() => call(Armor.cancelExportConfirm())}/></View>
          <View testID="armor-export-confirm-export"><Button label={t("ui.armor.export_button")} variant="primary" onPress={() => void runExport(() => Armor.confirmExport())}/></View>
        </View>
      </Surface>
    </View>
  );
}

function Overlay(props: { text: string; tone?: "error" }) {
  return (
    <View style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center" }}>
      <Text style={{ fontSize: 13, color: props.tone === "error" ? colors.destructive : colors.mutedForeground }}>{props.text}</Text>
    </View>
  );
}

function Pane(props: { pane: PaneView; several: boolean }) {
  const view = () => armor();
  const pane = () => props.pane;
  const state = (): { text: string; tone?: "error" } | null => {
    const current = view();
    if (!current) return null;
    if (current.gpu === "initializing") return { text: t("ui.armor.initializing") };
    if (current.gpu === "failed") return { text: t("ui.armor.init_failed", { reason: current.gpuError ?? "" }), tone: "error" };
    if (pane().loading) return { text: t("ui.armor.loading_named_ship", { ship: pane().loading! }) };
    if (pane().loadError) return { text: pane().loadError!, tone: "error" };
    if (!pane().ship) return { text: t("ui.armor.select_ship") };
    return null;
  };
  return (
    <View
      testID={`armor-viewport-pane-${pane().number}`}
      style={{
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        borderWidth: props.several ? 2 : 0,
        borderColor: pane().active ? colors.primary : "transparent",
        backgroundColor: "#1b1d22",
      }}
    >
      <Frame
        surface={pane().surface}
        accessibilityLabel={`Armor viewport ${pane().number}`}
        onSecondaryPress={({ x, y }) => {
          const current = hover();
          menuAt.set({ pane: pane().id, x, y, plate: current && current.paneId === pane().id ? current.plate ?? null : null });
        }}
      />
      <View style={{ position: "absolute", top: 6, left: 8, gap: 2 }}>
        <If when={pane().readout}>
          <Text style={{ fontSize: 12.5, fontWeight: "600", color: "#ffffff" }}>{pane().readout}</Text>
        </If>
        <Each of={pane().shotLog} key="index">
          {(shot) => <Text style={{ fontSize: 11.5, color: "#e6e6e6" }}>{`${shot().at} ${shot().outcome}`}</Text>}
        </Each>
      </View>
      <If when={state()}>
        {(current) => <Overlay text={current().text} tone={current().tone}/>}
      </If>
      <HoverTip pane={pane()}/>
      <ViewportMenu pane={pane()}/>
      <If when={pane().exportConfirm}>
        <ExportConfirm pane={pane()}/>
      </If>
      <View style={{ position: "absolute", top: 4, right: 4 }}>
        <IconButton id={`armor-viewport-pane-close-${pane().number}`} glyph="x" label={t("ui.armor.close_pane")} onPress={() => call(Armor.closePane(pane().id))}/>
      </View>
    </View>
  );
}

/** The viewports side by side (or stacked), the active one framed when there are several. */
export function ViewportDock() {
  const panes = () => armor()?.panes ?? [];
  const several = () => panes().length > 1;
  return (
    <View testID="armor-dock" style={{ flex: 1, minWidth: 0, minHeight: 0 }}>
      <ResizablePanelGroup
        axis={armor()?.stackPanes ? "vertical" : "horizontal"}
        handleClass="bg-border group-active:bg-primary"
        handleAccessibilityLabel={t("ui.armor.compare")}
      >
        <Each of={panes()} key={(pane) => pane.id}>
          {(pane) => (
            <ResizablePanel minSize={120}>
              <Pane pane={pane()} several={several()}/>
            </ResizablePanel>
          )}
        </Each>
      </ResizablePanelGroup>
    </View>
  );
}

const LEGEND_WIDTH = 150;

/** A legend drag going on: where the pointer and the legend started, and where the legend is now. */
const legendDrag = signal<{ fromX: number; fromY: number; startX: number; startY: number; x: number; y: number } | null>(null);

function finishLegendDrag() {
  const current = legendDrag();
  if (!current) return;
  legendDrag.set(null);
  call(Armor.moveLegend(current.x, current.y));
}

/** Follows the pointer over the whole viewer while the legend is dragged, so a fast drag does not drop it. */
export function followLegendDrag(event: MouseEvent) {
  const current = legendDrag();
  if (!current) return;
  if (event.pressedButton !== "left") {
    finishLegendDrag();
    return;
  }
  legendDrag.set({ ...current, x: Math.max(0, current.startX + event.pagePosition.x - current.fromX), y: Math.max(0, current.startY + event.pagePosition.y - current.fromY) });
}

/** The floating thickness legend: dragged by its title, folded or hidden from its header. */
export function Legend() {
  const legend = () => armor()?.legend;
  const position = () => legendDrag() ?? legend() ?? { x: 10, y: 100 };
  return (
    <If when={armor()?.shipLoaded && legend()?.visible}>
      <View
        testID="armor-legend"
        style={{ position: "absolute", left: position().x, top: position().y, minWidth: LEGEND_WIDTH, padding: 6, gap: 4, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover, opacity: 0.95 }}
      >
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
          <IconButton
            id="armor-legend-collapse"
            glyph={legend()?.collapsed ? ">" : "v"}
            label={t("ui.armor.legend_collapse")}
            onPress={() => call(Armor.setLegendCollapsed(!legend()!.collapsed))}
          />
          <View
            testID="armor-legend-drag-handle"
            accessibilityLabel={t("ui.armor.armor_thickness_title")}
            style={{ flex: 1, cursor: "grab" }}
            onPressDown={(event) => {
              const at = position();
              legendDrag.set({ fromX: event.pagePosition.x, fromY: event.pagePosition.y, startX: at.x, startY: at.y, x: at.x, y: at.y });
            }}
            onPressUp={finishLegendDrag}
          >
            <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.armor.armor_thickness_title")}</Text>
          </View>
          <IconButton id="armor-legend-close" glyph="x" label={t("ui.armor.legend_hide")} onPress={() => call(Armor.setLegendVisible(false))}/>
        </View>
        <If when={!legend()?.collapsed}>
          <Each of={legend()?.entries ?? []} key="index">
            {(entry) => (
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                <Swatch color={entry().color} width={16} height={12}/>
                <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{entry().label}</Text>
              </View>
            )}
          </Each>
        </If>
      </View>
    </If>
  );
}
