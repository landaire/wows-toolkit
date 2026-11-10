import type { JSX } from "nsh";
import { Clipboard, colorScheme, Image, List, openUrl, Scroll, signal, Text, View } from "nsh";
import { AlertDialog, Checkbox, DockArea, Input, Menu, Popover, Tooltip } from "nsh/base";
import {
  PlayerTracker,
  type ClanColumn,
  type ClanRowView,
  type ClansTable,
  type LiveView,
  type Period,
  type PlayerColumn,
  type PlayerRowView,
  type PlayersTable,
  type RatingBand,
  type RosterRowView,
  type Scope,
  type ScopeCellsView,
  type Severity,
  type StatsMark,
  type TeamView,
  type Tint,
  type TrackerControls,
  type TrackerLabels,
  type ViewMode,
} from "wows-toolkit/native";
import { Button, errorMessage, inputStyle, MenuRow, Surface } from "./components";
import { colors, controlRadius } from "./theme";

/** What the tracker asks of the shell around it. */
export type PlayerTrackerProps = {
  /** Shows the Search tab running `query`. */
  onSearch: (query: string) => void;
  /** Tells the reader something happened (a copy, a failure). */
  onNotice: (message: string) => void;
};

const labels = signal<TrackerLabels | null>(null);
const controls = signal<TrackerControls | null>(null);
const players = signal<PlayersTable | null>(null);
const clans = signal<ClansTable | null>(null);
const live = signal<LiveView | null>(null);
const failure = signal<string | null>(null);
const refreshing = signal(false);
const clansLoading = signal(false);
const clansShown = signal(false);
const filterDraft = signal("");
const expandedPlayers = signal<Record<string, boolean>>({});
const expandedClans = signal<Record<string, boolean>>({});
const editingNote = signal<string | null>(null);
const noteDraft = signal("");
let opened = false;
let liveRevision = -1;
let tabShown = false;
let currentMatchActive = false;
let filterTimer: number | undefined;

/** How often the Current Match section asks whether the battle changed. */
const LIVE_POLL_MS = 1000;
/** How long typing rests before the filter is applied and saved. */
const FILTER_DEBOUNCE_MS = 200;

/** Opens the tracker the first time its tab is shown, and on every showing
 * restarts the battle watch if the WoWs directory has changed since. */
export function activatePlayerTracker(locale: string) {
  if (!opened) {
    opened = true;
    void open(locale);
    setInterval(() => void pollLive(), LIVE_POLL_MS);
  }
  void PlayerTracker.syncLive().catch((error) => failure.set(errorMessage(error)));
}

/** Records whether the tracker's tab is the one on screen. */
export function setPlayerTrackerShown(shown: boolean) {
  tabShown = shown;
  syncCurrentMatchShown();
}

/** The battle's stats lookup is sent only while the Current Match section
 * is on screen, as the GPUI tracker sends it. */
function syncCurrentMatchShown() {
  PlayerTracker.setCurrentMatchShown(tabShown && currentMatchActive);
}

async function open(locale: string) {
  try {
    const first = await PlayerTracker.open(locale);
    labels.set(first.labels);
    controls.set(first.controls);
    filterDraft.set(first.controls.filter);
    await refresh();
  } catch (error) {
    failure.set(errorMessage(error));
  }
}

/** Re-reads the index for the period on screen: both tables start over. */
async function refresh() {
  refreshing.set(true);
  failure.set(null);
  try {
    controls.set(await PlayerTracker.refresh());
  } catch (error) {
    failure.set(errorMessage(error));
  } finally {
    refreshing.set(false);
  }
  await reloadPlayers();
  await reloadClans();
  await reloadLive();
}

async function reloadPlayers() {
  try {
    players.set(await PlayerTracker.players());
  } catch (error) {
    failure.set(errorMessage(error));
  }
}

/** Reads the clans table, first running its index reads when the table is
 * on screen and they have not run since the last refresh. */
async function reloadClans() {
  try {
    const table = await PlayerTracker.clans();
    clans.set(table);
    if (table.needsLoad && clansShown() && !clansLoading()) {
      clansLoading.set(true);
      try {
        await PlayerTracker.loadClans();
      } finally {
        clansLoading.set(false);
      }
      clans.set(await PlayerTracker.clans());
    }
  } catch (error) {
    failure.set(errorMessage(error));
  }
}

async function reloadLive() {
  try {
    live.set(await PlayerTracker.liveView());
  } catch (error) {
    failure.set(errorMessage(error));
  }
}

