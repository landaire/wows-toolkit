import type { JSX } from "nsh";
import { colorScheme, Scroll, signal, Text, View } from "nsh";
import { Menu, Slider } from "nsh/base";
import { Settings, Shell, Twitch } from "wows-toolkit/native";
import { Button, EmptyState, MenuRow, Surface } from "./components";
import { currentLocale, t } from "./i18n";
import { appSettings, editReplayPreferences, editSetting, settingsLoadError, settingsLoading } from "./settings_store";
import { DEFAULT_ZOOM, MAX_ZOOM, MIN_ZOOM, setZoom, wowsDirInvalid, ZOOM_STEP } from "./shell/chrome";
import { Choice, CommitInput, Toggle } from "./shell/controls";
import { chooseLanguage } from "./shell/first_run";
import * as toast from "./shell/toast";
import { restartTwitch, twitchStatus } from "./shell/twitch";
import { colors, controlRadius, semantic } from "./theme";
import { GameDataCacheSection, ReplayDirectoriesSection, ReplayIndexSection } from "./game_data";

/** Where a Twitch credential with the permissions the poll needs is issued: Chatterino's login page, as the other front ends use. */
const TWITCH_TOKEN_URL = "https://chatterino.com/client_login";

const STUCK_WOWS_DIR = "wows-dir";

/** The formats a battle can be auto-exported in, in the order the other front ends list them. */
const EXPORT_FORMATS = [
  { id: "json", key: "ui.settings.replay.format_json" },
  { id: "csv", key: "ui.settings.replay.format_csv" },
  { id: "cbor", key: "ui.settings.replay.format_cbor" },
];

/** What the last Twitch paste did, shown beside the button: who it signed in, or why it did not. */
const twitchPaste = signal<{ who?: string; problem?: string } | null>(null);

/** The palette shown while the pointer rests on a theme tile, before it is chosen. */
const themePreview = signal<string | null>(null);

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Section(props: { title: string; description: string; children: JSX.Element }) {
  return (
    <View style={{ gap: 8 }}>
      <View style={{ gap: 2 }}>
        <Text role="heading" style={{ fontSize: 14, fontWeight: "700", color: colors.foreground }}>{props.title}</Text>
        <Text style={{ fontSize: 12, color: colors.mutedForeground }}>{props.description}</Text>
      </View>
      <View style={{ padding: 12, gap: 12, borderWidth: 1, borderColor: colors.border, borderRadius: 6, backgroundColor: colors.surface }}>
        {props.children}
      </View>
    </View>
  );
}

/** One row of a section: the label in a column of its own, so the tab reads down one edge. */
function Field(props: { label?: string; description?: string; invalid?: boolean; children: JSX.Element }) {
  return (
    <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 12 }}>
      <Text style={{ width: 260, paddingTop: 6, fontSize: 12.5, color: colors.cardForeground }}>{props.label ?? ""}</Text>
      <View style={{ flex: 1, minWidth: 0, gap: 4 }}>
        {props.children}
        <If when={props.description}>
          <Text style={{ fontSize: 11.5, lineHeight: 16, color: props.invalid ? semantic.error : colors.mutedForeground }}>{props.description}</Text>
        </If>
      </View>
    </View>
  );
}

function LanguageField() {
  const languages = Shell.languages();
  const current = () => languages.find((language) => language.code === (appSettings()?.locale ?? currentLocale()))?.nativeName ?? "English";
  return (
    <Field label={t("ui.settings.app.language")}>
      <Menu
        align="start"
        offset={4}
        margin={8}
        trigger={
          <View
            accessibilityLabel={t("ui.settings.app.language")}
            hoverStyle={{ backgroundColor: colors.accent }}
            style={{ width: 200, minHeight: 28, paddingHorizontal: 8, justifyContent: "center", borderWidth: 1, borderColor: colors.input, borderRadius: controlRadius(), backgroundColor: colors.background }}
          >
            <Text style={{ fontSize: 12.5, color: colors.foreground }}>{current()}</Text>
          </View>
        }
      >
        <Surface style={{ width: 200, maxHeight: 360, padding: 4, backgroundColor: colors.popover }}>
          <Scroll style={{ maxHeight: 350 }}>
            <Each of={languages}>
              {(language) => <MenuRow label={language().nativeName} onSelect={() => void chooseLanguage(language().code)}/>}
            </Each>
          </Scroll>
        </Surface>
      </Menu>
    </Field>
  );
}

