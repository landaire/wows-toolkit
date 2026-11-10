import type { JSX } from "nsh";
import { keymap, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, Checkbox, Combobox, Dialog, Input, Popover, Tooltip } from "nsh/base";
import { Menu } from "nsh/desktop";
import { Tactics, type RangeName, type TacticsView, type ToolName } from "wows-toolkit/native";
import { Button, Surface } from "./components";
import { Frame } from "./frame";
import { colors, controlRadius } from "./theme";

const SURFACE = "tactics-board";

const boardOpen = signal(false);
const board = signal<TacticsView | null>(null);
const boardError = signal<string | null>(null);
const notice = signal<string | null>(null);
const presetName = signal("");
let following = false;
let lastNotice = 0;

type ToolEntry = { name: ToolName; label: string; short: string; shortcut: string };

// In the order the egui board takes them up.
const TOOLS: ToolEntry[] = [
  { name: "freehand", label: "Freehand", short: "Draw", shortcut: "2" },
  { name: "line", label: "Line", short: "Line", shortcut: "4" },
  { name: "arrow", label: "Arrow", short: "Arrow", shortcut: "1" },
  { name: "circle", label: "Circle", short: "Circle", shortcut: "5" },
  { name: "rectangle", label: "Rectangle", short: "Rect", shortcut: "6" },
  { name: "triangle", label: "Triangle", short: "Tri", shortcut: "7" },
  { name: "measurement", label: "Measure", short: "Measure", shortcut: "m" },
  { name: "eraser", label: "Eraser", short: "Erase", shortcut: "3" },
];