async function pollLive() {
  const revision = PlayerTracker.liveRevision();
  if (revision === liveRevision) return;
  liveRevision = revision;
  await reloadLive();
}

/** Applies a control change, then redraws what it affects. */
async function change(apply: () => Promise<TrackerControls>, redraw: { players?: boolean; clans?: boolean; live?: boolean }) {
  try {
    controls.set(await apply());
  } catch (error) {
    failure.set(errorMessage(error));
  }
  if (redraw.players) await reloadPlayers();
  if (redraw.clans) await reloadClans();
  if (redraw.live) await reloadLive();
}

async function selectPeriod(period: Period) {
  if (controls()?.period === period) return;
  refreshing.set(true);
  players.set(null);
  await change(() => PlayerTracker.setPeriod(period), { players: true, clans: true, live: true });
  refreshing.set(false);
}

function typeFilter(value: string) {
  filterDraft.set(value);
  clearTimeout(filterTimer);
  filterTimer = setTimeout(() => {
    filterTimer = undefined;
    void change(() => PlayerTracker.setFilter(value), { players: true, clans: true });
  }, FILTER_DEBOUNCE_MS);
}

function toggleDivisionMates(show: boolean) {
  void change(() => PlayerTracker.setShowDivisionMates(show), { players: true, clans: true, live: true });
}

function togglePlayer(accountId: string) {
  const next = { ...expandedPlayers() };
  if (next[accountId]) {
    delete next[accountId];
    if (editingNote() === accountId) editingNote.set(null);
  } else {
    next[accountId] = true;
  }
  expandedPlayers.set(next);
}

function toggleClan(clan: string) {
  const next = { ...expandedClans() };
  if (next[clan]) delete next[clan];
  else next[clan] = true;
  expandedClans.set(next);
}

/** Opens `row`'s note for editing, which opens the row it belongs to. */
function editNote(row: PlayerRowView) {
  noteDraft.set(row.note);
  editingNote.set(row.accountId);
  expandedPlayers.set({ ...expandedPlayers(), [row.accountId]: true });
}

/** Saved on Enter or when the field loses focus, never per keystroke. */
function commitNote(accountId: string) {
  if (editingNote() !== accountId) return;
  const note = noteDraft();
  void change(() => PlayerTracker.saveNote(accountId, note), { players: true, live: true });
}

async function confirmClear() {
  const text = labels();
  if (!text) return;
  const { action } = await AlertDialog.show({
    title: text.clear,
    description: text.clearConfirm,
    actions: [
      { label: text.cancel, role: "cancel", id: "cancel" },
      { label: text.clear, role: "destructive", id: "clear" },
    ],
  });
  if (action !== "clear") return;
  try {
    await PlayerTracker.clear();
  } catch (error) {
    failure.set(errorMessage(error));
    return;
  }
  editingNote.set(null);
  await reloadPlayers();
  await reloadClans();
  await reloadLive();
}

function copy(text: string, notice: string, props: PlayerTrackerProps) {
  void Clipboard.writeText(text);
  props.onNotice(notice);
}

/** The GPUI app's semantic tones, by appearance. */
function semantic() {
  return colorScheme() === "dark"
    ? { win: "#6fd98a", loss: "#ea7078", warn: "#e8a54a", division: "#e5c158", abuser: "#f09bc0", strong: "#e8e5dc" }
    : { win: "#0c5027", loss: "#601118", warn: "#8a4b00", division: "#775800", abuser: "#a33270", strong: "#0a0a08" };
}

/** The tone a number of encounters is read in; none for one not worth marking. */
function severityColor(severity: Severity): string | undefined {
  switch (severity) {
    case "noted": return semantic().division;
    case "warned": return semantic().warn;
    case "heavy": return semantic().loss;
    default: return undefined;
  }
}

function tintColor(tint: Tint): string {
  switch (tint) {
    case "selfPlayer": return semantic().strong;
    case "ally": return semantic().win;
    case "enemy": return semantic().loss;
    case "divisionMate": return semantic().division;
    case "abuser": return semantic().abuser;
  }
}

function Hint(props: { text: string }) {
  return (
    <Tooltip delay={400} class="max-w-[360px] rounded-md border border-border bg-popover px-2 py-1">
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{props.text}</Text>
    </Tooltip>
  );
}

