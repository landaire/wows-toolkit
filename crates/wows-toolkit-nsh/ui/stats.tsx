import { Scroll, signal, Text, View } from "nsh";
import { Stats, type StatisticsState } from "wows-toolkit/native";
import { Button, EmptyState, errorMessage, Surface } from "./components";
import { colors } from "./theme";

const statistics = signal<StatisticsState | null>(null);
const statisticsLoading = signal(false);
const statisticsError = signal<string | null>(null);
let statisticsRequested = false;

export function activateStats() {
  if (statisticsRequested) return;
  statisticsRequested = true;
  void loadStatistics();
}

async function loadStatistics() {
  statisticsLoading.set(true);
  statisticsError.set(null);
  try {
    statistics.set(await Stats.loadStatistics());
  } catch (error) {
    statisticsError.set(`Statistics could not be loaded: ${errorMessage(error)}`);
  } finally {
    statisticsLoading.set(false);
  }
}

function metric(label: string, value: () => string) {
  return (
    <Surface style={{ minWidth: 145, flex: 1, padding: 14, gap: 5 }}>
      <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{label}</Text>
      <Text style={{ fontSize: 23, fontWeight: "600", color: colors.cardForeground }}>{value()}</Text>
    </Surface>
  );
}

export function StatsView() {
  const current = () => statistics();
  return (
    <View disabled={statisticsLoading()} style={{ flex: 1, minHeight: 0 }}>
      <View style={{ minHeight: 54, paddingHorizontal: 18, flexDirection: "row", alignItems: "center", gap: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
        <View style={{ flex: 1 }}>
          <Text style={{ fontSize: 19, fontWeight: "600", color: colors.foreground }}>Stats</Text>
          <Text style={{ fontSize: 12, color: colors.mutedForeground }}>Recorded battles from the shared session database</Text>
        </View>
        <Button label={statisticsLoading() ? "Loading..." : "Refresh"} variant="ghost" disabled={statisticsLoading()} onPress={() => void loadStatistics()}/>
      </View>
      <If when={statisticsLoading() && !current()}>
        <EmptyState title="Loading statistics" description="Reading recorded battles."/>
      </If>
      <If when={!statisticsLoading() && statisticsError() && (!current() || current()!.games === 0)}>
        <EmptyState title="Statistics unavailable" description={statisticsError()!} action={<Button label="Try Again" onPress={() => void loadStatistics()}/>}/>
      </If>
      <If when={current() && current()!.games === 0 && !statisticsError()}>
        <EmptyState title="No recorded battles" description="Battle results appear here after replays have been processed." action={<Button label="Refresh" onPress={() => void loadStatistics()}/>}/>
      </If>
      <If when={current() && current()!.games > 0}>
        <Scroll style={{ flex: 1 }}>
          <View style={{ width: "100%", maxWidth: 1100, alignSelf: "center", padding: 18, gap: 14 }}>
            <If when={statisticsError()}>
              <Surface style={{ padding: 12, borderColor: colors.destructive }}>
                <Text style={{ fontSize: 12, color: colors.destructive }}>{statisticsError()}</Text>
              </Surface>
            </If>
            <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 10 }}>
              {metric("Battles", () => current()!.games.toFixed(0))}
              {metric("Win rate", () => `${current()!.winRate.toFixed(1)}%`)}
              {metric("Average damage", () => current()!.averageDamage.toFixed(0))}
              {metric("Average base XP", () => current()!.averageBaseXp.toFixed(0))}
              {metric("Average frags", () => current()!.averageFrags.toFixed(2))}
            </View>
            <Scroll horizontal style={{ width: "100%" }} contentStyle={{ minWidth: 650 }}>
              <Surface style={{ minWidth: 650, overflow: "hidden" }}>
                <View role="table" accessibilityLabel="Statistics by ship">
                <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border, backgroundColor: colors.muted }}>
                  <Text role="columnheader" style={{ flex: 1, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Ship</Text>
                  <Text role="columnheader" style={{ width: 90, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Battles</Text>
                  <Text role="columnheader" style={{ width: 100, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Win rate</Text>
                  <Text role="columnheader" style={{ width: 130, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Avg damage</Text>
                  <Text role="columnheader" style={{ width: 110, fontSize: 12, fontWeight: "600", color: colors.mutedForeground }}>Avg XP</Text>
                </View>
                <Each of={current()!.ships}>
                  {(ship) => (
                    <View role="row" style={{ minHeight: 34, flexDirection: "row", alignItems: "center", paddingHorizontal: 12, borderBottomWidth: 1, borderBottomColor: colors.border }}>
                      <View role="cell" style={{ flex: 1 }}>
                        <Text style={{ fontSize: 13, color: colors.cardForeground }}>{ship().shipName}</Text>
                        <Text style={{ fontSize: 10, color: colors.mutedForeground }}>{ship().shipId}</Text>
                      </View>
                      <Text role="cell" style={{ width: 90, fontSize: 12, color: colors.cardForeground }}>{ship().games.toFixed(0)}</Text>
                      <Text role="cell" style={{ width: 100, fontSize: 12, color: colors.cardForeground }}>{ship().winRate.toFixed(1)}%</Text>
                      <Text role="cell" style={{ width: 130, fontSize: 12, color: colors.cardForeground }}>{ship().averageDamage.toFixed(0)}</Text>
                      <Text role="cell" style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{ship().averageBaseXp.toFixed(0)}</Text>
                    </View>
                  )}
                </Each>
                </View>
              </Surface>
            </Scroll>
          </View>
        </Scroll>
      </If>
    </View>
  );
}
