import { Scroll, signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { Shell } from "wows-toolkit/native";
import { Button, Surface } from "../components";
import { t } from "../i18n";
import { colors } from "../theme";
import * as toast from "./toast";

const crashReport = signal<string | null>(null);

/** Reports what the last run's crash left behind, once: the report is taken as it is read. */
export function reportLastCrash() {
  const report = Shell.takeCrashReport();
  if (report) crashReport.set(report.trim());
}

/** Puts the newest log file on the clipboard, for a bug report. */
export async function copyLatestLog() {
  try {
    const copied = await Shell.copyLatestLog();
    if (copied) {
      toast.ok(t("ui.messages.log_copied", { bytes: copied.kilobytes, file: copied.file }));
    } else {
      toast.warn(t("ui.messages.log_not_found", { dir: Shell.logDirectory() }));
    }
  } catch (error) {
    toast.failed(t("ui.messages.log_copy_failed", { error: error instanceof Error ? error.message : String(error) }));
  }
}

/** The Crash Detected window: what the last run left, with a way to copy it into a report. */
export function CrashDialog() {
  return (
    <Dialog open={crashReport() !== null} onOpenChange={({ open }) => { if (!open) crashReport.set(null); }}>
      <Surface style={{ width: "100%", maxWidth: 640, padding: 20, gap: 12, backgroundColor: colors.popover }}>
        <Text style={{ fontSize: 17, fontWeight: "600", color: colors.popoverForeground }}>{t("ui.windows.crash_detected")}</Text>
        <Text style={{ fontSize: 13, lineHeight: 19, color: colors.popoverForeground }}>{t("ui.dialogs.crash_message")}</Text>
        <Scroll style={{ maxHeight: 320 }}>
          <View selectable>
            <Text style={{ fontSize: 12, lineHeight: 17, color: colors.popoverForeground }}>{crashReport() ?? ""}</Text>
          </View>
        </Scroll>
        <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
          <Button label={t("ui.buttons.cancel")} onPress={() => crashReport.set(null)}/>
          <Button
            label={t("ui.buttons.copy")}
            variant="primary"
            onPress={() => {
              void Shell.copyText(crashReport() ?? "");
              toast.ok(t("ui.stats.copied"));
              crashReport.set(null);
            }}
          />
        </View>
      </Surface>
    </Dialog>
  );
}
