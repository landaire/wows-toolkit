import { Shell } from "wows-toolkit/native";
import { t } from "../i18n";
import { editSetting } from "../settings_store";
import * as toast from "./toast";

export { copyLatestLog } from "./diagnostics";
export { contributeAll } from "./uploads";

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Draws the app in a named palette and remembers it. Takes the stored name
 * (`"System"`, `"Dracula"`, ...) or the palette commands' `"system"`,
 * `"dark"` and `"light"`.
 */
export function setTheme(theme: string) {
  const stored = theme.length > 0 ? theme[0].toUpperCase() + theme.slice(1) : theme;
  void Shell.applyTheme(stored);
  void editSetting("theme", stored);
}

/** Puts the bug report details on the clipboard. */
export async function copyBugReportInfo() {
  try {
    await Shell.copyBugReportInfo();
    toast.ok(t("ui.messages.bug_report_info_copied"));
  } catch (error) {
    toast.failed(errorText(error));
  }
}

/**
 * Installs a result mapping the reader picks for the loaded build. Resolves
 * to the build it was installed for, so an open replay can be read again.
 */
export async function importConstants(): Promise<number | null> {
  try {
    const imported = await Shell.importConstants(t("ui.replay.constants_import_title"));
    if (imported.state === "noBuild") {
      toast.warn(t("ui.replay.constants_no_build"));
      return null;
    }
    if (imported.state === "imported" && imported.build !== undefined && imported.build !== null) {
      toast.ok(t("ui.replay.constants_imported", { build: imported.build }));
      return imported.build;
    }
    return null;
  } catch (error) {
    toast.failed(t("ui.replay.constants_import_failed", { reason: errorText(error) }));
    return null;
  }
}
