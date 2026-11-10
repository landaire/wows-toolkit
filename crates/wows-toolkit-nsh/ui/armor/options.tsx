import type { JSX } from "nsh";
import { Scroll, signal, Text, View } from "nsh";
import { Select } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { t } from "../i18n";
import { colors, controlRadius } from "../theme";
import { PenSearch } from "./analysis";
import { armor, type CamoGroup, type CamoView, call, type DisplayView, type HullView, type PartView, type ZoneView } from "./state";
import { CheckRow, ChoiceRow, ColorRow, Heading, Hint, IconButton, Rule, SliderRow, Swatch, ToolButton } from "./widgets";

type Section = "Visibility" | "Hull" | "Display";

/** Which rail sections are unfolded; a folded section's body is not built. */
const openSections = signal<Section[]>(["Display"]);
/** The unfolded rows of the visibility and hull trees, keyed by pane and ship so a new ship starts folded. */
const openRows = signal<string[]>([]);

function toggleRow(key: string) {
  const open = openRows();
  openRows.set(open.includes(key) ? open.filter((k) => k !== key) : [...open, key]);
}

function rowKey(...parts: (string | number)[]): string {
  const view = armor();
  const pane = view?.panes.find((candidate) => candidate.id === view.activePane);
  return [pane?.id ?? 0, pane?.shipGeneration ?? 0, ...parts].join("/");
}

function Chevron(props: { rowKey: string; label: string }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      accessibilityState={{ expanded: openRows().includes(props.rowKey) }}
      onPress={() => toggleRow(props.rowKey)}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ width: 16, height: 18, alignItems: "center", justifyContent: "center", borderRadius: 3 }}
    >
      <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{openRows().includes(props.rowKey) ? "v" : ">"}</Text>
    </View>
  );
}

function Tools() {
  const view = () => armor();
  const active = () => view()?.active;
  const loaded = () => active()?.hasArmor ?? false;
  const splashData = () => active()?.splash.hasData ?? false;
  const gaps = () => active()?.gaps;
  const trajectory = () => active()?.trajectory;
  const shipCount = () => view()?.pen.ships.length ?? 0;
  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        <ToolButton
          id="armor-show-hidden-button"
          label={t("ui.armor.show_hidden")}
          tooltip={t("ui.armor.show_hidden_tooltip")}
          on={active()?.showHidden}
          disabled={!loaded()}
          onPress={() => call(Armor.setShowHidden(!active()!.showHidden))}
        />
        <ToolButton
          id="armor-show-gaps-button"
          label={gaps()?.shown ? `${gaps()!.count > 0 ? "! " : ""}${t("ui.armor.gaps")} (${gaps()!.count})` : t("ui.armor.gaps")}
          tooltip={t("ui.armor.gaps_tooltip")}
          on={gaps()?.shown}
          disabled={!loaded()}
          onPress={() => call(Armor.setShowGaps(!gaps()!.shown))}
        />
        <ToolButton
          id="armor-splash-button"
          label={active()?.splash.shown ? `${t("ui.armor.splash_mode")} (${active()!.splash.zonesReached})` : t("ui.armor.splash_mode")}
          tooltip={splashData() ? t("ui.armor.splash_tooltip_mode") : t("ui.armor.splash_no_data")}
          on={active()?.splash.shown}
          disabled={!splashData()}
          onPress={() => call(Armor.setSplashMode(!active()!.splash.shown))}
        />
        <ToolButton
          id="armor-splash-boxes-button"
          label={t("ui.armor.splash_boxes")}
          tooltip={splashData() ? t("ui.armor.splash_tooltip") : t("ui.armor.splash_no_data")}
          on={active()?.splash.boxesShown}
          disabled={!splashData()}
          onPress={() => call(Armor.setSplashBoxes(!active()!.splash.boxesShown))}
        />
      </View>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        <ToolButton
          id="armor-trajectory-button"
          label={trajectory()?.shown ? `${t("ui.armor.trajectory_on")} (${trajectory()!.count})` : t("ui.armor.trajectory")}
          tooltip={t("ui.armor.trajectory_tooltip")}
          on={trajectory()?.shown}
          disabled={!loaded()}
          onPress={() => call(Armor.setTrajectoryMode(!trajectory()!.shown))}
        />
        <ToolButton
          id="armor-analysis-toggle"
          label={shipCount() > 0 ? `${t("ui.armor.pen_check")} (${shipCount()})` : t("ui.armor.pen_check")}
          tooltip={t("ui.armor.pen_check_tooltip")}
          on={view()?.penCheckOpen}
          onPress={() => call(Armor.setPenCheckOpen(!view()?.penCheckOpen))}
        />
        <ToolButton
          id="armor-export-trigger"
          label={t("ui.replay.section_export")}
          tooltip={t("ui.armor.export_tooltip")}
          disabled={!loaded()}
          onPress={() => call(Armor.openExportConfirm())}
        />
      </View>
    </View>
  );
}

