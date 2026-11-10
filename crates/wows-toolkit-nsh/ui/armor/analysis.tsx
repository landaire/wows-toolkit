import { Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { Armor } from "wows-toolkit/native";
import { inputStyle } from "../shell/controls";
import { t } from "../i18n";
import { colors, controlRadius, semantic } from "../theme";
import { armor, type ArcView, call, type HealthStripView, type IncomingView, type PenShipView, type ShellVerdict } from "./state";
import { CheckRow, Heading, Hint, IconButton, Swatch, ToolButton } from "./widgets";

/** What is typed in the Pen Check ship search, shared by the panel and the trajectory panel. */
const penDraft = signal("");

/** The ship search of the comparison list: a field and up to ten ships to add. */
export function PenSearch(props: { id: string }) {
  const results = () => armor()?.pen.results ?? [];
  return (
    <View style={{ gap: 4 }}>
      <Input
        testID={props.id}
        accessibilityLabel={t("ui.armor.pen.search_hint")}
        placeholder={t("ui.armor.pen.search_hint")}
        value={penDraft()}
        onChange={({ value }) => {
          penDraft.set(value);
          call(Armor.setPenSearch(value));
        }}
        style={inputStyle}
      />
      <If when={penDraft().trim() !== "" && results().length > 0}>
        <Scroll style={{ maxHeight: 150 }}>
          <View style={{ gap: 2 }}>
            <Each of={results()} key={(result) => result.paramIndex}>
              {(result, index) => (
                <View
                  testID={`armor-pen-add-${index()}`}
                  role="button"
                  accessibilityLabel={result().label}
                  onPress={() => {
                    penDraft.set("");
                    call(Armor.addPenShip(result().paramIndex));
                  }}
                  hoverStyle={{ backgroundColor: colors.accent }}
                  style={{ minHeight: 22, paddingHorizontal: 6, justifyContent: "center", borderRadius: 4 }}
                >
                  <Text style={{ fontSize: 12.5, color: colors.foreground }}>{result().label}</Text>
                </View>
              )}
            </Each>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function verdictColor(verdict: ShellVerdict): string {
  switch (verdict) {
    case "penetrates": return semantic.ok;
    case "shatters": return semantic.error;
    case "angleDependent": return semantic.warn;
    default: return colors.mutedForeground;
  }
}

function ShipBlock(props: { ship: PenShipView; index: number }) {
  return (
    <View style={{ gap: 4, padding: 8, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.card }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <Text style={{ flex: 1, fontSize: 12.5, fontWeight: "600", color: colors.cardForeground }}>{props.ship.label}</Text>
        <ToolButton
          id={`armor-pen-firing-button-${props.index}`}
          label={t("ui.armor.pen.use_for_trajectory")}
          tooltip={t("ui.armor.pen.firing_ship_tooltip")}
          on={props.ship.firing}
          onPress={() => call(Armor.setFiringShip(props.ship.paramIndex))}
        />
        <IconButton id={`armor-pen-remove-${props.index}`} glyph="x" label={t("ui.armor.pen.remove")} onPress={() => call(Armor.removePenShip(props.index))}/>
      </View>
      <If when={props.ship.noShells}>
        <Hint text={props.ship.noShells ?? ""}/>
      </If>
      <Each of={props.ship.shells} key={(shell) => shell.index}>
        {(shell) => (
          <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
            <Text style={{ width: 96, fontSize: 12, color: colors.cardForeground }}>{shell().label}</Text>
            <Text style={{ width: 48, fontSize: 12, color: colors.mutedForeground }}>{shell().pen}</Text>
            <Text style={{ flex: 1, fontSize: 12, color: verdictColor(shell().verdict) }}>{shell().verdictText}</Text>
          </View>
        )}
      </Each>
    </View>
  );
}

function outcomeColor(arc: ArcView): string {
  switch (arc.outcomeKind) {
    case "detonated": return semantic.ok;
    case "overpenetrated": return semantic.warn;
    case "notSimulated": return colors.mutedForeground;
    default: return semantic.error;
  }
}

function ArcRow(props: { arc: ArcView; index: number }) {
  const arc = () => props.arc;
  return (
    <View style={{ gap: 4, paddingVertical: 6, borderTopWidth: 1, borderTopColor: colors.border }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <Swatch color={arc().color} width={12} height={12}/>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.cardForeground }}>{arc().heading}</Text>
          <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{arc().summary}</Text>
        </View>
        <IconButton id={`armor-arc-delete-${props.index}`} glyph="x" label={t("ui.armor.arc_delete")} onPress={() => call(Armor.removeArc(props.index))}/>
      </View>
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 4 }}>
        <ToolButton
          id={`armor-arc-isolate-plates-${props.index}`}
          label={t("ui.armor.arc_isolate_plates")}
          tooltip={t("ui.armor.arc_isolate_plates_tooltip")}
          on={arc().isolatingPlates}
          onPress={() => call(Armor.isolateArc(props.index, "plates", !arc().isolatingPlates))}
        />
        <ToolButton
          id={`armor-arc-isolate-zones-${props.index}`}
          label={t("ui.armor.arc_isolate_zones")}
          tooltip={t("ui.armor.arc_isolate_zones_tooltip")}
          on={arc().isolatingZones}
          onPress={() => call(Armor.isolateArc(props.index, "zones", !arc().isolatingZones))}
        />
        <View style={{ flex: 1 }}/>
        <ToolButton
          id={`armor-arc-range-down-${props.index}`}
          label="-"
          tooltip={t("ui.armor.arc_range_tooltip")}
          disabled={!arc().canStepDown}
          onPress={() => call(Armor.setArcRange(props.index, Math.max(0, arc().range - 0.5)))}
        />
        <Text style={{ fontSize: 12, color: colors.cardForeground }}>{`${arc().range.toFixed(1)} km`}</Text>
        <ToolButton
          id={`armor-arc-range-up-${props.index}`}
          label="+"
          tooltip={t("ui.armor.arc_range_tooltip")}
          disabled={!arc().canStepUp}
          onPress={() => call(Armor.setArcRange(props.index, Math.min(30, arc().range + 0.5)))}
        />
      </View>
      <Text style={{ fontSize: 12, color: outcomeColor(arc()) }}>{arc().outcome}</Text>
    </View>
  );
}

function Arcs(props: { arcs: ArcView[] }) {
  return (
    <View style={{ gap: 6 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
        <View style={{ flex: 1 }}><Heading text={t("ui.armor.trajectory")}/></View>
        <ToolButton
          id="armor-arcs-isolate-plates"
          label={t("ui.armor.arc_isolate_plates")}
          tooltip={t("ui.armor.arc_isolate_plates_tooltip")}
          onPress={() => call(Armor.isolateAllArcs("plates"))}
        />
        <ToolButton
          id="armor-arcs-isolate-zones"
          label={t("ui.armor.arc_isolate_zones")}
          tooltip={t("ui.armor.arc_isolate_zones_tooltip")}
          onPress={() => call(Armor.isolateAllArcs("zones"))}
        />
      </View>
      <Hint tone="warn" text={t("ui.armor.arc_simulation_caveat")}/>
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}><Swatch color={semantic.ok}/><Hint text={t("ui.armor.angle_good")}/></View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}><Swatch color={semantic.warn}/><Hint text={t("ui.armor.angle_mid")}/></View>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}><Swatch color={semantic.error}/><Hint text={t("ui.armor.angle_bad")}/></View>
      </View>
      <Each of={props.arcs} key={(arc) => arc.number}>
        {(arc, index) => <ArcRow arc={arc()} index={index()}/>}
      </Each>
    </View>
  );
}

