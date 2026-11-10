import type { JSX } from "nsh";
import { Text, View } from "nsh";
import { Icon, type IconName } from "./icons";
import { colors, semantic } from "./theme";

/** One section of the Settings tab: its icon and title over a framed form. */
export function Section(props: { icon: IconName; title: string; description: string; children: JSX.Element }) {
  return (
    <View style={{ gap: 8 }}>
      <View style={{ gap: 2 }}>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
          <Icon name={props.icon} size={16} color={colors.primary}/>
          <Text role="header" style={{ fontSize: 14, fontWeight: "700", color: colors.foreground }}>{props.title}</Text>
        </View>
        <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{props.description}</Text>
      </View>
      <View style={{ padding: 12, gap: 12, borderWidth: 1, borderColor: colors.border, borderRadius: 6, backgroundColor: colors.surface }}>
        {props.children}
      </View>
    </View>
  );
}

/** One row of a section: the label in a column of its own, so the tab reads down one edge. */
export function Field(props: { label?: string; description?: string; invalid?: boolean; children: JSX.Element }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 12 }}>
      <Text style={{ width: 260, paddingTop: 6, fontSize: 12.5, color: colors.cardForeground }}>{props.label ?? ""}</Text>
      <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
        {props.children}
        <If when={props.description}>
          <Text style={{ fontSize: 11.5, lineHeight: 16, color: props.invalid ? semantic.error : colors.mutedForeground }}>{props.description}</Text>
        </If>
      </View>
    </View>
  );
}
