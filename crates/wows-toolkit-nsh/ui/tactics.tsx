import type { JSX } from "nsh";
import { cleanup, keymap, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, Checkbox, Combobox, Input, Popover, Tooltip } from "nsh/base";
import { Menu } from "nsh/desktop";
import { Tactics, type RangeName, type TacticsView, type ToolName } from "wows-toolkit/native";
import { Surface } from "./components";
import { Frame } from "./frame";
import { t } from "./i18n";
import { CheckMark, Icon, type IconName } from "./icons";
import { colors, controlRadius } from "./theme";

const SURFACE = "tactics-board";

const board = signal<TacticsView | null>(null);
const boardError = signal<string | null>(null);
const notice = signal<string | null>(null);
const presetName = signal("");
let following = false;
let lastNotice = 0;

type ToolEntry = { name: ToolName; icon: IconName; key: string; chord: string };

// The measurement chord: Ctrl+M, which is the command key elsewhere, since
// macOS keeps Cmd+M for minimizing the window.
const MEASURE_CHORD = "ctrl+m";

// In the order the egui board takes them up.
const TOOLS: ToolEntry[] = [
  { name: "freehand", icon: "paintbrush", key: "ui.renderer.annotations.tool_freehand", chord: "mod+2" },
  { name: "line", icon: "slash", key: "ui.renderer.annotations.tool_line", chord: "mod+4" },
  { name: "arrow", icon: "move-up-right", key: "ui.renderer.annotations.tool_arrow", chord: "mod+1" },
  { name: "circle", icon: "circle", key: "ui.renderer.annotations.tool_circle", chord: "mod+5" },
  { name: "rectangle", icon: "square", key: "ui.renderer.annotations.tool_rectangle", chord: "mod+6" },
  { name: "triangle", icon: "triangle", key: "ui.renderer.annotations.tool_triangle", chord: "mod+7" },
  { name: "measurement", icon: "ruler", key: "ui.renderer.annotations.tool_measurement", chord: MEASURE_CHORD },
  { name: "eraser", icon: "eraser", key: "ui.renderer.annotations.tool_eraser", chord: "mod+3" },
];

const INKS = [
  { color: "#ffd73a", key: "ui.tactics.ink_yellow" },
  { color: "#e8737b", key: "ui.tactics.ink_red" },
  { color: "#6fd98a", key: "ui.tactics.ink_green" },
  { color: "#7fb4e8", key: "ui.tactics.ink_blue" },
  { color: "#e9e5dd", key: "ui.tactics.ink_white" },
  { color: "#1a1a18", key: "ui.tactics.ink_black" },
];

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function say(message: string) {
  notice.set(message);
  setTimeout(() => {
    if (notice() === message) notice.set(null);
  }, 4000);
}

function adopt(view: TacticsView) {
  const current = board();
  // A command's answer can arrive after a newer change was heard.
  if (current && view.revision < current.revision) return;
  board.set(view);
  if (view.notice && view.notice.id !== lastNotice) {
    lastNotice = view.notice.id;
    say(view.notice.text);
  }
}

// Runs a board command and shows what the board then reads; a refusal is
// said rather than thrown.
function run(command: () => TacticsView) {
  try {
    adopt(command());
  } catch (error) {
    say(errorMessage(error));
  }
}

async function follow() {
  if (following) return;
  following = true;
  for (;;) {
    const next = await Tactics.nextChange(board()?.revision ?? 0);
    adopt(next);
  }
}

function openBoard() {
  boardError.set(null);
  void Tactics.open()
    .then((view) => {
      adopt(view);
      void follow();
    })
    .catch((error) => boardError.set(t("ui.tactics.open_failed", { error: errorMessage(error) })));
}

function Divider() {
  return <View style={{ width: 1, alignSelf: "stretch", marginVertical: 4, backgroundColor: colors.border }}/>;
}

function Label(props: { text: string }) {
  return <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.text}</Text>;
}

