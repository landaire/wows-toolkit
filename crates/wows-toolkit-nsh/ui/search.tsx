import { Clipboard, colorScheme, effect, keymap, List, Scroll, signal, Text, View } from "nsh";
import { Calendar, type CalendarEntry, Input } from "nsh/base";
import { Menu, type MenuItem } from "nsh/desktop";
import {
  QueryBar,
  ReplaySearch,
  type BarToken,
  type CompletionView,
  type ParseFailure,
  type PillEdit,
  type QueryBarView,
  type ResultColumn,
  type SearchPage,
  type SearchRow,
  type SortState,
  type ValueLookup,
} from "wows-toolkit/native";
import { Button, Surface } from "./components";
import { colors, controlRadius } from "./theme";

// How long a typed edit waits before the results follow it, and how long the
// caret sits still before its value lookup is sent: the GPUI bar's figures.
const RERUN_DELAY = 250;
const VALUE_DEBOUNCE = 150;
// How long the pointer rests on a row before its battle's facts show.
const DWELL = 300;
const ROW_HEIGHT = 28;
const HEADER_HEIGHT = 30;
// The narrowest a dragged column may be made, and the strip on a header's
// trailing edge that starts a drag.
const COLUMN_MIN = 28;
const GRIP = 6;
const ACTIONS_WIDTH = 64;
// Two releases of a grip within this, without moving, put its column back
// on its default width.
const DOUBLE_PRESS = 400;
const OUTCOME_COLORS = {
  win: { dark: "#6fd98a", light: "#0c5027" },
  loss: { dark: "#ea7078", light: "#601118" },
  draw: { dark: "#e9e5dd", light: "#5c5950" },
} as const;
const WARNING = "#e8a54a";

type RunState =
  | { kind: "idle" }
  | { kind: "invalid"; failure: ParseFailure }
  | { kind: "running" }
  | { kind: "failed"; reason: string }
  | { kind: "done" };

/** What a result row can do beyond opening and copying, when the app has it. */
export type SearchRowActions = {
  renderReplay?: (path: string) => void;
  renderReplayToClipboard?: (path: string) => void;
};

const text = ReplaySearch.text();
const columns: ResultColumn[] = ReplaySearch.columns();
const bar = new QueryBar();
const view = signal<QueryBarView>(bar.view());
const completionsOpen = signal(false);
const completionCursor = signal<number | null>(null);
const valueOptions = signal<CompletionView[]>([]);
const valueLookupRunning = signal(false);
const inputFocused = signal(false);
const caret = signal({ start: 0, end: 0 });
const valueLiteral = signal("");
const valueInvalid = signal(false);
const runState = signal<RunState>({ kind: "idle" });
const page = signal<SearchPage | null>(null);
const sort = signal<SortState>(ReplaySearch.defaultSort());
const widths = signal<(number | null)[]>(columns.map(() => null));
const settingsLoadError = signal<string | null>(null);
const settingsSaveError = signal<{ message: string; retry: () => void } | null>(null);
const hovered = signal<{ row: SearchRow; index: number } | null>(null);
const listOffset = signal(0);
const rowActions = signal<SearchRowActions>({});
let lastRunQuery = "";
let generation = 0;
let rerunTimer: number | undefined;
let valueTimer: number | undefined;
let blurTimer: number | undefined;
let dwellTimer: number | undefined;
let currentLookup: ValueLookup | null = null;
let lookupGeneration = 0;
let opened = false;
let queryDirty = false;
let sortDirty = false;
let historyDirty = false;
let preferencesDirty = false;
let showSearchTab: () => void = () => {};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function fill(template: string, reason: string): string {
  return template.replace("%{reason}", reason);
}

/** Has the app bring the Search tab forward for {@link searchFor}. */
export function bindSearchTab(show: () => void) {
  showSearchTab = show;
}

/** Gives the result rows the actions other parts of the app provide. */
export function bindSearchRowActions(actions: SearchRowActions) {
  rowActions.set(actions);
}

/** Puts `query` in the bar as pills, runs it and shows the Search tab. */
export function searchFor(query: string) {
  queryDirty = true;
  bar.setQuery(query);
  closeCompletions();
  refresh();
  run();
  showSearchTab();
}

function refresh() {
  view.set(bar.view());
}

function closeCompletions() {
  completionsOpen.set(false);
  completionCursor.set(null);
}

