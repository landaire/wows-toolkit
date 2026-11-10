import type { JSX } from "nsh";
import { IconView, type Icon as VendoredIcon } from "wows-toolkit/native";
import { colors } from "./theme";

/** A vendored icon by its file name (`"chevron-down"`): `src/icons.rs` `Icon`. */
export type IconName = VendoredIcon;

export type IconProps = {
  name: IconName;
  /** Width and height in points; 16 when unset. */
  size?: number;
  /** Paint color; the text color around it when unset. */
  color?: string;
  /** Stroke width in the icon's 24 point grid; 2 when unset. A logo has no strokes. */
  strokeWidth?: number;
  /** What a screen reader says; unset leaves the icon to the element around it. */
  accessibilityLabel?: string;
};

export function Icon(props: IconProps): JSX.Element {
  return (
    <IconView
      name={props.name}
      size={props.size}
      color={props.color}
      strokeWidth={props.strokeWidth}
      accessibilityLabel={props.accessibilityLabel}
    />
  );
}

/** A checkbox's `indicator`: a check while `checked`, on the box's checked fill. */
export function CheckMark(props: { checked: boolean; size?: number }): JSX.Element {
  return (
    <If when={props.checked}>
      <Icon name="check" size={props.size ?? 11} strokeWidth={3} color={colors.primaryForeground}/>
    </If>
  );
}
