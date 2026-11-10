import type { JSX, ViewStyle } from "nsh";
import { Text, View } from "nsh";
import { Button as BaseButton, MenuItem, Tooltip } from "nsh/base";
import { colors, controlRadius } from "./theme";

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** A `Dialog`'s popup style that puts its content in the middle of the window, as the other front ends' modal windows are. */
export const centeredDialog: ViewStyle = { flex: 1, alignItems: "center", justifyContent: "center", padding: 24 };

export const inputStyle = {
  minHeight: 34,
  paddingHorizontal: 9,
  borderWidth: 1,
  borderColor: colors.input,
  borderRadius: controlRadius(),
  backgroundColor: colors.background,
  color: colors.foreground,
};

/** A hover hint for an element's `tooltip` prop, which takes an element, not text. */
export function Hint(props: { text: string }) {
  return (
    <Tooltip delay={400} placement="top" style={{ maxWidth: 360, paddingHorizontal: 8, paddingVertical: 4, borderRadius: 6, borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}>
      <Text style={{ fontSize: 12, color: colors.popoverForeground }}>{props.text}</Text>
    </Tooltip>
  );
}

export function Button(props: {
  label: string;
  variant?: "primary" | "secondary" | "ghost" | "danger";
  disabled?: boolean;
  tooltip?: string;
  accessibilityLabel?: string;
  testID?: string;
  onPress: () => void;
}) {
  const variant = () => props.variant ?? "secondary";
  const background = () => {
    switch (variant()) {
      case "primary": return colors.primary;
      case "danger": return colors.destructive;
      case "ghost": return "transparent";
      default: return colors.secondary;
    }
  };
  const foreground = () => variant() === "primary" || variant() === "danger" ? colors.primaryForeground : colors.secondaryForeground;
  return (
    <BaseButton
      onPress={props.onPress}
      accessibilityLabel={props.accessibilityLabel}
      testID={props.testID}
      tooltip={props.tooltip ? <Hint text={props.tooltip}/> : undefined}
      disabled={props.disabled}
      hoverStyle={{ opacity: 0.86 }}
      activeStyle={{ opacity: 0.68 }}
      focusStyle={{ borderColor: colors.ring }}
      style={{
        minHeight: 30,
        paddingHorizontal: 12,
        justifyContent: "center",
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: variant() === "ghost" ? "transparent" : colors.border,
        backgroundColor: background(),
        opacity: props.disabled ? 0.45 : 1,
      }}
    >
      <Text style={{ fontSize: 13, fontWeight: "500", color: foreground() }}>{props.label}</Text>
    </BaseButton>
  );
}

export function MenuRow(props: { label: string; note?: string; danger?: boolean; onSelect: () => void }) {
  return (
    <MenuItem
      onSelect={props.onSelect}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.72 }}
      style={{ height: 30, paddingHorizontal: 10, borderRadius: 6, flexDirection: "row", alignItems: "center", gap: 12 }}
    >
      <Text style={{ flex: 1, fontSize: 13, color: props.danger ? colors.destructive : colors.popoverForeground }}>{props.label}</Text>
      <If when={props.note}>
        <Text style={{ fontSize: 11, color: colors.mutedForeground }}>{props.note}</Text>
      </If>
    </MenuItem>
  );
}

export function Surface(props: { style?: ViewStyle; children: JSX.Element }) {
  return (
    <View style={[{
      backgroundColor: colors.card,
      borderWidth: 1,
      borderColor: colors.border,
      borderRadius: controlRadius(),
    }, props.style ?? null]}>
      {props.children}
    </View>
  );
}

export function EmptyState(props: { title: string; description: string; action?: JSX.Element }) {
  return (
    <View style={{ flex: 1, alignItems: "center", justifyContent: "center", padding: 32 }}>
      <Surface style={{ width: "100%", maxWidth: 620, padding: 28, gap: 10 }}>
        <Text style={{ fontSize: 22, fontWeight: "600", color: colors.cardForeground }}>{props.title}</Text>
        <Text style={{ fontSize: 14, lineHeight: 21, color: colors.mutedForeground }}>{props.description}</Text>
        {props.action}
      </Surface>
    </View>
  );
}
