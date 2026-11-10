import type { JSX } from "nsh";
import { cleanup, Clipboard, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { Button as BaseButton, Checkbox, Input, Popover, Progress, Slider, Tooltip } from "nsh/base";
import {
  ReplayRenderer,
  type BatchView,
  type PreviewView,
  type RendererText,
  type RendererTool,
  type RendererView,
  type SettingsView,
  type ShipMenuView,
  type TimelineView,
  type WatchedView,
} from "wows-toolkit/native";
import { Surface } from "./components";
import { Frame } from "./frame";
import { t } from "./i18n";
import { CheckMark, Icon, type IconName } from "./icons";
import { beforeSoftwareEncode } from "./shell/notices";
import { colors, controlRadius } from "./theme";

let labels: RendererText | null = null;

// The controls' labels, read once in the reader's language.
function text(): RendererText {
  if (!labels) labels = ReplayRenderer.text();
  return labels;
}

const SPECIES = ["DD", "CA", "BB", "CV", "SS"];
const FRIENDLY_TINT = "#4ce8aa";
const ENEMY_TINT = "#fe4d2a";
// The inks `ReplayRenderer.setInk` takes, by index.
const INKS = [
  { color: "#ffffff", key: "ui.renderer.annotations.ink_white" },
  { color: "#a0a0a0", key: "ui.renderer.annotations.ink_gray" },
  { color: "#e63232", key: "ui.renderer.annotations.ink_red" },
  { color: "#f08c1e", key: "ui.renderer.annotations.ink_orange" },
  { color: "#f0e632", key: "ui.renderer.annotations.ink_yellow" },
  { color: "#32c832", key: "ui.renderer.annotations.ink_green" },
  { color: "#3278e6", key: "ui.renderer.annotations.ink_blue" },
  { color: "#b43ce6", key: "ui.renderer.annotations.ink_purple" },
  { color: "#ff82b4", key: "ui.renderer.annotations.ink_pink" },
];
// The ship menu's width, and the room it keeps from the map's edges.
const SHIP_MENU_WIDTH = 220;
const SHIP_MENU_MARGIN = 8;
const MIN_ZOOM = 1;
const MAX_ZOOM = 10;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Divider() {
  return <View style={{ width: 1, alignSelf: "stretch", marginVertical: 4, backgroundColor: colors.border }}/>;
}

function Label(props: { text: string }) {
  return <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.text}</Text>;
}

function Tip(props: { text: string; placement?: "top" | "left" }) {
  return (
    <Tooltip placement={props.placement ?? "top"} style={{ paddingHorizontal: 8, paddingVertical: 4, borderRadius: 6, backgroundColor: colors.popover, borderWidth: 1, borderColor: colors.border }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{props.text}</Text>
    </Tooltip>
  );
}

// A compact transport button, a glyph or a word, that can read as chosen,
// with a hint on hover.
function ToolButton(props: {
  label?: string;
  icon?: IconName;
  accessibilityLabel?: string;
  hint?: string;
  chosen?: boolean;
  disabled?: boolean;
  color?: string;
  children?: JSX.Element;
  onPress: () => void;
}) {
  const ink = () => props.color ?? (props.chosen ? colors.primaryForeground : colors.secondaryForeground);
  return (
    <BaseButton
      accessibilityLabel={props.accessibilityLabel ?? props.label ?? props.hint}
      accessibilityState={{ selected: props.chosen ?? false }}
      disabled={props.disabled}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      tooltip={props.hint ? <Tip text={props.hint}/> : undefined}
      style={{
        minHeight: 26,
        minWidth: 26,
        paddingHorizontal: 7,
        flexDirection: "row",
        alignItems: "center",
        justifyContent: "center",
        gap: 5,
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: props.chosen ? colors.ring : colors.border,
        backgroundColor: props.chosen ? colors.primary : colors.secondary,
        opacity: props.disabled ? 0.45 : 1,
      }}
    >
      {props.children}
      <If when={props.icon}>
        <Icon name={props.icon!} size={14} color={ink()}/>
      </If>
      <If when={props.label}>
        <Text style={{ fontSize: 12, color: ink() }}>{props.label}</Text>
      </If>
    </BaseButton>
  );
}

// A button that opens a popover under the transport, a glyph or a word.
function PopoverButton(props: { label?: string; icon?: IconName; hint: string; disabled?: boolean; onOpen?: () => void; children: JSX.Element }) {
  return (
    <Popover
      offset={4}
      margin={8}
      placement="top"
      onOpen={props.onOpen}
      disabled={props.disabled}
      trigger={
        <View
          accessibilityLabel={props.hint}
          role="button"
          hoverStyle={{ backgroundColor: colors.accent }}
          tooltip={<Tip text={props.hint}/>}
          style={{ minHeight: 26, minWidth: 26, paddingHorizontal: 7, alignItems: "center", justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, opacity: props.disabled ? 0.45 : 1 }}
        >
          <If when={props.icon}>
            <Icon name={props.icon!} size={14} color={colors.foreground}/>
          </If>
          <If when={props.label}>
            <Text style={{ fontSize: 12, color: colors.foreground }}>{props.label}</Text>
          </If>
        </View>
      }
    >
      <Surface style={{ padding: 8, gap: 6, backgroundColor: colors.popover }}>{props.children}</Surface>
    </Popover>
  );
}

function Toggle(props: { label: string; checked: boolean; hint?: string; disabled?: boolean; onChange: (checked: boolean) => void }) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      disabled={props.disabled}
      tooltip={props.hint ? <Tip text={props.hint}/> : undefined}
      class="group flex-row items-center gap-2 py-[2px]"
      indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<CheckMark checked={props.checked}/>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <Text style={{ fontSize: 12.5, color: colors.popoverForeground }}>{props.label}</Text>
    </Checkbox>
  );
}