/** A small text control: a caret, a find button, a chip. */
function Chip(props: { label: string; accessibilityLabel: string; hint?: string; color?: string; onPress: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.accessibilityLabel}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      tooltip={props.hint ? <Hint text={props.hint}/> : undefined}
      style={{ minWidth: 20, height: 22, paddingHorizontal: 5, borderRadius: 4, alignItems: "center", justifyContent: "center" }}
    >
      <Text style={{ fontSize: 12, color: props.color ?? colors.mutedForeground }}>{props.label}</Text>
    </View>
  );
}

function Toggle(props: { label: string; selected: boolean; onPress: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      accessibilityState={{ selected: props.selected }}
      onPress={props.onPress}
      hoverStyle={{ opacity: 0.86 }}
      style={{
        height: 26,
        paddingHorizontal: 10,
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: props.selected ? colors.primary : colors.border,
        backgroundColor: props.selected ? colors.primary : "transparent",
        justifyContent: "center",
      }}
    >
      <Text style={{ fontSize: 12, color: props.selected ? colors.primaryForeground : colors.foreground }}>{props.label}</Text>
    </View>
  );
}

function Status(props: { text: string }) {
  return (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 24 }}>
      <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{props.text}</Text>
    </View>
  );
}

function SortHeader(props: { label: string; active: boolean; descending: boolean; width: number; onPress: () => void }) {
  return (
    <View
      role="columnheader"
      accessibilityLabel={props.label}
      accessibilityState={{ selected: props.active }}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ width: props.width, minHeight: 30, justifyContent: "center", paddingHorizontal: 4, borderRadius: 4 }}
    >
      <Text style={{ fontSize: 12, fontWeight: "600", color: props.active ? colors.foreground : colors.mutedForeground }}>
        {props.active ? `${props.label} (${props.descending ? "desc" : "asc"})` : props.label}
      </Text>
    </View>
  );
}

const PLAYER_WIDTHS: Record<PlayerColumn, number> = { name: 220, clan: 120, totalEncounters: 130, encounters: 180, lastEncountered: 200 };
const CLAN_WIDTHS: Record<ClanColumn, number> = { clan: 160, members: 100, encounters: 100, encountersInRange: 180, sightings: 100, lastEncountered: 200 };

function TrackerToolbar() {
  const text = () => labels()!;
  const current = () => controls()!;
  const periodLabel = () => current().periods.find((option) => option.period === current().period)?.label ?? "";
  return (
    <View style={{ minHeight: 48, paddingHorizontal: 12, flexDirection: "row", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border }}>
      <Menu
        align="start"
        offset={5}
        margin={8}
        trigger={
          <View accessibilityLabel={text().timePeriod} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 30, minWidth: 150, paddingHorizontal: 10, borderWidth: 1, borderColor: colors.border, borderRadius: controlRadius(), justifyContent: "center" }}>
            <Text style={{ fontSize: 12.5, color: colors.foreground }}>{periodLabel()}</Text>
          </View>
        }
      >
        <Surface style={{ minWidth: 190, padding: 5, backgroundColor: colors.popover }}>
          <Each of={current().periods}>
            {(option) => (
              <MenuRow
                label={option().label}
                note={option().period === current().period ? "x" : undefined}
                onSelect={() => void selectPeriod(option().period)}
              />
            )}
          </Each>
        </Surface>
      </Menu>
      <View style={{ width: 240 }}>
        <Input
          accessibilityLabel={text().playerFilter}
          value={filterDraft()}
          placeholder={text().filterHint}
          onChange={({ value }) => typeFilter(value)}
          style={inputStyle}
        />
      </View>
      <View style={{ flex: 1 }}/>
      <View tooltip={<Hint text={text().populate}/>}>
        <Button label={refreshing() ? `${text().populate}...` : text().populate} variant="ghost" disabled={refreshing()} onPress={() => void refresh()}/>
      </View>
      <Button label={text().clear} variant="ghost" onPress={() => void confirmClear()}/>
      <View tooltip={<Hint text={text().showDivisionMatesHover}/>}>
        <Checkbox
          accessibilityLabel={text().showDivisionMates}
          checked={current().showDivisionMates}
          class="group flex-row items-center gap-2"
          indicatorClass="w-[16px] h-[16px] rounded-[4px] border border-border items-center justify-center checked:bg-primary"
          indicator={<Text class="text-[10px] text-primary-foreground">{current().showDivisionMates ? "x" : ""}</Text>}
          onChange={({ checked }) => toggleDivisionMates(checked)}
        >
          <Text style={{ fontSize: 12.5, color: colors.foreground }}>{text().showDivisionMates}</Text>
        </Checkbox>
      </View>
    </View>
  );
}

