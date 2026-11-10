import type { JSX, ViewStyle } from "nsh";
import { signal, Text, View } from "nsh";
import { Checkbox, ColorArea, ColorField, ColorPicker, ColorSlider, Popover, Slider } from "nsh/base";
import { colors, controlRadius, semantic } from "../theme";
import type { TriState } from "./state";

/** The hint shown while the pointer rests on a control. */
export function tip(text: string | undefined): JSX.Element | undefined {
  if (!text) return undefined;
  return (
    <View style={{ maxWidth: 320, paddingHorizontal: 8, paddingVertical: 5, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}>
      <Text style={{ fontSize: 12, lineHeight: 16, color: colors.popoverForeground }}>{text}</Text>
    </View>
  );
}

/** A compact button: a label, optionally held down (`on`), with a hint. */
export function ToolButton(props: {
  id: string;
  label: string;
  tooltip?: string;
  on?: boolean;
  disabled?: boolean;
  danger?: boolean;
  style?: ViewStyle;
  onPress: () => void;
}) {
  return (
    <View
      testID={props.id}
      role="button"
      accessibilityLabel={props.label}
      accessibilityState={{ disabled: props.disabled, selected: props.on }}
      disabled={props.disabled}
      tooltip={tip(props.tooltip)}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: props.on ? colors.primary : colors.accent }}
      activeStyle={{ opacity: 0.7 }}
      style={[{
        minHeight: 26,
        paddingHorizontal: 8,
        justifyContent: "center",
        alignItems: "center",
        borderRadius: controlRadius(),
        borderWidth: 1,
        borderColor: props.on ? colors.primary : colors.border,
        backgroundColor: props.on ? colors.primary : colors.secondary,
        opacity: props.disabled ? 0.45 : 1,
      }, props.style ?? null]}
    >
      <Text style={{ fontSize: 12, color: props.danger ? colors.destructive : props.on ? colors.primaryForeground : colors.secondaryForeground }}>{props.label}</Text>
    </View>
  );
}

/** A square glyph button without a frame (collapse, close). */
export function IconButton(props: { id: string; glyph: string; label: string; disabled?: boolean; onPress: () => void }) {
  return (
    <View
      testID={props.id}
      role="button"
      accessibilityLabel={props.label}
      disabled={props.disabled}
      tooltip={tip(props.label)}
      onPress={props.onPress}
      hoverStyle={{ backgroundColor: colors.accent }}
      activeStyle={{ opacity: 0.7 }}
      style={{ minWidth: 22, height: 22, paddingHorizontal: 4, alignItems: "center", justifyContent: "center", borderRadius: controlRadius(), opacity: props.disabled ? 0.45 : 1 }}
    >
      <Text style={{ fontSize: 12, fontWeight: "600", color: colors.foreground }}>{props.glyph}</Text>
    </View>
  );
}

const indicatorClass = "w-[14px] h-[14px] rounded-[3px] border border-border items-center justify-center checked:bg-primary indeterminate:bg-secondary";

/** A checkbox with its label; `state` "partial" shows the dash of a mixed group. */
export function CheckRow(props: {
  id?: string;
  label: string;
  checked?: boolean;
  state?: TriState;
  disabled?: boolean;
  tooltip?: string;
  bold?: boolean;
  /** The label names the box for screen readers only; the row shows its own. */
  labelHidden?: boolean;
  onChange: (checked: boolean) => void;
}) {
  const state = (): TriState => props.state ?? (props.checked ? "on" : "off");
  return (
    <Checkbox
      testID={props.id}
      accessibilityLabel={props.label}
      checked={state() === "on"}
      indeterminate={state() === "partial"}
      disabled={props.disabled}
      tooltip={tip(props.tooltip)}
      class="group flex-row items-center gap-2 disabled:opacity-50"
      indicatorClass={indicatorClass}
      indicator={
        <Text style={{ fontSize: 10, fontWeight: "700", color: state() === "partial" ? semantic.warn : colors.primaryForeground }}>
          {state() === "on" ? "x" : state() === "partial" ? "-" : ""}
        </Text>
      }
      onChange={({ checked }) => props.onChange(checked)}
    >
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6, flexShrink: 1 }}>
        <If when={!props.labelHidden}>
          <Text style={{ fontSize: 12.5, fontWeight: props.bold ? "600" : "400", color: colors.cardForeground }}>{props.label}</Text>
        </If>
      </View>
    </Checkbox>
  );
}

/**
 * A labelled slider with its value beside it. The value is the service's;
 * while dragged the slider shows the dragged value so a late report does not
 * pull the thumb back.
 */