function TrajectoryPanel() {
  const view = () => armor();
  const trajectory = () => view()!.active.trajectory;
  const pen = () => view()!.pen;
  const firing = () => pen().ships.find((ship) => ship.paramIndex === pen().firingShip);
  const shells = () => firing()?.shells ?? [];
  const firingShell = () => shells().find((shell) => shell.index === pen().firingShell);
  const selectTrigger = (id: string, label: string) => (
    <View
      testID={id}
      role="button"
      accessibilityLabel={label}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background }}
    >
      <Text style={{ fontSize: 12.5, color: colors.foreground }}>{label}</Text>
    </View>
  );
  return (
    <View testID="armor-trajectory-panel" style={{ gap: 6, padding: 8, borderWidth: 1, borderColor: colors.primary, borderRadius: controlRadius() }}>
      <Heading text={t("ui.armor.trajectory_on")}/>
      <PenSearch id="armor-trajectory-search"/>
      <If when={pen().ships.length === 0}>
        <Hint text={t("ui.armor.pen.add_a_ship")}/>
      </If>
      <If when={pen().ships.length > 0}>
        <Select
          accessibilityLabel={t("ui.armor.trajectory_ship_tooltip")}
          items={pen().ships.map((ship) => ({ value: ship.paramIndex, title: ship.paramIndex === pen().firingShip ? `${ship.label} (selected)` : ship.label }))}
          value={pen().firingShip ?? null}
          onChange={({ value }) => { if (value) call(Armor.setFiringShip(value)); }}
          popupStyle={{ padding: 4, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}
          itemStyle={{ minHeight: 26, paddingHorizontal: 8, borderRadius: 4, fontSize: 12.5, color: colors.popoverForeground }}
          itemClass="hover:bg-accent focus:bg-accent"
          trigger={selectTrigger("armor-trajectory-ship", firing() ? `${t("ui.armor.trajectory_ship")} ${firing()!.label}` : t("ui.armor.trajectory_select_ship"))}
        />
        <Select
          accessibilityLabel={t("ui.armor.trajectory_shell_tooltip")}
          items={shells().map((shell) => ({ value: String(shell.index), title: shell.index === pen().firingShell ? `${shell.label} (selected)` : shell.label }))}
          value={pen().firingShell != null ? String(pen().firingShell) : null}
          disabled={shells().length === 0}
          onChange={({ value }) => { if (value) call(Armor.setFiringShell(Number(value))); }}
          popupStyle={{ padding: 4, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}
          itemStyle={{ minHeight: 26, paddingHorizontal: 8, borderRadius: 4, fontSize: 12.5, color: colors.popoverForeground }}
          itemClass="hover:bg-accent focus:bg-accent"
          trigger={selectTrigger("armor-trajectory-shell", firingShell() ? `${t("ui.armor.trajectory_shell")} ${firingShell()!.label}` : t("ui.armor.trajectory_select_shell"))}
        />
        <View style={{ gap: 2 }}>
          <Each of={pen().ships} key={(ship) => ship.paramIndex}>
            {(ship, index) => (
              <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                <Text style={{ flex: 1, fontSize: 12, color: colors.cardForeground }}>{ship().label}</Text>
                <IconButton id={`armor-trajectory-remove-${index()}`} glyph="x" label="Remove ship" onPress={() => call(Armor.removePenShip(index()))}/>
              </View>
            )}
          </Each>
        </View>
      </If>
      <SliderRow
        id="armor-trajectory-range"
        label={t("ui.armor.trajectory_range")}
        value={trajectory().castRange}
        min={0}
        max={30}
        step={0.5}
        digits={1}
        disabled={!trajectory().shown}
        onChange={(km) => call(Armor.setCastRange(km))}
      />
      <CheckRow
        id="armor-trajectory-ricochet"
        label={t("ui.armor.continue_ricochet")}
        checked={trajectory().continueOnRicochet}
        onChange={(on) => call(Armor.setContinueOnRicochet(on))}
      />
      <View style={{ alignItems: "flex-start" }}>
        <ToolButton id="armor-trajectory-clear" label={t("ui.armor.trajectory_clear")} disabled={trajectory().count === 0} onPress={() => call(Armor.clearArcs())}/>
      </View>
      <Hint text={t("ui.armor.trajectory_tooltip")}/>
    </View>
  );
}

function hoverable(enter: () => Promise<unknown>) {
  return {
    onMouseEnter: () => call(enter()),
    onMouseLeave: () => call(Armor.clearTreeHover()),
  };
}

function PartRow(props: { zone: ZoneView; part: PartView }) {
  const key = () => rowKey("part", props.zone.name, props.part.name);
  const open = () => !props.part.single && openRows().includes(key());
  return (
    <View>
      <View {...hoverable(() => Armor.hoverTreePart(props.zone.name, props.part.name))} style={{ flexDirection: "row", alignItems: "center", gap: 2, paddingLeft: 18 }}>
        <If when={!props.part.single} else={<View style={{ width: 16 }}/>}>
          <Chevron rowKey={key()} label={props.part.label}/>
        </If>
        <CheckRow
          label={props.part.label}
          state={props.part.state}
          onChange={(checked) => call(Armor.setPart(props.zone.name, props.part.name, checked))}
        />
      </View>
      <If when={open()}>
        <Each of={props.part.plates} key={(plate) => plate.tenths}>
          {(plate) => (
            <View {...hoverable(() => Armor.hoverTreePlate(props.zone.name, props.part.name, plate().tenths))} style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingLeft: 54 }}>
              <Swatch color={plate().color}/>
              <CheckRow
                label={plate().label}
                checked={plate().checked}
                onChange={() => call(Armor.togglePlate(props.zone.name, props.part.name, plate().tenths))}
              />
            </View>
          )}
        </Each>
      </If>
    </View>
  );
}