const INKS = [
  { color: "#ffd73a", label: "Yellow ink" },
  { color: "#e8737b", label: "Red ink" },
  { color: "#6fd98a", label: "Green ink" },
  { color: "#7fb4e8", label: "Blue ink" },
  { color: "#e9e5dd", label: "White ink" },
  { color: "#1a1a18", label: "Black ink" },
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

export function openTacticsBoard() {
  boardOpen.set(true);
  boardError.set(null);
  void Tactics.open()
    .then((view) => {
      adopt(view);
      void follow();
    })
    .catch((error) => boardError.set(`The tactics board could not be opened: ${errorMessage(error)}`));
}

function closeTacticsBoard() {
  Tactics.close();
  boardOpen.set(false);
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
      empty={<Text style={{ padding: 10, fontSize: 12, color: colors.mutedForeground }}>Nothing matches.</Text>}
      popupStyle={{ width: props.width + 80, maxHeight: 360, padding: 4, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}
      itemClass="rounded-md px-2 py-1.5 text-[12.5px] text-popover-foreground focus:bg-accent selected:font-semibold"
      inputStyle={{ flex: 1, fontSize: 12.5, color: colors.foreground, placeholderColor: colors.mutedForeground }}
      triggerButton={<Text style={{ paddingHorizontal: 4, fontSize: 11, color: colors.mutedForeground }}>v</Text>}
      style={{ width: props.width, height: 28, paddingHorizontal: 8, flexDirection: "row", alignItems: "center", borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background }}
    />
  );
}

function MapPickers(props: { view: TacticsView }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Label text="Map"/>
      <Picker
        label="Map"
        placeholder="Find a map"
        width={280}
        items={props.view.maps}
        value={props.view.map?.value ?? null}
        onSelect={(value) => run(() => Tactics.setMap(value))}
      />
      <If when={props.view.map}>
        <Divider/>
        <Label text="Mode"/>
        <Picker
          label="Mode"
          placeholder="Mode"
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
          accessibilityLabel="Find a ship"
          placeholder="Find a ship"
          disabled={props.view.locked}
          value={props.view.shipQuery}
          onChange={({ value }) => run(() => Tactics.searchShips(value))}
          style={{ width: 180, minHeight: 28, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
        />
        <If when={props.view.placing}>
          <Label text={`Placing ${props.view.placing!.name}`}/>
          <ToolButton
            label={props.view.placing!.friendly ? "Friendly" : "Enemy"}
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
        <View accessibilityLabel="Ranges" hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, opacity: props.view.locked ? 0.45 : 1 }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>{`Ranges (${props.view.rangesActive})`}</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 200, padding: 8, gap: 6, backgroundColor: colors.popover }}>
        <View style={{ flexDirection: "row", gap: 4 }}>
          <ToolButton label="Enable All" disabled={props.view.locked || props.view.rangesAllOn} onPress={() => run(() => Tactics.setAllRangeCircles(true))}/>
          <ToolButton label="Disable All" disabled={props.view.locked || props.view.rangesAllOff} onPress={() => run(() => Tactics.setAllRangeCircles(false))}/>
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
                indicator={<Text class="text-[10px] text-primary-foreground">{range().checked ? "x" : ""}</Text>}
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
        <View accessibilityLabel="Saved boards" hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>
            {props.view.presets.length > 0 ? `Saved boards (${props.view.presets.length})` : "Saved boards"}
          </Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 300, padding: 8, gap: 8, backgroundColor: colors.popover }}>
        <ToolButton
          label="Reset board"
          hint="Restore caps from the selected mode and clear drawings"
          disabled={!props.view.maySteer || props.view.locked}
          onPress={() => run(() => Tactics.resetBoard())}
        />
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Input
            accessibilityLabel="Board name"
            placeholder="Board name"
            value={presetName()}
            onChange={({ value }) => presetName.set(value)}
            style={{ width: 180, minHeight: 28, paddingHorizontal: 8, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background, color: colors.foreground, fontSize: 12.5 }}
          />
          <ToolButton label="Save" disabled={!canSave()} onPress={() => run(() => Tactics.savePreset(presetName()))}/>
        </View>
        <If when={props.view.presets.length > 0}>
          <View style={{ height: 1, backgroundColor: colors.border }}/>
          <Scroll style={{ maxHeight: 320 }}>
            <View style={{ gap: 4 }}>
              <Each of={props.view.presets}>
                {(name) => (
                  <View role="row" accessibilityLabel={`Saved board ${name()}`} style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                    <Text style={{ flex: 1, fontSize: 12.5, color: colors.popoverForeground }}>{name()}</Text>
                    <ToolButton label="Open" accessibilityLabel={`Open ${name()}`} disabled={props.view.locked} onPress={() => run(() => Tactics.loadPreset(name()))}/>
                    <ToolButton label="Delete" accessibilityLabel={`Delete ${name()}`} onPress={() => run(() => Tactics.deletePreset(name()))}/>
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
          label="Populate caps from replays"
          hint="Read the replays for the capture layouts of the modes they were played in"
          onPress={() => run(() => Tactics.scanReplays())}
        />
      }
    >
      <Label text={props.view.scanning?.total != null
        ? `Reading replays... ${props.view.scanning!.read} / ${props.view.scanning!.total}`
        : "Reading replays..."}/>
    </If>
  );
}

function CapTools(props: { view: TacticsView }) {
  const cap = () => props.view.selectedCap;
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
      <ToolButton
        label="Add cap"
        hint="Place a capture point with the next map click"
        chosen={props.view.adding}
        onPress={() => run(() => Tactics.setAdding(!props.view.adding))}
      />
      <If when={cap()}>
        <Label text={`Cap ${cap()!.letter}`}/>
        <Label text={`${cap()!.radiusKm} km`}/>
        <ToolButton label="-" accessibilityLabel="Decrease the selected capture point radius" hint="Decrease the selected capture point radius" disabled={!cap()!.canNarrow} onPress={() => run(() => Tactics.narrowCap())}/>
        <ToolButton label="+" accessibilityLabel="Increase the selected capture point radius" hint="Increase the selected capture point radius" disabled={!cap()!.canWiden} onPress={() => run(() => Tactics.widenCap())}/>
        <Label text="Team"/>
        <ToolButton
          label={cap()!.team}
          accessibilityLabel={`Team: ${cap()!.team}`}
          hint="Cycle the selected capture point's team"
          onPress={() => run(() => Tactics.cycleCapTeam())}
        >
          <View style={{ width: 10, height: 10, borderRadius: 5, borderWidth: 1, borderColor: colors.border, backgroundColor: cap()!.teamColor }}/>
        </ToolButton>
        <ToolButton
          label="Delete"
          accessibilityLabel="Delete cap"
          hint={cap()!.frozen ? "A capture point loaded from a replay cannot be deleted" : "Remove the selected capture point"}
          disabled={cap()!.frozen}
          onPress={() => run(() => Tactics.deleteCap())}
        />
      </If>
      <If when={props.view.hasRemovableCaps}>
        <ToolButton label="Clear added caps" hint="Remove user-added capture points" onPress={() => run(() => Tactics.clearCaps())}/>
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
        <View accessibilityLabel="Keyboard Shortcuts" hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border }}>
          <Text style={{ fontSize: 12, color: colors.foreground }}>?</Text>
        </View>
      }
    >
      <Surface style={{ minWidth: 240, padding: 8, gap: 4, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.popoverForeground }}>Keyboard Shortcuts</Text>
        <Label text="Tools"/>
        <Each of={TOOLS}>{(tool) => row(keymap.label(`mod+${tool().shortcut}`), tool().label)}</Each>
        <Label text="Actions"/>
        {row(keymap.label("mod+z"), "Undo")}
        {row(`${keymap.label("mod+y")} / ${keymap.label("mod+shift+z")}`, "Redo")}
        {row(`${keymap.label("ctrl")}+Click`, "Multi-select")}
        {row("[ / ]", "Stroke width")}
        {row("Del", "Delete")}
        {row("Esc", "Cancel / Deselect")}
      </Surface>
    </Popover>
  );
}

