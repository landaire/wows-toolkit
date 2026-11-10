import type { JSX } from "nsh";
import { Clipboard, colorScheme, effect, Image, Scroll, signal, Text, untracked, View } from "nsh";
import { Checkbox, Dialog, DockArea, Input, Menu, NumberInput, Popover, Toolbar, type DockAreaApi, type DockAreaLayout, type DockTab } from "nsh/base";
import { Stats, type ChartSnapshot, type DirectoryOpened, type DirectoryStatus, type FilterState, type RatingChip, type ScopeView, type ShipSection, type StatsText } from "wows-toolkit/native";
import { Button, EmptyState, errorMessage, Hint, inputStyle, MenuRow, Surface } from "./components";
import { Frame } from "./frame";
import { currentLocale, t } from "./i18n";
import { Icon, type IconName } from "./icons";
import { colors, controlRadius } from "./theme";

// The catalog text, read again after a language change; a read inside a render
// follows the locale.
let textCache: { locale: string; value: StatsText } | null = null;
const text = new Proxy({} as StatsText, {
  get: (_target, key) => {
    const locale = currentLocale();
    if (textCache?.locale !== locale) textCache = { locale, value: Stats.text() };
    return textCache.value[key as keyof StatsText];
  },
});

// The GPUI app's win and loss colours, per theme.
const semantic = {
  get win() { return colorScheme() === "dark" ? "#6fd98a" : "#0c5027"; },
  get loss() { return colorScheme() === "dark" ? "#ea7078" : "#601118"; },
  get error() { return colorScheme() === "dark" ? "#e8737b" : "#ae2230"; },
};

// How long a dock rearrangement settles before it is written, as GPUI waits.
const LAYOUT_SAVE_DELAY_MS = 500;
const NOTICE_MS = 3000;

/// One Stats view's state: the recorded session, or a directory's replays.
export type StatsModel = {
  scope: string;
  persists: boolean;
  view: () => ScopeView | null;
  charts: () => ChartSnapshot[];
  setView: (view: ScopeView) => void;
  setChart: (chart: ChartSnapshot) => void;
  dropChart: (id: number) => void;
  saveError: () => string | null;
  setSaveError: (error: string | null) => void;
  /// The layout the dock area opens with; a new one rebuilds the area.
  layout: () => DockAreaLayout | null;
  setLayout: (layout: DockAreaLayout) => void;
  /// The arrangement shown, as the area last reported it.
  current: () => DockAreaLayout | null;
  setCurrent: (layout: DockAreaLayout) => void;
  dock: () => DockAreaApi | undefined;
  setDock: (api: DockAreaApi) => void;
  /// Numbers each request that publishes a whole view, or one chart (by
  /// id), so a reply overtaken by a newer one is dropped.
  begin: (chart?: number) => number;
  isLatest: (request: number, chart?: number) => boolean;
};

export function createStatsModel(scope: string, persists: boolean): StatsModel {
  const view = signal<ScopeView | null>(null);
  const charts = signal<ChartSnapshot[]>([]);
  const saveError = signal<string | null>(null);
  const layout = signal<DockAreaLayout | null>(null);
  const current = signal<DockAreaLayout | null>(null);
  let dock: DockAreaApi | undefined;
  const latest = new Map<number | undefined, number>();
  let requests = 0;
  const watched = new Set<number>();

  const watchView = async (id: number) => {
    if (watched.has(id)) return;
    watched.add(id);
    let moved = false;
    for (;;) {
      try {
        moved = await Stats.chartViewMoved(scope, id, moved);
      } catch {
        watched.delete(id);
        return;
      }
      charts.set(charts().map((chart) => chart.id === id ? { ...chart, viewMoved: moved } : chart));
    }
  };

  const model: StatsModel = {
    scope,
    persists,
    view,
    charts,
    setView: (next) => {
      view.set(next);
      charts.set(next.charts);
      for (const chart of next.charts) void watchView(chart.id);
      if (next.saveError) saveError.set(next.saveError);
    },
    setChart: (chart) => {
      const current = charts();
      charts.set(current.some((known) => known.id === chart.id) ? current.map((known) => known.id === chart.id ? chart : known) : [...current, chart]);
      void watchView(chart.id);
    },
    dropChart: (id) => charts.set(charts().filter((chart) => chart.id !== id)),
    saveError,
    setSaveError: (error) => saveError.set(error),
    layout,
    setLayout: (next) => {
      current.set(next);
      layout.set(next);
    },
    current,
    setCurrent: (next) => current.set(next),
    dock: () => dock,
    setDock: (api) => { dock = api; },
    begin: (chart) => {
      requests += 1;
      latest.set(chart, requests);
      return requests;
    },
    isLatest: (request, chart) => latest.get(chart) === request,
  };
  return model;
}

const session = createStatsModel("session", true);

type DirectoryView = { model: StatsModel; status: () => DirectoryStatus; setStatus: (status: DirectoryStatus) => void };
const directories = signal<DirectoryView[]>([]);
const activeScope = signal("session");
const sessionLoading = signal(false);
const sessionError = signal<string | null>(null);
/// A saved chart layout that could not be read (`save` false) or written.
type LayoutFailure = { message: string; save: boolean };
const layoutError = signal<LayoutFailure | null>(null);
const clearError = signal<string | null>(null);
const notice = signal<string | null>(null);
const clearConfirmOpen = signal(false);
let sessionRequested = false;
let layoutTimer: number | undefined;
let noticeTimer: number | undefined;

effect(() => {
  colorScheme();
  try {
    Stats.setChartTheme({ background: colors.card, axis: colors.border, text: colors.mutedForeground });
  } catch (error) {
    console.warn(`Chart colours could not be read from the theme: ${errorMessage(error)}`);
  }
});

// A language change words every figure and label again, as GPUI rebuilds the tab.
let shownLocale: string | null = null;
effect(() => {
  const locale = currentLocale();
  if (shownLocale === null || shownLocale === locale) {
    shownLocale = locale;
    return;
  }
  shownLocale = locale;
  untracked(() => {
    for (const model of [session, ...directories().map((view) => view.model)]) {
      // Outside the request numbering, so a filter change still in flight keeps its answer.
      if (model.view()) void Stats.relabel(model.scope).then(model.setView, (error) => showNotice(errorMessage(error)));
    }
  });
});