function ZoneRow(props: { zone: ZoneView }) {
  const key = () => rowKey("zone", props.zone.name);
  return (
    <View>
      <View {...hoverable(() => Armor.hoverTreeZone(props.zone.name))} style={{ flexDirection: "row", alignItems: "center", gap: 2 }}>
        <Chevron rowKey={key()} label={props.zone.label}/>
        <CheckRow
          label={props.zone.label}
          labelHidden
          state={props.zone.state}
          onChange={(checked) => call(Armor.setZone(props.zone.name, checked, false))}
        />
        <View
          role="button"
          accessibilityLabel={props.zone.label}
          onPress={(event) => {
            if (event.modifiers.control || event.modifiers.platform) call(Armor.setZone(props.zone.name, true, true));
            else call(Armor.setZone(props.zone.name, props.zone.state !== "on", false));
          }}
          style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 1 }}
        >
          <Text style={{ fontSize: 12.5, color: colors.cardForeground }}>{props.zone.label}</Text>
          <Text style={{ fontSize: 11, color: colors.textFaint }}>{t("ui.armor.ctrl_click_solo")}</Text>
        </View>
      </View>
      <If when={openRows().includes(key())}>
        <Each of={props.zone.parts} key={(part) => part.name}>
          {(part) => <PartRow zone={props.zone} part={part()}/>}
        </Each>
      </If>
    </View>
  );
}

function VisibilityBody() {
  const active = () => armor()?.active;
  return (
    <If when={active()?.hasArmor} else={<Hint text={t("ui.armor.no_ship_loaded")}/>}>
      <View style={{ gap: 6 }}>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
          <ToolButton id="armor-vis-all" label={t("ui.armor.all_btn")} onPress={() => call(Armor.setAllParts(true))}/>
          <ToolButton id="armor-vis-none" label={t("ui.armor.none_btn")} onPress={() => call(Armor.setAllParts(false))}/>
          <If when={active()?.hasPlateOverrides}>
            <ToolButton id="armor-vis-reset-plates" label={t("ui.armor.reset_plates")} onPress={() => call(Armor.resetPlateOverrides())}/>
          </If>
        </View>
        <Scroll style={{ maxHeight: 360 }}>
          <View style={{ gap: 1 }}>
            <Each of={active()?.zones ?? []} key={(zone) => zone.name}>
              {(zone) => <ZoneRow zone={zone()}/>}
            </Each>
          </View>
        </Scroll>
      </View>
    </If>
  );
}

