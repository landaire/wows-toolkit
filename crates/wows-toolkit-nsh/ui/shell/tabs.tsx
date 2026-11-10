import type { JSX } from "nsh";
import { Text, View } from "nsh";
import { ContextMenu, MenuItem } from "nsh/base";
import { ReplayInspectorService, type AppTab } from "wows-toolkit/native";
import { Surface } from "../components";
import { t } from "../i18n";
import { Icon, type IconName } from "../icons";
import { closeReplayDirectory, pinReplayDirectory, replayDirectories } from "../replay_inspector";
import { closeDirectoryStats, directoryStatsTabs } from "../stats";
import { colors, controlRadius } from "../theme";

/** What a top-level tab shows: a fixed view, an opened replay directory, or a directory's stats. */
export type TabTarget =
  | { kind: "fixed"; id: string }
  | { kind: "directory"; path: string }
  | { kind: "directoryStats"; scope: string };

export function sameTarget(a: TabTarget, b: TabTarget): boolean {
  switch (a.kind) {
    case "fixed":
      return b.kind === "fixed" && a.id === b.id;
    case "directory":
      return b.kind === "directory" && a.path === b.path;
    case "directoryStats":
      return b.kind === "directoryStats" && a.scope === b.scope;
  }
}

/** The fixed tab a target is shown under: the inspector for a directory, Stats for a directory's stats. */
export function hostTab(target: TabTarget): string {
  switch (target.kind) {
    case "fixed":
      return target.id;
    case "directory":
      return "replay-inspector";
    case "directoryStats":
      return "statistics";
  }
}

type StripTab = {
  key: string;
  target: TabTarget;
  label: string;
  icon: IconName;
  /** A directory tab's full path. */
  tooltip?: string;
  /** Drawn after the label: a close button, or the pinned mark. */
  trailing?: JSX.Element;
  /** The tab's context menu. */
  menu?: JSX.Element;
};

/** The tabs in GPUI's order: each opened directory after the Replay Inspector, each directory's stats after Stats. */
function stripTabs(fixed: readonly AppTab[], active: TabTarget, onSelect: (target: TabTarget) => void): StripTab[] {
  const tabs: StripTab[] = [];
  fixed.forEach((tab, index) => {
    tabs.push({ key: `fixed:${tab.id}`, target: { kind: "fixed", id: tab.id }, label: t(tab.labelKey), icon: tab.icon });
    if (index === 0) tabs.push(...replayDirectories().map(directoryTab));
    if (tab.id === "statistics") {
      tabs.push(...directoryStatsTabs().map((stats) => directoryStatsTab(stats.scope, stats.root, active, onSelect)));
    }
  });
  return tabs;
}

function directoryTab(entry: { path: string; label: string; pinned: boolean }): StripTab {
  return {
    key: `directory:${entry.path}`,
    target: { kind: "directory", path: entry.path },
    label: entry.label,
    icon: "folder-open",
    tooltip: entry.path,
    trailing: entry.pinned
      ? <Text style={{ fontSize: 11, color: colors.mutedForeground }}>[Pinned]</Text>
      : <CloseButton label={`Close directory ${entry.label}`} tooltip="Close directory" onPress={() => closeReplayDirectory(entry.path)}/>,
    menu: (
      <Surface style={{ minWidth: 200, padding: 5, backgroundColor: colors.popover }}>
        <TabMenuItem label={entry.pinned ? "Unpin directory" : "Pin directory"} onSelect={() => void pinReplayDirectory(entry.path, !entry.pinned)}/>
        <TabMenuItem label="Close directory" disabled={entry.pinned} onSelect={() => closeReplayDirectory(entry.path)}/>
      </Surface>
    ),
  };
}

