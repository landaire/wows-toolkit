import { host } from "nsh";
import { t } from "../i18n";
import * as toast from "./toast";

/** What a window of its own shows, as the desktop runner opens it (`WindowRequest` in `src/shell/window.rs`). */
export type WindowRequest =
  | { kind: "tacticsBoard" }
  | { kind: "replayRenderer"; path: string; alts: string[] };

/** What this window was opened to show, `null` in the main window. */
export function requestedWindow(): WindowRequest | null {
  const asked = host.params.window;
  if (typeof asked !== "object" || asked === null || Array.isArray(asked)) return null;
  if (asked.kind === "tacticsBoard") return { kind: "tacticsBoard" };
  const alts = asked.alts;
  if (asked.kind === "replayRenderer" && typeof asked.path === "string" && Array.isArray(alts) && alts.every((alt) => typeof alt === "string")) {
    return { kind: "replayRenderer", path: asked.path, alts: alts as string[] };
  }
  return null;
}

/** Opens the tactics board in a window of its own, or brings the open one forward. */
export function openTacticsWindow() {
  host.call("shell.openWindow", { kind: "tacticsBoard" }).catch(() => toast.failed(t("ui.tactics.window_failed")));
}

/** Opens a renderer on `path` in a window of its own. Rejects when the window could not be opened. */
export async function popOutRenderer(path: string, alts: string[]): Promise<void> {
  await host.call("shell.openWindow", { kind: "replayRenderer", path, alts });
}