function CamoList(props: { schemes: CamoView[] }) {
  return (
    <Each of={props.schemes} key={(scheme) => scheme.id}>
      {(scheme) => (
        <ToolButton
          id={`armor-camo-${scheme().id}`}
          label={scheme().name}
          on={scheme().selected}
          style={{ alignItems: "flex-start" }}
          onPress={() => call(Armor.selectCamo(scheme().id))}
        />
      )}
    </Each>
  );
}

function CamoGroupRow(props: { group: CamoGroup }) {
  const key = () => rowKey("camo", props.group.heading);
  return (
    <View style={{ gap: 2 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 2 }}>
        <Chevron rowKey={key()} label={t(props.group.heading)}/>
        <Text style={{ fontSize: 12, color: colors.cardForeground }}>{t(props.group.heading)}</Text>
      </View>
      <If when={openRows().includes(key())}>
        <Scroll style={{ maxHeight: 240 }}>
          <View style={{ gap: 2, paddingLeft: 18 }}>
            <CamoList schemes={props.group.schemes}/>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function HullBody() {
  const hull = (): HullView | null | undefined => armor()?.active.hull;
  return (
    <If when={hull()} else={<Hint text={t("ui.armor.no_ship_loaded")}/>}>
      {(current) => (
        <View style={{ gap: 8 }}>
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 4 }}>
            <ToolButton id="armor-hull-vis-all" label={t("ui.armor.all_btn")} onPress={() => call(Armor.setHullAll(true))}/>
            <ToolButton id="armor-hull-vis-none" label={t("ui.armor.none_btn")} onPress={() => call(Armor.setHullAll(false))}/>
            <CheckRow id="armor-hull-vis-opaque" label={t("ui.armor.opaque")} checked={current().opaque} onChange={(on) => call(Armor.setHullOpaque(on))}/>
          </View>
          <If when={current().upgrades.length > 1}>
            <ChoiceRow
              label={t("ui.armor.upgrade")}
              choices={current().upgrades.map((upgrade) => ({ value: upgrade.key, label: upgrade.label, selected: upgrade.selected, id: `armor-hull-upgrade-${upgrade.key}` }))}
              onSelect={(key) => call(Armor.selectHullUpgrade(key))}
            />
          </If>
          <Each of={current().modules} key={(module) => module.component}>
            {(module) => (
              <ChoiceRow
                label={`${module().component}:`}
                choices={module().options.map((option) => ({ value: option.key, label: option.label, tooltip: option.key, selected: option.selected }))}
                onSelect={(name) => call(Armor.selectModule(module().component, name))}
              />
            )}
          </Each>
          <If when={current().camo}>
            {(camo) => (
              <View style={{ gap: 4 }}>
                <Heading text={t("ui.armor.camouflage")}/>
                <ToolButton
                  id="armor-camo-none"
                  label={t("ui.armor.camo_none")}
                  on={camo().selected == null}
                  style={{ alignItems: "flex-start" }}
                  onPress={() => call(Armor.selectCamo(null))}
                />
                <CamoList schemes={camo().own}/>
                <Each of={camo().groups} key={(group) => group.heading}>
                  {(group) => <CamoGroupRow group={group()}/>}
                </Each>
              </View>
            )}
          </If>
          <If when={current().lods.length > 1}>
            <ChoiceRow
              label={t("ui.armor.lod")}
              choices={current().lods.map((lod) => ({ value: lod.lod, label: lod.lod === 0 ? t("ui.armor.lod_highest") : String(lod.lod), selected: lod.selected, id: `armor-hull-lod-${lod.lod}` }))}
              onSelect={(lod) => call(Armor.selectLod(lod))}
            />
          </If>
          <Scroll style={{ maxHeight: 360 }}>
            <View style={{ gap: 1 }}>
              <Each of={current().groups} key={(group) => group.name}>
                {(group) => {
                  const key = () => rowKey("hull", group().name);
                  return (
                    <View>
                      <View style={{ flexDirection: "row", alignItems: "center", gap: 2 }}>
                        <Chevron rowKey={key()} label={group().name}/>
                        <CheckRow label={group().name} state={group().state} onChange={(on) => call(Armor.setHullGroup(group().name, on))}/>
                      </View>
                      <If when={openRows().includes(key())}>
                        <Each of={group().meshes} key={(mesh) => mesh.name}>
                          {(mesh) => (
                            <View style={{ paddingLeft: 36 }}>
                              <CheckRow label={mesh().name} checked={mesh().visible} onChange={(on) => call(Armor.setHullMesh(mesh().name, on))}/>
                            </View>
                          )}
                        </Each>
                      </If>
                    </View>
                  );
                }}
              </Each>
            </View>
          </Scroll>
        </View>
      )}
    </If>
  );
}