function PlayerDetail(props: { row: PlayerRowView; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  const editing = () => editingNote() === props.row.accountId;
  return (
    <View testID={`player-detail-${props.row.accountId}`} style={{ paddingLeft: 228, paddingRight: 8, paddingVertical: 6, gap: 4 }}>
      <View style={{ flexDirection: "row" }}>
        <Chip
          label={props.row.accountId}
          accessibilityLabel={text().copyAccountId}
          hint={text().copyAccountId}
          color={colors.foreground}
          onPress={() => copy(props.row.accountId, text().copiedAccountId.replace("%{id}", props.row.accountId), props.tracker)}
        />
      </View>
      <If when={props.row.detail?.aliases}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.row.detail!.aliases!}</Text>
      </If>
      <If when={props.row.detail?.lastEncountered}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.row.detail!.lastEncountered!}</Text>
      </If>
      <If when={props.row.detail}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.row.detail!.matchCount}</Text>
      </If>
      <If when={!editing()}>
        <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{text().notesHint}</Text>
      </If>
      <If when={editing()}>
        <Input
          accessibilityLabel={text().notesHint}
          value={noteDraft()}
          placeholder={text().notesHint}
          autoFocus
          onChange={({ value }) => noteDraft.set(value)}
          onSubmit={() => commitNote(props.row.accountId)}
          onBlur={() => commitNote(props.row.accountId)}
          style={inputStyle}
        />
        <If when={controls()?.noteError}>
          <Text style={{ fontSize: 11.5, color: colors.destructive }}>{controls()!.noteError!}</Text>
        </If>
      </If>
    </View>
  );
}

function PlayerRow(props: { row: () => PlayerRowView; index: () => number; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  const row = () => props.row();
  const open = () => expandedPlayers()[row().accountId] === true;
  const tone = () => severityColor(row().severity);
  return (
    <View testID={`player-row-${props.index()}`} style={{ backgroundColor: props.index() % 2 === 1 ? colors.muted : "transparent" }}>
      <View role="row" accessibilityLabel={row().name} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 28, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8 }}>
        <View style={{ width: PLAYER_WIDTHS.name, flexDirection: "row", alignItems: "center", gap: 4 }}>
          <Chip label={open() ? "v" : ">"} accessibilityLabel={`${open() ? "Collapse" : "Expand"} ${row().name}`} onPress={() => togglePlayer(row().accountId)}/>
          <Text numberOfLines={1} style={{ flex: 1, fontSize: 13, color: colors.foreground }}>{row().name}</Text>
        </View>
        <Text numberOfLines={1} style={{ width: PLAYER_WIDTHS.clan, fontSize: 13, color: colors.mutedForeground }}>{row().clan}</Text>
        <Text style={{ width: PLAYER_WIDTHS.totalEncounters, fontSize: 13, color: tone() ?? colors.foreground }}>{row().totalEncounters}</Text>
        <Text style={{ width: PLAYER_WIDTHS.encounters, fontSize: 13, color: tone() ?? colors.foreground }}>{row().encountersInRange}</Text>
        <View style={{ width: PLAYER_WIDTHS.lastEncountered }} tooltip={row().lastSeenExact ? <Hint text={row().lastSeenExact}/> : undefined}>
          <Text numberOfLines={1} style={{ fontSize: 13, color: colors.mutedForeground }}>{row().lastSeen}</Text>
        </View>
        <Chip
          label={text().findMatches}
          accessibilityLabel={`${text().findMatches} ${row().name}`}
          onPress={() => props.tracker.onSearch(PlayerTracker.playerMatchesQuery(row().accountId))}
        />
        <View
          role="button"
          accessibilityLabel={`${text().notesHint} ${row().name}`}
          onPress={() => editNote(row())}
          hoverStyle={{ backgroundColor: colors.accent }}
          tooltip={<Hint text={row().note || text().notesHint}/>}
          style={{ flex: 1, minWidth: 120, height: 22, paddingHorizontal: 5, borderRadius: 4, justifyContent: "center" }}
        >
          <Text numberOfLines={1} style={{ fontSize: 12, color: row().note ? colors.foreground : colors.mutedForeground }}>
            {row().note || text().notesHint}
          </Text>
        </View>
      </View>
      <If when={open()}>
        <PlayerDetail row={row()} tracker={props.tracker}/>
      </If>
    </View>
  );
}

