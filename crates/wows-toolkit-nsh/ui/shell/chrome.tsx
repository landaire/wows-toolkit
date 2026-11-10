import type { JSX } from "nsh";
import { host, keymap, signal, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { Menu, Toolbar } from "nsh/base";
import { Settings, Shell } from "wows-toolkit/native";
import { MenuRow, Surface } from "../components";
import { t } from "../i18n";
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

/** Whether the saved game directory is one an install could not be in, which the Settings tab carries a mark for. */
export function wowsDirInvalid(): boolean {
  const dir = appSettings()?.wowsDir ?? "";
  return dir !== "" && !Settings.isGameDirectory(dir);
}

/** Scales the window's text to `zoom`. The window is the embedding app's, so the scale is its to apply. */
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
    <Toolbar accessibilityLabel="Application menu">
      <Menu
        align="end"
        offset={5}
        margin={8}
        trigger={
          <View
            accessibilityLabel="Open application menu"
            hoverStyle={{ backgroundColor: colors.accent }}
            activeStyle={{ opacity: 0.68 }}
            style={{ width: 32, height: 28, borderRadius: controlRadius(), alignItems: "center", justifyContent: "center" }}
          >
            <Text style={{ fontSize: 16, color: colors.foreground }}>...</Text>
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

/** The tab strip: every top-level tab, the selected one underlined, and the menu at its end. */
export function TabStrip(props: {
  tabs: readonly { id: string; label: string }[];
  active: string;
  /** Tabs that need looking at, marked as the other front ends mark them. */
  attention: (id: string) => boolean;
  onSelect: (id: string) => void;
  menu: JSX.Element;
}) {
  return (
    <View style={{ borderBottomWidth: 1, borderBottomColor: colors.borderBright, backgroundColor: colors.surface }}>
      <View role="tablist" style={{ minHeight: 36, flexDirection: "row", alignItems: "center", gap: 2, paddingHorizontal: 6 }}>
        <Each of={props.tabs}>
          {(tab) => {
            const selected = () => props.active === tab().id;
            const flagged = () => props.attention(tab().id);
            return (
              <View
                role="tab"
                accessibilityLabel={tab().label}
                accessibilityState={{ selected: selected() }}
                onPress={() => props.onSelect(tab().id)}
                hoverStyle={selected() ? undefined : { backgroundColor: colors.accent }}
                activeStyle={{ opacity: 0.7 }}
                style={{
                  height: 32,
                  paddingHorizontal: 10,
                  justifyContent: "center",
                  borderBottomWidth: 2,
                  borderBottomColor: selected() ? colors.primary : "transparent",
                  backgroundColor: selected() ? colors.background : "transparent",
                }}
              >
                <Text style={{
                  fontSize: 12.5,
                  fontWeight: selected() ? "600" : "400",
                  color: flagged() ? colors.destructive : selected() ? colors.textBright : colors.mutedForeground,
                }}>
                  {flagged() ? `! ${tab().label}` : tab().label}
                </Text>
              </View>
            );
          }}
        </Each>
        <View style={{ flex: 1 }}/>
        {props.menu}
      </View>
    </View>
  );
}

/** Whether files are being dragged over the window. */
export const hoveringFiles = signal(false);

/**
 * Over the window while files are dragged onto it, so a drag says it will be
 * taken before it lands. nsh reports a drag's start and end but not which
 * files it carries, so this names the rule rather than the file.
 */
export function DropScrim() {
  return (
    <If when={hoveringFiles()}>
      <View
        accessibilityLabel={t("ui.app.drop_to_load", { file: "replay" })}
        style={{ position: "absolute", top: 0, left: 0, right: 0, bottom: 0, alignItems: "center", justifyContent: "center", backgroundColor: "rgba(0,0,0,0.75)" }}
      >
        <Text style={{ fontSize: 18, color: "#ffffff" }}>{t("ui.app.drop_to_load", { file: "replay" })}</Text>
      </View>
    </If>
  );
}