function RingsSection(props: { display: DisplayView }) {
  const rings = () => props.display.rings;
  const perspective = () => props.display.perspective;
  return (
    <View style={{ gap: 6 }}>
      <Heading text={t("ui.armor.camera_rings")}/>
      <If when={rings().available} else={<Hint text={t("ui.armor.no_ship_loaded")}/>}>
        <CheckRow id="armor-camera-rings" label={t("ui.armor.show_camera_rings")} checked={rings().shown} onChange={(on) => call(Armor.setRingsShown(on))}/>
        <ChoiceRow
          disabled={!rings().shown}
          choices={rings().modes.map((mode) => ({ value: mode.name, label: mode.name, selected: mode.selected, id: `armor-camera-mode-${mode.name}` }))}
          onSelect={(mode) => call(Armor.setRingsMode(mode))}
        />
        <SliderRow id="armor-camera-fov" label={t("ui.armor.camera_fov")} value={rings().fov} min={0} max={1} disabled={!rings().shown} onChange={(v) => call(Armor.setRingsFov(v))}/>
        <SliderRow id="armor-camera-height" label={t("ui.armor.camera_height")} value={rings().height} min={-1} max={1} disabled={!rings().shown} onChange={(v) => call(Armor.setRingsHeight(v))}/>
        <CheckRow id="armor-camera-zoom-path" label={t("ui.armor.show_camera_zoom_path")} checked={rings().zoomPath} disabled={!rings().shown} onChange={(on) => call(Armor.setZoomPath(on))}/>
        <If when={rings().zoomPath}>
          <View style={{ paddingLeft: 18, gap: 4 }}>
            <CheckRow id="armor-camera-zoom-path-fov" label={t("ui.armor.zoom_path_regular_fov")} checked={rings().zoomPathAtFov} disabled={!rings().shown} onChange={(on) => call(Armor.setZoomPathAtFov(on))}/>
            <CheckRow id="armor-camera-zoom-path-max-fov" label={t("ui.armor.zoom_path_max_fov")} checked={rings().zoomPathAtMaxFov} disabled={!rings().shown} onChange={(on) => call(Armor.setZoomPathAtMaxFov(on))}/>
          </View>
        </If>
        <CheckRow id="armor-camera-perspective" label={t("ui.armor.camera_perspective")} checked={perspective().enabled} disabled={!rings().shown} onChange={(on) => call(Armor.setPerspective(on))}/>
        <If when={perspective().enabled}>
          <View style={{ paddingLeft: 18, gap: 4 }}>
            <SliderRow id="armor-perspective-fov" label={t("ui.armor.perspective_fov")} value={perspective().fov} min={30} max={120} step={1} digits={0} onChange={(v) => call(Armor.setPerspectiveFov(v))}/>
            <ChoiceRow
              label={t("ui.armor.perspective_projection")}
              choices={[
                { value: "game" as const, label: t("ui.armor.perspective_proj_game"), selected: perspective().look === "game", id: "armor-perspective-game" },
                { value: "throughCenter" as const, label: t("ui.armor.perspective_proj_center"), selected: perspective().look === "throughCenter", id: "armor-perspective-center" },
              ]}
              onSelect={(look) => call(Armor.setPerspectiveLook(look))}
            />
          </View>
        </If>
      </If>
    </View>
  );
}