function PlayersSection(props: { tracker: PlayerTrackerProps }) {
  const current = () => controls()!;
  const table = () => players();
  const status = () => refreshing() && !table() ? labels()!.loadingPlayers : table()?.status;
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View role="row" style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <Each of={current().playerColumns}>
          {(header) => (
            <SortHeader
              label={header().label}
              active={header().active}
              descending={header().descending}
              width={PLAYER_WIDTHS[header().column]}
              onPress={() => void change(() => PlayerTracker.sortPlayers(header().column), { players: true })}
            />
          )}
        </Each>
      </View>
      <If when={status()}>
        <Status text={status()!}/>
      </If>
      <If when={!status() && table()}>
        <List each={table()!.rows} estimatedItemSize={28} keyExtractor={(row) => row.accountId} style={{ flex: 1 }}>
          {(row, index) => <PlayerRow row={row} index={index} tracker={props.tracker}/>}
        </List>
      </If>
    </View>
  );
}

function ClanRow(props: { row: () => ClanRowView; index: () => number; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  const row = () => props.row();
  const open = () => expandedClans()[row().clan] === true;
  const tone = () => severityColor(row().severity);
  return (
    <View style={{ backgroundColor: props.index() % 2 === 1 ? colors.muted : "transparent" }}>
      <View role="row" accessibilityLabel={row().clan} hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 28, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8 }}>
        <View style={{ width: CLAN_WIDTHS.clan, flexDirection: "row", alignItems: "center", gap: 4 }}>
          <Chip label={open() ? "v" : ">"} accessibilityLabel={`${open() ? "Collapse" : "Expand"} ${row().clan}`} onPress={() => toggleClan(row().clan)}/>
          <Text numberOfLines={1} style={{ flex: 1, fontSize: 13, color: tone() ?? colors.foreground }}>{row().clan}</Text>
        </View>
        <Text style={{ width: CLAN_WIDTHS.members, fontSize: 13, color: colors.foreground }}>{row().members}</Text>
        <Text style={{ width: CLAN_WIDTHS.encounters, fontSize: 13, color: colors.foreground }}>{row().matches}</Text>
        <Text style={{ width: CLAN_WIDTHS.encountersInRange, fontSize: 13, color: tone() ?? colors.foreground }}>{row().matchesInRange}</Text>
        <View style={{ width: CLAN_WIDTHS.sightings }} tooltip={<Hint text={row().sightingsHover}/>}>
          <Text style={{ fontSize: 13, color: colors.foreground }}>{row().sightings}</Text>
        </View>
        <View style={{ width: CLAN_WIDTHS.lastEncountered }} tooltip={<Hint text={row().lastSeenExact}/>}>
          <Text numberOfLines={1} style={{ fontSize: 13, color: colors.mutedForeground }}>{row().lastSeen}</Text>
        </View>
        <Chip
          label={text().findMatches}
          accessibilityLabel={`${text().findMatches} ${row().clan}`}
          hint={text().findClanMatches}
          onPress={() => props.tracker.onSearch(PlayerTracker.clanMatchesQuery(row().clan))}
        />
      </View>
      <If when={open()}>
        <View testID={`clan-members-${row().clan}`} style={{ paddingLeft: CLAN_WIDTHS.clan + 8, paddingVertical: 4, gap: 2 }}>
          <Each of={row().memberList}>
            {(member) => (
              <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
                <Chip
                  label={text().findMatches}
                  accessibilityLabel={`${text().findMatches} ${member().name}`}
                  onPress={() => props.tracker.onSearch(PlayerTracker.playerMatchesQuery(member().accountId))}
                />
                <Text style={{ fontSize: 12, color: colors.foreground }}>{member().name}</Text>
                <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{member().matches}</Text>
              </View>
            )}
          </Each>
        </View>
      </If>
    </View>
  );
}

function ClansSection(props: { tracker: PlayerTrackerProps }) {
  const current = () => controls()!;
  const table = () => clans();
  const status = () => clansLoading() ? labels()!.loadingClans : table()?.status;
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View role="row" style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <Each of={current().clanColumns}>
          {(header) => (
            <SortHeader
              label={header().label}
              active={header().active}
              descending={header().descending}
              width={CLAN_WIDTHS[header().column]}
              onPress={() => void change(() => PlayerTracker.sortClans(header().column), { clans: true })}
            />
          )}
        </Each>
      </View>
      <If when={status()}>
        <Status text={status()!}/>
      </If>
      <If when={!status() && table()}>
        <List each={table()!.rows} estimatedItemSize={28} keyExtractor={(row) => row.clan} style={{ flex: 1 }}>
          {(row, index) => <ClanRow row={row} index={index} tracker={props.tracker}/>}
        </List>
      </If>
    </View>
  );
}