function ThemeField() {
  const chosen = () => appSettings()?.theme ?? "System";
  const themes = () => Shell.themes(colorScheme() === "dark");
  const show = (name: string) => void Shell.applyTheme(name);
  const choose = (name: string) => {
    themePreview.set(null);
    show(name);
    void editSetting("theme", name);
  };
  return (
    <Field label={t("ui.settings.app.theme")}>
      <View accessibilityLabel={t("ui.settings.app.theme")} role="radiogroup" style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
        <Each of={themes()}>
          {(theme) => (
            <View
              onMouseEnter={() => { themePreview.set(theme().id); show(theme().id); }}
              onMouseLeave={() => {
                if (themePreview() !== theme().id) return;
                themePreview.set(null);
                show(chosen());
              }}
            >
              <Choice
                label={theme().label}
                accessibilityLabel={theme().label}
                selected={chosen() === theme().id}
                style={{ width: 142, height: 40, paddingHorizontal: 8 }}
                onPress={() => choose(theme().id)}
              >
                <View style={{ gap: 3 }}>
                  <Text style={{ fontSize: 11, color: colors.secondaryForeground }}>{theme().label}</Text>
                  <View style={{ flexDirection: "row", gap: 3 }}>
                    <Each of={theme().swatches}>
                      {(swatch) => <View style={{ width: 22, height: 9, borderRadius: 1, backgroundColor: swatch() }}/>}
                    </Each>
                  </View>
                </View>
              </Choice>
            </View>
          )}
        </Each>
      </View>
    </Field>
  );
}

function ZoomField() {
  const zoom = () => appSettings()?.zoom ?? DEFAULT_ZOOM;
  return (
    <Field label={t("ui.settings.app.zoom_factor")}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <Slider
          accessibilityLabel={t("ui.settings.app.zoom_factor")}
          value={zoom()}
          min={MIN_ZOOM}
          max={MAX_ZOOM}
          step={ZOOM_STEP}
          class="w-[160px] h-[20px] justify-center"
          trackClass="h-[4px] rounded-full bg-secondary"
          indicatorClass="h-[4px] rounded-full bg-primary"
          thumbClass="w-[14px] h-[14px] rounded-full bg-primary"
          onChangeEnd={({ value }) => setZoom(value as number)}
        />
        <Text style={{ width: 40, fontSize: 12.5, color: colors.foreground }}>{zoom().toFixed(2)}</Text>
        <Button label={t("ui.buttons.reset")} onPress={() => setZoom(DEFAULT_ZOOM)}/>
      </View>
    </Field>
  );
}

function DataSharingField() {
  const modes = Settings.sharingModes();
  const chosen = () => appSettings()?.dataSharing ?? "off";
  return (
    <Field label={t("ui.settings.app.data_sharing_mode")} description={modes.find((mode) => mode.id === chosen())?.description}>
      <View role="radiogroup" accessibilityLabel={t("ui.settings.app.data_sharing_mode")} style={{ flexDirection: "row", gap: 8 }}>
        <Each of={modes}>
          {(mode) => <Choice label={mode().label} selected={chosen() === mode().id} onPress={() => void editSetting("dataSharing", mode().id)}/>}
        </Each>
      </View>
    </Field>
  );
}