/// Opens the session the first time the tab shows, and reads its games again
/// each time after.
export function activateStats() {
  if (!sessionRequested) {
    sessionRequested = true;
    void openSession();
    return;
  }
  if (session.view()) void refreshSession();
}

async function openSession() {
  sessionLoading.set(true);
  sessionError.set(null);
  try {
    const opened = await Stats.openSession();
    session.setView(opened.view);
    session.setLayout(opened.layout);
    layoutError.set(opened.layoutError ? { message: opened.layoutError, save: false } : null);
  } catch (error) {
    sessionError.set(errorMessage(error));
  } finally {
    sessionLoading.set(false);
  }
}

async function refreshSession() {
  try {
    session.setView(await Stats.refreshSession());
  } catch (error) {
    showNotice(errorMessage(error));
  }
}

function adoptDirectory(opened: DirectoryOpened) {
  if (directories().some((view) => view.model.scope === opened.scope)) return;
  const model = createStatsModel(opened.scope, false);
  model.setView(opened.status.view);
  model.setLayout(opened.layout);
  const status = signal(opened.status);
  directories.set([...directories(), { model, status, setStatus: (next) => status.set(next) }]);
  void followDirectory(model, status);
}

async function followDirectory(model: StatsModel, status: { (): DirectoryStatus; set: (value: DirectoryStatus) => void }) {
  for (;;) {
    let next: DirectoryStatus;
    try {
      next = await Stats.directoryStatus(model.scope, status().revision);
    } catch {
      return;
    }
    status.set(next);
    model.setView(next.view);
  }
}

/// Opens a stats view over every replay below `path`, or finds the one open
/// over it, as a replay listing's Directory Stats action asks. Returns its
/// scope.
export async function openDirectoryStats(path: string): Promise<string> {
  const opened = await Stats.openDirectory(path);
  adoptDirectory(opened);
  return opened.scope;
}

/// The open directory views, in the order they were opened.
export function directoryStatsTabs(): { scope: string; root: string }[] {
  return directories().map((view) => ({ scope: view.model.scope, root: view.status().root }));
}

/// Shows `scope`: the session's stats, or an open directory's.
export function showStatsScope(scope: string) {
  activeScope.set(scope);
}

/// Closes a directory view, which stops its read.
export function closeDirectoryStats(scope: string) {
  try {
    Stats.closeDirectory(scope);
  } catch (error) {
    console.warn(`The directory view could not be closed: ${errorMessage(error)}`);
  }
  directories.set(directories().filter((view) => view.model.scope !== scope));
}

function showNotice(message: string) {
  clearTimeout(noticeTimer);
  notice.set(message);
  noticeTimer = setTimeout(() => notice.set(null), NOTICE_MS);
}