// A compact toolbar button that can read as chosen, with a hint on hover.
function ToolButton(props: {
  label: string;
  accessibilityLabel?: string;
  hint?: string;
  chosen?: boolean;
  disabled?: boolean;
  children?: JSX.Element;
  onPress: () => void;
}) {
  return (
    <BaseButton
      accessibilityLabel={props.accessibilityLabel ?? props.label}
      accessibilityState={{ selected: props.chosen ?? false }}
      disabled={props.disabled}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.68 }}
      tooltip={props.hint ? (
        <Tooltip placement="bottom" style={{ paddingHorizontal: 8, paddingVertical: 4, borderRadius: 6, backgroundColor: colors.popover, borderWidth: 1, borderColor: colors.border }}>
          <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{props.hint}</Text>
        </Tooltip>
      ) : undefined}
      style={{
        minHeight: 26,
        paddingHorizontal: 8,
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
      <If when={props.label}>
        <Text style={{ fontSize: 12, color: props.chosen ? colors.primaryForeground : colors.secondaryForeground }}>{props.label}</Text>
      </If>
    </BaseButton>
  );
}

function Picker(props: {
  label: string;
  placeholder: string;
  width: number;
  items: { value: string; title: string }[];
  value: string | null;
  onSelect: (value: string) => void;
}) {
  return (
    <Combobox
      accessibilityLabel={props.label}
      items={props.items}
      value={props.value ? [props.value] : []}
      placeholder={props.placeholder}
      onChange={({ value }) => {
        if (value.length > 0) props.onSelect(value[value.length - 1]);
      }}
      offset={4}
      empty={<Text style={{ padding: 10, fontSize: 12, color: colors.mutedForeground }}>{t("ui.tactics.no_matches")}</Text>}
      popupStyle={{ width: props.width + 80, maxHeight: 360, padding: 4, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}
      itemClass="rounded-md px-2 py-1.5 text-[12.5px] text-popover-foreground focus:bg-accent selected:font-semibold"
      inputStyle={{ flex: 1, fontSize: 12.5, color: colors.foreground, placeholderColor: colors.mutedForeground }}
      triggerButton={<View style={{ paddingHorizontal: 4 }}><Icon name="chevron-down" size={14} color={colors.mutedForeground}/></View>}
      style={{ width: props.width, height: 28, paddingHorizontal: 8, flexDirection: "row", alignItems: "center", borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background }}
    />
  );
}

function MapPickers(props: { view: TacticsView }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Label text={t("ui.tactics.map")}/>
      <Picker
        label={t("ui.tactics.map")}
        placeholder={t("ui.tactics.map_hint")}
        width={280}
        items={props.view.maps}
        value={props.view.map?.value ?? null}
        onSelect={(value) => run(() => Tactics.setMap(value))}
      />
      <If when={props.view.map}>
        <Divider/>
        <Label text={t("ui.tactics.mode")}/>
        <Picker
          label={t("ui.tactics.mode")}
          placeholder={t("ui.tactics.mode")}
          width={240}
          items={props.view.modes}
          value={props.view.mode.value}
          onSelect={(value) => run(() => Tactics.setMode(value))}
        />
      </If>
    </View>
  );
}