function offered(): CompletionView[] {
  return valueOptions().length > 0 ? valueOptions() : view().completions;
}

function saveFailed(message: string, retry: () => void) {
  settingsSaveError.set({ message, retry });
}

function persist(write: () => Promise<void>) {
  void write().then(
    () => settingsSaveError.set(null),
    (error) => saveFailed(errorMessage(error), () => persist(write)),
  );
}

function saveHistory() {
  historyDirty = true;
  const history = bar.history;
  persist(() => ReplaySearch.saveHistory(history));
}

/** Parses the bar's query and searches the index with it. */
function run() {
  clearTimeout(rerunTimer);
  const plan = bar.prepareRun();
  lastRunQuery = plan.query;
  generation += 1;
  const asked = generation;
  if (plan.failure) {
    runState.set({ kind: "invalid", failure: plan.failure });
    return;
  }
  if (plan.preferencesChanged) {
    preferencesDirty = true;
    const preferences = bar.operatorPreferences();
    persist(() => ReplaySearch.saveOperatorPreferences(preferences));
  }
  persist(() => ReplaySearch.saveQuery(plan.query));
  runState.set({ kind: "running" });
  void ReplaySearch.search(plan.query, sort()).then(
    (found) => {
      if (asked !== generation) return;
      clearTimeout(dwellTimer);
      page.set(found);
      hovered.set(null);
      runState.set({ kind: "done" });
    },
    (error) => {
      if (asked !== generation) return;
      runState.set({ kind: "failed", reason: errorMessage(error) });
    },
  );
}

/** Enter and the Search button: the typed term becomes pills and runs. */
function commitAndRun() {
  if (bar.rememberQuery()) saveHistory();
  queryDirty = true;
  closeCompletions();
  bar.commitTyped();
  refresh();
  run();
}

function typed(value: string) {
  queryDirty = true;
  bar.setPending(value);
  refresh();
  completionsOpen.set(inputFocused());
  completionCursor.set(null);
  refreshValueOptions();
  clearTimeout(rerunTimer);
  if (bar.fullQuery() !== lastRunQuery) rerunTimer = setTimeout(run, RERUN_DELAY);
}

/** Asks for the values the caret's field takes, when it takes any. */
function refreshValueOptions() {
  const lookup = view().valueLookup ?? null;
  if (lookup?.kind === currentLookup?.kind && lookup?.needle === currentLookup?.needle) return;
  currentLookup = lookup;
  valueOptions.set([]);
  clearTimeout(valueTimer);
  lookupGeneration += 1;
  const asked = lookupGeneration;
  if (!lookup) {
    valueLookupRunning.set(false);
    return;
  }
  valueLookupRunning.set(true);
  const pending = view().pending;
  valueTimer = setTimeout(() => {
    void ReplaySearch.lookupValues(pending).then(
      (options) => {
        if (asked !== lookupGeneration) return;
        valueLookupRunning.set(false);
        valueOptions.set(options);
      },
      () => {
        // The static vocabulary still shows; only the index's values are missing.
        if (asked === lookupGeneration) valueLookupRunning.set(false);
      },
    );
  }, VALUE_DEBOUNCE);
}

function takeCompletion(replacement: string) {
  queryDirty = true;
  completionCursor.set(null);
  const committed = bar.takeCompletion(replacement);
  refresh();
  if (committed) {
    closeCompletions();
    run();
  } else {
    refreshValueOptions();
  }
}

function takeDate(day: string) {
  queryDirty = true;
  const committed = bar.takeDate(day);
  refresh();
  closeCompletions();
  if (committed) run();
}

function setQuery(query: string) {
  queryDirty = true;
  bar.setQuery(query);
  closeCompletions();
  refresh();
  run();
}

function structural(changed: boolean) {
  refresh();
  if (changed) {
    queryDirty = true;
    run();
  }
}

function moveCursor(delta: number) {
  const count = offered().length;
  if (count === 0) return;
  const at = completionCursor();
  completionCursor.set(at === null ? (delta > 0 ? 0 : count - 1) : (at + delta + count) % count);
}

function walkHistory(delta: number) {
  if (!bar.walkHistory(delta)) return;
  queryDirty = true;
  closeCompletions();
  refresh();
  run();
}