/** The possible-stream-sniper chip: one login copies on press, several open
 * a picker, since copying an arbitrary one of them would be a guess. */
function TwitchChip(props: { row: RosterRowView; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  const copyLogin = (login: string) => copy(login, text().copiedLogin.replace("%{name}", login), props.tracker);
  const chip = (onPress?: () => void) => (
    <View
      role="button"
      accessibilityLabel={`Twitch ${props.row.name}`}
      onPress={onPress}
      tooltip={<Hint text={props.row.twitchHover}/>}
      style={{ width: 22, height: 18, borderRadius: 4, backgroundColor: "#9146ff", alignItems: "center", justifyContent: "center" }}
    >
      <Text style={{ fontSize: 10, fontWeight: "700", color: "#ffffff" }}>TV</Text>
    </View>
  );
  return (
    <View style={{ width: 24 }}>
      <If when={props.row.twitchLogins.length === 1}>
        {chip(() => copyLogin(props.row.twitchLogins[0]))}
      </If>
      <If when={props.row.twitchLogins.length > 1}>
        <Popover trigger={chip()} placement="bottom" popupClass="rounded-md border border-border bg-popover p-1">
          <View style={{ width: 180, gap: 2 }}>
            <Each of={props.row.twitchLogins}>
              {(login) => <MenuRow label={login()} onSelect={() => copyLogin(login())}/>}
            </Each>
          </View>
        </Popover>
      </If>
    </View>
  );
}

function RosterMenu(props: { row: RosterRowView; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  return (
    <View style={{ width: 24 }}>
      <If when={props.row.wowsNumbersUrl && props.row.shipbuildsUrl && props.row.accountId}>
        <Menu align="end" offset={4} margin={8} trigger={<Chip label="..." accessibilityLabel={`Actions ${props.row.name}`} onPress={() => undefined}/>}>
          <Surface style={{ minWidth: 220, padding: 5, backgroundColor: colors.popover }}>
            <MenuRow label={text().openWowsNumbers} onSelect={() => void openUrl(props.row.wowsNumbersUrl!)}/>
            <MenuRow label={text().openShipbuilds} onSelect={() => void openUrl(props.row.shipbuildsUrl!)}/>
            <MenuRow label={text().findMatches} onSelect={() => props.tracker.onSearch(PlayerTracker.playerMatchesQuery(props.row.accountId!))}/>
          </Surface>
        </Menu>
      </If>
    </View>
  );
}

/** A band's text colour on the current appearance. */
function bandText(band: RatingBand): string {
  return colorScheme() === "dark" ? band.darkText : band.lightText;
}

/** The GPUI tracker's chip and row tints, as hex alpha over a band's hue. */
const CHIP_TINT = "38";
const ROW_TINT = "14";
const STAT_WIDTH = 64;

function markGlyph(mark: StatsMark): string {
  switch (mark) {
    case "hidden": return "x";
    case "unavailable": return "!";
    case "unknown": return "i";
  }
}

/** The four figures one scope contributes to a roster row. */
function ScopeCells(props: { cells: ScopeCellsView }) {
  const cells = () => props.cells;
  const figure = (text: string, color?: string) => (
    <Text style={{ width: STAT_WIDTH, fontSize: 11.5, color: color ?? colors.mutedForeground }}>{text}</Text>
  );
  return (
    <View style={{ flexDirection: "row", gap: 8 }}>
      <If when={cells().mark}>
        <View accessibilityLabel={cells().markHover!} tooltip={<Hint text={cells().markHover!}/>} style={{ width: STAT_WIDTH }}>
          <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{markGlyph(cells().mark!)}</Text>
        </View>
      </If>
      <If when={!cells().mark}>
        <View
          accessibilityLabel={cells().winRateHover}
          tooltip={cells().winRateHover ? <Hint text={cells().winRateHover!}/> : undefined}
          style={{ width: STAT_WIDTH }}
        >
          <Text style={{ fontSize: 11.5, color: cells().winRateBand ? bandText(cells().winRateBand!) : colors.mutedForeground }}>{cells().winRate}</Text>
        </View>
      </If>
      <If when={cells().personalRatingBand}>
        <View accessibilityLabel={cells().personalRatingHover} tooltip={<Hint text={cells().personalRatingHover}/>} style={{ width: STAT_WIDTH, flexDirection: "row" }}>
          <View style={{ paddingHorizontal: 4, borderRadius: 3, backgroundColor: `${cells().personalRatingBand!.hue}${CHIP_TINT}` }}>
            <Text style={{ fontSize: 11.5, color: bandText(cells().personalRatingBand!) }}>{cells().personalRating}</Text>
          </View>
        </View>
      </If>
      <If when={!cells().personalRatingBand}>
        {figure(cells().personalRating)}
      </If>
      {figure(cells().avgDamage)}
      {figure(cells().battles)}
    </View>
  );
}

function TeamHeading(props: { team: TeamView }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, paddingVertical: 4 }}>
      <Text style={{ fontSize: 13, fontWeight: "700", color: colors.foreground }}>{props.team.title}</Text>
      <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>{props.team.players}</Text>
      <If when={props.team.averageWinRate}>
        <Text style={{ fontSize: 11.5, color: bandText(props.team.averageWinRate!.band) }}>{props.team.averageWinRate!.text}</Text>
      </If>
      <If when={props.team.averageRating}>
        <Text style={{ fontSize: 11.5, color: bandText(props.team.averageRating!.band) }}>{props.team.averageRating!.text}</Text>
      </If>
    </View>
  );
}

