import type { ViewStyle } from "nsh";
import { View } from "nsh";
import { nativeComponent } from "nsh/native";

const NativeFrame = nativeComponent("wows.frame", { props: ["surface"], events: ["secondaryPress"] });

// A CPU-rendered frame surface. The Rust producer named by `surface` draws
// into it and receives its pointer, wheel, key and focus input. Headless
// tests have no native components, so the surface shows as an empty view.
export function Frame(props: {
  surface: string;
  accessibilityLabel: string;
  style?: ViewStyle;
  onSecondaryPress?: (event: { x: number; y: number }) => void;
}) {
  return (
    <View accessibilityLabel={props.accessibilityLabel} style={[{ flex: 1, minHeight: 0, minWidth: 0 }, props.style ?? null]}>
      <NativeFrame surface={props.surface} onSecondaryPress={props.onSecondaryPress} style={{ flex: 1 }}/>
    </View>
  );
}