function ApplicationSection() {
  const settings = () => appSettings()!;
  return (
    <Section title={t("ui.settings.app.heading")} description={t("ui.settings.app.description")}>
      <LanguageField/>
      <ThemeField/>
      <ZoomField/>
      <DataSharingField/>
      <Field label={t("ui.settings.app.proxy_url")} description={t("ui.settings.app.proxy_url_hint")}>
        <CommitInput label={t("ui.settings.app.proxy_url")} value={settings().proxyUrl} placeholder="http://host:port" onCommit={(value) => void editSetting("proxyUrl", value)}/>
      </Field>
      <Field>
        <View style={{ gap: 8 }}>
          <Toggle label={t("ui.settings.app.check_for_updates")} checked={settings().checkForUpdates} onChange={(checked) => void editSetting("checkForUpdates", checked)}/>
          <Toggle
            label={t("ui.settings.app.enable_logging")}
            description={t("ui.settings.app.enable_logging_tooltip")}
            checked={settings().enableLogging}
            onChange={(checked) => void editSetting("enableLogging", checked)}
          />
        </View>
      </Field>
      <Field>
        <View style={{ flexDirection: "row" }}>
          <Button
            label={t("ui.settings.app.open_data_dir")}
            onPress={() => void Shell.openDataDirectory().catch((error) => toast.failed(errorText(error)))}
          />
        </View>
      </Field>
    </Section>
  );
}

/** Adopts a game directory: one that is not an install is said to be wrong and not saved, as every front end refuses it. */
export function applyWowsDir(path: string) {
  if (path !== "" && !Settings.isGameDirectory(path)) {
    toast.stuck(STUCK_WOWS_DIR, t("ui.messages.wows_dir_invalid"));
    return;
  }
  toast.resolved(STUCK_WOWS_DIR);
  void editSetting("wowsDir", path);
}

const rejectedWowsDir = signal<string | null>(null);

function GameSection() {
  const invalid = () => rejectedWowsDir() !== null || wowsDirInvalid();
  const adopt = (path: string) => {
    const refused = path !== "" && !Settings.isGameDirectory(path);
    rejectedWowsDir.set(refused ? path : null);
    applyWowsDir(path);
  };
  return (
    <Section title={t("ui.settings.wows.heading")} description={t("ui.settings.wows.description")}>
      <Field label={t("ui.settings.wows.directory_hint")} description={invalid() ? t("ui.messages.wows_dir_invalid") : ""} invalid={invalid()}>
        <View style={{ flexDirection: "row", gap: 8 }}>
          <View style={{ flex: 1 }}>
            <CommitInput
              label={t("ui.settings.wows.directory_hint")}
              value={rejectedWowsDir() ?? appSettings()!.wowsDir}
              placeholder={t("ui.settings.wows.directory_hint")}
              invalid={invalid()}
              onCommit={adopt}
            />
          </View>
          <Button
            label={t("ui.settings.wows.browse")}
            onPress={() => {
              void Shell.pickDirectory("World of Warships directory")
                .then((picked) => { if (picked) adopt(picked); })
                .catch((error) => toast.failed(errorText(error)));
            }}
          />
        </View>
      </Field>
    </Section>
  );
}

function SessionSection() {
  const settings = () => appSettings()!;
  return (
    <Section title={t("ui.settings.session.heading")} description={t("ui.settings.session.description")}>
      <Field label={t("ui.settings.session.display_name")}>
        <CommitInput
          label={t("ui.settings.session.display_name")}
          value={settings().collabDisplayName}
          placeholder={t("ui.collab.display_name_hint")}
          onCommit={(value) => void editSetting("collabDisplayName", value)}
        />
      </Field>
      <Field>
        <View style={{ gap: 8 }}>
          <Toggle
            label={t("ui.settings.session.suppress_ip_warning")}
            description={t("ui.settings.session.ip_warning_tooltip")}
            checked={settings().suppressP2pIpWarning}
            onChange={(checked) => void editSetting("suppressP2pIpWarning", checked)}
          />
          <Toggle
            label={t("ui.settings.session.disable_auto_open")}
            description={t("ui.settings.session.auto_open_tooltip")}
            checked={settings().disableAutoOpenSessionWindows}
            onChange={(checked) => void editSetting("disableAutoOpenSessionWindows", checked)}
          />
        </View>
      </Field>
    </Section>
  );
}