function commitValueEdit() {
  const outcome = bar.commitValueEdit(valueLiteral());
  if (outcome === "invalid") {
    valueInvalid.set(true);
    return;
  }
  valueInvalid.set(false);
  structural(outcome === "committed");
}

function cancelValueEdit() {
  bar.cancelValueEdit();
  valueInvalid.set(false);
  refresh();
}

function beginValueEdit(path: number[]) {
  if (!bar.beginValueEdit(path)) return;
  closeCompletions();
  valueInvalid.set(false);
  refresh();
  valueLiteral.set(view().valueEdit?.literal ?? "");
}

/** Reads the query the bar was last left holding, and runs it. */
function openSavedQuery() {
  if (opened) return;
  opened = true;
  void ReplaySearch.loadSettings().then(
    (saved) => {
      settingsLoadError.set(null);
      if (!historyDirty) bar.setHistory(saved.history);
      if (!preferencesDirty) bar.setOperatorPreferences(saved.operatorPreferences);
      if (!sortDirty) sort.set(saved.sort);
      if (!queryDirty) {
        bar.restore(saved.query);
        refresh();
        // An empty query matches everything, which is the page the tab opens on.
        run();
      } else if (!sortDirty && lastRunQuery === bar.fullQuery()) {
        run();
      }
    },
    (error) => {
      const message = errorMessage(error);
      settingsLoadError.set(message);
      if (!queryDirty && !sortDirty && !historyDirty) runState.set({ kind: "failed", reason: message });
    },
  );
  loadColumnWidths();
}

function loadColumnWidths() {
  void ReplaySearch.loadColumnWidths().then(
    (saved) => {
      settingsSaveError.set(null);
      widths.set(columns.map((_, index) => {
        const width = saved[index];
        // A width narrower than the grip leaves a column that cannot be
        // grabbed to widen again.
        return width == null ? null : Math.max(COLUMN_MIN, width);
      }));
    },
    (error) => saveFailed(errorMessage(error), loadColumnWidths),
  );
}

function retryOpen() {
  settingsLoadError.set(null);
  opened = false;
  openSavedQuery();
}

function sortBy(key: NonNullable<ResultColumn["sort"]>) {
  sortDirty = true;
  sort.set(ReplaySearch.sortAfterPress(sort(), key));
  const chosen = sort();
  persist(() => ReplaySearch.saveSort(chosen));
  const state = runState().kind;
  if (state === "done" || state === "running") run();
}

function widthOf(index: number): number {
  return widths()[index] ?? columns[index].defaultWidth;
}

function setWidth(index: number, width: number | null) {
  widths.set(widths().map((current, at) => (at === index ? width : current)));
}

function saveWidths() {
  const saved = widths();
  persist(() => ReplaySearch.saveColumnWidths(saved));
}

function cellText(row: SearchRow, key: string): string {
  switch (key) {
    case "date": return row.date;
    case "map": return row.map;
    case "mode": return row.mode;
    case "ship": return row.ship;
    case "outcome": return row.outcome;
    case "damage": return row.damage;
    case "kills": return row.kills;
    default: return row.pr;
  }
}

function cellColor(row: SearchRow, key: string): string {
  const dark = colorScheme() === "dark";
  if (key === "outcome" && row.outcomeTone !== "unknown") {
    const tone = OUTCOME_COLORS[row.outcomeTone];
    return dark ? tone.dark : tone.light;
  }
  if (key === "pr" && row.prTone) return dark ? row.prTone.dark : row.prTone.light;
  return colors.cardForeground;
}

function rowMenu(row: SearchRow): MenuItem[] {
  const actions = rowActions();
  const items: MenuItem[] = [];
  if (!row.exists) {
    items.push({ label: text.openMissing, disabled: true }, { type: "separator" });
  }
  items.push({ label: text.open, disabled: !row.exists, onSelect: () => openRow(row) });
  const render = actions.renderReplay;
  if (render) items.push({ label: text.renderReplay, disabled: !row.exists, onSelect: () => render(row.replayPath) });
  const toClipboard = actions.renderReplayToClipboard;
  if (toClipboard) {
    items.push({ label: text.renderToClipboard, disabled: !row.exists, onSelect: () => toClipboard(row.replayPath) });
  }
  items.push(
    { type: "separator" },
    { label: text.copyPath, onSelect: () => void Clipboard.writeText(row.replayPath).then(() => notice(text.pathCopied)) },
    {
      label: text.showInExplorer,
      disabled: !row.exists,
      onSelect: () => void ReplaySearch.revealReplay(row.replayPath).catch((error) => notice(errorMessage(error))),
    },
  );
  return items;
}