function ShipPicker(props: { view: TacticsView }) {
  return (
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <Input
          accessibilityLabel={t("ui.renderer.annotations.ship_hint")}
          placeholder={t("ui.renderer.annotations.ship_hint")}
          disabled={props.view.locked}
          value={props.view.shipQuery}
          onChange={({ value }) => run(() => Tactics.searchShips(value))}
          style={{ width: 180, minHeight: 28, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
        />
        <If when={props.view.placing}>
          <Label text={t("ui.tactics.placing", { ship: props.view.placing!.name })}/>
          <ToolButton
            label={t(props.view.placing!.friendly ? "ui.tactics.side_friendly" : "ui.tactics.side_enemy")}
            disabled={props.view.locked}
            onPress={() => run(() => Tactics.setPlacingFriendly(!props.view.placing!.friendly))}
          />
        </If>
      </View>
      <If when={props.view.shipMatches.length > 0}>
        <Scroll style={{ maxWidth: 360, maxHeight: 144 }}>
          <View style={{ gap: 2 }}>
            <Each of={props.view.shipMatches}>
              {(ship) => (
                <ToolButton label={ship().label} disabled={props.view.locked} onPress={() => run(() => Tactics.pickShip(ship().index))}/>
              )}
            </Each>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function RangeCircles(props: { view: TacticsView }) {
  return (
    <Popover
      offset={4}
      margin={8}
      trigger={
        <View accessibilityLabel={t("ui.renderer.context.ranges")} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, opacity: props.view.locked ? 0.45 : 1 }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>{`${t("ui.renderer.context.ranges")} (${props.view.rangesActive})`}</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 200, padding: 8, gap: 6, backgroundColor: colors.popover }}>
        <View style={{ flexDirection: "row", gap: 4 }}>
          <ToolButton label={t("ui.renderer.context.enable_all")} disabled={props.view.locked || props.view.rangesAllOn} onPress={() => run(() => Tactics.setAllRangeCircles(true))}/>
          <ToolButton label={t("ui.renderer.context.disable_all")} disabled={props.view.locked || props.view.rangesAllOff} onPress={() => run(() => Tactics.setAllRangeCircles(false))}/>
        </View>
        <Each of={props.view.ranges}>
          {(range) => {
            const label = () => range().enabled > 0 && range().enabled < range().total
              ? `${range().label} (${range().enabled}/${range().total})`
              : range().label;
            return (
              <Checkbox
                accessibilityLabel={range().label}
                checked={range().checked}
                disabled={props.view.locked}
                class="group flex-row items-center gap-2"
                indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
                indicator={<CheckMark checked={range().checked}/>}
                onChange={({ checked }) => run(() => Tactics.setRangeCircle(range().circle as RangeName, checked))}
              >
                <Text style={{ fontSize: 12.5, color: colors.popoverForeground }}>{label()}</Text>
              </Checkbox>
            );
          }}
        </Each>
      </Surface>
    </Popover>
  );
}

function Presets(props: { view: TacticsView }) {
  const canSave = () => props.view.maySteer && props.view.map !== null && presetName().trim() !== "";
  return (
    <Popover
      offset={4}
      margin={8}
      trigger={
        <View accessibilityLabel={t("ui.tactics.presets")} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>
            {props.view.presets.length > 0 ? `${t("ui.tactics.presets")} (${props.view.presets.length})` : t("ui.tactics.presets")}
          </Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 300, padding: 8, gap: 8, backgroundColor: colors.popover }}>
        <ToolButton
          label={t("ui.tactics.reset_board")}
          hint={t("ui.tactics.reset_board_tooltip")}
          disabled={!props.view.maySteer || props.view.locked}
          onPress={() => run(() => Tactics.resetBoard())}
        />
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Input
            accessibilityLabel={t("ui.tactics.preset_name")}
            placeholder={t("ui.tactics.preset_name")}
            value={presetName()}
            onChange={({ value }) => presetName.set(value)}
            style={{ width: 180, minHeight: 28, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
          />
          <ToolButton label={t("ui.tactics.preset_save")} disabled={!canSave()} onPress={() => run(() => Tactics.savePreset(presetName()))}/>
        </View>
        <If when={props.view.presets.length > 0}>
          <View style={{ height: 1, backgroundColor: colors.border }}/>
          <Scroll style={{ maxHeight: 320 }}>
            <View style={{ gap: 4 }}>
              <Each of={props.view.presets}>
                {(name) => (
                  <View role="row" accessibilityLabel={`${t("ui.tactics.presets")}: ${name()}`} style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                    <Text style={{ flex: 1, fontSize: 12.5, color: colors.popoverForeground }}>{name()}</Text>
                    <ToolButton label={t("ui.tactics.preset_open")} accessibilityLabel={`${t("ui.tactics.preset_open")} ${name()}`} disabled={props.view.locked} onPress={() => run(() => Tactics.loadPreset(name()))}/>
                    <ToolButton label={t("ui.tactics.preset_delete")} accessibilityLabel={`${t("ui.tactics.preset_delete")} ${name()}`} onPress={() => run(() => Tactics.deletePreset(name()))}/>
                  </View>
                )}
              </Each>
            </View>
          </Scroll>
        </If>
      </Surface>
    </Popover>
  );
}

function Scan(props: { view: TacticsView }) {
  return (
    <If
      when={props.view.scanning}
      else={
        <ToolButton
          label={t("ui.tactics.scan_replays")}
          hint={t("ui.tactics.scan_replays_tooltip")}
          onPress={() => run(() => Tactics.scanReplays())}
        />
      }
    >
      <Label text={props.view.scanning?.total != null
        ? t("ui.tactics.scan_progress", { read: props.view.scanning!.read, total: props.view.scanning!.total })
        : t("ui.tactics.scan_running")}/>
    </If>
  );
}

function CapTools(props: { view: TacticsView }) {
  const cap = () => props.view.selectedCap;
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
      <ToolButton
        label={t("ui.tactics.add_cap")}
        hint={t("ui.tactics.add_cap_tooltip")}
        chosen={props.view.adding}
        onPress={() => run(() => Tactics.setAdding(!props.view.adding))}
      />
      <If when={cap()}>
        <Label text={t("ui.tactics.selected_cap", { letter: cap()!.letter })}/>
        <Label text={t("ui.tactics.cap_radius", { km: cap()!.radiusKm })}/>
        <ToolButton label="-" accessibilityLabel={t("ui.tactics.cap_narrow_tooltip")} hint={t("ui.tactics.cap_narrow_tooltip")} disabled={!cap()!.canNarrow} onPress={() => run(() => Tactics.narrowCap())}/>
        <ToolButton label="+" accessibilityLabel={t("ui.tactics.cap_widen_tooltip")} hint={t("ui.tactics.cap_widen_tooltip")} disabled={!cap()!.canWiden} onPress={() => run(() => Tactics.widenCap())}/>
        <Label text={t("ui.tactics.cap_team")}/>
        <ToolButton
          label={cap()!.team}
          accessibilityLabel={`${t("ui.tactics.cap_team")}: ${cap()!.team}`}
          hint={t("ui.tactics.cap_team_tooltip")}
          onPress={() => run(() => Tactics.cycleCapTeam())}
        >
          <View style={{ width: 10, height: 10, borderRadius: 5, borderWidth: 1, borderColor: colors.border, backgroundColor: cap()!.teamColor }}/>
        </ToolButton>
        <ToolButton
          label={t("ui.tactics.delete_cap")}
          accessibilityLabel={t("ui.tactics.delete_cap_tooltip")}
          hint={t(cap()!.frozen ? "ui.tactics.cap_replay_delete_tooltip" : "ui.tactics.delete_cap_tooltip")}
          disabled={cap()!.frozen}
          onPress={() => run(() => Tactics.deleteCap())}
        />
      </If>
      <If when={props.view.hasRemovableCaps}>
        <ToolButton label={t("ui.tactics.clear_caps")} hint={t("ui.tactics.clear_added_caps_tooltip")} onPress={() => run(() => Tactics.clearCaps())}/>
      </If>
    </View>
  );
}

function Shortcuts() {
  const row = (chord: string, label: string) => (
    <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 12 }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{chord}</Text>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{label}</Text>
    </View>
  );
  return (
    <Popover
      offset={4}
      margin={8}
      trigger={
        <View accessibilityLabel={t("ui.renderer.annotations.shortcuts_title")} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>?</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 240, padding: 8, gap: 4, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.renderer.annotations.shortcuts_title")}</Text>
        <Label text={t("ui.renderer.annotations.shortcuts_tools")}/>
        <Each of={TOOLS}>{(tool) => row(keymap.label(tool().chord), t(tool().key))}</Each>
        <Label text={t("ui.renderer.annotations.shortcuts_actions")}/>
        {row(keymap.label("mod+z"), t("ui.renderer.annotations.action_undo"))}
        {row(`${keymap.label("mod+y")} / ${keymap.label("mod+shift+z")}`, t("ui.renderer.annotations.redo"))}
        {row(`${keymap.label("mod+z").replace(/\+?Z$/, "")}+Click`, t("ui.renderer.annotations.action_multi_select"))}
        {row("[ / ]", t("ui.renderer.annotations.action_stroke_width"))}
        {row(keymap.label("delete"), t("ui.renderer.annotations.action_delete"))}
        {row(keymap.label("escape"), t("ui.renderer.annotations.action_cancel"))}
      </Surface>
    </Popover>
  );
}

function DrawTools(props: { view: TacticsView }) {
  const locked = () => props.view.locked;
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
      <If when={locked()}>
        <Icon name="lock" size={14} color={colors.mutedForeground} accessibilityLabel={t("ui.tactics.drawing_locked")}/>
      </If>
      <Each of={TOOLS}>
        {(tool) => (
          <ToolButton
            label=""
            accessibilityLabel={t(tool().key)}
            hint={`${t(tool().key)} (${keymap.label(tool().chord)})`}
            chosen={props.view.tool === tool().name}
            disabled={locked()}
            onPress={() => run(() => Tactics.setTool(tool().name))}
          >
            <Icon
              name={tool().icon}
              color={props.view.tool === tool().name ? colors.primaryForeground : colors.secondaryForeground}
            />
          </ToolButton>
        )}
      </Each>
      <Divider/>
      <Each of={INKS}>
        {(ink, index) => (
          <ToolButton label="" accessibilityLabel={t(ink().key)} hint={t(ink().key)} chosen={props.view.ink === index()} disabled={locked()} onPress={() => run(() => Tactics.setInk(index()))}>
            <View style={{ width: 14, height: 14, borderRadius: 7, borderWidth: 1, borderColor: "#171715", backgroundColor: ink().color }}/>
          </ToolButton>
        )}
      </Each>
      <Divider/>
      <Label text={t("ui.renderer.annotations.nib")}/>
      <ToolButton label="-" accessibilityLabel={t("ui.renderer.annotations.thinner")} hint={t("ui.renderer.annotations.thinner")} disabled={!props.view.canThin} onPress={() => run(() => Tactics.stepNib(-1))}/>
      <Label text={props.view.nib.toFixed(0)}/>
      <ToolButton label="+" accessibilityLabel={t("ui.renderer.annotations.thicker")} hint={t("ui.renderer.annotations.thicker")} disabled={!props.view.canThicken} onPress={() => run(() => Tactics.stepNib(1))}/>
      <Divider/>
      <Shortcuts/>
      <Divider/>
      <ToolButton label={t("ui.renderer.annotations.undo")} hint={t("ui.tactics.undo_tooltip", { shortcut: keymap.label("mod+z") })} disabled={!props.view.canUndo} onPress={() => run(() => Tactics.undo())}/>
      <ToolButton label={t("ui.renderer.annotations.redo")} hint={t("ui.tactics.redo_tooltip", { shortcut: keymap.label("mod+y"), alternate: keymap.label("mod+shift+z") })} disabled={!props.view.canRedo} onPress={() => run(() => Tactics.redo())}/>
      <If when={props.view.hasDrawing}>
        <ToolButton label={t("ui.tactics.clear_drawing")} disabled={locked()} onPress={() => run(() => Tactics.clearDrawing())}/>
      </If>
    </View>
  );
}

function Toolbar(props: { view: TacticsView }) {
  return (
    <View style={{ paddingHorizontal: 10, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.card }}>
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 8, paddingVertical: 5 }}>
        <If when={props.view.maySteer}>
          <MapPickers view={props.view}/>
          <Divider/>
        </If>
        <ShipPicker view={props.view}/>
        <Divider/>
        <RangeCircles view={props.view}/>
        <If when={props.view.maySteer}>
          <Divider/>
          <Presets view={props.view}/>
          <Scan view={props.view}/>
        </If>
      </View>
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 10, paddingVertical: 5, borderTopWidth: 1, borderTopColor: colors.border }}>
        <If when={props.view.maySteer}>
          <CapTools view={props.view}/>
          <Divider/>
        </If>
        <DrawTools view={props.view}/>
      </View>
      <If when={props.view.aPeersBoard}>
        <Text style={{ paddingBottom: 4, fontSize: 11.5, color: colors.mutedForeground }}>{t("ui.tactics.a_peers_board")}</Text>
      </If>
    </View>
  );
}