// One viewport's state on the TSX side: what the producer last said, and how
// commands report back.
type Viewport = {
  view: () => RendererView;
  run: (command: () => RendererView) => void;
  say: (message: string) => void;
  /** Opens the viewport in its own window; absent once it is in one. */
  popOut?: () => void;
};

function Transport(props: { port: Viewport }) {
  const view = () => props.port.view();
  const id = () => view().id;
  const ready = () => view().status === "ready";
  const steerable = () => ready() && view().export == null;
  const run = props.port.run;
  const labels = text();
  return (
    <View testID="renderer-transport" style={{ borderTopWidth: 1, borderTopColor: colors.border, backgroundColor: colors.card }}>
      <If when={view().exportFailure}>
        <Text style={{ paddingHorizontal: 10, paddingTop: 4, fontSize: 12, color: colors.destructive }}>{view().exportFailure}</Text>
      </If>
      <View style={{ paddingHorizontal: 13, paddingTop: 6 }}>
        <View style={{ position: "relative", height: 20, justifyContent: "center" }}>
          <Slider
            accessibilityLabel={t("ui.renderer.controls.seek")}
            value={view().frame}
            min={0}
            max={Math.max(view().lastFrame, 1)}
            step={1}
            disabled={!steerable()}
            class="w-full h-[20px] justify-center"
            trackClass="h-[4px] rounded-full bg-secondary"
            indicatorClass="h-[4px] rounded-full bg-primary"
            thumbClass="w-[14px] h-[14px] rounded-full bg-primary"
            onChange={({ value }) => run(() => ReplayRenderer.seek(id(), Math.round(value as number)))}
          />
          <Each of={view().marks}>
            {(mark) => (
              <View style={{ position: "absolute", top: 3, bottom: 3, width: 1.5, left: `${mark() * 100}%`, backgroundColor: colors.mutedForeground }}/>
            )}
          </Each>
        </View>
      </View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8, paddingVertical: 5, flexWrap: "wrap" }}>
        <ToolButton icon="skip-back" hint={labels.jumpToStart} disabled={!steerable()} onPress={() => run(() => ReplayRenderer.seek(id(), 0))}/>
        <ToolButton icon="rewind" hint={labels.previousEvent} disabled={!steerable() || !view().hasPreviousEvent} onPress={() => run(() => ReplayRenderer.previousEvent(id()))}/>
        <ToolButton icon="rotate-ccw" hint={labels.back10s} disabled={!steerable()} onPress={() => run(() => ReplayRenderer.seekBy(id(), -10))}/>
        <ToolButton
          icon={view().playing ? "pause" : "play"}
          hint={view().playing ? labels.pause : labels.play}
          disabled={!steerable()}
          onPress={() => run(() => ReplayRenderer.togglePlaying(id()))}
        />
        <ToolButton icon="rotate-cw" hint={labels.forward10s} disabled={!steerable()} onPress={() => run(() => ReplayRenderer.seekBy(id(), 10))}/>
        <ToolButton icon="fast-forward" hint={labels.nextEvent} disabled={!steerable() || !view().hasNextEvent} onPress={() => run(() => ReplayRenderer.nextEvent(id()))}/>
        <ToolButton icon="skip-forward" hint={labels.jumpToEnd} disabled={!steerable()} onPress={() => run(() => ReplayRenderer.seekEnd(id()))}/>
        <Text accessibilityLabel={t("ui.renderer.controls.clock")} style={{ width: 92, fontSize: 12, color: colors.mutedForeground }}>{view().clock}</Text>
        <View style={{ flex: 1, minWidth: 0 }}/>
        <Icon name="search" size={14} color={colors.mutedForeground}/>
        <Slider
          accessibilityLabel={t("ui.renderer.controls.zoom")}
          value={view().zoom}
          min={MIN_ZOOM}
          max={MAX_ZOOM}
          step={0.1}
          disabled={!ready()}
          class="w-[90px] h-[20px] justify-center"
          trackClass="h-[4px] rounded-full bg-secondary"
          indicatorClass="h-[4px] rounded-full bg-primary"
          thumbClass="w-[12px] h-[12px] rounded-full bg-primary"
          onChange={({ value }) => run(() => ReplayRenderer.setZoom(id(), value as number))}
        />
        <ToolButton label={labels.zoomReset} disabled={!ready() || view().wholeMap} onPress={() => run(() => ReplayRenderer.resetZoom(id()))}/>
        <Divider/>
        <SpeedPicker port={props.port}/>
        <Divider/>
        <ToolButton icon="download" hint={labels.exportVideo} disabled={!steerable()} onPress={() => exportVideo(props.port, false)}/>
        <ToolsPopover port={props.port}/>
        <SettingsPopover port={props.port}/>
        <TimelinePopover port={props.port}/>
        <If when={props.port.popOut}>
          <ToolButton icon="square-arrow-out-up-right" hint={labels.popOut} onPress={() => props.port.popOut!()}/>
        </If>
        <ToolButton icon="clipboard" hint={labels.exportClipboard} disabled={!steerable()} onPress={() => exportVideo(props.port, true)}/>
      </View>
    </View>
  );
}