let openReplay: (path: string) => void = () => {};
let notice: (message: string) => void = () => {};

function openRow(row: SearchRow) {
  if (!row.exists) {
    notice(text.openMissing);
    return;
  }
  openReplay(row.replayPath);
}

function pillMenu(token: BarToken): MenuItem[] {
  const menu = token.menu!;
  const edit = (pillEdit: PillEdit) => () => structural(bar.editPill(token.path, pillEdit));
  return [
    {
      label: menu.selected ? text.pillDeselect : text.pillSelect,
      onSelect: () => {
        bar.toggleSelected(token.path);
        refresh();
      },
    },
    { type: "separator" },
    { label: text.pillGroupAll, disabled: !menu.canGroup, onSelect: edit("groupAll") },
    { label: text.pillGroupAny, disabled: !menu.canGroup, onSelect: edit("groupAny") },
    { label: text.pillUngroup, disabled: !menu.canUngroup, onSelect: edit("ungroup") },
    { type: "separator" },
    { label: text.pillNegate, onSelect: edit("negate") },
    { label: text.pillFlipConnector, onSelect: edit("flipConnector") },
    { type: "separator" },
    { label: text.pillDelete, disabled: !menu.canDelete, onSelect: edit("delete") },
  ];
}

/** The keys the bar reads while its box has the keyboard. */
function bindBarKeys() {
  effect(() => {
    const always = { when: "always" as const };
    const current = view();
    if (current.valueEdit) {
      keymap.bind("escape", cancelValueEdit, always);
      return;
    }
    if (!inputFocused()) return;
    const open = completionsOpen() && offered().length > 0;
    keymap.bind("up", () => (open ? moveCursor(-1) : walkHistory(1)), always);
    keymap.bind("down", () => (open ? moveCursor(1) : walkHistory(-1)), always);
    keymap.bind("escape", closeCompletions, always);
    keymap.bind("mod+z", () => structural(bar.undo()), always);
    keymap.bind("mod+shift+z", () => structural(bar.redo()), always);
    keymap.bind("mod+y", () => structural(bar.redo()), always);
    const cursor = completionCursor();
    if (open && cursor !== null) {
      keymap.bind("enter", () => {
        const taken = offered()[cursor];
        if (taken) takeCompletion(taken.replacement);
      }, always);
    }
    // Nothing left to erase in the box, so the key means the pill before it.
    if (current.pending.length === 0) keymap.bind("backspace", () => structural(bar.deleteAtCaret()), always);
    if (current.hasSelection) keymap.bind("delete", () => structural(bar.deleteAtCaret()), always);
    const { start, end } = caret();
    if (start === 0 && end === 0) {
      keymap.bind("left", () => {
        bar.stepSelection(true, false);
        refresh();
      }, always);
      keymap.bind("shift+left", () => {
        bar.stepSelection(true, true);
        refresh();
      }, always);
    }
    if (current.hasSelection && start === current.pending.length && end === start) {
      keymap.bind("right", () => {
        bar.stepSelection(false, false);
        refresh();
      }, always);
      keymap.bind("shift+right", () => {
        bar.stepSelection(false, true);
        refresh();
      }, always);
    }
  });
}

function CaretBox() {
  return (
    <View style={{ flex: 1, minWidth: 120 }}>
      <Input
        testID="search-query"
        accessibilityLabel="Replay search query"
        value={view().pending}
        placeholder="Add filter"
        onChange={({ value }) => typed(value)}
        onSubmit={commitAndRun}
        submitBehavior="submit"
        onSelectionChange={({ start, end }) => caret.set({ start, end })}
        onFocus={() => {
          clearTimeout(blurTimer);
          inputFocused.set(true);
        }}
        onBlur={() => {
          inputFocused.set(false);
          // A press on a dropdown row blurs the box first; the row's own
          // press then takes it before the dropdown goes.
          blurTimer = setTimeout(closeCompletions, 200);
        }}
        style={{ minHeight: 28, fontSize: 13, color: colors.foreground, placeholderColor: colors.mutedForeground }}
      />
    </View>
  );
}