function statusText(view: TacticsView): string | null {
  switch (view.status) {
    case "pickAMap": return t("ui.tactics.pick_a_map");
    case "noGameData": return t("ui.tactics.scan_needs_game_data");
    case "noArt": return t("ui.tactics.no_map_art");
    case "drawing": return t("ui.tactics.drawing");
    default: return null;
  }
}

function openContextMenu(position: { x: number; y: number }) {
  let view: TacticsView;
  try {
    view = Tactics.contextMenu(position.x, position.y);
  } catch (error) {
    say(errorMessage(error));
    return;
  }
  adopt(view);
  const items: Parameters<typeof Menu.popup>[0] = [
    { label: t("ui.tactics.reset_view"), onSelect: () => run(() => Tactics.resetView()) },
  ];
  if (!view.locked) {
    items.push({ type: "separator" });
    for (const tool of TOOLS) {
      items.push({ label: `${t(tool.key)} (${keymap.label(tool.chord)})`, onSelect: () => run(() => Tactics.setTool(tool.name)) });
    }
  }
  const cap = view.selectedCap;
  if (view.canEditCaps && (cap || view.hasMapInfo)) {
    items.push({ type: "separator" });
    if (cap) {
      items.push({ label: `${t("ui.tactics.cap_team")}: ${cap.team}`, onSelect: () => run(() => Tactics.cycleCapTeam()) });
      if (cap.canNarrow) items.push({ label: t("ui.tactics.cap_narrow_tooltip"), onSelect: () => run(() => Tactics.narrowCap()) });
      if (cap.canWiden) items.push({ label: t("ui.tactics.cap_widen_tooltip"), onSelect: () => run(() => Tactics.widenCap()) });
      if (!cap.frozen) items.push({ label: t("ui.tactics.delete_cap"), onSelect: () => run(() => Tactics.deleteCap()) });
    }
    if (view.hasMapInfo) items.push({ label: t("ui.tactics.add_cap"), onSelect: () => run(() => Tactics.setAdding(true)) });
  }
  if (view.canUndo || view.canRedo || (view.hasDrawing && !view.locked)) items.push({ type: "separator" });
  if (view.canUndo) items.push({ label: t("ui.renderer.annotations.undo"), onSelect: () => run(() => Tactics.undo()) });
  if (view.canRedo) items.push({ label: t("ui.renderer.annotations.redo"), onSelect: () => run(() => Tactics.redo()) });
  if (view.hasDrawing && !view.locked) items.push({ label: t("ui.tactics.clear_drawing"), onSelect: () => run(() => Tactics.clearDrawing()) });
  void Menu.popup(items);
}

