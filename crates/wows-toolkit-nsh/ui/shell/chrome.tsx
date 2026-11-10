import { host, keymap, signal, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { Menu, Toolbar } from "nsh/base";
import { Settings, Shell } from "wows-toolkit/native";
import { MenuRow, Surface } from "../components";
import { t } from "../i18n";
import { Icon } from "../icons";
import { appSettings, editSetting } from "../settings_store";
import { colors, controlRadius } from "../theme";
import { openAbout } from "./about";
import { checkForUpdate } from "./update";

/** Where the menu's links go, as the other front ends send them. */
const ISSUES_URL = "https://github.com/landaire/wows-toolkit/issues/new/choose";
const DISCORD_URL = "https://discord.gg/RJXjXHUj7rh";

/** The zoom bounds and step every front end's slider uses. */
export const MIN_ZOOM = 0.5;
export const MAX_ZOOM = 2.0;
export const ZOOM_STEP = 0.05;
export const DEFAULT_ZOOM = 1.15;

/** A game directory given in Settings or the Launcher that is not an install; it stays in the field until another is given. */
export const rejectedWowsDir = signal<string | null>(null);

/** Whether the game directory given or saved is one an install could not be in, which the Settings tab carries a mark for. */
export function wowsDirInvalid(): boolean {
  if (rejectedWowsDir() !== null) return true;
  const dir = appSettings()?.wowsDir ?? "";
  return dir !== "" && !Settings.isGameDirectory(dir);
}

/** Scales the whole window to `zoom`. The window is the embedding app's, so the scale is its to apply. */
export function applyZoom(zoom: number) {
  void host.call("shell.zoom", { zoom }).catch(() => undefined);
}

/** Sets the zoom, applies it, and saves it, held to the slider's bounds and step. */
export function setZoom(zoom: number) {
  const held = Math.round(Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom)) / ZOOM_STEP) * ZOOM_STEP;
  const rounded = Number(held.toFixed(2));
  applyZoom(rounded);
  void editSetting("zoom", rounded);
}

keymap.bind({
  "mod+=": () => setZoom((appSettings()?.zoom ?? DEFAULT_ZOOM) + ZOOM_STEP),
  "mod+-": () => setZoom((appSettings()?.zoom ?? DEFAULT_ZOOM) - ZOOM_STEP),
});

/** The application menu at the end of the tab strip: what the other front ends' menu bar carries. */
export function ApplicationMenu(props: { onCommandPalette: () => void; onTacticsBoard: () => void }) {
  return (
    <Toolbar>
      <Menu
        align="end"
        offset={5}
        margin={8}
        trigger={
          <View
            accessibilityLabel={t("ui.menu.file")}
            tooltip={t("ui.menu.file")}
            hoverStyle={{ backgroundColor: colors.accent }}
            activeStyle={{ opacity: 0.68 }}
            style={{ width: 32, height: 28, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}
          >
            <Icon name="ellipsis" color={colors.foreground}/>
          </View>
        }
      >
        <Surface style={{ minWidth: 230, padding: 5, backgroundColor: colors.popover }}>
          <MenuRow label={t("ui.palette.title")} note={keymap.label("mod+k")} onSelect={props.onCommandPalette}/>
          <MenuRow label={t("ui.windows.tactics_board")} onSelect={props.onTacticsBoard}/>
          <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>
          <MenuRow label={t("ui.menu.check_updates")} onSelect={() => void checkForUpdate("byHand")}/>
          <MenuRow label={t("ui.menu.about")} onSelect={openAbout}/>
          <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>
          <MenuRow label={t("ui.buttons.create_issue")} onSelect={() => void Shell.openUrl(ISSUES_URL)}/>
          <MenuRow label={t("ui.buttons.discord")} onSelect={() => void Shell.openUrl(DISCORD_URL)}/>
          <View style={{ height: 1, marginVertical: 4, backgroundColor: colors.border }}/>
          <MenuRow label={t("ui.menu.quit")} note={keymap.label("mod+q")} danger onSelect={() => Window.close()}/>
        </Surface>
      </Menu>
    </Toolbar>
  );
}

/** Whether files are being dragged over the window. */
export const hoveringFiles = signal(false);

/** The `file:` URIs the drag over the window carries, once the shell has read them; `null` until then. */
export const hoveredUris = signal<string[] | null>(null);

/**
 * Set once nsh says the drag left or landed. GTK on X11 says nothing to the
 * shell after a drop, so a file list it read late belongs to a drag that is
 * over, until the shell says that drag left or nsh says another began.
 */
let dragEnded = false;

// The shell reads the drag's file list as it enters the window, which nsh's
// own drag events do not carry, and clears it as the drag leaves.
host.on("shell.hoveredFiles", (data: unknown) => {
  const uris = (data as { uris?: unknown } | null)?.uris;
  if (!Array.isArray(uris)) {
    dragEnded = false;
    hoveredUris.set(null);
    return;
  }
  if (dragEnded) return;
  hoveredUris.set(uris.filter((uri): uri is string => typeof uri === "string"));
});

/** nsh's word that files are dragged over the window. */
export function startFileDrag() {
  dragEnded = false;
  hoveringFiles.set(true);
}

/** nsh's word that the drag left the window or was dropped on it. */
export function endFileDrag() {
  dragEnded = true;
  hoveringFiles.set(false);
  hoveredUris.set(null);
}

/** The local replay files among `uris`, as paths: what a drop of them opens. */
export function replayPaths(uris: string[]): string[] {
  const replays: string[] = [];
  for (const uri of uris) {
    try {
      const url = new URL(uri);
      if (url.protocol === "file:" && url.pathname.toLowerCase().endsWith(".wowsreplay")) replays.push(decodeURIComponent(url.pathname));
    } catch {
      continue;
    }
  }
  return replays;
}

/** What the scrim says of a drag: what dropping it will do, or the rule until its files are known. */
function scrimText(uris: string[] | null): string {
  if (uris === null) return t("ui.app.drop_replay");
  const replays = replayPaths(uris);
  if (replays.length === 0) return t("ui.messages.drop_not_a_replay");
  if (replays.length > 1) return t("ui.messages.drop_one_at_a_time");
  const path = replays[0];
  return t("ui.app.drop_to_load", { file: path.slice(path.lastIndexOf("/") + 1) });
}

/**
 * Over the window while files are dragged onto it, so a drag says what it
 * will do before it lands: the file it will open, or why it will not.
 */
export function DropScrim() {
  const said = () => scrimText(hoveredUris());
  return (
    <If when={hoveringFiles() || hoveredUris() !== null}>
      <View
        accessibilityLabel={said()}
        style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center", backgroundColor: "rgba(0,0,0,0.75)" }}
      >
        <Text style={{ fontSize: 18, color: "#ffffff" }}>{said()}</Text>
      </View>
    </If>
  );
}
