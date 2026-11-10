import type { JSX, ViewStyle } from "nsh";
import { signal, Text, View } from "nsh";
import { Checkbox, Input } from "nsh/base";
import { colors, controlRadius } from "../theme";

/** A checkbox with its label beside it, and an optional line under the label. */
export function Toggle(props: {
  label: string;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Checkbox
      accessibilityLabel={props.label}
      checked={props.checked}
      disabled={props.disabled}
      class="group flex-row items-start gap-2 disabled:opacity-50"
      indicatorClass="mt-0.5 w-[16px] h-[16px] rounded-[3px] border border-border items-center justify-center checked:bg-primary"
      indicator={<Text class="text-[10px] text-primary-foreground">{props.checked ? "x" : ""}</Text>}
      onChange={({ checked }) => props.onChange(checked)}
    >
      <View style={{ flexShrink: 1, gap: 2 }}>
        <Text style={{ fontSize: 13, color: colors.cardForeground }}>{props.label}</Text>
        <If when={props.description}>
          <Text style={{ fontSize: 11.5, lineHeight: 16, color: colors.mutedForeground }}>{props.description}</Text>
        </If>
      </View>
    </Checkbox>
  );
}

/** One of a row of mutually exclusive choices, filled while chosen. */
export function Choice(props: {
  label: string;
  selected: boolean;
  disabled?: boolean;
  accessibilityLabel?: string;
  style?: ViewStyle;
  children?: JSX.Element;
  onPress: () => void;
}) {
  return (
    <View
      role="radio"
      accessibilityLabel={props.accessibilityLabel ?? props.label}
      accessibilityState={{ checked: props.selected, disabled: props.disabled }}
      disabled={props.disabled}
      onPress={props.onPress}
      hoverStyle={props.selected ? undefined : { backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.75 }}
      style={[{
        minHeight: 28,
        paddingHorizontal: 10,
        justifyContent: "center",
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: props.selected ? colors.primary : colors.border,
        backgroundColor: props.selected ? colors.selection : colors.secondary,
        opacity: props.disabled ? 0.45 : 1,
      }, props.style ?? null]}
    >
      {props.children ?? <Text style={{ fontSize: 12.5, color: colors.secondaryForeground }}>{props.label}</Text>}
    </View>
  );
}

export const inputStyle = {
  minHeight: 30,
  paddingHorizontal: 8,
  borderWidth: 1,
  borderColor: colors.input,
  borderRadius: controlRadius(),
  backgroundColor: colors.background,
  color: colors.foreground,
};

/**
 * A text field that commits on Enter or when it loses focus, as the other
 * front ends' settings fields do, so a half-typed value never reaches the
 * database. `value` is what is stored; the field keeps its own draft until it
 * commits.
 */
export function CommitInput(props: {
  label: string;
  value: string;
  placeholder?: string;
  masked?: boolean;
  invalid?: boolean;
  onCommit: (value: string) => void;
}) {
  const draft = signal<string | null>(null);
  const commit = () => {
    const typed = draft();
    if (typed === null) return;
    draft.set(null);
    const value = typed.trim();
    if (value !== props.value) props.onCommit(value);
  };
  return (
    <Input
      accessibilityLabel={props.label}
      value={draft() ?? props.value}
      placeholder={props.placeholder}
      masked={props.masked}
      onChange={({ value }) => draft.set(value)}
      onSubmit={commit}
      onBlur={commit}
      style={[inputStyle, props.invalid ? { borderColor: colors.destructive, color: colors.destructive } : null]}
    />
  );
}