function MapArea(props: { view: TacticsView }) {
  const status = () => statusText(props.view);
  const aiming = () => props.view.tool !== null || props.view.adding;
  return (
    <View
      accessibilityLabel={t("ui.windows.tactics_board")}
      onContextMenu={(event) => openContextMenu(event.position)}
      style={{ flex: 1, minHeight: 0, backgroundColor: "#171715", cursor: aiming() ? "crosshair" : "grab" }}
    >
      <Frame surface={SURFACE} accessibilityLabel={t("ui.tactics.map")}/>
      <If when={status()}>
        <View style={{ position: "absolute", left: 0, right: 0, top: 0, bottom: 0, alignItems: "center", justifyContent: "center", backgroundColor: colors.background }}>
          <Text style={{ fontSize: 14, color: colors.mutedForeground }}>{status()}</Text>
        </View>
      </If>
    </View>
  );
}

// The board's keyboard, the chords the egui board takes. Bound while the board
// is mounted, so they take the place of the app's own for as long as it is
// open; plain keys and undo leave a focused text field alone.
function BoardKeys() {
  const shortcut = (name: ToolName) => () => run(() => Tactics.shortcutTool(name));
  const hasTool = () => board()?.tool != null && !board()?.locked;
  keymap.bind({
    "mod+z": () => run(() => Tactics.undo()),
    "mod+y": () => run(() => Tactics.redo()),
    "mod+shift+z": () => run(() => Tactics.redo()),
    "mod+1": shortcut("arrow"),
    "mod+2": shortcut("freehand"),
    "mod+3": shortcut("eraser"),
    "mod+4": shortcut("line"),
    "mod+5": shortcut("circle"),
    "mod+6": shortcut("rectangle"),
    "mod+7": shortcut("triangle"),
    [MEASURE_CHORD]: shortcut("measurement"),
    escape: () => run(() => Tactics.setTool(null)),
    "[": () => { if (hasTool()) run(() => Tactics.stepNib(-1)); },
    "]": () => { if (hasTool()) run(() => Tactics.stepNib(1)); },
    delete: () => run(() => Tactics.deletePicked()),
    backspace: () => run(() => Tactics.deletePicked()),
  });
  return <View/>;
}

/** The board, filling the window it was opened in; it closes with the view. */
export function TacticsBoard() {
  openBoard();
  cleanup(() => Tactics.close());
  return (
    <View style={{ flex: 1, minHeight: 0, backgroundColor: colors.background }}>
      <BoardKeys/>
      <If when={boardError()}>
        <Text style={{ padding: 12, fontSize: 13, color: colors.destructive }}>{boardError()}</Text>
      </If>
      <If when={board()} else={<Text style={{ padding: 18, fontSize: 13, color: colors.mutedForeground }}>{t("ui.tactics.opening")}</Text>}>
        <Toolbar view={board()!}/>
        <MapArea view={board()!}/>
      </If>
      <If when={notice()}>
        <Surface style={{ position: "absolute", right: 16, bottom: 16, minWidth: 260, maxWidth: 420, padding: 12, backgroundColor: colors.popover }}>
          <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{notice()}</Text>
        </Surface>
      </If>
    </View>
  );
}
