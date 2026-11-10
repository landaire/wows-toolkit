import { Unpacker as NativeUnpacker } from "wows-toolkit/native";

// The Resource Unpacker. The native view reads the WoWs and extraction
// directories from the shared settings database each time it is shown.
export function UnpackerView(props: { active: boolean; locale: string }) {
  return <NativeUnpacker active={props.active} locale={props.locale}/>;
}