/** The row's ship class icon from the battle's build, tinted like its name
 * and turned upright. A fixed slot either way, so the names stay aligned. */
function ClassIcon(props: { row: RosterRowView }) {
  return (
    <View style={{ width: 16, height: 16 }} tooltip={props.row.className ? <Hint text={props.row.className}/> : undefined}>
      <If when={props.row.classIcon}>
        <Image
          src={props.row.classIcon!}
          accessibilityLabel={props.row.className ?? ""}
          style={{ width: 16, height: 16, objectFit: "contain", tintColor: tintColor(props.row.tint), transform: [{ rotate: "90deg" }] }}
        />
      </If>
    </View>
  );
}

function TeamColumn(props: { team: TeamView; view: LiveView; tracker: PlayerTrackerProps }) {
  const text = () => labels()!;
  return (
    <View style={{ flex: 1, minWidth: 0 }}>
      <TeamHeading team={props.team}/>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, paddingVertical: 4, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ width: 16 }}/>
        <View style={{ width: 16 }}/>
        <View style={{ width: 24 }}/>
        <Text style={{ flex: 1, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{text().playerColumn}</Text>
        <Text style={{ width: 130, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{text().shipColumn}</Text>
        <Each of={props.view.scopeHeadings}>
          {(scope) => (
            <View style={{ flexDirection: "row", gap: 8 }}>
              <Text style={{ width: STAT_WIDTH, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{scope().winRate}</Text>
              <Text style={{ width: STAT_WIDTH, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{scope().personalRating}</Text>
              <Text style={{ width: STAT_WIDTH, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{scope().avgDamage}</Text>
              <Text style={{ width: STAT_WIDTH, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{scope().battles}</Text>
            </View>
          )}
        </Each>
        <Text style={{ width: 80, fontSize: 11.5, fontWeight: "700", color: colors.foreground }}>{text().seenColumn}</Text>
        <View style={{ width: 24 }}/>
      </View>
      <Each of={props.team.rows}>
        {(row) => (
          <View
            role="row"
            accessibilityLabel={row().name}
            style={{ minHeight: 26, flexDirection: "row", alignItems: "center", gap: 8, paddingHorizontal: 8, backgroundColor: row().band ? `${row().band!.hue}${ROW_TINT}` : "transparent" }}
          >
            <ClassIcon row={row()}/>
            <View style={{ width: 16 }} tooltip={row().note ? <Hint text={row().note!}/> : undefined}>
              <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{row().note ? "*" : ""}</Text>
            </View>
            <TwitchChip row={row()} tracker={props.tracker}/>
            <Text numberOfLines={1} style={{ flex: 1, fontSize: 13, color: tintColor(row().tint) }}>{row().name}</Text>
            <Text numberOfLines={1} style={{ width: 130, fontSize: 11.5, color: colors.mutedForeground }}>{row().ship}</Text>
            <Each of={row().scopes}>
              {(cells) => <ScopeCells cells={cells()}/>}
            </Each>
            <View accessibilityLabel={row().metHover} style={{ width: 80 }} tooltip={<Hint text={row().metHover}/>}>
              <Text style={{ fontSize: 11.5, color: severityColor(row().metSeverity) ?? colors.mutedForeground }}>{row().met}</Text>
            </View>
            <RosterMenu row={row()} tracker={props.tracker}/>
          </View>
        )}
      </Each>
    </View>
  );
}

function CurrentMatchSection(props: { tracker: PlayerTrackerProps }) {
  const current = () => controls()!;
  const view = () => live();
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingHorizontal: 8, paddingVertical: 6, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <Each of={current().viewModes}>
          {(option) => (
            <Toggle
              label={option().label}
              selected={current().viewMode === option().mode}
              onPress={() => void change(() => PlayerTracker.setViewMode(option().mode as ViewMode), { live: true })}
            />
          )}
        </Each>
        <If when={current().viewMode === "compact"}>
          <View style={{ width: 8 }}/>
          <Each of={current().scopes}>
            {(option) => (
              <Toggle
                label={option().label}
                selected={current().scope === option().scope}
                onPress={() => void change(() => PlayerTracker.setScope(option().scope as Scope), { live: true })}
              />
            )}
          </Each>
        </If>
      </View>
      <If when={view()?.status}>
        <Status text={view()!.status!}/>
      </If>
      <If when={view() && !view()!.status}>
        <If when={view()!.note}>
          <Text style={{ paddingHorizontal: 8, paddingVertical: 4, fontSize: 11.5, color: colors.mutedForeground }}>{view()!.note!}</Text>
        </If>
        <Scroll style={{ flex: 1 }}>
          <View style={{ flexDirection: "row", alignItems: "flex-start" }}>
            <TeamColumn team={view()!.friendly} view={view()!} tracker={props.tracker}/>
            <View style={{ width: 1, alignSelf: "stretch", backgroundColor: colors.border }}/>
            <TeamColumn team={view()!.enemy} view={view()!} tracker={props.tracker}/>
          </View>
        </Scroll>
      </If>
    </View>
  );
}

const PANELS = ["players", "current-match", "clans"];

function Section(props: { panel: string; tracker: PlayerTrackerProps }): JSX.Element {
  switch (props.panel) {
    case "players": return <PlayersSection tracker={props.tracker}/>;
    case "clans": return <ClansSection tracker={props.tracker}/>;
    default: return <CurrentMatchSection tracker={props.tracker}/>;
  }
}

function tabLabel(panel: string): string {
  const text = labels()!;
  switch (panel) {
    case "players": return text.playersTab;
    case "clans": return text.clansTab;
    default: return text.currentMatchTab;
  }
}

/** The tracker's three sections, which can be split and docked as the GPUI
 * tracker's dock panels can. */
function Sections(props: { tracker: PlayerTrackerProps }) {
  return (
    <DockArea
      class="flex-1"
      defaultLayout={{ center: { type: "split", axis: "horizontal", children: [{ type: "tabs", panels: PANELS, activeIndex: 0 }] } }}
      panels={PANELS.map((id) => ({ id, closable: false }))}
      onPanelActiveChange={({ panel, active }) => {
        if (panel === "current-match") {
          currentMatchActive = active;
          syncCurrentMatchShown();
        }
        if (panel !== "clans") return;
        clansShown.set(active);
        if (active) void reloadClans();
      }}
      groupClass="flex-1"
      tabBarClass="flex-row border-b border-border"
      tabClass="group px-3 py-1.5 selected:bg-muted"
      contentClass="flex-1"
      handleClass="bg-border group-hover:bg-muted-foreground group-active:bg-primary"
      handleAccessibilityLabel="Resize"
      dropIndicatorClass="rounded-(--radius) border-2 border-primary bg-primary/10"
      renderTab={(tab) => (
        <Text class="text-[13px] text-muted-foreground group-selected:text-foreground">{tabLabel(tab.panel)}</Text>
      )}
      renderDragPreview={(tab) => (
        <View class="rounded-(--radius) border border-border bg-background px-3 py-1.5">
          <Text class="text-[13px] text-foreground">{tabLabel(tab.panel)}</Text>
        </View>
      )}
      renderPanel={(panel) => <Section panel={panel.id} tracker={props.tracker}/>}
    />
  );
}

export function PlayerTrackerView(props: PlayerTrackerProps) {
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={!labels() || !controls()}>
        <Status text={failure() ?? ""}/>
      </If>
      <If when={labels() && controls()}>
        <TrackerToolbar/>
        <If when={failure()}>
          <Text style={{ paddingHorizontal: 12, paddingVertical: 6, fontSize: 12, color: colors.destructive }}>{failure()}</Text>
        </If>
        <Sections tracker={props}/>
      </If>
    </View>
  );
}
