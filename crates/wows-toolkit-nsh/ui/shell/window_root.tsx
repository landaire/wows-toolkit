import { host, mount, signal, View } from "nsh";
import { Shell } from "wows-toolkit/native";
import { followLocale } from "../i18n";
import { ReplayRendererView } from "../renderer";
import { loadAppSettings } from "../settings_store";
import { TacticsBoard } from "../tactics";
import { colors } from "../theme";
import { ErrorDialog, WarningDialog } from "./notices";
import * as toast from "./toast";
import type { WindowRequest } from "./windows";

/**
 * Draws in the theme and language the main window last applied, and follows
 * its changes. Following announces nothing, so no window hears its own echo.
 */
async function followLook() {
  let revision = 0;
  for (;;) {
    const look = await Shell.nextLook(revision);
    revision = look.revision;
    if (look.locale) followLocale(look.locale);
    if (look.theme) await Shell.followTheme(look.theme);
  }
}

function WindowRoot(props: { request: WindowRequest }) {
  // The window waits for this before it closes: a stopped engine runs no
  // cleanups, so the content is let go here, closing its renderer or board.
  const closing = signal(false);
  host.on("window.closing", () => {
    closing.set(true);
    setTimeout(() => void host.post("window.released", null), 0);
  });
  const request = props.request;
  return (
    <View style={{ flex: 1, backgroundColor: colors.background }}>
      <If when={!closing()}>
        {request.kind === "tacticsBoard" ? <TacticsBoard/> : <ReplayRendererView source={{ kind: "live", renderer: request.renderer }}/>}
      </If>
      <toast.ToastLayer/>
      <WarningDialog/>
      <ErrorDialog/>
    </View>
  );
}

/** Runs this bundle as a window of its own, showing what `request` asks for. */
export function mountWindow(request: WindowRequest) {
  // The warnings the content may put read their switches from the settings.
  void loadAppSettings();
  void followLook();
  mount(() => <WindowRoot request={request}/>);
}
