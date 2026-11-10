import type { JSX } from "nsh";
import { cleanup, Image, keymap, Scroll, signal, Text, View } from "nsh";
import { Button as BaseButton, Checkbox, Combobox, Input, Popover, Tooltip } from "nsh/base";
import { Menu } from "nsh/desktop";
import { Tactics, type RangeName, type TacticsView, type ToolName } from "wows-toolkit/native";
import { Surface } from "./components";
import { Frame } from "./frame";
import { t } from "./i18n";
import { colors, controlRadius } from "./theme";

const SURFACE = "tactics-board";

const board = signal<TacticsView | null>(null);
const boardError = signal<string | null>(null);
const notice = signal<string | null>(null);
const presetName = signal("");
let following = false;
let lastNotice = 0;

type ToolEntry = { name: ToolName; key: string; chord: string };

// In the order the egui board takes them up. Measurement takes a plain key:
// nsh keeps `mod+m` for minimizing the window.
const TOOLS: ToolEntry[] = [
  { name: "freehand", key: "ui.renderer.annotations.tool_freehand", chord: "mod+2" },
  { name: "line", key: "ui.renderer.annotations.tool_line", chord: "mod+4" },
  { name: "arrow", key: "ui.renderer.annotations.tool_arrow", chord: "mod+1" },
  { name: "circle", key: "ui.renderer.annotations.tool_circle", chord: "mod+5" },
  { name: "rectangle", key: "ui.renderer.annotations.tool_rectangle", chord: "mod+6" },
  { name: "triangle", key: "ui.renderer.annotations.tool_triangle", chord: "mod+7" },
  { name: "measurement", key: "ui.renderer.annotations.tool_measurement", chord: "m" },
  { name: "eraser", key: "ui.renderer.annotations.tool_eraser", chord: "mod+3" },
];

