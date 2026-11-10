import { host } from "nsh";
import { t } from "../i18n";
import * as toast from "./toast";

/** What a window of its own shows, as the desktop runner opens it (`WindowRequest` in `src/shell/window.rs`). */
export type WindowRequest =
  | { kind: "tacticsBoard" }
  | { kind: "replayRenderer"; path: string; renderer: number };

/** What this window was opened to show, `null` in the main window. */
export function requestedWindow(): WindowRequest | null {
  const asked = host.params.window;
  if (typeof asked !== "object" || asked === null || Array.isArray(asked)) return null;
  if (asked.kind === "tacticsBoard") return { kind: "tacticsBoard" };
  if (asked.kind === "replayRenderer" && typeof asked.path === "string" && typeof asked.renderer === "number") {
    return { kind: "replayRenderer", path: asked.path, renderer: asked.renderer };
  }
  return null;
}

/** Opens the tactics board in a window of its own, or brings the open one forward. */
export function openTacticsWindow() {
  host.call("shell.openWindow", { kind: "tacticsBoard" }).catch(() => toast.failed(t("ui.tactics.window_failed")));
}

/**
 * Moves the live viewport `renderer`, showing `path`, into a window of its
 * own. Rejects when the window could not be opened.
 */
export async function popOutRenderer(path: string, renderer: number): Promise<void> {
  await host.call("shell.openWindow", { kind: "replayRenderer", path, renderer });
}
