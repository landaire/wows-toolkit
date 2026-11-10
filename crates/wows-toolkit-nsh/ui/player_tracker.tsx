import { Scroll, signal, Text, View } from "nsh";
import { Input, Menu } from "nsh/base";
import { PlayerTracker, type PlayerTrackerState } from "wows-toolkit/native";
import { Button, EmptyState, errorMessage, inputStyle, MenuRow, Surface } from "./components";
import { colors, controlRadius } from "./theme";

const playerTracker = signal<PlayerTrackerState | null>(null);
const playerTrackerLoading = signal(false);
const playerTrackerError = signal<string | null>(null);
const playerTrackerPeriod = signal("day");
const playerTrackerQuery = signal("");
let playerTrackerRequested = false;
let playerTrackerGeneration = 0;
let playerTrackerFilterTimer: number | undefined;
let playerTrackerRequestActive = false;
let playerTrackerRequestPending = false;

export function activatePlayerTracker() {
  if (playerTrackerRequested) return;
  playerTrackerRequested = true;
  void loadPlayerTracker();
}

async function loadPlayerTracker() {
  if (playerTrackerRequestActive) {
    playerTrackerRequestPending = true;
    return;
  }
  playerTrackerRequestActive = true;
  playerTrackerLoading.set(true);
  do {
    playerTrackerRequestPending = false;
    const generation = ++playerTrackerGeneration;
    const period = playerTrackerPeriod();
    const query = playerTrackerQuery();
    playerTrackerError.set(null);
    try {
      const loaded = await PlayerTracker.loadPlayerTracker(period, query);
      if (generation === playerTrackerGeneration) playerTracker.set(loaded);
    } catch (error) {
      if (generation === playerTrackerGeneration) {
        playerTrackerError.set(`Player tracker could not be loaded: ${errorMessage(error)}`);
      }
    }
  } while (playerTrackerRequestPending);
  playerTrackerRequestActive = false;
  playerTrackerLoading.set(false);
}

function selectPlayerTrackerPeriod(period: string) {
  if (period === playerTrackerPeriod()) return;
  clearTimeout(playerTrackerFilterTimer);
  playerTrackerFilterTimer = undefined;
  playerTrackerGeneration += 1;
  playerTrackerPeriod.set(period);
  playerTracker.set(null);
  void loadPlayerTracker();
}

function updatePlayerTrackerQuery(query: string) {
  playerTrackerQuery.set(query);
  playerTrackerGeneration += 1;
  playerTracker.set(null);
  playerTrackerError.set(null);
  playerTrackerLoading.set(true);
  clearTimeout(playerTrackerFilterTimer);
  if (playerTrackerRequestActive) {
    playerTrackerFilterTimer = undefined;
    playerTrackerRequestPending = true;
  } else {
    playerTrackerFilterTimer = setTimeout(() => {
      playerTrackerFilterTimer = undefined;
      void loadPlayerTracker();
    }, 200);
  }
}

const playerTrackerPeriods = [
  { id: "hour", label: "Past hour" },
  { id: "six_hours", label: "Past six hours" },
  { id: "day", label: "Past day" },
  { id: "week", label: "Past week" },
  { id: "month", label: "Past month" },
  { id: "all", label: "All time" },
];

export function PlayerTrackerView() {
  const current = () => playerTracker();
  const periodLabel = () => playerTrackerPeriods.find((period) => period.id === playerTrackerPeriod())?.label ?? "Past day";
  const visiblePlayers = () => current()?.players ?? [];
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 10, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Player Tracker</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Players met in indexed battles</Text>
        </View>
        <Menu
          align="end"
          offset={5}
          margin={8}
          trigger={
            <View accessibilityLabel="Select player tracker period" hoverStyle={{ backgroundColor: colors.accent }} style={{ minHeight: 32, minWidth: 112, paddingHorizontal: 10, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}>
              <Text style={{ fontSize: 12.5, color: colors.foreground }}>{periodLabel()}</Text>
            </View>
          }
        >
          <Surface style={{ minWidth: 190, padding: 5, backgroundColor: colors.popover }}>
            <Each of={playerTrackerPeriods}>
              {(period) => <MenuRow label={period().label} note={period().id === playerTrackerPeriod() ? "Selected" : undefined} onSelect={() => selectPlayerTrackerPeriod(period().id)}/>}
            </Each>
          </Surface>
        </Menu>
        <Button label={playerTrackerLoading() ? "Loading..." : "Refresh"} variant="ghost" disabled={playerTrackerLoading()} onPress={() => void loadPlayerTracker()}/>
      </View>
      <View style={{ paddingHorizontal: 18, paddingTop: 14 }}>
        <Input
          accessibilityLabel="Filter tracked players"
          value={playerTrackerQuery()}
          placeholder="Filter by player, clan, or account ID"
          onChange={({ value }) => updatePlayerTrackerQuery(value)}
          style={inputStyle}
        />
      </View>
      <If when={playerTrackerLoading() && !current()}>
        <EmptyState title="Loading player history" description="Reading indexed battles."/>
      </If>
      <If when={!playerTrackerLoading() && playerTrackerError() && !current()}>
        <EmptyState title="Player tracker unavailable" description={playerTrackerError()!} action={<Button label="Try Again" onPress={() => void loadPlayerTracker()}/>}/>
      </If>
      <If when={current()}>
        <View style={{ flex: 1, minHeight: 0, padding: 18, gap: 12 }}>
          <If when={playerTrackerError()}>
            <Surface style={{ padding: 12, borderColor: colors.destructive }}>
              <Text style={{ fontSize: 12, color: colors.destructive }}>{playerTrackerError()}</Text>
            </Surface>
          </If>
          <If when={current()!.players.length === 0}>
            <EmptyState
              title={playerTrackerQuery().trim() ? "No matching players" : "No players found"}
              description={playerTrackerQuery().trim() ? "Change the filter to show tracked players." : "Index replays to build encounter history."}
            />
          </If>
          <If when={visiblePlayers().length > 0}>
            <View style={{ flex: 1, minHeight: 0, gap: 6 }}>
              <Text style={{ fontSize: 11.5, color: colors.mutedForeground }}>
                Showing {visiblePlayers().length.toFixed(0)} of {current()!.totalPlayers.toFixed(0)} players
              </Text>
              <Scroll style={{ flex: 1 }}>
                <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 620 }}>
                  <Surface style={{ minWidth: 620, overflow: "hidden" }}>
                  <View role="table" accessibilityLabel="Tracked players">
                    <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.muted }}>
                      <Text role="columnheader" style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Player</Text>
                      <Text role="columnheader" style={{ width: 180, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Clan</Text>
                      <Text role="columnheader" style={{ width: 110, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Encounters</Text>
                    </View>
                    <Each of={visiblePlayers()}>
                      {(player) => (
                        <View role="row" style={{ minHeight: 38, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                          <View role="cell" style={{ flex: 1 }}>
                            <Text style={{ fontSize: 13, color: colors.cardForeground }}>{player().name}</Text>
                            <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{player().accountId}</Text>
                          </View>
                          <Text role="cell" style={{ width: 180, fontSize: 12, color: colors.cardForeground }}>{player().clan || "No clan"}</Text>
                          <Text role="cell" style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{player().encounters.toFixed(0)}</Text>
                        </View>
                      )}
                    </Each>
                  </View>
                  </Surface>
                </Scroll>
              </Scroll>
            </View>
          </If>
        </View>
      </If>
    </View>
  );
}