function Segment(props: { token: BarToken; index: number; slot: number }) {
  const segment = () => props.token.segments[props.slot];
  const editing = () => props.token.editing && segment().editable;
  return (
    <View style={{ flexDirection: "row", alignItems: "center" }}>
      <If when={props.slot > 0}>
        <View style={{ width: 1, height: 14, backgroundColor: colors.border }}/>
      </If>
      <If when={editing()}>
        <View style={{ width: 140 }}>
          <Input
            accessibilityLabel="Pill value"
            autoFocus
            value={valueLiteral()}
            onChange={({ value }) => {
              valueLiteral.set(value);
              valueInvalid.set(false);
            }}
            onSubmit={commitValueEdit}
            onBlur={() => {
              if (view().valueEdit) commitValueEdit();
            }}
            style={{ minHeight: 22, fontSize: 12, color: colors.foreground }}
          />
        </View>
      </If>
      <If when={!editing()}>
        <View
          testID={`search-pill-segment-${props.index}-${props.slot}`}
          accessibilityLabel={segment().editable ? `Edit value ${segment().text}` : segment().text}
          onPress={(event) => {
            const current = segment();
            if (current.choices.length > 0) {
              void Menu.popup(
                current.choices.map((choice) => ({
                  label: choice.label,
                  checked: choice.current,
                  onSelect: () => setQuery(choice.taken),
                })),
                event?.pagePosition,
              );
            } else if (current.editable) {
              beginValueEdit(props.token.path);
            }
          }}
          hoverStyle={{ backgroundColor: colors.accent }}
          style={{ paddingHorizontal: 5, paddingVertical: 1, borderRadius: 8 }}
        >
          <Text style={{ fontSize: 12, color: colors.foreground }}>{segment().text}</Text>
        </View>
      </If>
    </View>
  );
}

function Pill(props: { token: BarToken; index: number }) {
  const selected = () => props.token.menu?.selected ?? false;
  return (
    <View
      testID={`search-pill-${props.index}`}
      accessibilityLabel={`Filter ${props.token.segments.map((segment) => segment.text).join(" ")}`}
      accessibilityState={{ selected: selected() }}
      onContextMenu={(event) => void Menu.popup(pillMenu(props.token), event?.pagePosition)}
      style={{
        flexDirection: "row",
        alignItems: "center",
        gap: 2,
        paddingHorizontal: 4,
        paddingVertical: 1,
        borderRadius: 10,
        borderWidth: 1,
        borderColor: selected() ? colors.ring : colors.border,
        backgroundColor: selected() ? colors.accent : colors.muted,
      }}
    >
      <Each of={props.token.segments}>
        {(_, slot) => <Segment token={props.token} index={props.index} slot={slot()}/>}
      </Each>
    </View>
  );
}

function BarTokens() {
  // The caret token is always the stream's last, so the box sits after the
  // tokens: one input for the life of the bar, which keeps its focus while
  // the pills around it change.
  return (
    <View testID="search-pills" style={{ flex: 1, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 4, paddingHorizontal: 8, paddingVertical: 3, borderWidth: 1, borderColor: colors.border, borderRadius: controlRadius() }}>
      <Each of={view().tokens.filter((token) => token.kind !== "caret")}>
        {(token, index) => {
          const kind = () => token().kind;
          return (
            <>
              <If when={kind() === "pill"}>
                <Pill token={token()} index={index()}/>
              </If>
              <If when={kind() !== "pill"}>
                <Text style={{ fontSize: 12, color: kind() === "not" ? colors.primary : colors.mutedForeground }}>
                  {kind() === "quantOpen" ? `${token().text} [` : token().text}
                </Text>
              </If>
            </>
          );
        }}
      </Each>
      <CaretBox/>
    </View>
  );
}

function CalendarDay(props: { item: CalendarEntry }) {
  const kind = props.item.kind;
  const label = kind === "previous" ? "<" : kind === "next" ? ">" : String(props.item.value);
  return <Text style={{ minWidth: 26, textAlign: "center", fontSize: 12, color: colors.popoverForeground }}>{label}</Text>;
}