async function run(model: StatsModel, action: () => Promise<ScopeView>) {
  const request = model.begin();
  try {
    const view = await action();
    if (model.isLatest(request)) model.setView(view);
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

async function runChart(model: StatsModel, chart: number, action: () => Promise<{ chart: ChartSnapshot; saveError?: string | null }>) {
  const request = model.begin(chart);
  try {
    const update = await action();
    if (model.isLatest(request, chart)) model.setChart(update.chart);
    if (update.saveError) model.setSaveError(update.saveError);
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

function setFilters(model: StatsModel, change: Partial<FilterState>) {
  const current = model.view()?.stats.filters;
  if (!current) return;
  void run(model, () => Stats.setFilters(model.scope, { ...current, ...change }));
}

function layoutChanged(model: StatsModel, layout: DockAreaLayout) {
  model.setCurrent(layout);
  if (!model.persists) return;
  clearTimeout(layoutTimer);
  layoutTimer = setTimeout(() => {
    layoutTimer = undefined;
    Stats.saveLayout(layout).then(
      () => { if (layoutError()?.save) layoutError.set(null); },
      (error) => layoutError.set({ message: `Chart layout could not be saved: ${errorMessage(error)}`, save: true }),
    );
  }, LAYOUT_SAVE_DELAY_MS);
}

async function addChart(model: StatsModel, stat: string) {
  try {
    const update = await Stats.addChart(model.scope, stat);
    model.setChart(update.chart);
    // Where GPUI puts a new chart: the right dock, opened, 500 wide.
    const dock = model.dock();
    if (dock) {
      dock.addPanel(`chart-${update.chart.id}`, "right", 500);
      layoutChanged(model, dock.layout());
    }
    if (update.saveError) model.setSaveError(update.saveError);
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

function closeChart(model: StatsModel, id: number) {
  // The area then reports the panel removed, which lets the chart go.
  const dock = model.dock();
  if (!dock) return;
  dock.removePanel(`chart-${id}`);
}

async function chartRemoved(model: StatsModel, id: number) {
  model.dropChart(id);
  try {
    const saveError = await Stats.removeChart(model.scope, id);
    if (saveError) model.setSaveError(saveError);
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

async function keepCurrentLayout() {
  const layout = session.current();
  if (!layout) return;
  try {
    await Stats.keepCurrentLayout(layout);
    layoutError.set(null);
    session.setSaveError(null);
  } catch (error) {
    layoutError.set({ message: errorMessage(error), save: true });
  }
}

async function copyText(value: string) {
  try {
    await Clipboard.writeText(value);
    showNotice(text.copied);
  } catch (error) {
    showNotice(errorMessage(error));
  }
}

async function copyChartImage(model: StatsModel, id: number) {
  try {
    await Clipboard.writeImage(await Stats.chartImage(model.scope, id));
    showNotice(text.copyImageDone);
  } catch (error) {
    console.warn(`The chart image could not be copied: ${errorMessage(error)}`);
    showNotice(text.copyImageFailed);
  }
}

export function StatsView() {
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={activeScope() === "session"}>
      <If when={sessionLoading() && !session.view()}>
        <EmptyState title={text.overview} description={text.noGames}/>
      </If>
      <If when={!sessionLoading() && sessionError() && !session.view()}>
        <EmptyState title={text.overview} description={sessionError()!} action={<Button label={text.retry} onPress={() => void openSession()}/>}/>
      </If>
      </If>
      <View style={{ flex: 1, minHeight: 0, display: activeScope() === "session" ? "flex" : "none" }}>
        <If when={session.view()}>
          <StatsScopeView model={session} header={null}/>
        </If>
      </View>
      <Each of={directories()}>
        {(view) => (
          <View style={{ flex: 1, minHeight: 0, display: activeScope() === view().model.scope ? "flex" : "none" }}>
            <StatsScopeView model={view().model} header={<DirectoryHeader view={view()}/>}/>
          </View>
        )}
      </Each>
      <ClearSessionDialog/>
      <If when={notice()}>
        <Surface style={{ position: "absolute", right: 16, bottom: 16, minWidth: 220, maxWidth: 420, padding: 12, backgroundColor: colors.popover }}>
          <Text accessibilityLiveRegion="polite" style={{ fontSize: 13, color: colors.popoverForeground }}>{notice()}</Text>
        </Surface>
      </If>
    </View>
  );
}

function DirectoryHeader(props: { view: DirectoryView }) {
  const status = () => props.view.status();
  return (
    <View style={{ borderBottomWidth: 1, borderBottomColor: colors.border }}>
      <View style={{ minHeight: 34, paddingHorizontal: 10, flexDirection: "row", alignItems: "center", gap: 10 }}>
        <Text style={{ flexShrink: 1, numberOfLines: 1, fontSize: 12.5, color: colors.foreground }}>{status().root}</Text>
        <Rule/>
        <If when={status().status}>
          <Text accessibilityLiveRegion="polite" style={{ fontSize: 12, color: colors.mutedForeground }}>{status().status}</Text>
        </If>
        <If when={status().finished}>
          <Button
            label={text.directoryReload}
            variant="ghost"
            tooltip={t("ui.stats.directory_reload_tooltip")}
            onPress={() => {
              try {
                Stats.reloadDirectory(props.view.model.scope);
              } catch (error) {
                showNotice(errorMessage(error));
              }
            }}
          />
        </If>
        <If when={status().skipped}>
          <Rule/>
          <Text tooltip={status().skippedDetail ?? undefined} accessibilityLabel={`${status().skipped}: ${status().skippedDetail}`} style={{ fontSize: 12, color: colors.mutedForeground }}>{status().skipped}</Text>
        </If>
        <If when={status().noGames}>
          <Rule/>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{status().noGames}</Text>
        </If>
      </View>
      <If when={status().progress !== null && status().progress !== undefined}>
        <View accessibilityRole="progressbar" accessibilityLabel={status().status ?? ""} style={{ height: 3, backgroundColor: colors.muted }}>
          <View style={{ height: 3, width: `${Math.round((status().progress ?? 0) * 100)}%`, backgroundColor: colors.primary }}/>
        </View>
      </If>
    </View>
  );
}

/// A Stats view of any scope: the filter bar over the dock of panels.
export function StatsScopeView(props: { model: StatsModel; header: JSX.Element | null }) {
  const model = props.model;
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      {props.header}
      <FilterBar model={model}/>
      <View style={{ flex: 1, minHeight: 0 }}>
       <View style={{ flex: 1, minHeight: 0, flexDirection: "row" }}>
        <ExpandRail model={model} side="left"/>
        <View style={{ flex: 1, minWidth: 0, minHeight: 0 }}>
        <Each of={model.layout() ? [model.layout()!] : []}>
          {(opened) => (
          <DockArea
            class="flex-1"
            ref={(api: DockAreaApi) => model.setDock(api)}
            defaultLayout={opened()}
            onLayoutChange={({ layout }) => layoutChanged(model, layout)}
            onPanelRemove={({ panel }) => {
              // The arrangement to save is the last one known without the
              // removed panel, which the area may not have reported yet.
              const current = model.current();
              if (current) layoutChanged(model, withoutPanel(current, panel));
              const id = chartId(panel);
              if (id !== null && model.charts().some((chart) => chart.id === id)) void chartRemoved(model, id);
            }}
            panels={[{ id: "overview", closable: false }, { id: "ships", closable: false }]}
            renderPanel={(panel) => <StatsPanel model={model} id={panel.id}/>}
            renderTab={(tab) => <PanelTab model={model} tab={tab}/>}
            renderEmpty={() => <EmptyState title={text.overview} description={text.noGames}/>}
            groupClass="flex-1 min-h-0 bg-background"
            tabBarClass="flex-row items-center gap-1 px-1 border-b border-border bg-card"
            tabClass="px-2.5 py-1.5 rounded-md selected:bg-accent"
            contentClass="flex-1 min-h-0"
            dockClass="border-l border-border"
            handleClass="bg-border group-hover:bg-primary"
            handleAccessibilityLabel="Resize panels"
          />
          )}
        </Each>
        </View>
        <ExpandRail model={model} side="right"/>
       </View>
       <ExpandRail model={model} side="bottom"/>
      </View>
    </View>
  );
}

type DockSide = "left" | "right" | "bottom";
const DOCK_SIDES: DockSide[] = ["left", "right", "bottom"];

function panelsOf(node: DockAreaLayout["center"]): string[] {
  return node.type === "split" ? node.children.flatMap(panelsOf) : node.panels;
}

/// The open dock holding `panel`, which its collapse button closes; none in the center.
function openDockOf(layout: DockAreaLayout | null, panel: string): DockSide | null {
  if (!layout) return null;
  return DOCK_SIDES.find((side) => {
    const region = layout[side];
    return region && region.open !== false && panelsOf(region.layout).includes(panel);
  }) ?? null;
}

/// A dock's collapse (`open`) or expand icon, as GPUI's `panel_collapse_button` and `panel_expand_rail`.
function dockIcon(side: DockSide, open: boolean): IconName {
  switch (side) {
    case "left": return open ? "panel-left-close" : "panel-left-open";
    case "right": return open ? "panel-right-close" : "panel-right-open";
    case "bottom": return open ? "panel-bottom" : "panel-bottom-open";
  }
}

function DockToggle(props: { label: string; icon: IconName; onPress: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      tooltip={<Hint text={props.label}/>}
      hoverStyle={{ backgroundColor: colors.muted }}
      onPress={props.onPress}
      style={{ width: 22, height: 22, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
    >
      <Icon name={props.icon} size={14} color={colors.mutedForeground}/>
    </View>
  );
}

/// The strip a closed dock leaves on its edge, which opens it again.
function ExpandRail(props: { model: StatsModel; side: DockSide }) {
  const closed = () => {
    const region = props.model.current()?.[props.side];
    return Boolean(region && region.open === false && panelsOf(region.layout).length > 0);
  };
  return (
    <If when={closed()}>
      <View style={{ paddingHorizontal: 8, paddingVertical: 4 }}>
        <DockToggle
          label={t("ui.buttons.expand_panel")}
          icon={dockIcon(props.side, false)}
          onPress={() => props.model.dock()?.setDockOpen(props.side, true)}
        />
      </View>
    </If>
  );
}

/// `layout` with `panel` taken out of every tab group.
function withoutPanel(layout: DockAreaLayout, panel: string): DockAreaLayout {
  const prune = (node: DockAreaLayout["center"]): DockAreaLayout["center"] => {
    if (node.type === "split") return { ...node, children: node.children.map(prune) };
    const panels = node.panels.filter((key) => key !== panel);
    return { ...node, panels, activeIndex: Math.max(0, Math.min(node.activeIndex, panels.length - 1)) };
  };
  const region = (dock: DockAreaLayout["right"]) => dock ? { ...dock, layout: prune(dock.layout) } : dock;
  return { ...layout, center: prune(layout.center), left: region(layout.left), right: region(layout.right), bottom: region(layout.bottom) };
}

function chartId(panel: string): number | null {
  const match = /^chart-(\d+)$/.exec(panel);
  return match ? Number(match[1]) : null;
}

function PanelTab(props: { model: StatsModel; tab: DockTab }) {
  const id = () => chartId(props.tab.panel);
  const title = () => {
    if (props.tab.panel === "overview") return text.overview;
    if (props.tab.panel === "ships") return text.ships;
    return props.model.charts().find((chart) => chart.id === id())?.title ?? props.tab.panel;
  };
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
      <Text style={{ fontSize: 12.5, color: props.tab.active ? colors.foreground : colors.mutedForeground }}>{title()}</Text>
      <If when={id() !== null}>
        <View
          role="button"
          accessibilityLabel={`Close ${title()}`}
          hoverStyle={{ backgroundColor: colors.muted }}
          onPress={() => closeChart(props.model, id()!)}
          style={{ width: 16, height: 16, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
        >
          <Icon name="x" size={11} color={colors.mutedForeground}/>
        </View>
      </If>
      <If when={props.tab.active && openDockOf(props.model.current(), props.tab.panel)}>
        <DockToggle
          label={t("ui.buttons.collapse_panel")}
          icon={dockIcon(openDockOf(props.model.current(), props.tab.panel)!, true)}
          onPress={() => {
            const side = openDockOf(props.model.current(), props.tab.panel);
            if (side) props.model.dock()?.setDockOpen(side, false);
          }}
        />
      </If>
    </View>
  );
}

function StatsPanel(props: { model: StatsModel; id: string }) {
  const id = chartId(props.id);
  if (props.id === "overview") return <OverviewPanel model={props.model}/>;
  if (props.id === "ships") return <ShipsPanel model={props.model}/>;
  if (id !== null) return <ChartPanel model={props.model} id={id}/>;
  return <View/>;
}

function Chip(props: { label?: string; icon?: IconName; accessibilityLabel?: string; tooltip?: string; selected: boolean; disabled?: boolean; onPress: () => void }) {
  const tint = () => (props.selected ? colors.primaryForeground : colors.secondaryForeground);
  return (
    <View
      tooltip={props.tooltip ? <Hint text={props.tooltip}/> : undefined}
      role="button"
      accessibilityLabel={props.accessibilityLabel ?? props.label}
      accessibilityState={{ selected: props.selected, disabled: props.disabled ?? false }}
      disabled={props.disabled}
      hoverStyle={{ opacity: 0.86 }}
      onPress={props.onPress}
      style={{
        minHeight: 26,
        paddingHorizontal: 9,
        justifyContent: "center",
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: props.selected ? colors.primary : colors.border,
        backgroundColor: props.selected ? colors.primary : colors.secondary,
        opacity: props.disabled ? 0.45 : 1,
      }}
    >
      <If when={props.icon}>
        <Icon name={props.icon!} size={12} color={tint()}/>
      </If>
      <If when={!props.icon}>
        <Text style={{ fontSize: 12, color: tint() }}>{props.label}</Text>
      </If>
    </View>
  );
}

function Check(props: { label: string; checked: boolean; disabled?: boolean; tooltip?: string; onChange: (checked: boolean) => void }) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      disabled={props.disabled}
      tooltip={props.tooltip}
      class="group flex-row items-center gap-2"
      indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
      indicator={<If when={props.checked}><Icon name="check" size={12} color={colors.primaryForeground}/></If>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <Text style={{ fontSize: 12.5, color: colors.foreground, opacity: props.disabled ? 0.5 : 1 }}>{props.label}</Text>
    </Checkbox>
  );
}

function Heading(props: { label: string }) {
  return <Text style={{ fontSize: 12, fontWeight: "600", color: colors.popoverForeground }}>{props.label}</Text>;
}

function Rule() {
  return <View style={{ width: 1, alignSelf: "stretch", marginVertical: 4, backgroundColor: colors.border }}/>;
}

function FilterBar(props: { model: StatsModel }) {
  const model = props.model;
  const stats = () => model.view()?.stats;
  const filters = () => stats()?.filters;
  const divisions = ["all", "solo", "division"] as const;
  return (
    <Toolbar accessibilityLabel="Stats filters" class="w-full">
      <View style={{ minHeight: 44, paddingHorizontal: 10, paddingVertical: 6, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.card }}>
        <Check
          label={text.limitToRecent}
          tooltip={text.limitToRecentTooltip}
          checked={filters()?.limitEnabled ?? false}
          onChange={(checked) => setFilters(model, { limitEnabled: checked })}
        />
        <NumberInput
          accessibilityLabel="Recent game count"
          value={filters()?.limitCount ?? 25}
          min={1}
          max={999}
          step={1}
          disabled={!filters()?.limitEnabled}
          onChange={({ value }) => {
            // A half-typed or empty box is not a limit of zero; the last
            // applied value stands until the field reads as a number again.
            if (value !== null && value !== undefined && value !== filters()?.limitCount) setFilters(model, { limitCount: Math.round(value) });
          }}
          style={{ width: 96, height: 28 }}
          inputStyle={inputStyle}
        />
        <Rule/>
        <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{text.division}</Text>
        <Each of={[0, 1, 2]}>
          {(index) => (
            <Chip
              label={text.divisionChoices[index()]}
              accessibilityLabel={`${text.division} ${text.divisionChoices[index()]}`}
              selected={filters()?.division === divisions[index()]}
              onPress={() => setFilters(model, { division: divisions[index()] })}
            />
          )}
        </Each>
        <If when={(stats()?.modes.length ?? 0) > 1}>
          <Rule/>
          <Popover
            placement="bottom"
            align="start"
            offset={4}
            trigger={<Chip
              label={stats()!.modeLabel}
              accessibilityLabel={`${text.mode} ${stats()!.modeSummary}`}
              tooltip={stats()!.modes.some((mode) => mode.selected) ? stats()!.modes.filter((mode) => mode.selected).map((mode) => mode.label).join(", ") : text.all}
              selected={stats()!.modes.some((mode) => mode.selected)}
              onPress={() => {}}
            />}
          >
            <Surface style={{ minWidth: 220, padding: 8, gap: 6, backgroundColor: colors.popover }}>
              <Text style={{ fontSize: 11, color: colors.mutedForeground }}>{text.mode}</Text>
              <Chip label={text.all} accessibilityLabel={`${text.mode} ${text.all}`} selected={!stats()!.modes.some((mode) => mode.selected)} onPress={() => setFilters(model, { gameModes: [] })}/>
              <Each of={stats()!.modes}>
                {(mode) => (
                  <Chip
                    label={mode().label}
                    selected={mode().selected}
                    onPress={() => setFilters(model, {
                      gameModes: mode().selected
                        ? filters()!.gameModes.filter((chosen) => chosen !== mode().id)
                        : [...filters()!.gameModes, mode().id],
                    })}
                  />
                )}
              </Each>
            </Surface>
          </Popover>
        </If>
        <Rule/>
        <Menu
          align="start"
          offset={4}
          margin={8}
          trigger={<Chip label={`+ ${text.addChart}`} accessibilityLabel={text.addChart} selected={false} onPress={() => {}}/>}
        >
          <Surface style={{ minWidth: 180, padding: 5, backgroundColor: colors.popover }}>
            <Each of={text.addChartChoices}>
              {(choice) => <MenuRow label={choice().label} onSelect={() => void addChart(model, choice().stat)}/>}
            </Each>
          </Surface>
        </Menu>
        <View style={{ flex: 1 }}/>
        <If when={model.persists && layoutError()}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text style={{ fontSize: 12, color: semantic.error }}>{layoutError()!.message}</Text>
            <Button label={text.retry} variant="ghost" onPress={() => void (layoutError()?.save ? keepCurrentLayout() : openSession())}/>
            <Button label={text.keepCurrentLayout} variant="ghost" onPress={() => void keepCurrentLayout()}/>
          </View>
        </If>
        <If when={model.saveError()}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text style={{ fontSize: 12, color: semantic.error }}>{model.saveError()}</Text>
            <If when={model.persists}>
              <Button label={text.retry} variant="ghost" onPress={() => void keepCurrentLayout()}/>
              <Button label={text.keepCurrentLayout} variant="ghost" onPress={() => void keepCurrentLayout()}/>
            </If>
          </View>
        </If>
        <If when={model.persists && clearError()}>
          <Text style={{ fontSize: 12, color: semantic.error }}>{clearError()}</Text>
        </If>
        <If when={model.persists}>
          <Button
            label={text.clear}
            variant="ghost"
            tooltip={text.clearTooltip}
            disabled={(stats()?.gameCount ?? 0) === 0}
            onPress={() => clearConfirmOpen.set(true)}
          />
        </If>
      </View>
    </Toolbar>
  );
}

function ClearSessionDialog() {
  return (
    <Dialog open={clearConfirmOpen()} onOpenChange={({ open }) => clearConfirmOpen.set(open)}>
      <Surface style={{ width: "100%", maxWidth: 420, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 18, fontWeight: "600", color: colors.popoverForeground }}>{text.clear}</Text>
        <Text style={{ fontSize: 13, lineHeight: 19, color: colors.mutedForeground }}>{text.clearConfirm}</Text>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={text.cancel} variant="ghost" onPress={() => clearConfirmOpen.set(false)}/>
          <Button
            label={text.clear}
            testID="stats-clear-confirm"
            variant="danger"
            onPress={() => {
              clearConfirmOpen.set(false);
              void Stats.clearSession().then(
                (view) => { session.setView(view); clearError.set(null); },
                (error) => clearError.set(t("ui.stats.clear_failed", { reason: errorMessage(error) })),
              );
            }}
          />
        </View>
      </Surface>
    </Dialog>
  );
}

function RatingBadge(props: { rating: RatingChip }) {
  return (
    <View tooltip={props.rating.category} style={{ paddingHorizontal: 4, borderRadius: 3, backgroundColor: `${props.rating.hue}2e` }}>
      <Text style={{ fontSize: 12.5, fontWeight: "600", color: colorScheme() === "dark" ? props.rating.darkText : props.rating.lightText }}>
        {props.rating.text}
      </Text>
    </View>
  );
}

function OverviewPanel(props: { model: StatsModel }) {
  const overview = () => props.model.view()?.stats.overview;
  const achievementsOpen = signal(true);
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={!overview()?.hasGames}>
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{text.noGames}</Text>
        </View>
      </If>
      <If when={overview()?.hasGames}>
        <View style={{ paddingHorizontal: 10, paddingVertical: 8, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
          <Text style={{ fontSize: 13, fontWeight: "600", color: colors.foreground }}>{overview()!.record}</Text>
          <If when={overview()!.rating}>
            <Rule/>
            <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{text.personalRating}</Text>
            <RatingBadge rating={{ ...overview()!.rating!, text: `${overview()!.rating!.text} (${overview()!.rating!.category})` }}/>
          </If>
          <Rule/>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{overview()!.frags}</Text>
          <If when={overview()!.bestFrags}>
            <Rule/>
            <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{overview()!.bestFrags}</Text>
          </If>
          <If when={overview()!.maxDamage}>
            <Rule/>
            <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{overview()!.maxDamage}</Text>
          </If>
        </View>
        <If when={overview()!.achievements.length > 0}>
          <View style={{ paddingHorizontal: 10, paddingVertical: 6, gap: 4, borderBottomWidth: 1, borderBottomColor: colors.border, maxHeight: achievementsOpen() ? 200 : undefined }}>
            <View
              role="button"
              accessibilityLabel={text.achievements}
              accessibilityState={{ expanded: achievementsOpen() }}
              onPress={() => achievementsOpen.set(!achievementsOpen())}
              style={{ flexDirection: "row", alignItems: "center", gap: 6 }}
            >
              <Icon name={achievementsOpen() ? "chevron-down" : "chevron-right"} size={12} color={colors.mutedForeground}/>
              <Text style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.foreground }}>{text.achievements}</Text>
              <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{overview()!.achievements.length.toFixed(0)}</Text>
            </View>
            <If when={achievementsOpen()}>
              <Scroll style={{ flexShrink: 1 }}>
                <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
                  <Each of={overview()!.achievements}>
                    {(achievement) => (
                      <View
                        tooltip={achievement().description ? `${achievement().name}: ${achievement().description}` : achievement().name}
                        accessibilityLabel={`${achievement().name} ${achievement().count}`}
                        style={{ width: 84, alignItems: "center", paddingVertical: 4 }}
                      >
                        <If when={achievement().icon}>
                          <Image src={achievement().icon!} accessibilityLabel={achievement().name} style={{ width: 48, height: 48 }}/>
                        </If>
                        <If when={!achievement().icon}>
                          <View style={{ width: 48, height: 48, borderRadius: 24, backgroundColor: colors.muted, alignItems: "center", justifyContent: "center" }}>
                            <Icon name="star" size={22} color={colors.mutedForeground}/>
                          </View>
                        </If>
                        <Text style={{ fontSize: 11, fontWeight: "600", color: colors.foreground }}>{achievement().count}</Text>
                        <Text style={{ fontSize: 11, textAlign: "center", color: colors.mutedForeground }}>{achievement().name}</Text>
                      </View>
                    )}
                  </Each>
                </View>
              </Scroll>
            </If>
          </View>
        </If>
        <Scroll horizontal style={{ flex: 1, minHeight: 0 }} contentStyle={{ minWidth: 640 }}>
          <View role="table" accessibilityLabel="Overview by ship" style={{ minWidth: 640, flex: 1 }}>
            <View role="row" style={{ flexDirection: "row", gap: 8, paddingHorizontal: 10, paddingVertical: 5, borderBottomWidth: 1, borderBottomColor: colors.border }}>
              <Text role="columnheader" style={{ width: 200, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{text.columnShip}</Text>
              <Text role="columnheader" style={{ width: 96, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{text.columnBattles}</Text>
              <Text role="columnheader" style={{ width: 96, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{text.columnWinRate}</Text>
              <Text role="columnheader" style={{ width: 96, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{text.columnAvgDamage}</Text>
              <Text role="columnheader" style={{ width: 96, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{text.columnAvgFrags}</Text>
            </View>
            <Scroll style={{ flex: 1 }}>
              <Each of={overview()!.ships}>
                {(ship, index) => (
                  <View role="row" style={{ height: 24, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 10, backgroundColor: index() % 2 === 1 ? colors.muted : "transparent" }}>
                    <Text role="cell" style={{ width: 200, fontSize: 12.5, color: colors.foreground }}>{ship().name}</Text>
                    <Text role="cell" style={{ width: 96, fontSize: 12.5, color: colors.foreground }}>{ship().battles}</Text>
                    <Text role="cell" style={{ width: 96, fontSize: 12.5, color: colors.foreground }}>{ship().winRate}</Text>
                    <Text role="cell" style={{ width: 96, fontSize: 12.5, color: colors.foreground }}>{ship().averageDamage}</Text>
                    <Text role="cell" style={{ width: 96, fontSize: 12.5, color: colors.foreground }}>{ship().averageFrags}</Text>
                  </View>
                )}
              </Each>
            </Scroll>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

function ShipsPanel(props: { model: StatsModel }) {
  const query = signal("");
  const expanded = signal<string[]>([]);
  const armed = signal<string | null>(null);
  const sections = () => props.model.view()?.stats.ships ?? [];
  const visible = () => {
    const key = Stats.searchKey(query());
    return key ? sections().filter((section) => section.searchKey.includes(key)) : sections();
  };
  // A confirmation only stands for the table it was asked about: new
  // figures are a new question.
  effect(() => {
    props.model.view();
    armed.set(null);
  });

  const clearShip = (section: ShipSection, skipConfirmation: boolean) => {
    if (!skipConfirmation && armed() !== section.shipId) {
      armed.set(section.shipId);
      return;
    }
    armed.set(null);
    void Stats.clearShip(section.shipId).then(
      (view) => { props.model.setView(view); clearError.set(null); },
      (error) => clearError.set(t("ui.stats.clear_failed", { reason: errorMessage(error) })),
    );
  };

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={sections().length === 0}>
        <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{text.noGames}</Text>
        </View>
      </If>
      <If when={sections().length > 0}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, padding: 8 }}>
          <Input
            accessibilityLabel={text.searchShips}
            value={query()}
            placeholder={text.searchShips}
            onChange={({ value }) => query.set(value)}
            style={{ ...inputStyle, flex: 1, minHeight: 28 }}
          />
          <If when={query()}>
            <Button label={text.clear} variant="ghost" onPress={() => query.set("")}/>
          </If>
          <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{`${visible().length}/${sections().length}`}</Text>
        </View>
        <If when={visible().length === 0}>
          <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
            <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{text.noShipMatches}</Text>
          </View>
        </If>
        <Scroll style={{ flex: 1 }}>
          <Each of={visible()}>
            {(section) => {
              const open = () => expanded().includes(section().shipId);
              return (
                <View>
                  <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8, paddingVertical: 4, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                    <View
                      role="button"
                      accessibilityLabel={section().header}
                      accessibilityState={{ expanded: open() }}
                      hoverStyle={{ backgroundColor: colors.accent }}
                      onPress={() => expanded.set(open() ? expanded().filter((id) => id !== section().shipId) : [...expanded(), section().shipId])}
                      style={{ flex: 1, minWidth: 0, flexDirection: "row", alignItems: "center", gap: 6, paddingVertical: 2, borderRadius: controlRadius() }}
                    >
                      <View style={{ width: 12 }}>
                        <Icon name={open() ? "chevron-down" : "chevron-right"} size={12} color={colors.mutedForeground}/>
                      </View>
                      <If when={section().nationFlag}>
                        <Image src={section().nationFlag!} style={{ width: 23, height: 16 }}/>
                      </If>
                      <If when={section().classIcon}>
                        <Image src={section().classIcon!} style={{ width: 16, height: 16, tintColor: colors.foreground, transform: [{ rotate: "90deg" }] }}/>
                      </If>
                      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                        <Text style={{ numberOfLines: 1, fontSize: 13, color: colors.foreground }}>{section().name}</Text>
                        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
                          <Text style={{ fontSize: 11.5, color: semantic.win }}>{section().wins}</Text>
                          <Text style={{ fontSize: 11.5, color: semantic.loss }}>{section().losses}</Text>
                          <If when={section().draws}>
                            <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{section().draws}</Text>
                          </If>
                          <If when={section().winRate}>
                            <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{section().winRate}</Text>
                          </If>
                          <If when={section().rating}>
                            <Text style={{ fontSize: 11.5, color: colorScheme() === "dark" ? section().rating!.darkText : section().rating!.lightText }}>
                              {`PR: ${section().rating!.text}`}
                            </Text>
                          </If>
                        </View>
                      </View>
                    </View>
                    <Menu
                      align="end"
                      offset={4}
                      margin={8}
                      trigger={
                        <View
                          role="button"
                          accessibilityLabel={`${text.copyTable}: ${section().name}`}
                          tooltip={<Hint text={text.copyTable}/>}
                          hoverStyle={{ backgroundColor: colors.muted }}
                          style={{ padding: 5, borderRadius: controlRadius() }}
                        >
                          <Icon name="copy" size={14} color={colors.mutedForeground}/>
                        </View>
                      }
                    >
                      <Surface style={{ minWidth: 180, padding: 5, backgroundColor: colors.popover }}>
                        <MenuRow label={text.copyMarkdown} onSelect={() => void copyText(section().markdown)}/>
                        <MenuRow label={text.copyCsv} onSelect={() => void copyText(section().csv)}/>
                      </Surface>
                    </Menu>
                    <If when={props.model.persists}>
                      <View
                        role="button"
                        accessibilityLabel={armed() === section().shipId ? `${text.clearShipConfirm}: ${section().clearLabel}` : section().clearLabel}
                        accessibilityState={{ selected: armed() === section().shipId }}
                        tooltip={armed() === section().shipId ? text.clearShipConfirm : section().clearLabel}
                        hoverStyle={{ backgroundColor: colors.accent }}
                        onPress={(event: { modifiers?: { control?: boolean; platform?: boolean } }) => clearShip(section(), Boolean(event?.modifiers?.control || event?.modifiers?.platform))}
                        style={{ minHeight: 26, paddingHorizontal: 8, justifyContent: "center", borderRadius: controlRadius(), backgroundColor: armed() === section().shipId ? colors.destructive : "transparent" }}
                      >
                        <Text style={{ fontSize: 12, color: armed() === section().shipId ? colors.primaryForeground : colors.mutedForeground }}>
                          {armed() === section().shipId ? text.clearShipConfirm : text.clear}
                        </Text>
                      </View>
                    </If>
                  </View>
                  <If when={open()}>
                    <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 628 }}>
                      <View role="table" accessibilityLabel={`${section().name} statistics`} style={{ minWidth: 628, paddingVertical: 4, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                        <View role="row" style={{ flexDirection: "row", gap: 8, paddingHorizontal: 8, paddingVertical: 3 }}>
                          <View style={{ width: 140 }}/>
                          <Each of={props.model.view()!.stats.columns}>
                            {(column) => <Text role="columnheader" style={{ width: 110, fontSize: 11.5, fontWeight: "600", color: colors.foreground }}>{column()}</Text>}
                          </Each>
                        </View>
                        <Each of={section().rows}>
                          {(row, index) => (
                            <View role="row" style={{ flexDirection: "row", gap: 8, paddingHorizontal: 8, paddingVertical: 2, backgroundColor: index() % 2 === 1 ? colors.muted : "transparent" }}>
                              <Text role="cell" style={{ width: 140, fontSize: 12.5, color: colors.foreground }}>{row().label}</Text>
                              <Each of={row().cells}>
                                {(cell) => (
                                  <View role="cell" style={{ width: 110, flexDirection: "row" }}>
                                    <If when={cell().rating}>
                                      <RatingBadge rating={cell().rating!}/>
                                    </If>
                                    <If when={!cell().rating}>
                                      <Text style={{ fontSize: 12.5, fontWeight: cell().emphasized ? "600" : "400", color: colors.foreground }}>{cell().text}</Text>
                                    </If>
                                  </View>
                                )}
                              </Each>
                            </View>
                          )}
                        </Each>
                      </View>
                    </Scroll>
                  </If>
                </View>
              );
            }}
          </Each>
        </Scroll>
      </If>
    </View>
  );
}

function ChartPanel(props: { model: StatsModel; id: number }) {
  const chart = () => props.model.charts().find((candidate) => candidate.id === props.id);
  const shipQuery = signal("");
  const visibleShips = () => {
    const current = chart();
    if (!current) return [];
    const key = Stats.searchKey(shipQuery());
    return key ? current.ships.filter((ship) => ship.searchKey.includes(key)) : current.ships;
  };
  const scope = props.model.scope;
  const update = (action: () => Promise<{ chart: ChartSnapshot; saveError?: string | null }>) => void runChart(props.model, props.id, action);
  const divisions = ["all", "solo", "division"] as const;

  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={chart()}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, paddingVertical: 4, borderBottomWidth: 1, borderBottomColor: colors.border }}>
          <Popover
            placement="bottom"
            align="start"
            offset={4}
            trigger={<Chip label={text.settings} accessibilityLabel={`${text.settings}: ${chart()!.title}`} selected={false} onPress={() => {}}/>}
          >
            <Surface style={{ minWidth: 240, maxWidth: 320, maxHeight: 420, backgroundColor: colors.popover }}>
              <Scroll style={{ maxHeight: 420 }}>
                <View style={{ padding: 8, gap: 8 }}>
                  <Heading label={text.statistic}/>
                  <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
                    <Each of={chart()!.stats}>
                      {(stat) => <Chip label={stat().label} selected={stat().selected} onPress={() => update(() => Stats.chartStat(scope, props.id, stat().stat))}/>}
                    </Each>
                  </View>
                  <Heading label={text.chart}/>
                  <View style={{ flexDirection: "row", gap: 4 }}>
                    <Chip label={text.chartLine} selected={chart()!.line} disabled={chart()!.combined} onPress={() => update(() => Stats.chartMode(scope, props.id, true))}/>
                    <Chip label={text.chartBar} selected={!chart()!.line} disabled={chart()!.combined} onPress={() => update(() => Stats.chartMode(scope, props.id, false))}/>
                  </View>
                  <Heading label={text.options}/>
                  <Check label={text.combineShips} checked={chart()!.combined} onChange={(checked) => update(() => Stats.chartCombined(scope, props.id, checked))}/>
                  <If when={chart()!.line}>
                    <Check label={text.runningAverage} checked={chart()!.running} disabled={chart()!.combined} onChange={(checked) => update(() => Stats.chartRunning(scope, props.id, checked))}/>
                  </If>
                  <Check label={text.valueLabels} checked={chart()!.showValues} onChange={(checked) => update(() => Stats.chartValues(scope, props.id, checked))}/>
                  <Heading label={text.filters}/>
                  <Check
                    label={text.ownFilters}
                    tooltip={text.ownFiltersTooltip}
                    checked={chart()!.overrides}
                    onChange={(checked) => update(() => Stats.chartOwnFilters(scope, props.id, checked))}
                  />
                  <If when={chart()!.overrides}>
                    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
                      <Each of={[0, 1, 2]}>
                        {(index) => (
                          <Chip
                            label={text.divisionChoices[index()]}
                            accessibilityLabel={`Chart ${text.division} ${text.divisionChoices[index()]}`}
                            selected={chart()!.division === divisions[index()]}
                            onPress={() => update(() => Stats.chartDivision(scope, props.id, divisions[index()]))}
                          />
                        )}
                      </Each>
                    </View>
                    <If when={chart()!.modes.length > 1}>
                      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
                        <Chip
                          label={text.all}
                          accessibilityLabel={`Chart ${text.mode} ${text.all}`}
                          selected={!chart()!.modes.some((mode) => mode.selected)}
                          onPress={() => update(() => Stats.chartClearModes(scope, props.id))}
                        />
                        <Each of={chart()!.modes}>
                          {(mode) => (
                            <Chip
                              label={mode().label}
                              accessibilityLabel={`Chart ${text.mode} ${mode().label}`}
                              selected={mode().selected}
                              onPress={() => update(() => Stats.chartToggleMode(scope, props.id, mode().id))}
                            />
                          )}
                        </Each>
                      </View>
                    </If>
                  </If>
                </View>
              </Scroll>
            </Surface>
          </Popover>
          <Popover
            placement="bottom"
            align="start"
            offset={4}
            trigger={
              <Chip
                label={`${text.ships} ${chart()!.selection}`}
                accessibilityLabel={`${text.filterShips}: ${chart()!.selection}`}
                selected={!chart()!.allSelected}
                onPress={() => {}}
              />
            }
          >
            <Surface style={{ minWidth: 240, maxWidth: 320, maxHeight: 420, backgroundColor: colors.popover }}>
              <View style={{ padding: 8, gap: 8 }}>
                <View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between" }}>
                  <Heading label={text.ships}/>
                  <Text style={{ fontSize: 11, color: colors.mutedForeground }}>{chart()!.selection}</Text>
                </View>
                <View style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
                  <Input
                    accessibilityLabel={`${text.searchShips} ${chart()!.title}`}
                    value={shipQuery()}
                    placeholder={text.searchShips}
                    onChange={({ value }) => shipQuery.set(value)}
                    style={{ ...inputStyle, flex: 1, minHeight: 28 }}
                  />
                  <If when={shipQuery()}>
                    <Chip icon="x" accessibilityLabel={text.clear} selected={false} onPress={() => shipQuery.set("")}/>
                  </If>
                </View>
                <View style={{ flexDirection: "row", gap: 4 }}>
                  <Chip
                    label={text.allShips}
                    accessibilityLabel={text.allShipsTooltip}
                    selected={false}
                    disabled={visibleShips().length === 0}
                    onPress={() => update(() => Stats.chartSelectShips(scope, props.id, visibleShips().map((ship) => ship.shipId), true))}
                  />
                  <Chip
                    label={text.noShips}
                    accessibilityLabel={text.noShipsTooltip}
                    selected={false}
                    disabled={visibleShips().length === 0}
                    onPress={() => update(() => Stats.chartSelectShips(scope, props.id, visibleShips().map((ship) => ship.shipId), false))}
                  />
                </View>
                <If when={shipQuery() && visibleShips().length === 0}>
                  <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{text.noShipMatches}</Text>
                </If>
                <Scroll style={{ maxHeight: 300 }}>
                  <View style={{ gap: 4 }}>
                    <Each of={visibleShips()}>
                      {(ship) => <Check label={ship().name} checked={ship().selected} onChange={() => update(() => Stats.chartToggleShip(scope, props.id, ship().shipId))}/>}
                    </Each>
                  </View>
                </Scroll>
              </View>
            </Surface>
          </Popover>
          <View style={{ flex: 1 }}/>
          <If when={!chart()!.empty}>
            <Button label={text.copyImage} variant="ghost" onPress={() => void copyChartImage(props.model, props.id)}/>
          </If>
          <If when={chart()!.viewMoved}>
            <Button
              label={text.resetView}
              variant="ghost"
              onPress={() => {
                try {
                  props.model.setChart(Stats.chartResetView(scope, props.id));
                } catch (error) {
                  props.model.setSaveError(errorMessage(error));
                }
              }}
            />
          </If>
        </View>
        <Text style={{ paddingVertical: 4, textAlign: "center", fontSize: 13, fontWeight: "600", color: colors.foreground }}>{chart()!.valueLabel}</Text>
        <View style={{ flex: 1, minHeight: 0, padding: 8 }}>
          <If when={chart()!.empty}>
            <View style={{ flex: 1, alignItems: "center", justifyContent: "center" }}>
              <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{text.nothingToPlot}</Text>
            </View>
          </If>
          <If when={!chart()!.empty}>
            <Frame surface={chart()!.surface} accessibilityLabel={`${chart()!.title} chart`}/>
          </If>
        </View>
      </If>
    </View>
  );
}