function directoryStatsTab(scope: string, root: string, active: TabTarget, onSelect: (target: TabTarget) => void): StripTab {
  const target: TabTarget = { kind: "directoryStats", scope };
  const label = t("ui.tabs.directory_stats", { dir: ReplayInspectorService.shortenedPath(root) });
  const close = () => {
    closeDirectoryStats(scope);
    if (sameTarget(active, target)) onSelect({ kind: "fixed", id: "statistics" });
  };
  return {
    key: `directory-stats:${scope}`,
    target,
    label,
    icon: "chart-column",
    tooltip: root,
    trailing: <CloseButton label={`${t("ui.tabs.close_directory_stats")} ${label}`} tooltip={t("ui.tabs.close_directory_stats")} onPress={close}/>,
  };
}

function CloseButton(props: { label: string; tooltip: string; onPress: () => void }) {
  return (
    <View
      role="button"
      accessibilityLabel={props.label}
      tooltip={props.tooltip}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      style={{ paddingHorizontal: 4, borderRadius: controlRadius() }}
    >
      <Icon name="x" size={12} color={colors.mutedForeground}/>
    </View>
  );
}

function TabMenuItem(props: { label: string; disabled?: boolean; onSelect: () => void }) {
  return (
    <MenuItem
      onSelect={props.onSelect}
      disabled={props.disabled}
      accessibilityLabel={props.label}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.72 }}
      style={{ height: 30, paddingHorizontal: 10, borderRadius: controlRadius(), flexDirection: "row", alignItems: "center", opacity: props.disabled ? 0.45 : 1 }}
    >
      <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{props.label}</Text>
    </MenuItem>
  );
}

function TabButton(props: { tab: StripTab; selected: boolean; flagged: boolean; onSelect: () => void }) {
  const color = () => (props.flagged ? colors.destructive : props.selected ? colors.textBright : colors.mutedForeground);
  return (
    <View
      role="tab"
      accessibilityLabel={props.tab.label}
      accessibilityState={{ selected: props.selected }}
      tooltip={props.tab.tooltip}
      onPress={props.onSelect}
      hoverStyle={props.selected ? undefined : { backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.7 }}
      style={{
        height: 32,
        flexDirection: "row",
        alignItems: "center",
        gap: 6,
        paddingHorizontal: 10,
        borderBottomWidth: 2,
        borderBottomColor: props.selected ? colors.primary : "transparent",
        backgroundColor: props.selected ? colors.background : "transparent",
      }}
    >
      <Icon name={props.tab.icon} size={14} color={color()}/>
      <Text style={{ fontSize: 12.5, fontWeight: props.selected ? "600" : "400", color: color() }}>
        {props.flagged ? `! ${props.tab.label}` : props.tab.label}
      </Text>
      {props.tab.trailing}
    </View>
  );
}

/** The tab strip: every top-level tab, the selected one underlined, and the menu at its end. */
export function TabStrip(props: {
  fixed: readonly AppTab[];
  active: TabTarget;
  /** Fixed tabs that need looking at, marked as the other front ends mark them. */
  attention: (id: string) => boolean;
  onSelect: (target: TabTarget) => void;
  menu: JSX.Element;
}) {
  return (
    <View style={{ borderBottomWidth: 1, borderBottomColor: colors.borderBright, backgroundColor: colors.surface }}>
      <View role="tablist" style={{ minHeight: 36, flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 2, paddingHorizontal: 6 }}>
        <Each of={stripTabs(props.fixed, props.active, props.onSelect)} key={(tab) => tab.key}>
          {(tab) => {
            const button = () => (
              <TabButton
                tab={tab()}
                selected={sameTarget(props.active, tab().target)}
                flagged={tab().target.kind === "fixed" && props.attention(hostTab(tab().target))}
                onSelect={() => props.onSelect(tab().target)}
              />
            );
            return (
              <If when={tab().menu} else={button()}>
                {(menu) => <ContextMenu trigger={button()}>{menu()}</ContextMenu>}
              </If>
            );
          }}
        </Each>
        <View style={{ flex: 1 }}/>
        {props.menu}
      </View>
    </View>
  );
}