function AutoExportRow() {
  const replay = () => appSettings()!.replay;
  const path = () => replay().autoExportPath;
  // A directory that is not there cannot be written to, and the reader would
  // otherwise find out only when a battle's export is lost.
  const missing = () => path() !== "" && !Settings.isDirectory(path());
  return (
    <View style={{ gap: 8 }}>
      <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 16 }}>
        <Toggle label={t("ui.settings.replay.auto_export_data")} checked={replay().autoExportData} onChange={(checked) => void editReplayPreferences({ autoExportData: checked })}/>
        <View role="radiogroup" accessibilityLabel="Export format" style={{ flexDirection: "row", gap: 8 }}>
          <Each of={EXPORT_FORMATS}>
            {(format) => (
              <Choice
                label={t(format().key)}
                selected={replay().autoExportFormat === format().id}
                disabled={!replay().autoExportData}
                onPress={() => void editReplayPreferences({ autoExportFormat: format().id })}
              />
            )}
          </Each>
        </View>
      </View>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
        <Button
          label={t("ui.buttons.choose")}
          disabled={!replay().autoExportData}
          onPress={() => {
            void Shell.pickDirectory(t("ui.settings.replay.export_path_hint"))
              .then((picked) => { if (picked) void editReplayPreferences({ autoExportPath: picked }); })
              .catch((error) => toast.failed(errorText(error)));
          }}
        />
        <View selectable>
          <Text accessibilityLabel="Auto-export directory" style={{ fontSize: 12.5, color: missing() ? semantic.error : colors.foreground }}>
            {path() === "" ? t("ui.settings.not_set") : path()}
          </Text>
        </View>
      </View>
    </View>
  );
}

function ReplaySection() {
  const replay = () => appSettings()!.replay;
  const current = () => appSettings()!.currentReplayPath;
  return (
    <Section title={t("ui.settings.replay.heading")} description={t("ui.settings.replay.description")}>
      <Field label={t("ui.settings.replay.current_path")}>
        <View selectable style={{ paddingTop: 6 }}>
          <Text style={{ fontSize: 12.5, color: colors.foreground }}>{current() === "" ? t("ui.settings.not_set") : current()}</Text>
        </View>
      </Field>
      <Field>
        <View style={{ flexDirection: "row", flexWrap: "wrap", columnGap: 16, rowGap: 8 }}>
          <Toggle label={t("ui.settings.replay.show_raw_xp")} checked={replay().showRawXp} onChange={(checked) => void editReplayPreferences({ showRawXp: checked })}/>
          <Toggle label={t("ui.settings.replay.show_observed_damage")} checked={replay().showObservedDamage} onChange={(checked) => void editReplayPreferences({ showObservedDamage: checked })}/>
          <Toggle label={t("ui.settings.replay.show_entity_id")} checked={replay().showEntityId} onChange={(checked) => void editReplayPreferences({ showEntityId: checked })}/>
          <Toggle label={t("ui.settings.replay.show_heals")} checked={replay().showHeals} onChange={(checked) => void editReplayPreferences({ showHeals: checked })}/>
        </View>
      </Field>
      <Field description={t("ui.settings.replay.enable_previews_tooltip")}>
        <Toggle label={t("ui.settings.replay.enable_previews")} checked={replay().enableReplayPreviews} onChange={(checked) => void editReplayPreferences({ enableReplayPreviews: checked })}/>
      </Field>
      <Field label={t("ui.settings.replay.auto_export_data")} description={t("ui.settings.replay.export_path_hint")}>
        <AutoExportRow/>
      </Field>
    </Section>
  );
}