const lightingSliders: { field: "flat" | "intensity" | "azimuth" | "elevation" | "rim" | "specular" | "shininess"; key: string; min: number; max: number; step: number; digits: number }[] = [
  { field: "flat", key: "ui.armor.lighting_flat", min: 0, max: 1.5, step: 0.01, digits: 2 },
  { field: "intensity", key: "ui.armor.lighting_intensity", min: 0, max: 1.5, step: 0.01, digits: 2 },
  { field: "azimuth", key: "ui.armor.lighting_azimuth", min: 0, max: 360, step: 1, digits: 0 },
  { field: "elevation", key: "ui.armor.lighting_elevation", min: -90, max: 90, step: 1, digits: 0 },
  { field: "rim", key: "ui.armor.lighting_rim", min: 0, max: 1, step: 0.01, digits: 2 },
  { field: "specular", key: "ui.armor.lighting_specular", min: 0, max: 1, step: 0.01, digits: 2 },
  { field: "shininess", key: "ui.armor.lighting_shininess", min: 1, max: 128, step: 1, digits: 0 },
];

function LightingSection(props: { display: DisplayView }) {
  const lighting = () => props.display.lighting;
  return (
    <View style={{ gap: 6 }}>
      <Heading text={t("ui.armor.lighting")}/>
      <CheckRow id="armor-lighting-enabled" label={t("ui.armor.lighting_enabled")} checked={lighting().enabled} onChange={(on) => call(Armor.setLightingEnabled(on))}/>
      <View style={{ flexDirection: "row", gap: 4 }}>
        <ToolButton id="armor-lighting-ingame" label={t("ui.armor.lighting_preset_ingame")} onPress={() => call(Armor.setLightingPreset("inGame"))}/>
        <ToolButton id="armor-lighting-flat" label={t("ui.armor.lighting_preset_flat")} onPress={() => call(Armor.setLightingPreset("flat"))}/>
        <ToolButton id="armor-lighting-studio" label={t("ui.armor.lighting_preset_studio")} onPress={() => call(Armor.setLightingPreset("studio"))}/>
      </View>
      <Each of={lightingSliders}>
        {(slider) => (
          <SliderRow
            id={`armor-lighting-${slider().field}`}
            label={t(slider().key)}
            value={lighting()[slider().field]}
            min={slider().min}
            max={slider().max}
            step={slider().step}
            digits={slider().digits}
            disabled={!lighting().enabled}
            onChange={(value) => call(Armor.setLighting(slider().field, value))}
          />
        )}
      </Each>
      <ColorRow id="armor-lighting-ambient-color" label={t("ui.armor.lighting_ambient_color")} color={lighting().ambientColor} disabled={!lighting().enabled} onChange={(color) => call(Armor.setLightingColor("ambient", color))}/>
      <ColorRow id="armor-lighting-key-color" label={t("ui.armor.lighting_key_color")} color={lighting().keyColor} disabled={!lighting().enabled} onChange={(color) => call(Armor.setLightingColor("key", color))}/>
    </View>
  );
}

function DisplayBody() {
  const display = () => armor()?.active.display;
  return (
    <If when={armor()?.active.hasArmor && display()} else={<Hint text={t("ui.armor.no_ship_loaded")}/>}>
      <View style={{ gap: 6 }}>
        <CheckRow id="armor-display-legend" label={t("ui.armor.show_armor_thickness")} checked={display()!.legendVisible} onChange={(on) => call(Armor.setLegendVisible(on))}/>
        <CheckRow id="armor-display-plate-edges" label={t("ui.armor.plate_edges")} checked={display()!.plateEdges} onChange={(on) => call(Armor.setPlateEdges(on))}/>
        <CheckRow id="armor-display-waterline" label={t("ui.armor.waterline")} checked={display()!.waterline} onChange={(on) => call(Armor.setWaterline(on))}/>
        <If when={display()!.waterline}>
          <View style={{ paddingLeft: 18 }}>
            <SliderRow id="armor-display-waterline-opacity" label={t("ui.armor.opacity")} value={display()!.waterlineOpacity} min={0.05} max={1} onChange={(v) => call(Armor.setWaterlineOpacity(v))}/>
          </View>
        </If>
        <CheckRow id="armor-display-zero-mm" label={t("ui.armor.zero_mm_plates")} checked={display()!.zeroMm} onChange={(on) => call(Armor.setZeroMm(on))}/>
        <CheckRow id="armor-display-ship-center" label={t("ui.armor.ship_center")} checked={display()!.shipCenter} onChange={(on) => call(Armor.setShipCenter(on))}/>
        <SliderRow id="armor-display-armor-opacity" label={t("ui.armor.armor_opacity")} value={display()!.armorOpacity} min={0.1} max={1} onChange={(v) => call(Armor.setArmorOpacity(v))}/>
        <If when={display()!.hitsDrawn}>
          <SliderRow id="armor-display-marker-opacity" label={t("ui.armor.marker_opacity")} value={display()!.markerOpacity} min={0} max={1} onChange={(v) => call(Armor.setMarkerOpacity(v))}/>
        </If>
        <SliderRow id="armor-display-roll" label={t("ui.armor.roll")} value={display()!.roll} min={-25} max={25} step={0.5} digits={1} onChange={(v) => call(Armor.setRoll(v))}/>
        <Rule/>
        <RingsSection display={display()!}/>
        <Rule/>
        <LightingSection display={display()!}/>
      </View>
    </If>
  );
}