/** Health over the battle as a stepped line, hits as ticks, playback as a cursor; a press seeks. */
function HealthStrip(props: { strip: HealthStripView }) {
  const width = signal(0);
  const height = 60;
  const steps = () => {
    const line = props.strip.line;
    const w = width();
    const segments: { left: number; top: number; width: number; height: number }[] = [];
    for (let i = 0; i < line.length; i++) {
      const point = line[i];
      const next = line[i + 1];
      const y = (1 - point.y) * (height - 2);
      if (!next) break;
      segments.push({ left: point.x * w, top: y, width: Math.max(1, (next.x - point.x) * w), height: 2 });
      const nextY = (1 - next.y) * (height - 2);
      if (nextY !== y) segments.push({ left: next.x * w, top: Math.min(y, nextY), width: 2, height: Math.abs(nextY - y) + 2 });
    }
    return segments;
  };
  return (
    <View
      testID="armor-incoming-strip"
      role="adjustable"
      accessibilityLabel={t("ui.armor.realtime.incoming_fire")}
      onLayout={({ bounds }) => width.set(bounds.size.width)}
      onPress={(event) => {
        if (width() > 0) call(Armor.seekStrip(Math.min(1, Math.max(0, event.position.x / width()))));
      }}
      style={{ height, borderRadius: 4, overflow: "hidden", backgroundColor: colors.muted }}
    >
      <Each of={steps()} key="index">
        {(segment) => (
          <View style={{ position: "absolute", left: segment().left, top: segment().top, width: segment().width, height: segment().height, backgroundColor: semantic.ok }}/>
        )}
      </Each>
      <Each of={props.strip.hits} key="index">
        {(hit) => <View style={{ position: "absolute", left: hit() * width(), bottom: 0, width: 1, height: 8, backgroundColor: semantic.error }}/>}
      </Each>
      <If when={props.strip.cursor != null}>
        <View style={{ position: "absolute", left: (props.strip.cursor ?? 0) * width(), top: 0, bottom: 0, width: 2, backgroundColor: colors.foreground }}/>
      </If>
    </View>
  );
}

function standingColor(standing: "agrees" | "deferred" | "disagrees"): string {
  return standing === "agrees" ? semantic.ok : standing === "deferred" ? semantic.warn : semantic.error;
}

