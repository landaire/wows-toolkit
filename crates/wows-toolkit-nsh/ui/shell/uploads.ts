import { Uploads, type Ledger } from "wows-toolkit/native";
import { t } from "../i18n";
import { appSettings } from "../settings_store";
import * as toast from "./toast";

const CONTRIBUTE_PROGRESS = "contribute-all";
let running = false;

/** Contributes every replay in the game's replay directory, as the data-sharing setting asks, reporting as it goes. */
export async function contributeAll(ledger: Ledger) {
  if ((appSettings()?.dataSharing ?? "off") === "off") {
    toast.warn(t("ui.replay.contribute_nothing_shared"));
    return;
  }
  const paths = Uploads.listReplays();
  if (paths.length === 0) {
    toast.warn(t("ui.replay.contribute_none_listed"));
    return;
  }
  if (running) {
    toast.warn(t("ui.replay.contribute_already_running"));
    return;
  }
  running = true;
  let sent = 0;
  try {
    for (const [index, path] of paths.entries()) {
      toast.progress(CONTRIBUTE_PROGRESS, t("ui.replay.contributing", { done: index + 1, total: paths.length }));
      try {
        const contribution = await Uploads.contributeReplay(path, ledger);
        if (contribution.outcome === "sent") sent += 1;
      } catch {
        // A replay this build cannot read is skipped; the pass goes on, as
        // the other front ends' passes do.
        continue;
      }
    }
  } finally {
    running = false;
    toast.resolved(CONTRIBUTE_PROGRESS);
  }
  toast.ok(t("ui.replay.contributed", { sent, total: paths.length }));
}