// The Phosphor glyphs the GPUI board shows, as outlines in a 1024 unit em.
const TOOL_ICONS: Record<ToolName, string> = {
  freehand: "M928 128Q928 115 918.5 105.5Q909 96 896 96L896 96Q830 96 764 129Q699 162 639 212Q579 262 527 319Q476 377 438 427Q423 422 405 419Q387 416 368 416Q318 416 275 435Q231 454 198.5 486.5Q166 519 147 563Q128 606 128 656Q128 656 128 656Q128 656 128 656L128 656Q128 749 88.5 792.5Q49 836 46 837Q40 842 36 849Q32 856 32 864Q32 877 41.5 886.5Q51 896 64 896L368 896Q368 896 368 896Q368 896 368 896Q418 896 461 877Q505 858 537.5 825.5Q570 793 589 749Q608 706 608 656Q608 637 605 619Q602 601 597 584L597 586Q647 548 705 497Q763 445 812.5 385Q862 325 895 260Q928 194 928 128L928 128ZM368 832L139 832Q159 803 175.5 759.5Q192 716 192 656Q192 583 243.5 531.5Q295 480 368 480Q441 480 492.5 531.5Q544 583 544 656Q544 729 492.5 780.5Q441 832 368 832L368 832ZM498 454Q518 428 538 404Q562 421 582.5 441Q603 461 619 485L620 486Q596 506 570 526Q556 505 538 487Q520 469 499 455L498 454L498 454ZM668 443Q650 418 628.5 396Q607 374 582 356L580 355Q628 304 670 269Q712 234 747 212Q782 190 810 179Q839 168 860 164Q856 185 845 214Q834 242 812 277Q790 312 755 354Q720 396 668 443Z",
  line: "M859 165Q841 148 817.5 138Q794 128 768 128Q715 128 677.5 165.5Q640 203 640 256Q640 274 644.5 290.5Q649 307 658 322L658 321L321 658Q307 649 290.5 644.5Q274 640 256 640Q229 640 206 650Q183 660 165 678L165 678Q148 695 138 718.5Q128 742 128 768Q128 821 165.5 858.5Q203 896 256 896Q309 896 346.5 858.5Q384 821 384 768Q384 750 379.5 733.5Q375 717 366 702L366 703L703 366Q717 375 733.5 379.5Q750 384 768 384Q821 384 858.5 346.5Q896 309 896 256Q896 230 886 206.5Q876 183 859 165L859 165L859 165ZM301 813Q293 822 281 827Q269 832 256 832Q229 832 210.5 813.5Q192 795 192 768Q192 755 197 743.5Q202 732 211 723L211 723L211 723Q219 714 231 709Q243 704 256 704Q269 704 281 709Q293 714 301 723L301 723Q310 732 315 743.5Q320 755 320 768Q320 781 315 793Q310 805 301 813L301 813L301 813ZM813 301Q805 310 793 315Q781 320 768 320Q741 320 722.5 301.5Q704 283 704 256Q704 230 722.5 211Q741 192 768 192Q781 192 793 197Q805 202 813 211L813 211Q822 220 827 231.5Q832 243 832 256Q832 269 827 281Q822 293 813 301L813 301L813 301Z",
  arrow: "M919 439L727 631Q722 635 716.5 637.5Q711 640 704 640Q691 640 681.5 630.5Q672 621 672 608Q672 601 674.5 595.5Q677 590 681 585L819 448L512 448Q439 448 375 476Q311 503 263 551Q215 599 188 663Q160 727 160 800L160 800Q160 813 150.5 822.5Q141 832 128 832Q115 832 105.5 822.5Q96 813 96 800L96 800Q96 714 129 638Q162 562 218 506Q274 450 350 417Q426 384 512 384L819 384L681 247Q677 242 674.5 236.5Q672 231 672 224Q672 211 681.5 201.5Q691 192 704 192Q711 192 716.5 194.5Q722 197 727 201L727 201L919 393Q923 398 925.5 403.5Q928 409 928 416Q928 423 925.5 428.5Q923 434 919 439L919 439L919 439Z",
  circle: "M512 96Q426 96 350 129Q274 161 217.5 217.5Q161 274 129 350Q96 426 96 512Q96 598 129 674Q161 750 217.5 806.5Q274 863 350 895Q426 928 512 928Q598 928 674 895Q750 863 806.5 806.5Q863 750 895 674Q928 598 928 512L928 512Q928 426 895 350Q862 274 806 218Q750 162 674 129Q598 96 512 96L512 96ZM512 864Q439 864 375 836Q311 809 263 761Q215 713 188 649Q160 585 160 512Q160 439 188 375Q215 311 263 263Q311 215 375 188Q439 160 512 160Q585 160 649 188Q713 215 761 263Q809 311 836 375Q864 439 864 512L864 512Q864 585 836 649Q809 713 761 761Q713 809 649 836Q585 864 512 864L512 864L512 864Z",
  rectangle: "M832 128L192 128Q165 128 146.5 146.5Q128 165 128 192L128 192L128 832Q128 859 146.5 877.5Q165 896 192 896L192 896L832 896Q859 896 877.5 877.5Q896 859 896 832L896 832L896 192Q896 165 877.5 146.5Q859 128 832 128L832 128ZM832 832L192 832L192 192L832 192L832 832Z",
  triangle: "M947 752L597 145Q584 123 561.5 109.5Q539 96 512 96Q485 96 462.5 109.5Q440 123 427 144L427 145L77 752Q71 763 67.5 775Q64 787 64 800Q64 813 67.5 825Q71 837 77 848L77 847Q90 869 112 882.5Q134 896 161 896Q161 896 161.5 896Q162 896 162 896L862 896Q862 896 862.5 896Q863 896 863 896Q890 896 912 882.5Q934 869 947 848L947 847Q953 837 956.5 825Q960 813 960 800Q960 787 956.5 774.5Q953 762 947 752L947 752L947 752ZM892 815Q887 823 879.5 827.5Q872 832 862 832Q862 832 862 832Q862 832 862 832L162 832Q162 832 162 832Q162 832 162 832Q152 832 144.5 827.5Q137 823 132 815L132 815Q130 812 129 808Q128 804 128 800Q128 795 129 791.5Q130 788 132 784L132 784L482 177Q487 169 495 164.5Q503 160 512 160Q522 160 529.5 164.5Q537 169 542 177L542 177L892 784Q894 788 895 791.5Q896 795 896 800Q896 804 895 808Q894 812 892 815L892 815L892 815Z",
  measurement: "M941 293L731 83Q722 74 710.5 69Q699 64 685 64Q672 64 660.5 69Q649 74 640 83L83 640Q74 649 69 660.5Q64 672 64 685Q64 699 69 710.5Q74 722 83 731L293 941Q302 950 313.5 955Q325 960 339 960Q352 960 363.5 955Q375 950 384 941L941 384Q950 375 955 363.5Q960 352 960 339Q960 325 955 313.5Q950 302 941 293L941 293ZM339 896L128 685L256 557L361 663Q366 667 371.5 669.5Q377 672 384 672Q397 672 406.5 662.5Q416 653 416 640Q416 633 413.5 627.5Q411 622 407 617L301 512L384 429L489 535Q494 539 499.5 541.5Q505 544 512 544Q525 544 534.5 534.5Q544 525 544 512Q544 505 541.5 499.5Q539 494 535 489L429 384L512 301L617 407Q622 411 627.5 413.5Q633 416 640 416Q653 416 662.5 406.5Q672 397 672 384Q672 377 669.5 371.5Q667 366 663 361L557 256L685 128L896 339L339 896Z",
  eraser: "M900 322L734 156Q721 143 703.5 135.5Q686 128 667 128Q647 128 629.5 135.5Q612 143 599 156L124 631Q111 644 103.5 661.5Q96 679 96 699Q96 718 103.5 735.5Q111 753 124 766L244 887Q249 891 254.5 893.5Q260 896 267 896Q267 896 267 896Q267 896 267 896L864 896Q877 896 886.5 886.5Q896 877 896 864Q896 851 886.5 841.5Q877 832 864 832L864 832L525 832L900 457Q913 444 920.5 426.5Q928 409 928 389Q928 370 920.5 352.5Q913 335 900 322L900 322L900 322ZM435 832L280 832L169 721Q165 717 162.5 711Q160 705 160 699Q160 692 162.5 686Q165 680 169 676L384 461L595 672L435 832ZM855 412L640 627L429 416L644 201Q648 197 654 194.5Q660 192 667 192Q673 192 679 194.5Q685 197 689 201L689 201L855 367Q859 371 861.5 377Q864 383 864 389Q864 396 861.5 402Q859 408 855 412L855 412L855 412Z",
};

function toolIcon(name: ToolName, color: string): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><path fill="${color}" d="${TOOL_ICONS[name]}"/></svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

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
      triggerButton={<Text style={{ paddingHorizontal: 4, fontSize: 11, color: colors.mutedForeground }}>v</Text>}
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
        <Label text={t("ui.tactics.drawing_locked")}/>
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
            <Image
              source={toolIcon(tool().name, props.view.tool === tool().name ? colors.primaryForeground : colors.secondaryForeground)}
              style={{ width: 16, height: 16 }}
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
    m: shortcut("measurement"),
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