function Incoming(props: { incoming: IncomingView }) {
  const incoming = () => props.incoming;
  return (
    <View style={{ gap: 6 }}>
      <Heading text={t("ui.armor.realtime.incoming_fire")}/>
      <If when={incoming().strip}>
        {(strip) => <HealthStrip strip={strip()}/>}
      </If>
      <Hint text={incoming().tracked}/>
      <If when={incoming().simUnchecked}>
        <Hint tone="warn" text={incoming().simUnchecked ?? ""}/>
      </If>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{t("ui.armor.realtime.attacker_filter")}</Text>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        <Each of={incoming().attackers} key="index">
          {(attacker, index) => (
            <ToolButton
              id={`armor-incoming-attacker-${index()}`}
              label={attacker().label}
              on={attacker().selected}
              onPress={() => call(Armor.setIncomingAttacker(attacker().entity ?? null))}
            />
          )}
        </Each>
      </View>
      <CheckRow
        id="armor-incoming-secondaries"
        label={t("ui.armor.realtime.show_secondaries")}
        checked={incoming().secondaries}
        onChange={(on) => call(Armor.setIncomingSecondaries(on))}
      />
      <If when={incoming().salvos.length === 0}>
        <Hint text={t("ui.armor.realtime.nothing_landed")}/>
      </If>
      <Scroll style={{ maxHeight: 220 }}>
        <View style={{ gap: 6 }}>
          <Each of={incoming().salvos} key="index">
            {(salvo, index) => (
              <View style={{ gap: 2, paddingVertical: 4, borderTopWidth: 1, borderTopColor: colors.border }}>
                <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
                  <Text style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.cardForeground }}>{salvo().attacker}</Text>
                  <ToolButton
                    id={`armor-incoming-seek-${index()}`}
                    label={salvo().clock}
                    tooltip={t("ui.armor.realtime.seek_to_salvo")}
                    onPress={() => call(Armor.seekTo(salvo().seconds))}
                  />
                  <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{salvo().count}</Text>
                </View>
                <Each of={salvo().shells} key="index">
                  {(shell) => (
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
                      <Text style={{ fontSize: 12, color: colors.cardForeground }}>{shell().text}</Text>
                      <If when={shell().sim}>
                        {(sim) => <Text style={{ fontSize: 12, color: standingColor(sim().standing) }}>{sim().label}</Text>}
                      </If>
                    </View>
                  )}
                </Each>
              </View>
            )}
          </Each>
        </View>
      </Scroll>
    </View>
  );
}

/** The Pen Check panel: the plate it checks, the ships compared, the arcs cast and the fire a replay recorded. */
export function PenCheckPanel() {
  const view = () => armor();
  const pen = () => view()?.pen;
  const searchHere = () => !(view()?.optionsOpen && view()?.active.trajectory.shown);
  return (
    <View testID="armor-pen-panel" style={{ flex: 1, minWidth: 0, borderLeftWidth: 1, borderLeftColor: colors.border, backgroundColor: colors.surface }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, padding: 8 }}>
        <Text style={{ flex: 1, fontSize: 13, fontWeight: "600", color: colors.foreground }}>{t("ui.armor.pen_check_label")}</Text>
        <CheckRow
          id="armor-pen-ifhe"
          label={t("ui.armor.pen.ifhe")}
          tooltip={t("ui.armor.pen.ifhe_tooltip")}
          checked={pen()?.ifhe ?? false}
          onChange={(on) => call(Armor.setIfhe(on))}
        />
        <IconButton id="armor-pen-close" glyph="x" label={t("ui.armor.close_pen_check")} onPress={() => call(Armor.setPenCheckOpen(false))}/>
      </View>
      <Scroll style={{ flex: 1 }}>
        <View style={{ gap: 10, paddingHorizontal: 8, paddingBottom: 12 }}>
          <Text style={{ fontSize: 12.5, color: pen()?.plate ? colors.foreground : colors.mutedForeground }}>
            {pen()?.plate ? `${pen()!.plate!.mm} mm ${pen()!.plate!.zone}` : t("ui.armor.pen.hover_a_plate")}
          </Text>
          <If when={searchHere()}>
            <PenSearch id="armor-pen-search"/>
          </If>
          <If when={(pen()?.ships.length ?? 0) === 0}>
            <Hint text={t("ui.armor.pen.add_a_ship")}/>
          </If>
          <Each of={pen()?.ships ?? []} key={(ship) => ship.paramIndex}>
            {(ship, index) => <ShipBlock ship={ship()} index={index()}/>}
          </Each>
          <If when={(pen()?.ships.length ?? 0) > 0}>
            <View style={{ alignItems: "flex-start" }}>
              <ToolButton id="armor-pen-clear" label={t("ui.armor.pen.clear_all")} onPress={() => call(Armor.clearPenShips())}/>
            </View>
          </If>
          <If when={(view()?.active.arcs.length ?? 0) > 0}>
            <Arcs arcs={view()!.active.arcs}/>
          </If>
          <If when={view()?.incoming}>
            {(incoming) => <Incoming incoming={incoming()}/>}
          </If>
        </View>
      </Scroll>
    </View>
  );
}