function SpeedPicker(props: { port: Viewport }) {
  const view = () => props.port.view();
  const labels = text();
  return (
    <PopoverButton label={labels.speeds[view().speedIndex]} hint={labels.speed} disabled={view().status !== "ready"}>
      <View style={{ gap: 3 }}>
        <Each of={labels.speeds}>
          {(speed, index) => (
            <ToolButton
              label={speed()}
              accessibilityLabel={`${labels.speed} ${speed()}`}
              chosen={view().speedIndex === index()}
              onPress={() => props.port.run(() => ReplayRenderer.setSpeed(view().id, index()))}
            />
          )}
        </Each>
      </View>
    </PopoverButton>
  );
}

// Asks before an encode in software, as the other apps do, then starts it.
function exportVideo(port: Viewport, toClipboard: boolean) {
  const id = port.view().id;
  const start = async () => {
    try {
      if (toClipboard) port.run(() => ReplayRenderer.exportClipboard(id));
      else await ReplayRenderer.exportVideo(id);
    } catch (error) {
      port.say(errorMessage(error));
    }
  };
  if (port.view().softwareEncode) beforeSoftwareEncode(() => void start());
  else void start();
}

function SettingsPopover(props: { port: Viewport }) {
  const settings = signal<SettingsView | null>(null);
  const id = () => props.port.view().id;
  const labels = text();
  const apply = (command: () => SettingsView) => {
    try {
      settings.set(command());
    } catch (error) {
      props.port.say(errorMessage(error));
    }
  };
  return (
    <PopoverButton
      icon="settings"
      hint={labels.settings}
      disabled={props.port.view().status !== "ready"}
      onOpen={() => apply(() => ReplayRenderer.settings(id()))}
    >
      <If when={settings()}>
        <Scroll style={{ width: 250, maxHeight: 440 }}>
          <View style={{ gap: 2 }}>
            <Each of={settings()!.layers}>
              {(layer) => <Toggle label={layer().label} checked={layer().on} onChange={(on) => apply(() => ReplayRenderer.setLayer(id(), layer().id, on))}/>}
            </Each>
            <Text style={{ paddingTop: 8, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>{labels.exportSettings}</Text>
            <Toggle label={labels.preferCpu} hint={labels.preferCpuHint} checked={settings()!.preferCpu} onChange={(on) => apply(() => ReplayRenderer.setPreferCpu(id(), on))}/>
            <Toggle label={labels.includePreBattle} hint={labels.includePreBattleHint} checked={settings()!.includePreBattle} onChange={(on) => apply(() => ReplayRenderer.setIncludePreBattle(id(), on))}/>
            <Label text={labels.codec}/>
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
              <ToolButton label={settings()!.autoLabel} chosen={settings()!.codec == null} onPress={() => apply(() => ReplayRenderer.setCodec(id(), null))}/>
              <Each of={settings()!.codecs}>
                {(codec) => (
                  <ToolButton label={codec().label} chosen={settings()!.codec === codec().value} onPress={() => apply(() => ReplayRenderer.setCodec(id(), codec().value))}/>
                )}
              </Each>
            </View>
            <View style={{ paddingTop: 8 }}>
              <ToolButton label={labels.saveDefaults} onPress={() => void ReplayRenderer.saveDefaults(id()).then((next) => props.port.run(() => next), (error) => props.port.say(errorMessage(error)))}/>
            </View>
          </View>
        </Scroll>
      </If>
    </PopoverButton>
  );
}

function sideColor(side: string): string {
  switch (side) {
    case "friendly": return "#4ce8aa";
    case "enemy": return "#fe4d2a";
    default: return colors.mutedForeground;
  }
}

// Reads the timeline when the popover opens, and again on each change of the
// viewport until the event scan has finished, so a popover opened during the
// scan fills in without being reopened.
function FollowTimeline(props: { port: Viewport; read: () => boolean; refresh: () => void }) {
  let opened = false;
  effect(() => {
    props.port.view().revision;
    if (opened && untracked(props.read)) return;
    opened = true;
    untracked(props.refresh);
  });
  return <View/>;
}

function TimelinePopover(props: { port: Viewport }) {
  const timeline = signal<TimelineView | null>(null);
  const id = () => props.port.view().id;
  const labels = text();
  const apply = (command: () => TimelineView) => {
    try {
      timeline.set(command());
    } catch (error) {
      props.port.say(errorMessage(error));
    }
  };
  const refresh = () => apply(() => ReplayRenderer.timeline(id()));
  return (
    <PopoverButton icon="list" hint={labels.timeline}>
      <FollowTimeline port={props.port} read={() => timeline()?.read ?? false} refresh={refresh}/>
      <If when={timeline()}>
        <View style={{ width: 360, gap: 6 }}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text style={{ flex: 1, fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{labels.timeline}</Text>
            <ToolButton label={labels.timelineCopy} hint={labels.timelineCopyHint} disabled={timeline()!.rows.length === 0} onPress={() => void Clipboard.writeText(timeline()!.copyText).then(() => props.port.say(labels.timelineCopied))}/>
          </View>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
            <ToolButton label={labels.timelineAll} onPress={() => apply(() => ReplayRenderer.setTimelineKinds(id(), true))}/>
            <ToolButton label={labels.timelineNone} onPress={() => apply(() => ReplayRenderer.setTimelineKinds(id(), false))}/>
            <Input
              accessibilityLabel={labels.timelineSearch}
              placeholder={labels.timelineSearch}
              value={timeline()!.search}
              onChange={({ value }) => apply(() => ReplayRenderer.setTimelineSearch(id(), value))}
              style={{ flex: 1, minHeight: 26, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
            />
          </View>
          <View style={{ flexDirection: "row", flexWrap: "wrap", columnGap: 10 }}>
            <Each of={timeline()!.kinds}>
              {(kind) => <Toggle label={kind().label} checked={kind().on} onChange={(on) => apply(() => ReplayRenderer.setTimelineKind(id(), kind().index, on))}/>}
            </Each>
          </View>
          <View style={{ height: 1, backgroundColor: colors.border }}/>
          <If
            when={timeline()!.read}
            else={
              <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingVertical: 6 }}>
                <View style={{ width: 48 }}><Progress value={0} indeterminate accessibilityLabel={labels.timelineReading}/></View>
                <Label text={labels.timelineReading}/>
              </View>
            }
          >
            <If
              when={timeline()!.rows.length > 0}
              else={<Label text={timeline()!.anyEvents ? labels.timelineNoMatches : labels.timelineNoEvents}/>}
            >
              <Scroll style={{ maxHeight: 400 }}>
                <View style={{ gap: 1 }}>
                  <Each of={timeline()!.rows}>
                    {(row) => (
                      <BaseButton
                        accessibilityLabel={`${row().time} ${row().label}`}
                        onPress={() => props.port.run(() => ReplayRenderer.goToEvent(id(), row().at))}
                        hoverStyle={{ backgroundColor: colors.accent }}
                        tooltip={row().hover ? <Tip text={row().hover} placement="left"/> : undefined}
                        style={{ flexDirection: "row", gap: 8, paddingHorizontal: 4, paddingVertical: 2, borderRadius: 4 }}
                      >
                        <Text style={{ width: 40, fontSize: 12, color: colors.mutedForeground }}>{row().time}</Text>
                        <Text style={{ flex: 1, fontSize: 12, color: sideColor(row().side) }}>{row().label}</Text>
                      </BaseButton>
                    )}
                  </Each>
                </View>
              </Scroll>
            </If>
          </If>
        </View>
      </If>
    </PopoverButton>
  );
}

function ToolsPopover(props: { port: Viewport }) {
  const view = () => props.port.view();
  const id = () => view().id;
  const run = props.port.run;
  const labels = text();
  const chosenTool = (tool: RendererTool | null | undefined) => (tool ?? null) === (view().tool ?? null) && view().tool !== "ship";
  return (
    <PopoverButton icon="notebook-pen" hint={labels.annotations} disabled={!view().inSession}>
      <View style={{ width: 250, gap: 8 }}>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
          <Each of={labels.tools}>
            {(tool) => (
              <ToolButton
                label={tool().label}
                hint={tool().chord ? `${tool().label} (${tool().chord})` : tool().label}
                chosen={chosenTool(tool().tool)}
                onPress={() => run(() => ReplayRenderer.setTool(id(), tool().tool ?? null))}
              />
            )}
          </Each>
        </View>
        <If when={view().pickedShip}>
          <View style={{ gap: 4 }}>
            <Label text={view().pickedShip!.name ?? labels.noShip}/>
            <Input
              accessibilityLabel={labels.shipHint}
              placeholder={labels.shipHint}
              value={view().shipQuery}
              onChange={({ value }) => run(() => ReplayRenderer.searchShips(id(), value))}
              style={{ minHeight: 26, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
            />
            <Each of={view().shipMatches}>
              {(ship) => <ToolButton label={ship().label} onPress={() => run(() => ReplayRenderer.nameShip(id(), ship().index))}/>}
            </Each>
          </View>
        </If>
        <View style={{ height: 1, backgroundColor: colors.border }}/>
        <Each of={[true, false]}>
          {(friendly) => (
            <View style={{ flexDirection: "row", gap: 4 }}>
              <Each of={SPECIES}>
                {(short) => (
                  <ToolButton
                    label={short()}
                    accessibilityLabel={t(friendly() ? "ui.renderer.annotations.friendly_ship" : "ui.renderer.annotations.enemy_ship", { species: short() })}
                    color={friendly() ? FRIENDLY_TINT : ENEMY_TINT}
                    chosen={view().placing?.short === short() && view().placing?.friendly === friendly()}
                    onPress={() => run(() => ReplayRenderer.placeShip(id(), short(), friendly()))}
                  />
                )}
              </Each>
            </View>
          )}
        </Each>
        <View style={{ height: 1, backgroundColor: colors.border }}/>
        <ToolButton label={labels.undo} disabled={!view().canUndo} onPress={() => run(() => ReplayRenderer.undo(id()))}/>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
          <View style={{ flex: 1 }}><Label text={labels.nib}/></View>
          <ToolButton label="-" accessibilityLabel={labels.thinner} hint={labels.thinner} disabled={!view().canThin} onPress={() => run(() => ReplayRenderer.stepNib(id(), -1))}/>
          <Text accessibilityLabel={labels.nib} style={{ width: 16, fontSize: 12, color: colors.popoverForeground }}>{view().nib.toFixed(0)}</Text>
          <ToolButton label="+" accessibilityLabel={labels.thicker} hint={labels.thicker} disabled={!view().canThicken} onPress={() => run(() => ReplayRenderer.stepNib(id(), 1))}/>
        </View>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
          <Each of={INKS}>
            {(ink, index) => (
              <BaseButton
                accessibilityLabel={t(ink().key)}
                onPress={() => run(() => ReplayRenderer.setInk(id(), index()))}
                style={{ width: 18, height: 18, borderRadius: 3, backgroundColor: ink().color, borderWidth: view().ink === index() ? 2 : 0, borderColor: "#ffffff" }}
              />
            )}
          </Each>
        </View>
      </View>
    </PopoverButton>
  );
}

// What the keys do, in the corner while ctrl is held over the map.
function ShortcutSheet() {
  const labels = text();
  const row = (chord: string, label: string) => (
    <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 12 }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{chord}</Text>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{label}</Text>
    </View>
  );
  return (
    <Surface style={{ position: "absolute", right: 8, bottom: 8, minWidth: 230, padding: 8, gap: 4, backgroundColor: colors.popover }}>
      <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{labels.shortcutsTitle}</Text>
      <Label text={labels.shortcutsTools}/>
      <Each of={labels.tools.filter((tool) => tool.chord)}>{(tool) => row(tool().chord!, tool().label)}</Each>
      <Label text={labels.shortcutsActions}/>
      <Each of={labels.actions}>{(action) => row(action().chord, action().label)}</Each>
    </Surface>
  );
}

type Size = { width: number; height: number };

// The menu a right-click on a ship opens: its trail and its ranges. It stays
// where the click landed, kept inside the map, until it is closed, so several
// switches can be flipped in turn.
function ShipMenu(props: { port: Viewport; menu: ShipMenuView; area: Size }) {
  const id = () => props.port.view().id;
  const player = () => props.menu.player;
  const run = props.port.run;
  const labels = text();
  const height = signal(0);
  const left = () => Math.max(SHIP_MENU_MARGIN, Math.min(props.menu.x, props.area.width - SHIP_MENU_WIDTH - SHIP_MENU_MARGIN));
  const top = () => Math.max(SHIP_MENU_MARGIN, Math.min(props.menu.y, props.area.height - height() - SHIP_MENU_MARGIN));
  return (
    <View
      accessibilityLabel={player()}
      onLayout={({ bounds }) => height.set(bounds.size.height)}
      style={{ position: "absolute", left: left(), top: top(), width: SHIP_MENU_WIDTH }}
    >
      <Surface style={{ padding: 8, gap: 4, backgroundColor: colors.popover }}>
        <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
          <Text numberOfLines={1} style={{ flexShrink: 1, fontSize: 12, fontWeight: "700", color: colors.popoverForeground }}>{player()}</Text>
          <ToolButton icon="x" accessibilityLabel={t("ui.renderer.context.close")} onPress={() => run(() => ReplayRenderer.closeShipMenu(id()))}/>
        </View>
        <Toggle label={labels.showTrail} checked={props.menu.trail} onChange={(on) => run(() => ReplayRenderer.setTrail(id(), player(), on))}/>
        <If when={props.menu.realtimeArmor}>
          <ToolButton
            label={props.menu.realtimeArmor!}
            onPress={() => run(() => ReplayRenderer.showArmor(id(), props.menu.entityId))}
          />
        </If>
        <ToolButton label={labels.onlyTrail} onPress={() => run(() => ReplayRenderer.onlyTrail(id(), player()))}/>
        <View style={{ paddingTop: 4 }}><Label text={labels.ranges}/></View>
        <Each of={props.menu.ranges}>
          {(range) => <Toggle label={range().label} checked={range().on} onChange={(on) => run(() => ReplayRenderer.setRange(id(), player(), range().range, on))}/>}
        </Each>
        <ToolButton
          label={props.menu.allRangesOn ? labels.disableAll : labels.enableAll}
          onPress={() => run(() => ReplayRenderer.setEveryRange(id(), player(), !props.menu.allRangesOn))}
        />
        <ToolButton label={labels.onlyRanges} onPress={() => run(() => ReplayRenderer.onlyRanges(id(), player()))}/>
        <ToolButton label={labels.allRanges} onPress={() => run(() => ReplayRenderer.allRanges(id()))}/>
      </Surface>
    </View>
  );
}

function MapArea(props: { port: Viewport }) {
  const view = () => props.port.view();
  const labels = text();
  const area = signal<Size>({ width: 0, height: 0 });
  return (
    <View
      onLayout={({ bounds }) => area.set({ width: bounds.size.width, height: bounds.size.height })}
      style={{ flex: 1, minHeight: 0, backgroundColor: "#141923" }}
    >
      <If when={view().status === "ready"}>
        <Frame
          surface={view().surface}
          accessibilityLabel={t("ui.renderer.controls.map")}
          onSecondaryPress={(at) => props.port.run(() => ReplayRenderer.openShipMenu(view().id, at.x, at.y))}
          style={{ cursor: view().tool ? "crosshair" : view().wholeMap ? "default" : "grab" }}
        />
      </If>
      <If when={view().status === "baking"}>
        <View style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center", gap: 10 }}>
          <View style={{ width: 120 }}><Progress value={0} indeterminate accessibilityLabel={labels.baking}/></View>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{labels.baking}</Text>
        </View>
      </If>
      <If when={view().status === "failed"}>
        <View style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center", gap: 6, padding: 24 }}>
          <Text style={{ fontSize: 14, fontWeight: "600", color: colors.foreground }}>{labels.failed}</Text>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{view().failure}</Text>
        </View>
      </If>
      <If when={view().export}>
        <View style={{ position: "absolute", top: 8, left: 0, right: 0, alignItems: "center" }}>
          <Surface style={{ width: 300, paddingHorizontal: 12, paddingVertical: 8, gap: 4, backgroundColor: colors.popover }}>
            <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{view().export!.label}</Text>
            <Progress
              value={(view().export!.fraction ?? 0) * 100}
              indeterminate={view().export!.fraction == null}
              accessibilityLabel={view().export!.label}
            />
          </Surface>
        </View>
      </If>
      <Each of={view().cursors}>
        {(cursor) => (
          <View style={{ position: "absolute", left: cursor().x, top: cursor().y, flexDirection: "row", alignItems: "center", gap: 4 }}>
            <View style={{ width: 8, height: 8, borderRadius: 4, backgroundColor: cursor().color }}/>
            <Text style={{ fontSize: 11, color: cursor().color }}>{cursor().name}</Text>
          </View>
        )}
      </Each>
      <If when={view().reading}>
        <Surface style={{ position: "absolute", left: view().reading!.x + 16, top: view().reading!.y + 16, width: 280, padding: 8, gap: 2, backgroundColor: colors.popover }}>
          <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{view().reading!.title}</Text>
          <Each of={view().reading!.lines}>{(line) => <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{line()}</Text>}</Each>
          <If when={view().reading!.detail}>
            <View style={{ height: 1, marginVertical: 3, backgroundColor: colors.border }}/>
            <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{view().reading!.detail}</Text>
          </If>
        </Surface>
      </If>
      <If when={view().ctrlHeld}>
        <ShortcutSheet/>
      </If>
      <If when={view().shipMenu}>
        <ShipMenu port={props.port} menu={view().shipMenu!} area={area()}/>
      </If>
    </View>
  );
}

// Opens a viewport on `path` and follows it until the view goes away.
function openViewport(path: string, alts: string[]) {
  const view = signal<RendererView | null>(null);
  const failure = signal<string | null>(null);
  const notice = signal<string | null>(null);
  let alive = true;
  let lastNotice = 0;

  const say = (message: string) => {
    notice.set(message);
    setTimeout(() => {
      if (notice() === message) notice.set(null);
    }, 4000);
  };
  const adopt = (next: RendererView) => {
    const current = view();
    // A command's answer can arrive after a newer change was heard.
    if (current && next.revision < current.revision && next.revision > current.revision - 1000) return;
    view.set(next);
    if (next.notice && next.notice.id !== lastNotice) {
      lastNotice = next.notice.id;
      say(next.notice.text);
    }
  };
  const run = (command: () => RendererView) => {
    try {
      adopt(command());
    } catch (error) {
      say(errorMessage(error));
    }
  };

  void ReplayRenderer.open(path, alts).then(
    async (opened) => {
      if (!alive) {
        ReplayRenderer.close(opened.id);
        return;
      }
      adopt(opened);
      while (alive) {
        const next = await ReplayRenderer.nextChange(opened.id, view()!.revision);
        if (alive) adopt(next);
      }
    },
    (error) => failure.set(errorMessage(error)),
  );
  cleanup(() => {
    alive = false;
    const opened = view();
    if (opened) ReplayRenderer.close(opened.id);
  });

  return { view, failure, notice, say, run };
}

// A replay played back on its own minimap: the map, its overlays and the
// transport. `popOut`, given while docked, opens it in a window of its own.
export function ReplayRendererView(props: { path: string; alts?: string[]; popOut?: () => void }) {
  const opened = openViewport(props.path, props.alts ?? []);
  const labels = text();
  const port: Viewport = {
    view: () => opened.view()!,
    run: opened.run,
    say: opened.say,
    get popOut() {
      return props.popOut;
    },
  };

  return (
    <View accessibilityLabel="Replay renderer" style={{ flex: 1, minHeight: 0 }}>
      <If when={opened.failure()}>
        <Text style={{ padding: 12, fontSize: 13, color: colors.destructive }}>{opened.failure()}</Text>
      </If>
      <If when={opened.view()} else={<Text style={{ padding: 18, fontSize: 13, color: colors.mutedForeground }}>{labels.baking}</Text>}>
        <MapArea port={port}/>
        <Transport port={port}/>
      </If>
      <If when={opened.notice()}>
        <Surface style={{ position: "absolute", right: 16, bottom: 72, minWidth: 220, maxWidth: 420, padding: 10, backgroundColor: colors.popover }}>
          <Text style={{ fontSize: 12.5, color: colors.popoverForeground }}>{opened.notice()}</Text>
        </Surface>
      </If>
    </View>
  );
}

// A replay's battle played back on its minimap while the pointer rests on its
// row: mount it over the hovered row and unmount it when the pointer leaves.
export function MinimapPreview(props: { path: string; size?: number }) {
  const preview = signal<PreviewView | null>(null);
  let alive = true;
  const opened = ReplayRenderer.openPreview(props.path);
  preview.set(opened);
  void (async () => {
    while (alive) {
      const next = await ReplayRenderer.nextPreviewChange(opened.id, preview()!.revision);
      if (alive) preview.set(next);
    }
  })();
  cleanup(() => {
    alive = false;
    ReplayRenderer.closePreview(opened.id);
  });
  const size = () => props.size ?? 384;
  // The map's square stays when there is no map to draw, with the reason in
  // it, so the popup keeps its size.
  const failure = () => (preview()!.expecting ? null : preview()!.failure);
  return (
    <View accessibilityLabel="Minimap preview" style={{ width: size(), height: size(), backgroundColor: "#06272e", borderRadius: 4, overflow: "hidden" }}>
      <If
        when={failure()}
        else={
          <>
            <Frame surface={preview()!.surface} accessibilityLabel="Minimap preview map"/>
            <If when={preview()!.stalled}>
              <View style={{ position: "absolute", left: 0, right: 0, bottom: 8, alignItems: "center" }}>
                <View style={{ width: 80 }}><Progress value={0} indeterminate accessibilityLabel="Preview loading"/></View>
              </View>
            </If>
          </>
        }
      >
        <View accessibilityLabel={failure()!} style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 8 }}>
          <Text style={{ width: "100%", fontSize: 12, color: colors.mutedForeground }}>{failure()}</Text>
        </View>
      </If>
    </View>
  );
}

// A battle another end of the session is playing: drawn from the frames it
// sends, with where it has reached.
export function WatchedPlaybackView(props: { replayId: string }) {
  const watched = signal<WatchedView | null>(null);
  const failure = signal<string | null>(null);
  const labels = text();
  let alive = true;
  try {
    const opened = ReplayRenderer.openWatched(props.replayId);
    watched.set(opened);
    void (async () => {
      while (alive) {
        const next = await ReplayRenderer.nextWatchedChange(opened.id, watched()!.revision);
        if (alive) watched.set(next);
      }
    })();
    cleanup(() => {
      alive = false;
      ReplayRenderer.closeWatched(opened.id);
    });
  } catch (error) {
    failure.set(errorMessage(error));
  }
  return (
    <View accessibilityLabel="Watched playback" style={{ flex: 1, minHeight: 0 }}>
      <If when={failure()}>
        <Text style={{ padding: 12, fontSize: 13, color: colors.destructive }}>{failure()}</Text>
      </If>
      <If when={watched()}>
        <View style={{ borderBottomWidth: 1, borderBottomColor: colors.border }}>
          <If when={watched()!.clock}>
            <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, paddingVertical: 4 }}>
              <Label text={labels.watching}/>
              <Label text={watched()!.clock!}/>
              <If when={watched()!.through != null}>
                <View style={{ flex: 1, minWidth: 0, height: 3, borderRadius: 2, backgroundColor: colors.border }}>
                  <View style={{ height: 3, borderRadius: 2, width: `${watched()!.through! * 100}%`, backgroundColor: colors.primary }}/>
                </View>
              </If>
            </View>
          </If>
        </View>
        <View style={{ flex: 1, minHeight: 0, backgroundColor: "#141923" }}>
          <Frame surface={watched()!.surface} accessibilityLabel={t("ui.renderer.controls.map")}/>
          <If when={!watched()!.drawn}>
            <View style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center" }}>
              <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{labels.waitingForFrames}</Text>
            </View>
          </If>
        </View>
      </If>
    </View>
  );
}

const batch = signal<BatchView | null>(null);
let followingBatch = false;

async function followBatch() {
  if (followingBatch) return;
  followingBatch = true;
  batch.set(ReplayRenderer.batch());
  for (;;) {
    batch.set(await ReplayRenderer.nextBatchChange(batch()?.revision ?? 0));
  }
}

// Renders each replay in `paths` to its own video, into a folder the reader
// picks or onto the clipboard. Progress shows in `BatchRenderStatus`.
export async function renderReplaysToVideo(paths: string[], toClipboard: boolean): Promise<string | null> {
  void followBatch();
  try {
    await ReplayRenderer.renderMany(paths, toClipboard);
    return null;
  } catch (error) {
    return errorMessage(error);
  }
}

// The batch render's progress line, for the app's status area.
export function BatchRenderStatus() {
  void followBatch();
  return (
    <If when={batch()?.text}>
      <View accessibilityLabel="Batch render" style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <If when={batch()!.running}>
          <View style={{ width: 48 }}><Progress value={0} indeterminate accessibilityLabel={batch()!.text!}/></View>
        </If>
        <Text style={{ fontSize: 12, color: batch()!.failed ? colors.destructive : colors.mutedForeground }}>{batch()!.text}</Text>
      </View>
    </If>
  );
}

// Calls `open` with each session window the host asks every peer to open.
export async function followForcedWindows(open: (replayId: string) => void) {
  for (;;) {
    const asked = await ReplayRenderer.nextForcedWindows();
    if (asked.length === 0) return;
    for (const replayId of asked) open(replayId);
  }
}