function Dropdown() {
  return (
    <>
      <If when={completionsOpen() && view().wantsCalendar}>
        <Surface style={{ position: "absolute", top: 44, left: 40, padding: 6, backgroundColor: colors.popover, zIndex: 10 }}>
          <View testID="search-calendar">
            <Calendar
              value={view().calendarDate ?? null}
              onChange={({ value }) => takeDate(value)}
              renderItem={(item) => <CalendarDay item={item}/>}
              itemClass="rounded-md hover:bg-accent selected:bg-primary"
            />
          </View>
        </Surface>
      </If>
      <If when={completionsOpen() && !view().wantsCalendar && (offered().length > 0 || valueLookupRunning())}>
        <Surface style={{ position: "absolute", top: 44, left: 40, minWidth: 260, maxWidth: 460, maxHeight: 260, padding: 4, backgroundColor: colors.popover, zIndex: 10 }}>
          <Scroll testID="search-completions" style={{ maxHeight: 250 }}>
            <If when={valueLookupRunning()}>
              <Text style={{ padding: 6, fontSize: 11.5, color: colors.mutedForeground }}>{text.lookingUpValues}</Text>
            </If>
            <Each of={offered()}>
              {(completion, index) => (
                <View
                  testID={`search-completion-${index()}`}
                  accessibilityLabel={completion().label}
                  onPress={() => takeCompletion(completion().replacement)}
                  hoverStyle={{ backgroundColor: colors.accent }}
                  style={{
                    flexDirection: "row",
                    alignItems: "center",
                    gap: 16,
                    paddingHorizontal: 8,
                    paddingVertical: 4,
                    borderRadius: controlRadius(),
                    backgroundColor: completionCursor() === index() ? colors.accent : "transparent",
                  }}
                >
                  <Text style={{ flex: 1, fontSize: 13, color: colors.popoverForeground }}>{completion().label}</Text>
                  <Text style={{ fontSize: 11, color: colors.mutedForeground }}>{completion().context}</Text>
                </View>
              )}
            </Each>
          </Scroll>
        </Surface>
      </If>
    </>
  );
}

function ParseError(props: { failure: ParseFailure }) {
  const message = () => fill(text.parseFailed, props.failure.reason);
  return (
    <View testID="search-parse-error" accessibilityLabel={message()} style={{ paddingHorizontal: 20, gap: 1 }}>
      <Text style={{ fontSize: 11.5, color: WARNING }}>{message()}</Text>
      <If when={props.failure.span}>
        <View style={{ flexDirection: "row" }}>
          <Text style={{ fontSize: 11.5, fontFamily: "monospace", color: colors.mutedForeground }}>{props.failure.span!.before}</Text>
          <Text style={{ fontSize: 11.5, fontFamily: "monospace", color: WARNING, textDecorationLine: "underline" }}>{props.failure.span!.marked}</Text>
          <Text style={{ fontSize: 11.5, fontFamily: "monospace", color: colors.mutedForeground }}>{props.failure.span!.after}</Text>
        </View>
      </If>
    </View>
  );
}

