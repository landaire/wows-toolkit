import { Scroll, signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { Shell } from "wows-toolkit/native";
import { Button, Surface } from "../components";
import { t } from "../i18n";
import { appSettings, editSetting } from "../settings_store";
import { colors } from "../theme";
import { Toggle } from "./controls";

/** Where the networking note lives, which the address warning links. */
const NETWORKING_URL = "https://landaire.github.io/wows-toolkit/networking";

/** The two warnings a reader may switch off, in the rows the other front ends write. */
type Warning = "address" | "softwareEncode";

const asked = signal<{ warning: Warning; go: () => void } | null>(null);
const errorText = signal<string | null>(null);

function suppressed(warning: Warning): boolean {
  const settings = appSettings();
  if (!settings) return false;
  return warning === "address" ? settings.suppressP2pIpWarning : settings.suppressGpuEncoderWarning;
}

function ask(warning: Warning, go: () => void) {
  // A switched-off warning is not put at all: the work just happens.
  if (suppressed(warning)) {
    go();
    return;
  }
  asked.set({ warning, go });
}

/** Asks before hosting or joining a session, which tells the peers where this machine is. */
export function beforeRevealingAddress(go: () => void) {
  ask("address", go);
}

/** Asks before an export that has no GPU encoder to use, which takes far longer. */
export function beforeSoftwareEncode(go: () => void) {
  ask("softwareEncode", go);
}

/**
 * Shows an error worth reading in full, with a way to copy it: a toast is
 * right for a sentence and wrong for a chain of causes.
 */
export function showError(text: string) {
  errorText.set(text);
}

/** The suppressible warning, mounted once over the whole window. */
export function WarningDialog() {
  const warning = () => asked()?.warning ?? "address";
  const title = () => warning() === "address" ? t("ui.windows.network_warning") : t("ui.windows.software_encode");
  const message = () => warning() === "address" ? t("ui.dialogs.p2p_warning") : t("ui.dialogs.software_encode_message");
  const carryOn = () => {
    const pending = asked();
    asked.set(null);
    pending?.go();
  };
  return (
    <Dialog open={asked() !== null} onOpenChange={({ open }) => { if (!open) asked.set(null); }}>
      <Surface style={{ width: "100%", maxWidth: 520, padding: 20, gap: 14, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{title()}</Text>
        <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{message()}</Text>
        <If when={warning() === "address"}>
          <View style={{ alignItems: "flex-start" }}>
            <Button label={t("ui.labels.more_info")} variant="ghost" onPress={() => void Shell.openUrl(NETWORKING_URL)}/>
          </View>
        </If>
        <Toggle
          label={t("ui.labels.suppress_warning")}
          checked={suppressed(warning())}
          // Written as it is ticked, as the other front ends' checkbox is:
          // ticking and then cancelling still switches the warning off.
          onChange={(checked) => void editSetting(warning() === "address" ? "suppressP2pIpWarning" : "suppressGpuEncoderWarning", checked)}
        />
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.continue_")} variant="primary" onPress={carryOn}/>
          <Button label={t("ui.buttons.cancel")} onPress={() => asked.set(null)}/>
        </View>
      </Surface>
    </Dialog>
  );
}

/** The full-text error, mounted once over the whole window. */
export function ErrorDialog() {
  return (
    <Dialog open={errorText() !== null} onOpenChange={({ open }) => { if (!open) errorText.set(null); }}>
      <Surface style={{ width: "100%", maxWidth: 600, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.error")}</Text>
        <Text style={{ fontSize: 13, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.labels.error_occurred")}</Text>
        <Scroll style={{ maxHeight: 320 }}>
          <View selectable>
            <Text style={{ fontSize: 12, lineHeight: 17, color: colors.popoverForeground }}>{errorText() ?? ""}</Text>
          </View>
        </Scroll>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.copy")} onPress={() => void Shell.copyText(errorText() ?? "")}/>
          <Button label={t("ui.buttons.dismiss")} variant="primary" onPress={() => errorText.set(null)}/>
        </View>
      </Surface>
    </Dialog>
  );
}
