import type { JSX } from "nsh";
import { Clipboard, colorScheme, effect, Scroll, signal, Text, View } from "nsh";
import { Checkbox, Dialog, DockArea, Input, Menu, NumberInput, Popover, Toolbar, type DockAreaLayout, type DockTab } from "nsh/base";
import { Stats, type ChartSnapshot, type FilterState, type RatingChip, type ScopeView, type ShipSection, type StatsSnapshot } from "wows-toolkit/native";
import { Button, EmptyState, errorMessage, inputStyle, MenuRow, Surface } from "./components";
import { Frame } from "./frame";
import { colors, controlRadius } from "./theme";

const text = Stats.text();

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
  layout: () => DockAreaLayout | null;
  setLayout: (layout: DockAreaLayout) => void;
};

export function createStatsModel(scope: string, persists: boolean): StatsModel {
  const view = signal<ScopeView | null>(null);
  const charts = signal<ChartSnapshot[]>([]);
  const saveError = signal<string | null>(null);
  const layout = signal<DockAreaLayout | null>(null);
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
    setLayout: (next) => layout.set(next),
  };
  return model;
}

const session = createStatsModel("session", true);
const sessionLoading = signal(false);
const sessionError = signal<string | null>(null);
const layoutError = signal<string | null>(null);
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
    layoutError.set(opened.layoutError ?? null);
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

function showNotice(message: string) {
  clearTimeout(noticeTimer);
  notice.set(message);
  noticeTimer = setTimeout(() => notice.set(null), NOTICE_MS);
}

async function run(model: StatsModel, action: () => Promise<ScopeView>) {
  try {
    model.setView(await action());
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

async function runChart(model: StatsModel, action: () => Promise<{ chart: ChartSnapshot; saveError?: string | null }>) {
  try {
    const update = await action();
    model.setChart(update.chart);
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
  model.setLayout(layout);
  if (!model.persists) return;
  clearTimeout(layoutTimer);
  layoutTimer = setTimeout(() => {
    layoutTimer = undefined;
    Stats.saveLayout(layout).then(
      () => { if (layoutError()) layoutError.set(null); },
      (error) => layoutError.set(`Chart layout could not be saved: ${errorMessage(error)}`),
    );
  }, LAYOUT_SAVE_DELAY_MS);
}

async function addChart(model: StatsModel, stat: string) {
  try {
    const update = await Stats.addChart(model.scope, stat);
    model.setChart(update.chart);
    const layout = model.layout();
    if (layout) layoutChanged(model, Stats.layoutWithChart(layout, update.chart.id));
    if (update.saveError) model.setSaveError(update.saveError);
  } catch (error) {
    model.setSaveError(errorMessage(error));
  }
}

async function closeChart(model: StatsModel, id: number) {
  const layout = model.layout();
  if (layout) layoutChanged(model, Stats.layoutWithoutChart(layout, id));
  await chartRemoved(model, id);
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
  const layout = session.layout();
  if (!layout) return;
  try {
    await Stats.keepCurrentLayout(layout);
    layoutError.set(null);
    session.setSaveError(null);
  } catch (error) {
    layoutError.set(errorMessage(error));
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
      <If when={sessionLoading() && !session.view()}>
        <EmptyState title="Loading statistics" description="Reading recorded battles."/>
      </If>
      <If when={!sessionLoading() && sessionError() && !session.view()}>
        <EmptyState title="Statistics unavailable" description={sessionError()!} action={<Button label={text.retry} onPress={() => void openSession()}/>}/>
      </If>
      <If when={session.view()}>
        <StatsScopeView model={session} header={null}/>
      </If>
      <ClearSessionDialog/>
      <If when={notice()}>
        <Surface style={{ position: "absolute", right: 16, bottom: 16, minWidth: 220, maxWidth: 420, padding: 12, backgroundColor: colors.popover }}>
          <Text accessibilityLiveRegion="polite" style={{ fontSize: 13, color: colors.popoverForeground }}>{notice()}</Text>
        </Surface>
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
        <If when={model.layout()}>
          <DockArea
            layout={model.layout()!}
            onLayoutChange={({ layout }) => layoutChanged(model, layout)}
            onPanelRemove={({ panel }) => {
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
        </If>
      </View>
    </View>
  );
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
          onPress={() => void closeChart(props.model, id()!)}
          style={{ width: 16, height: 16, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
        >
          <Text style={{ fontSize: 11, color: colors.mutedForeground }}>x</Text>
        </View>
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

function Chip(props: { label: string; accessibilityLabel?: string; selected: boolean; disabled?: boolean; onPress: () => void }) {
  return (
    <View
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
      <Text style={{ fontSize: 12, color: props.selected ? colors.primaryForeground : colors.secondaryForeground }}>{props.label}</Text>
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
      indicator={<Text class="text-[10px] text-primary-foreground">{props.checked ? "x" : ""}</Text>}
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
    <Toolbar accessibilityLabel="Stats filters">
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
            trigger={<Chip label={stats()!.modeLabel} accessibilityLabel={`${text.mode} ${stats()!.modeSummary}`} selected={stats()!.modes.some((mode) => mode.selected)} onPress={() => {}}/>}
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
            <Text style={{ fontSize: 12, color: semantic.error }}>{layoutError()}</Text>
            <Button label={text.retry} variant="ghost" onPress={() => void openSession()}/>
            <Button label={text.keepCurrentLayout} variant="ghost" onPress={() => void keepCurrentLayout()}/>
          </View>
        </If>
        <If when={model.saveError()}>
          <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
            <Text style={{ fontSize: 12, color: semantic.error }}>{model.saveError()}</Text>
            <Button label={text.keepCurrentLayout} variant="ghost" onPress={() => void keepCurrentLayout()}/>
          </View>
        </If>
        <If when={model.persists}>
          <Button
            label={text.clear}
            variant="ghost"
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
            label={`${text.clear} all`}
            variant="danger"
            onPress={() => {
              clearConfirmOpen.set(false);
              void Stats.clearSession().then(
                (view) => { session.setView(view); session.setSaveError(null); },
                (error) => session.setSaveError(`Not cleared: ${errorMessage(error)}`),
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
              <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{achievementsOpen() ? "v" : ">"}</Text>
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
                        <View style={{ width: 32, height: 32, borderRadius: 16, backgroundColor: colors.muted, alignItems: "center", justifyContent: "center" }}>
                          <Text style={{ fontSize: 14, color: colors.mutedForeground }}>*</Text>
                        </View>
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
    void run(props.model, () => Stats.clearShip(section.shipId));
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
                      <Text style={{ width: 12, fontSize: 12, color: colors.mutedForeground }}>{open() ? "v" : ">"}</Text>
                      <View style={{ flex: 1, minWidth: 0, gap: 2 }}>
                        <Text numberOfLines={1} style={{ fontSize: 13, color: colors.foreground }}>{section().name}</Text>
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
                      trigger={<Chip label="Copy" accessibilityLabel={`${text.copyTable}: ${section().name}`} selected={false} onPress={() => {}}/>}
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
                              <Text role="rowheader" style={{ width: 140, fontSize: 12.5, color: colors.foreground }}>{row().label}</Text>
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
  const update = (action: () => Promise<{ chart: ChartSnapshot; saveError?: string | null }>) => void runChart(props.model, action);
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
                    <Chip label="x" accessibilityLabel={text.clear} selected={false} onPress={() => shipQuery.set("")}/>
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