function RailSection(props: { section: Section; title: string; body: () => JSX.Element }) {
  const open = () => openSections().includes(props.section);
  return (
    <View style={{ borderTopWidth: 1, borderTopColor: colors.border }}>
      <View
        testID={`armor-options-section-${props.section}`}
        role="button"
        accessibilityLabel={props.title}
        accessibilityState={{ expanded: open() }}
        onPress={() => openSections.set(open() ? openSections().filter((s) => s !== props.section) : [...openSections(), props.section])}
        hoverStyle={{ backgroundColor: colors.accent }}
        style={{ minHeight: 30, flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8 }}
      >
        <Text style={{ width: 12, fontSize: 10, color: colors.mutedForeground }}>{open() ? "v" : ">"}</Text>
        <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.foreground }}>{props.title}</Text>
      </View>
      <If when={open()}>
        <View testID={`armor-options-body-${props.section}`} style={{ paddingHorizontal: 8, paddingBottom: 10 }}>
          {props.body()}
        </View>
      </If>
    </View>
  );
}

/** The options rail: tools, the trajectory panel and the Armor, Hull and Display sections. */
export function OptionsRail() {
  const view = () => armor();
  const target = () => {
    const current = view();
    if (!current) return "";
    const ship = current.active.ship ?? "";
    if (current.panes.length > 1) {
      const pane = current.panes.find((candidate) => candidate.id === current.activePane);
      const heading = t("ui.armor.options_pane", { number: pane?.number ?? 1, count: current.panes.length });
      return ship ? `${heading}: ${ship}` : heading;
    }
    return ship;
  };
  return (
    <View testID="armor-options-panel" style={{ width: 300, borderLeftWidth: 1, borderLeftColor: colors.border, backgroundColor: colors.surface }}>
      <View style={{ paddingHorizontal: 8, paddingTop: 6, paddingBottom: 6, gap: 2 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Text style={{ flex: 1, fontSize: 13, fontWeight: "600", color: colors.foreground }}>{t("ui.armor.options")}</Text>
          <IconButton id="armor-options-collapse" glyph=">" label={t("ui.armor.collapse_options")} onPress={() => call(Armor.setOptionsOpen(false))}/>
        </View>
        <If when={target() !== ""}>
          <Text style={{ fontSize: 12, color: (view()?.panes.length ?? 0) > 1 ? colors.primary : colors.mutedForeground }}>{target()}</Text>
        </If>
      </View>
      <Scroll style={{ flex: 1 }}>
        <View style={{ gap: 8, paddingBottom: 12 }}>
          <View style={{ paddingHorizontal: 8 }}><Tools/></View>
          <If when={view()?.active.trajectory.shown}>
            <View style={{ paddingHorizontal: 8 }}><TrajectoryPanel/></View>
          </If>
          <View>
            <RailSection section="Visibility" title={t("ui.armor.armor_options")} body={() => <VisibilityBody/>}/>
            <RailSection section="Hull" title={t("ui.armor.hull_toggle")} body={() => <HullBody/>}/>
            <RailSection section="Display" title={t("ui.armor.display")} body={() => <DisplayBody/>}/>
          </View>
        </View>
      </Scroll>
    </View>
  );
}

/** The edge rail the options fold to. */
export function OptionsEdge() {
  return (
    <View style={{ width: 26, alignItems: "center", paddingTop: 6, borderLeftWidth: 1, borderLeftColor: colors.border, backgroundColor: colors.surface }}>
      <IconButton id="armor-options-expand" glyph="<" label={t("ui.armor.expand_options")} onPress={() => call(Armor.setOptionsOpen(true))}/>
    </View>
  );
}
