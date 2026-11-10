import { Scroll, signal, Text, View } from "nsh";
import { Dialog, TextView } from "nsh/base";
import { Shell, type ReleaseOffer } from "wows-toolkit/native";
import { Button, Surface } from "../components";
import { t } from "../i18n";
import { colors } from "../theme";
import { showError } from "./notices";
import * as toast from "./toast";

/** Who asked for the check: one nobody asked for says nothing when this build is the newest. */
export type Asked = "startup" | "byHand";

const CHECK_TOAST = "update-check";

const offer = signal<ReleaseOffer | null>(null);
let checking: Asked | null = null;

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Looks for a newer release and offers it. A check already running takes over a manual ask. */
export async function checkForUpdate(asked: Asked) {
  if (asked === "byHand") toast.progress(CHECK_TOAST, t("ui.messages.checking_app_updates"));
  if (checking !== null) {
    if (asked === "byHand") checking = "byHand";
    return;
  }
  checking = asked;
  try {
    const found = await Shell.checkForUpdate();
    if (found) {
      offer.set(found);
    } else if (checking === "byHand") {
      toast.ok(t("ui.messages.app_up_to_date"));
    }
  } catch (error) {
    // A startup check that fails is logged by the native side and says
    // nothing; one the reader asked for shows the whole cause.
    if (checking === "byHand") showError(`${t("ui.messages.update_check_failed")}\n\n${errorText(error)}`);
  } finally {
    checking = null;
    toast.resolved(CHECK_TOAST);
  }
}

/** The offer of a newer release: its version, its notes, and where to get it. */
export function UpdateDialog() {
  const current = () => offer();
  return (
    <Dialog open={current() !== null} onOpenChange={({ open }) => { if (!open) offer.set(null); }}>
      <Surface style={{ width: "100%", maxWidth: 680, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.update_available")}</Text>
        <View selectable>
          <Text style={{ fontSize: 13, color: colors.popoverForeground }}>{t("ui.dialogs.update_message", { tag: current()?.tag ?? "" })}</Text>
        </View>
        <If when={current()?.notes}>
          <Scroll style={{ maxHeight: 420 }}>
            <TextView markdown={current()?.notes ?? ""} selectable/>
          </Scroll>
        </If>
        {/* In-place installs exist only for Windows builds; this app runs on Linux. */}
        <Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.dialogs.update_windows_only")}</Text>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.view_release")} onPress={() => void Shell.openUrl(current()?.htmlUrl ?? "")}/>
          <Button label={t("ui.buttons.cancel")} onPress={() => offer.set(null)}/>
        </View>
      </Surface>
    </Dialog>
  );
}