function QueryBarRow() {
  bindBarKeys();
  const failure = () => {
    const state = runState();
    return state.kind === "invalid" ? state.failure : null;
  };
  return (
    <View style={{ paddingHorizontal: 12, paddingTop: 10, paddingBottom: 6, gap: 4, borderBottomWidth: 1, borderBottomColor: colors.border, zIndex: 5 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <BarTokens/>
        <Button label="Run Search" onPress={commitAndRun}/>
        <Button label={text.undo} variant="ghost" disabled={!view().canUndo} onPress={() => structural(bar.undo())}/>
        <Button label={text.redo} variant="ghost" disabled={!view().canRedo} onPress={() => structural(bar.redo())}/>
      </View>
      <If when={valueInvalid()}>
        <Text style={{ fontSize: 11.5, color: colors.primary }}>Invalid filter value. Correct the value or press Escape to cancel.</Text>
      </If>
      <If when={failure()}>
        <ParseError failure={failure()!}/>
      </If>
      <Dropdown/>
    </View>
  );
}

function HeaderCell(props: { index: number }) {
  const column = columns[props.index];
  const active = () => column.sort !== null && column.sort !== undefined && sort().column === column.sort;
  let startWidth = 0;
  let startX = 0;
  let lastRelease = 0;
  return (
    <View style={{ width: widthOf(props.index), height: HEADER_HEIGHT, flexDirection: "row", alignItems: "center" }}>
      <If when={column.sort}>
        <View
          role="columnheader"
          testID={`search-sort-${column.key}`}
          accessibilityLabel={active() ? `${column.title}, sorted ${sort().direction}` : `Sort by ${column.title}`}
          accessibilityState={{ selected: active() }}
          onPress={() => sortBy(column.sort!)}
          hoverStyle={{ backgroundColor: colors.accent }}
          style={{ flex: 1, height: HEADER_HEIGHT, justifyContent: "center", paddingHorizontal: 4 }}
        >
          <Text style={{ fontSize: 12, fontWeight: "700", color: active() ? colors.foreground : colors.mutedForeground }}>
            {active() ? `${column.title} ${sort().direction === "descending" ? "v" : "^"}` : column.title}
          </Text>
        </View>
      </If>
      <If when={!column.sort}>
        <Text role="columnheader" style={{ flex: 1, paddingHorizontal: 4, fontSize: 12, fontWeight: "700", color: colors.mutedForeground }}>{column.title}</Text>
      </If>
      <View
        testID={`search-header-grip-${props.index}`}
        accessibilityLabel={`Resize ${column.title}`}
        onStartShouldSetResponder={() => true}
        onResponderTerminationRequest={() => false}
        onResponderGrant={(event) => {
          startWidth = widthOf(props.index);
          startX = event.nativeEvent.pageX;
          return true;
        }}
        onResponderMove={(event) => setWidth(props.index, Math.max(COLUMN_MIN, startWidth + event.nativeEvent.pageX - startX))}
        onResponderRelease={(event) => {
          const now = Date.now();
          const still = Math.abs(event.nativeEvent.pageX - startX) < 2;
          if (still && now - lastRelease < DOUBLE_PRESS) setWidth(props.index, null);
          lastRelease = still ? now : 0;
          saveWidths();
        }}
        style={{ width: GRIP, height: HEADER_HEIGHT, backgroundColor: colors.border, opacity: 0.6 }}
      />
    </View>
  );
}

function ResultRow(props: { row: SearchRow; index: number }) {
  const actions = () => rowActions();
  return (
    <View
      role="row"
      testID={`search-row-${props.index}`}
      accessibilityLabel={`Open replay ${props.row.replayPath}`}
      onPress={() => openRow(props.row)}
      onContextMenu={(event) => void Menu.popup(rowMenu(props.row), event?.pagePosition)}
      onMouseEnter={() => {
        clearTimeout(dwellTimer);
        const row = props.row;
        const index = props.index;
        if (hovered()?.row.replayPath === row.replayPath) return;
        dwellTimer = setTimeout(() => hovered.set({ row, index }), DWELL);
      }}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.72 }}
      style={{
        height: ROW_HEIGHT,
        flexDirection: "row",
        alignItems: "center",
        paddingHorizontal: 8,
        backgroundColor: props.index % 2 === 1 ? colors.muted : "transparent",
      }}
    >
      <Each of={columns}>
        {(column, at) => (
          <Text role="cell" style={{ width: widthOf(at()) + GRIP, numberOfLines: 1, fontSize: 12.5, color: cellColor(props.row, column().key) }}>
            {cellText(props.row, column().key)}
          </Text>
        )}
      </Each>
      <View style={{ width: ACTIONS_WIDTH, flexDirection: "row", alignItems: "center", gap: 2 }}>
        <If when={actions().renderReplayToClipboard}>
          <View
            testID={`search-row-render-clipboard-${props.index}`}
            accessibilityLabel={text.renderToClipboard}
            onPress={() => props.row.exists && actions().renderReplayToClipboard?.(props.row.replayPath)}
            hoverStyle={{ backgroundColor: colors.secondary }}
            style={{ paddingHorizontal: 5, borderRadius: 6, opacity: props.row.exists ? 1 : 0.4 }}
          >
            <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Clip</Text>
          </View>
        </If>
        <View
          testID={`search-row-actions-${props.index}`}
          accessibilityLabel={text.rowActionsHint}
          onPress={(event) => void Menu.popup(rowMenu(props.row), event?.pagePosition)}
          hoverStyle={{ backgroundColor: colors.secondary }}
          style={{ paddingHorizontal: 6, borderRadius: 6 }}
        >
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>...</Text>
        </View>
      </View>
    </View>
  );
}