export function SliderRow(props: {
  id?: string;
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  digits?: number;
  disabled?: boolean;
  onChange: (value: number) => void;
}) {
  const dragged = signal<number | null>(null);
  const shown = () => dragged() ?? props.value;
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8, opacity: props.disabled ? 0.5 : 1 }}>
      <Text style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{props.label}</Text>
      <Slider
        testID={props.id}
        accessibilityLabel={props.label}
        value={shown()}
        min={props.min}
        max={props.max}
        step={props.step ?? 0.01}
        disabled={props.disabled}
        class="w-[110px] h-[20px] justify-center"
        trackClass="h-[4px] rounded-full bg-secondary"
        indicatorClass="h-[4px] rounded-full bg-primary"
        thumbClass="w-[12px] h-[12px] rounded-full bg-primary"
        onChange={({ value }) => {
          dragged.set(value as number);
          props.onChange(value as number);
        }}
        onChangeEnd={({ value }) => {
          props.onChange(value as number);
          dragged.set(null);
        }}
      />
      <Text style={{ width: 44, fontSize: 12, color: colors.mutedForeground }}>{shown().toFixed(props.digits ?? 2)}</Text>
    </View>
  );
}

/** A color swatch that opens a picker; the color reported is `#rrggbb`. */
export function ColorRow(props: { id: string; label: string; color: string; disabled?: boolean; onChange: (color: string) => void }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8, opacity: props.disabled ? 0.5 : 1 }}>
      <Text style={{ width: 110, fontSize: 12, color: colors.cardForeground }}>{props.label}</Text>
      <Popover
        placement="bottom"
        align="start"
        offset={4}
        trigger={
          <View
            testID={props.id}
            role="button"
            accessibilityLabel={props.label}
            disabled={props.disabled}
            style={{ width: 40, height: 18, borderRadius: 3, borderWidth: 1, borderColor: colors.border, backgroundColor: props.color }}
          />
        }
      >
        <View style={{ width: 220, padding: 8, gap: 8, borderRadius: controlRadius(), borderWidth: 1, borderColor: colors.border, backgroundColor: colors.popover }}>
          <ColorPicker
            value={props.color}
            disabled={props.disabled}
            onChangeEnd={({ formatted }) => props.onChange(String(formatted).slice(0, 7))}
          >
            <ColorArea accessibilityLabel={props.label} class="w-full h-[120px] rounded-[4px]" thumbClass="w-[12px] h-[12px] rounded-full border-2 border-white"/>
            <ColorSlider channel="hue" accessibilityLabel={`${props.label} hue`} class="w-full h-[14px] rounded-full" thumbClass="w-[12px] h-[12px] rounded-full border-2 border-white"/>
            <ColorField accessibilityLabel={`${props.label} hex`} style={{ minHeight: 26, paddingHorizontal: 6, borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), color: colors.foreground }}/>
          </ColorPicker>
        </View>
      </Popover>
    </View>
  );
}

/** A small filled square of `color`. */
export function Swatch(props: { color: string; width?: number; height?: number }) {
  return <View style={{ width: props.width ?? 10, height: props.height ?? 10, borderRadius: 2, backgroundColor: props.color, borderWidth: 1, borderColor: "rgba(0,0,0,0.4)" }}/>;
}

/** A row of mutually exclusive buttons. */
export function ChoiceRow<T>(props: {
  label?: string;
  choices: { value: T; label: string; selected: boolean; tooltip?: string; id?: string }[];
  disabled?: boolean;
  onSelect: (value: T) => void;
}) {
  return (
    <View style={{ gap: 4 }}>
      <If when={props.label}>
        <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{props.label}</Text>
      </If>
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
        <Each of={props.choices} key="index">
          {(choice) => (
            <ToolButton
              id={choice().id ?? `choice-${choice().label}`}
              label={choice().label}
              tooltip={choice().tooltip}
              on={choice().selected}
              disabled={props.disabled}
              onPress={() => props.onSelect(choice().value)}
            />
          )}
        </Each>
      </View>
    </View>
  );
}

/** A bold heading inside a section. */
export function Heading(props: { text: string }) {
  return <Text style={{ fontSize: 12.5, fontWeight: "600", color: colors.cardForeground }}>{props.text}</Text>;
}

/** Dim explanatory text. */
export function Hint(props: { text: string; tone?: "warn" | "error" | "muted" }) {
  const color = () => props.tone === "warn" ? semantic.warn : props.tone === "error" ? semantic.error : colors.mutedForeground;
  return <Text style={{ fontSize: 11.5, lineHeight: 16, color: color() }}>{props.text}</Text>;
}

/** A rule between groups of rows. */
export function Rule() {
  return <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>;
}