function DrawTools(props: { view: TacticsView }) {
  const locked = () => props.view.locked;
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, flexWrap: "wrap" }}>
      <If when={locked()}>
        <Label text="Drawing is locked"/>
      </If>
      <Each of={TOOLS}>
        {(tool) => (
          <ToolButton
            label={tool().short}
            accessibilityLabel={tool().label}
            hint={`${tool().label} (${keymap.label(`mod+${tool().shortcut}`)})`}
            chosen={props.view.tool === tool().name}
            disabled={locked()}
            onPress={() => run(() => Tactics.setTool(tool().name))}
          />
        )}
      </Each>
      <Divider/>
      <Each of={INKS}>
        {(ink, index) => (
          <ToolButton label="" accessibilityLabel={ink().label} hint={ink().label} chosen={props.view.ink === index()} disabled={locked()} onPress={() => run(() => Tactics.setInk(index()))}>
            <View style={{ width: 14, height: 14, borderRadius: 7, borderWidth: 1, borderColor: "#171715", backgroundColor: ink().color }}/>
          </ToolButton>
        )}
      </Each>
      <Divider/>
      <Label text="Stroke width"/>
      <ToolButton label="-" accessibilityLabel="Thinner stroke" hint="Thinner stroke" disabled={!props.view.canThin} onPress={() => run(() => Tactics.stepNib(-1))}/>
      <Label text={props.view.nib.toFixed(0)}/>
      <ToolButton label="+" accessibilityLabel="Thicker stroke" hint="Thicker stroke" disabled={!props.view.canThicken} onPress={() => run(() => Tactics.stepNib(1))}/>
      <Divider/>
      <Shortcuts/>
      <Divider/>
      <ToolButton label="Undo" hint={`Undo the last board change (${keymap.label("mod+z")})`} disabled={!props.view.canUndo} onPress={() => run(() => Tactics.undo())}/>
      <ToolButton label="Redo" hint={`Redo the last undone board change (${keymap.label("mod+y")} or ${keymap.label("mod+shift+z")})`} disabled={!props.view.canRedo} onPress={() => run(() => Tactics.redo())}/>
      <If when={props.view.hasDrawing}>
        <ToolButton label="Clear drawing" disabled={locked()} onPress={() => run(() => Tactics.clearDrawing())}/>
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
        <Text style={{ paddingBottom: 4, fontSize: 11.5, color: colors.mutedForeground }}>A board shared by someone else in this session</Text>
      </If>
    </View>
  );
}