function HoverFacts() {
  const top = () => {
    const at = hovered();
    return at ? Math.max(0, HEADER_HEIGHT + at.index * ROW_HEIGHT - listOffset()) : 0;
  };
  return (
    <If when={hovered()}>
      <Surface style={{ position: "absolute", right: 16, top: top(), width: 360, padding: 8, gap: 2, backgroundColor: colors.background, zIndex: 8 }}>
        <View testID="search-preview" accessibilityLabel={`Preview ${hovered()!.row.replayPath}`} style={{ gap: 2 }}>
          <Each of={hovered()!.row.facts}>
            {(fact) => (
              <View style={{ flexDirection: "row", gap: 8 }}>
                <Text style={{ width: 64, fontSize: 11.5, color: colors.mutedForeground }}>{fact().label}</Text>
                <Text style={{ flex: 1, fontSize: 11.5, color: colors.foreground }}>{fact().value}</Text>
              </View>
            )}
          </Each>
        </View>
      </Surface>
    </If>
  );
}

function statusText(): string | null {
  const state = runState();
  const rows = page()?.rows.length ?? 0;
  switch (state.kind) {
    case "idle": return text.typeAQuery;
    // Said in its own strip under the bar, not in place of the results.
    case "invalid": return rows > 0 ? null : text.typeAQuery;
    case "running": return rows > 0 ? null : text.searching;
    case "failed": return fill(text.failed, state.reason);
    case "done": return rows === 0 ? text.noMatches : null;
  }
}

function Results() {
  const tableWidth = () => columns.reduce((total, _, index) => total + widthOf(index) + GRIP, 0) + ACTIONS_WIDTH + 16;
  return (
    <View testID="search-results" onMouseLeave={() => {
      clearTimeout(dwellTimer);
      hovered.set(null);
    }} style={{ flex: 1, minHeight: 0 }}>
      <Scroll horizontal style={{ flex: 1 }} contentStyle={{ minWidth: tableWidth() }}>
        <View role="table" accessibilityLabel="Replay search results" style={{ flex: 1, width: tableWidth() }}>
          <View role="row" style={{ flexDirection: "row", paddingHorizontal: 8, borderBottomWidth: 1, borderBottomColor: colors.border }}>
            <Each of={columns}>{(_, index) => <HeaderCell index={index()}/>}</Each>
          </View>
          <If when={statusText() === null}>
            <List
              each={page()?.rows ?? []}
              keyExtractor={(row: SearchRow) => row.replayPath}
              estimatedItemSize={ROW_HEIGHT}
              onScroll={({ y }) => listOffset.set(y)}
              style={{ flex: 1 }}
            >
              {(row, index) => <ResultRow row={row()} index={index()}/>}
            </List>
          </If>
          <If when={statusText() !== null}>
            <View testID="search-status" accessibilityLabel={statusText() ?? ""} style={{ flex: 1, alignItems: "center", justifyContent: "center", gap: 8 }}>
              <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{statusText()}</Text>
              <If when={settingsLoadError()}>
                <Button label={text.retry} onPress={retryOpen}/>
              </If>
            </View>
          </If>
        </View>
      </Scroll>
      <HoverFacts/>
    </View>
  );
}

/** The Search tab: a query bar over the replay index, and its results. */
export function SearchView(props: { onOpenReplay: (path: string) => void; onNotice: (message: string) => void }) {
  openReplay = props.onOpenReplay;
  notice = props.onNotice;
  openSavedQuery();
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <QueryBarRow/>
      <Text testID="search-game-mode-gap" style={{ minHeight: 18, paddingHorizontal: 12, paddingVertical: 2, fontSize: 11.5, color: WARNING }}>
        {page()?.gameModeGap ?? ""}
      </Text>
      <If when={settingsSaveError()}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 12, paddingVertical: 2 }}>
          <Text style={{ fontSize: 11.5, color: WARNING }}>{settingsSaveError()!.message}</Text>
          <Button label={text.retry} onPress={() => settingsSaveError()!.retry()}/>
        </View>
      </If>
      <If when={settingsLoadError() && statusText() === null}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 12, paddingVertical: 2 }}>
          <Text style={{ fontSize: 11.5, color: WARNING }}>{settingsLoadError()}</Text>
          <Button label={text.retry} onPress={retryOpen}/>
        </View>
      </If>
      <Results/>
      <View style={{ height: 24, justifyContent: "center", paddingHorizontal: 12, borderTopWidth: 1, borderTopColor: colors.border }}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{runState().kind === "done" ? page()?.count ?? "" : ""}</Text>
      </View>
    </View>
  );
}