function TwitchSection() {
  const status = () => twitchStatus();
  const pasteLabel = () => {
    const pasted = twitchPaste();
    if (pasted?.problem) return t("ui.settings.twitch.paste_token_invalid");
    const current = status();
    if (current === null) return t("ui.settings.twitch.paste_token_unvalidated");
    if (current.state === "accepted") return t("ui.settings.twitch.paste_token_valid");
    if (current.state === "refused") return t("ui.settings.twitch.paste_token_invalid");
    return t("ui.settings.twitch.paste_token_no_token");
  };
  const pasteNote = () => {
    const pasted = twitchPaste();
    if (pasted?.who) return t("ui.settings.twitch.signed_in_as", { who: pasted.who });
    return pasted?.problem ?? "";
  };
  const paste = async () => {
    try {
      const who = await Twitch.pasteToken();
      if (who === null || who === undefined) {
        twitchPaste.set({ problem: t("ui.collab.clipboard_empty") });
        return;
      }
      twitchPaste.set({ who });
      await restartTwitch();
    } catch (error) {
      twitchPaste.set({ problem: errorText(error) });
    }
  };
  return (
    <Section title={t("ui.settings.twitch.heading")} description={t("ui.settings.twitch.description")}>
      <Field description={t("ui.settings.twitch.get_token_tooltip")}>
        <View style={{ flexDirection: "row" }}>
          <Button label={t("ui.settings.twitch.get_token")} onPress={() => void Shell.openUrl(TWITCH_TOKEN_URL)}/>
        </View>
      </Field>
      <Field description={pasteNote()} invalid={twitchPaste()?.problem !== undefined}>
        <View style={{ flexDirection: "row" }}>
          <Button label={pasteLabel()} onPress={() => void paste()}/>
        </View>
      </Field>
      <Field label={t("ui.settings.twitch.monitored_channel")}>
        <View style={{ width: 240 }}>
          <CommitInput
            label={t("ui.settings.twitch.monitored_channel")}
            value={appSettings()!.twitchChannel}
            placeholder={t("ui.settings.twitch.monitored_channel")}
            onCommit={(value) => {
              // The running poll reads the old channel, so it starts again.
              void editSetting("twitchChannel", value).then((saved) => { if (saved) void restartTwitch(); });
            }}
          />
        </View>
      </Field>
    </Section>
  );
}

/** The viewport writes these itself when a pane changes, so the tab reports them rather than offering a second way to set them. */
function ArmorSection() {
  const defaults = () => appSettings()!.armorDefaults;
  return (
    <Section title={t("ui.settings.armor.heading")} description={t("ui.settings.armor.description")}>
      <Field>
        <If when={defaults()} else={<Text style={{ fontSize: 12.5, color: colors.mutedForeground }}>{t("ui.settings.armor.no_defaults")}</Text>}>
          <View style={{ flexDirection: "row", gap: 16 }}>
            <Toggle label={t("ui.armor.plate_edges")} checked={defaults()?.showPlateEdges ?? false} disabled onChange={() => undefined}/>
            <Toggle label={t("ui.armor.waterline")} checked={defaults()?.showWaterline ?? false} disabled onChange={() => undefined}/>
            <Toggle label={t("ui.armor.opaque_hull")} checked={defaults()?.hullOpaque ?? false} disabled onChange={() => undefined}/>
          </View>
        </If>
      </Field>
    </Section>
  );
}

/** The Settings tab: every shared setting, written as it is edited. */
export function SettingsView() {
  return (
    <View style={{ flex: 1, minHeight: 0 }}>
      <If when={settingsLoading() && !appSettings()}>
        <View style={{ padding: 16 }}>
          <Text style={{ fontSize: 13, color: colors.mutedForeground }}>{t("ui.settings.loading")}</Text>
        </View>
      </If>
      <If when={!settingsLoading() && !appSettings()}>
        <EmptyState title={t("ui.settings.load_failed")} description={settingsLoadError() ?? ""}/>
      </If>
      <If when={appSettings()}>
        <Scroll accessibilityLabel="Settings" style={{ flex: 1 }}>
          <View style={{ padding: 16, gap: 24 }}>
            <ApplicationSection/>
            <GameSection/>
            <GameDataCacheSection/>
            <ReplayIndexSection/>
            <ReplayDirectoriesSection/>
            <SessionSection/>
            <ReplaySection/>
            <TwitchSection/>
            <ArmorSection/>
          </View>
        </Scroll>
      </If>
    </View>
  );
}