function statusText(view: TacticsView): string | null {
  switch (view.status) {
    case "pickAMap": return "Pick a map to draw on";
    case "noGameData": return "Load a game build first";
    case "noArt": return "This build ships no art for that map";
    case "drawing": return "Drawing the map...";
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
    { label: "Reset view", onSelect: () => run(() => Tactics.resetView()) },
  ];
  if (!view.locked) {
    items.push({ type: "separator" });
    for (const tool of TOOLS) {
      items.push({ label: `${tool.label} (${keymap.label(`mod+${tool.shortcut}`)})`, onSelect: () => run(() => Tactics.setTool(tool.name)) });
    }
  }
  const cap = view.selectedCap;
  if (view.canEditCaps && (cap || view.hasMapInfo)) {
    items.push({ type: "separator" });
    if (cap) {
      items.push({ label: `Team: ${cap.team}`, onSelect: () => run(() => Tactics.cycleCapTeam()) });
      if (cap.canNarrow) items.push({ label: "Decrease the selected capture point radius", onSelect: () => run(() => Tactics.narrowCap()) });
      if (cap.canWiden) items.push({ label: "Increase the selected capture point radius", onSelect: () => run(() => Tactics.widenCap()) });
      if (!cap.frozen) items.push({ label: "Delete", onSelect: () => run(() => Tactics.deleteCap()) });
    }
    if (view.hasMapInfo) items.push({ label: "Add cap", onSelect: () => run(() => Tactics.setAdding(true)) });
  }
  if (view.canUndo || view.canRedo || (view.hasDrawing && !view.locked)) items.push({ type: "separator" });
  if (view.canUndo) items.push({ label: "Undo", onSelect: () => run(() => Tactics.undo()) });
  if (view.canRedo) items.push({ label: "Redo", onSelect: () => run(() => Tactics.redo()) });
  if (view.hasDrawing && !view.locked) items.push({ label: "Clear drawing", onSelect: () => run(() => Tactics.clearDrawing()) });
  void Menu.popup(items);
}

function MapArea(props: { view: TacticsView }) {
  const status = () => statusText(props.view);
  const aiming = () => props.view.tool !== null || props.view.adding;
  return (
    <View
      accessibilityLabel="Tactics map area"
      onContextMenu={(event) => openContextMenu(event.position)}
      style={{ flex: 1, minHeight: 0, backgroundColor: "#171715", cursor: aiming() ? "crosshair" : "grab" }}
    >
      <Frame surface={SURFACE} accessibilityLabel="Tactics map"/>
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
    "mod+m": shortcut("measurement"),
    escape: () => run(() => Tactics.setTool(null)),
    "[": () => { if (hasTool()) run(() => Tactics.stepNib(-1)); },
    "]": () => { if (hasTool()) run(() => Tactics.stepNib(1)); },
    delete: () => run(() => Tactics.deletePicked()),
    backspace: () => run(() => Tactics.deletePicked()),
  });
  return <View/>;
}

export function TacticsBoard(props: { title: string }) {
  return (
    <Dialog
      open={boardOpen()}
      closeOnEscape={false}
      animation="fade"
      onOpenChange={({ open }) => {
        if (!open) closeTacticsBoard();
      }}
      style={{ flex: 1, backgroundColor: colors.background }}
    >
      <View style={{ flex: 1, minHeight: 0 }}>
        <BoardKeys/>
        <View style={{ minHeight: 40, paddingHorizontal: 12, flexDirection: "row", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.card }}>
          <Text style={{ flex: 1, fontSize: 15, fontWeight: "600", color: colors.foreground }}>
            {board()?.map ? `${props.title}: ${board()!.map!.title}` : props.title}
          </Text>
          <Button label="Close" variant="ghost" onPress={closeTacticsBoard}/>
        </View>
        <If when={boardError()}>
          <Text style={{ padding: 12, fontSize: 13, color: colors.destructive }}>{boardError()}</Text>
        </If>
        <If when={board()} else={<Text style={{ padding: 18, fontSize: 13, color: colors.mutedForeground }}>Opening the board...</Text>}>
          <Toolbar view={board()!}/>
          <MapArea view={board()!}/>
        </If>
        <If when={notice()}>
          <Surface style={{ position: "absolute", right: 16, bottom: 16, minWidth: 260, maxWidth: 420, padding: 12, backgroundColor: colors.popover }}>
            <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{notice()}</Text>
          </Surface>
        </If>
      </View>
    </Dialog>
  );
}
